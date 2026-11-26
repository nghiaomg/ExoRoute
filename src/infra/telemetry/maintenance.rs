use super::DropCounters;
use super::{STATISTICS_RANGES, cursor_record, unix_minute};
use crate::{
    infra::db,
    infra::storage::{Database, Field, Record, StorageError, Table},
};
use std::sync::atomic::Ordering;
use tokio::sync::Mutex as AsyncMutex;

pub(crate) async fn clear_history_in_database(db: &Database) -> Result<i64, String> {
    db.write(|transaction| {
        let request_count = transaction
            .get::<i64>(Table::Meta, "request_log_count")?
            .unwrap_or(0);
        if request_count < 0 {
            return Err(StorageError::Invalid(
                "request log counter is invalid".to_owned(),
            ));
        }
        transaction.clear(Table::RequestLogs)?;
        transaction.clear(Table::RequestLogIndex)?;
        transaction.clear(Table::UsageMinutes)?;
        transaction.clear(Table::UsageWindows)?;
        transaction.clear(Table::ProviderUsageMeters)?;
        transaction.clear(Table::StatisticsIndex)?;
        transaction.put(Table::Meta, "request_log_count", &0_i64)?;
        let generation = transaction
            .get::<i64>(Table::Meta, "statistics_generation")?
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                StorageError::Invalid("statistics generation is exhausted".to_owned())
            })?;
        transaction.put(Table::Meta, "statistics_generation", &generation)?;
        let now = db::utc_timestamp_now()?;
        transaction.put(
            Table::StatisticsState,
            "singleton",
            &Record::new()
                .with("collection_started_at", Field::Text(now.clone()))
                .with("dropped_events", Field::I64(0))
                .with("provider_usage_dropped_events", Field::I64(0))
                .with("updated_at", Field::Text(now)),
        )?;
        let current_minute = unix_minute();
        for (window_minutes, _) in STATISTICS_RANGES {
            transaction.put(
                Table::UsageExpiryCursor,
                &window_minutes.to_string(),
                &cursor_record(current_minute.saturating_sub(window_minutes), 4, ""),
            )?;
        }
        Ok(request_count)
    })
    .await
    .map_err(|error| format!("could not clear request history: {error}"))
}

pub(crate) async fn persist_drop_delta(
    db: &Database,
    counters: &DropCounters,
    drop_state_lock: &AsyncMutex<()>,
    persisted_local_drops: &mut u64,
) -> bool {
    let _drop_state_guard = drop_state_lock.lock().await;
    let observed = counters.total();
    let observed_provider_usage = counters.provider_usage();
    let provider_delta = observed_provider_usage.saturating_sub(
        counters
            .persisted_provider_usage_this_process
            .load(Ordering::Relaxed),
    );
    let delta = observed.saturating_sub(*persisted_local_drops);
    if delta == 0 && provider_delta == 0 {
        return true;
    }
    let delta = i64::try_from(delta).unwrap_or(i64::MAX);
    let provider_delta = i64::try_from(provider_delta).unwrap_or(i64::MAX);
    match db
        .write(move |transaction| {
            let mut state = transaction
                .get::<Record>(Table::StatisticsState, "singleton")?
                .ok_or_else(|| {
                    StorageError::Invalid("request statistics state is missing".to_owned())
                })?;
            let current = state.integer("dropped_events")?;
            if current < 0 {
                return Err(StorageError::Invalid(
                    "dropped event count is negative".to_owned(),
                ));
            }
            state.insert("dropped_events", Field::I64(current.saturating_add(delta)));
            let provider_current = state
                .optional_integer("provider_usage_dropped_events")?
                .unwrap_or(0);
            if provider_current < 0 {
                return Err(StorageError::Invalid(
                    "provider usage dropped event count is negative".to_owned(),
                ));
            }
            state.insert(
                "provider_usage_dropped_events",
                Field::I64(provider_current.saturating_add(provider_delta)),
            );
            state.insert("updated_at", Field::Text(db::utc_timestamp_now()?));
            transaction.put(Table::StatisticsState, "singleton", &state)
        })
        .await
    {
        Ok(()) => {
            *persisted_local_drops = observed;
            counters
                .persisted_this_process
                .store(observed, Ordering::Relaxed);
            counters
                .persisted_provider_usage_this_process
                .store(observed_provider_usage, Ordering::Relaxed);
            true
        }
        Err(error) => {
            tracing::warn!(%error, "could not persist telemetry drop count");
            false
        }
    }
}
