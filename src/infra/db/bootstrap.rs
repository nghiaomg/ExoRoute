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

/// Reopens every stream run in bounded pages. Older builds stopped after
/// 1,000 rows, which left an over-cap table permanently unable to create new
/// runs because the creation gate refused to run its own pruning. This pass
/// also recomputes the derived retained-byte counter from the primary rows.
pub(crate) async fn recover_abandoned_stream_runs(database: &Database) -> Result<(), StorageError> {
    use std::time::Duration;
    let now = super::clock::utc_timestamp_now()?;
    let cutoff = super::clock::utc_timestamp_before(Duration::from_secs(24 * 60 * 60))?;
    const PAGE_SIZE: usize = 256;
    database
        .write(move |transaction| {
            let mut after = None;
            let mut retained_total = 0i64;
            let mut expired = Vec::new();
            loop {
                let page = transaction.scan_prefix_after::<Record>(
                    Table::StreamRuns,
                    "",
                    after.as_deref(),
                    PAGE_SIZE,
                )?;
                if page.is_empty() {
                    break;
                }
                let page_len = page.len();
                for (id, mut run) in page {
                    after = Some(id.clone());
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
                        expired.push((id, run));
                        continue;
                    }
                    retained_total = retained_total
                        .saturating_add(run.optional_integer("retained_bytes")?.unwrap_or(0).max(0));
                }
                if page_len < PAGE_SIZE {
                    break;
                }
            }
            for (id, run) in expired {
                transaction.delete(Table::StreamRuns, &id)?;
                transaction.delete_prefix(Table::StreamEvents, &format!("{id}/"))?;
                let api_key_id = run.text("api_key_id")?;
                let index_key = format!("{api_key_id}/{id}");
                crate::infra::storage::validate_key(&index_key)?;
                transaction.delete(Table::StreamRunApiKeyIndex, &index_key)?;
            }
            super::set_stream_retained_bytes(transaction, retained_total)
        })
        .await
}
