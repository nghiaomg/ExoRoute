//! Request-log lifecycle: startup index repair and retention pruning.
//!
//! The time/descending secondary indexes are reconciled at startup so older
//! environments keep showing history, and pruned in bounded batches so the
//! retention worker can yield between batches.

use super::{REQUEST_LOG_INDEX_FORMAT_KEY, REQUEST_LOG_INDEX_FORMAT_VERSION};
use crate::infra::{
    db::keys::{
        REQUEST_LOG_DESC_INDEX_PREFIX, REQUEST_LOG_INDEX_PREFIX, request_log_desc_index_key,
        request_log_desc_index_key_from_time_key, request_log_index_key,
    },
    storage::{Database, Record, StorageError, Table},
};
use std::time::Duration;

pub(super) const REQUEST_LOG_INDEX_REPAIR_LIMIT: usize = 100_000;
pub(super) const REQUEST_LOG_PRUNE_BATCH_SIZE: usize = 1_000;

#[cfg(test)]
pub(super) const REQUEST_LOG_INDEX_FORMAT_KEY_FOR_TEST: &str = REQUEST_LOG_INDEX_FORMAT_KEY;
#[cfg(test)]
pub(super) const REQUEST_LOG_INDEX_FORMAT_VERSION_FOR_TEST: u32 = REQUEST_LOG_INDEX_FORMAT_VERSION;

/// Request history is ordered through secondary indexes. Older LMDB
/// environments may contain request records written before those indexes were
/// introduced, which makes the Requests page appear empty even though the
/// records are present. Reconcile the bounded index set during startup so a
/// stale "already repaired" marker cannot hide missing request history.
pub(super) async fn repair_request_log_indexes(database: &Database) -> Result<(), StorageError> {
    let needs_repair = database
        .read(|transaction| {
            let repaired = transaction.get::<u32>(Table::Meta, REQUEST_LOG_INDEX_FORMAT_KEY)?;
            let records = transaction.scan_prefix::<Record>(
                Table::RequestLogs,
                "",
                REQUEST_LOG_INDEX_REPAIR_LIMIT.saturating_add(1),
            )?;
            if records.len() > REQUEST_LOG_INDEX_REPAIR_LIMIT {
                return Err(StorageError::Invalid(
                    "request log table exceeds the bounded startup repair limit".to_owned(),
                ));
            }
            let request_count = transaction
                .get::<i64>(Table::Meta, "request_log_count")?
                .ok_or_else(|| {
                    StorageError::Invalid("request log counter is missing".to_owned())
                })?;
            if request_count < 0 {
                return Err(StorageError::Invalid(
                    "request log counter is invalid".to_owned(),
                ));
            }
            let expected_count = i64::try_from(records.len()).map_err(|_| {
                StorageError::Invalid("request log count is outside the supported range".to_owned())
            })?;
            if request_count != expected_count || repaired != Some(REQUEST_LOG_INDEX_FORMAT_VERSION)
            {
                return Ok(true);
            }

            let time_indexes = transaction.scan_prefix::<String>(
                Table::RequestLogIndex,
                REQUEST_LOG_INDEX_PREFIX,
                REQUEST_LOG_INDEX_REPAIR_LIMIT.saturating_add(1),
            )?;
            let descending_indexes = transaction.scan_prefix::<String>(
                Table::RequestLogIndex,
                REQUEST_LOG_DESC_INDEX_PREFIX,
                REQUEST_LOG_INDEX_REPAIR_LIMIT.saturating_add(1),
            )?;
            Ok(time_indexes.len() != records.len() || descending_indexes.len() != records.len())
        })
        .await?;
    if !needs_repair {
        return Ok(());
    }

    database
        .write(|transaction| {
            let time_indexes = transaction.scan_prefix::<String>(
                Table::RequestLogIndex,
                REQUEST_LOG_INDEX_PREFIX,
                REQUEST_LOG_INDEX_REPAIR_LIMIT.saturating_add(1),
            )?;
            let descending_indexes = transaction.scan_prefix::<String>(
                Table::RequestLogIndex,
                REQUEST_LOG_DESC_INDEX_PREFIX,
                REQUEST_LOG_INDEX_REPAIR_LIMIT.saturating_add(1),
            )?;
            if time_indexes.len() > REQUEST_LOG_INDEX_REPAIR_LIMIT
                || descending_indexes.len() > REQUEST_LOG_INDEX_REPAIR_LIMIT
            {
                return Err(StorageError::Invalid(
                    "request log index exceeds the bounded startup repair limit".to_owned(),
                ));
            }
            for (key, _) in time_indexes.iter().chain(descending_indexes.iter()) {
                transaction.delete(Table::RequestLogIndex, key)?;
            }

            let records = transaction.scan_prefix::<Record>(
                Table::RequestLogs,
                "",
                REQUEST_LOG_INDEX_REPAIR_LIMIT.saturating_add(1),
            )?;
            if records.len() > REQUEST_LOG_INDEX_REPAIR_LIMIT {
                return Err(StorageError::Invalid(
                    "request log table exceeds the bounded startup repair limit".to_owned(),
                ));
            }
            for (id, record) in &records {
                let created_at = record.text("created_at")?;
                let index_key = request_log_index_key(created_at, id)?;
                let descending_index_key = request_log_desc_index_key(created_at, id)?;
                transaction.put(Table::RequestLogIndex, &index_key, id)?;
                transaction.put(Table::RequestLogIndex, &descending_index_key, id)?;
            }
            let request_count = i64::try_from(records.len()).map_err(|_| {
                StorageError::Invalid("request log count is outside the supported range".to_owned())
            })?;
            transaction.put(Table::Meta, "request_log_count", &request_count)?;
            transaction.put(
                Table::Meta,
                REQUEST_LOG_INDEX_FORMAT_KEY,
                &REQUEST_LOG_INDEX_FORMAT_VERSION,
            )
        })
        .await
}

/// Remove at most one bounded batch from the oldest end of the time index.
/// The retention worker can yield and retry until this returns `false`.
pub async fn prune_request_logs_batch(
    database: &Database,
    retention_days: u32,
    max_rows: u32,
) -> Result<bool, String> {
    let retention_days = retention_days.clamp(1, 365);
    let max_rows = max_rows.clamp(1, 100_000);
    let cutoff =
        super::clock::utc_timestamp_before(Duration::from_secs(u64::from(retention_days) * 86_400))
            .map_err(|error| error.to_string())?;
    database
        .write(move |transaction| {
            let request_count = transaction
                .get::<i64>(Table::Meta, "request_log_count")?
                .ok_or_else(|| {
                    StorageError::Invalid("request log counter is missing".to_owned())
                })?;
            if request_count < 0 {
                return Err(StorageError::Invalid(
                    "request log counter is invalid".to_owned(),
                ));
            }
            let overflow = usize::try_from(request_count.saturating_sub(i64::from(max_rows)))
                .unwrap_or(usize::MAX);
            let entries = transaction.scan_prefix::<String>(
                Table::RequestLogIndex,
                REQUEST_LOG_INDEX_PREFIX,
                REQUEST_LOG_PRUNE_BATCH_SIZE,
            )?;
            let mut deleted = 0_i64;
            let mut changed = false;
            for (index_key, id) in entries {
                let Some((created_at, _)) = index_key
                    .strip_prefix(REQUEST_LOG_INDEX_PREFIX)
                    .and_then(|suffix| suffix.split_once('/'))
                else {
                    return Err(StorageError::Invalid(
                        "request log index entry is malformed".to_owned(),
                    ));
                };
                if created_at >= cutoff.as_str()
                    && usize::try_from(deleted).unwrap_or(usize::MAX) >= overflow
                {
                    break;
                }
                let record_removed = transaction.delete(Table::RequestLogs, &id)?;
                changed |= transaction.delete(Table::RequestLogIndex, &index_key)?;
                changed |= transaction.delete(
                    Table::RequestLogIndex,
                    &request_log_desc_index_key_from_time_key(&index_key, &id)?,
                )?;
                if record_removed {
                    deleted += 1;
                }
            }
            if deleted > 0 {
                transaction.put(
                    Table::Meta,
                    "request_log_count",
                    &request_count.saturating_sub(deleted),
                )?;
            }
            Ok(changed)
        })
        .await
        .map_err(|error| format!("could not prune request logs: {error}"))
}
