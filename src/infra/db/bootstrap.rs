//! First-open bootstrap: singleton defaults, counters, and stream recovery.
//!
//! Runs once per `connect`; every branch is idempotent so reopening an
//! existing environment is a no-op.

use super::{SINGLETON_KEY, STATISTICS_WINDOWS};
use crate::infra::storage::{Database, Field, Record, StorageError, Table};

pub(super) async fn initialize_storage(database: &Database) -> Result<(), StorageError> {
    use super::settings_records::{
        gateway_limits_record, operational_settings_record, output_styles_record,
    };
    use crate::config::{GatewayResourceLimits, OperationalSettings};
    use crate::support::output_styles::OutputStylesSnapshot;
    let gateway_defaults = GatewayResourceLimits::default();
    let operational_defaults = OperationalSettings::default();
    let output_styles_defaults = OutputStylesSnapshot::default();
    let current_minute = super::clock::unix_minute_now()?;
    let now = super::clock::utc_timestamp_now()?;
    database
        .write(move |transaction| {
            if transaction
                .get::<Record>(Table::GatewayResourceLimits, SINGLETON_KEY)?
                .is_none()
            {
                transaction.put(
                    Table::GatewayResourceLimits,
                    SINGLETON_KEY,
                    &gateway_limits_record(gateway_defaults)?,
                )?;
            }
            if transaction
                .get::<Record>(Table::OperationalSettings, SINGLETON_KEY)?
                .is_none()
            {
                transaction.put(
                    Table::OperationalSettings,
                    SINGLETON_KEY,
                    &operational_settings_record(operational_defaults, 0, false)?,
                )?;
            }
            if transaction
                .get::<Record>(Table::OutputStyles, SINGLETON_KEY)?
                .is_none()
            {
                transaction.put(
                    Table::OutputStyles,
                    SINGLETON_KEY,
                    &output_styles_record(&output_styles_defaults)?,
                )?;
            }
            if transaction
                .get::<Record>(Table::StatisticsState, SINGLETON_KEY)?
                .is_none()
            {
                transaction.put(
                    Table::StatisticsState,
                    SINGLETON_KEY,
                    &Record::new()
                        .with("dropped_events", Field::I64(0))
                        .with("collection_started_at", Field::Null)
                        .with("updated_at", Field::Text(now.clone())),
                )?;
            }
            if transaction
                .get::<i64>(Table::Meta, "request_log_count")?
                .is_none()
            {
                transaction.put(Table::Meta, "request_log_count", &0_i64)?;
            }
            for key in ["provider_count", "route_count"] {
                if transaction.get::<i64>(Table::Meta, key)?.is_none() {
                    transaction.put(Table::Meta, key, &0_i64)?;
                }
            }
            if transaction
                .get::<i64>(Table::Meta, "statistics_generation")?
                .is_none()
            {
                transaction.put(Table::Meta, "statistics_generation", &0_i64)?;
            }
            for window in STATISTICS_WINDOWS {
                if transaction
                    .get::<Record>(Table::UsageExpiryCursor, &window.to_string())?
                    .is_none()
                {
                    transaction.put(
                        Table::UsageExpiryCursor,
                        &window.to_string(),
                        &Record::new()
                            .with(
                                "cursor_minute",
                                Field::I64(current_minute.saturating_sub(window)),
                            )
                            .with("cursor_dimension_kind", Field::I64(4))
                            .with("cursor_dimension_id", Field::Text(String::new())),
                    )?;
                }
            }
            Ok(())
        })
        .await
}

pub(super) async fn recover_abandoned_stream_runs(database: &Database) -> Result<(), StorageError> {
    use std::time::Duration;
    let now = super::clock::utc_timestamp_now()?;
    let cutoff = super::clock::utc_timestamp_before(Duration::from_secs(24 * 60 * 60))?;
    database
        .write(move |transaction| {
            let runs = transaction.scan_prefix::<Record>(Table::StreamRuns, "", 1_000)?;
            for (id, mut run) in runs {
                if run.optional_text("status")? == Some("running") {
                    run.insert("status", Field::Text("failed".to_owned()));
                    run.insert(
                        "error",
                        Field::Text("gateway restarted before the stream completed".to_owned()),
                    );
                    run.insert("finished_at", Field::Text(now.clone()));
                    transaction.put(Table::StreamRuns, &id, &run)?;
                } else if run
                    .optional_text("created_at")?
                    .is_some_and(|created| created < cutoff.as_str())
                {
                    transaction.delete(Table::StreamRuns, &id)?;
                    transaction.delete_prefix(Table::StreamEvents, &format!("{id}/"))?;
                }
            }
            Ok(())
        })
        .await
}
