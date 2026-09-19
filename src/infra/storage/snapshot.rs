//! Snapshot inclusion rules and backup validation.
//!
//! Backup and restore share these bounds so a snapshot rejected on import
//! could never have been produced by export.

use crate::infra::storage::{
    Field, MAX_KEY_BYTES, MAX_RECORD_FIELD_BYTES, MAX_RECORD_FIELD_NAME_BYTES, MAX_RECORD_FIELDS,
    MAX_SNAPSHOT_ENTRIES, MAX_SNAPSHOT_RECORD_BYTES, Record, SnapshotEntry, StorageError, Table,
    codec, keys,
};

pub(super) fn excluded_from_snapshot(table: Table, include_request_logs: bool) -> bool {
    matches!(
        table,
        Table::AdminSessionFamilies
            | Table::AdminRefreshTokens
            | Table::SessionIndex
            | Table::StreamRuns
            | Table::StreamEvents
            | Table::StreamRunApiKeyIndex
    ) || (!include_request_logs
        && matches!(
            table,
            Table::RequestLogs
                | Table::RequestLogIndex
                | Table::UsageMinutes
                | Table::UsageWindows
                | Table::StatisticsState
                | Table::UsageExpiryCursor
                | Table::StatisticsIndex
                | Table::ProviderUsageMeters
        ))
}

fn validate_snapshot_value(table: Table, value: &[u8]) -> Result<(), StorageError> {
    match table {
        Table::Meta => {
            if bincode::deserialize::<i64>(value).is_err() {
                let _: u32 = codec::decode_record(value)?;
            }
        }
        Table::Indexes
        | Table::RequestLogIndex
        | Table::StatisticsIndex
        | Table::ApiKeyIndex
        | Table::ApiKeyTokenIndex
        | Table::ProviderNameIndex
        | Table::ProviderApiKeyIndex
        | Table::ProviderApiKeyAvailabilityIndex
        | Table::ProviderApiKeyCreatedIndex
        | Table::ProviderModelIndex
        | Table::RouteNameIndex
        | Table::RouteTargetProviderIndex => {
            let decoded: String = codec::decode_record(value)?;
            if decoded.len() > MAX_KEY_BYTES {
                return Err(StorageError::Invalid(
                    "backup contains an oversized index value".to_owned(),
                ));
            }
        }
        _ => {
            let record: Record = codec::decode_record(value)?;
            if record.fields.len() > MAX_RECORD_FIELDS {
                return Err(StorageError::Invalid(
                    "backup contains a record with too many fields".to_owned(),
                ));
            }
            for (name, field) in &record.fields {
                if name.is_empty() || name.len() > MAX_RECORD_FIELD_NAME_BYTES {
                    return Err(StorageError::Invalid(
                        "backup contains a record with an invalid field name".to_owned(),
                    ));
                }
                let size = match field {
                    Field::Text(value) => value.len(),
                    Field::Bytes(value) => value.len(),
                    Field::Null | Field::Bool(_) | Field::I64(_) => 0,
                };
                if size > MAX_RECORD_FIELD_BYTES {
                    return Err(StorageError::Invalid(
                        "backup contains an oversized record field".to_owned(),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Validate a full snapshot before it is written or restored. Keeps the
/// backup path from persisting oversized records, unknown table indices, or
/// keys that would corrupt LMDB B-tree ordering.
pub fn validate_snapshot_entries(entries: &[SnapshotEntry]) -> Result<(), StorageError> {
    if entries.len() > MAX_SNAPSHOT_ENTRIES {
        return Err(StorageError::Invalid(
            "snapshot exceeds the maximum number of entries".to_owned(),
        ));
    }
    for entry in entries {
        if excluded_from_snapshot(entry.table, true) {
            return Err(StorageError::Invalid(format!(
                "snapshot contains runtime-only table '{}' entries",
                entry.table.name()
            )));
        }
        if entry.key.is_empty() || entry.key.len() > MAX_KEY_BYTES || entry.key.contains('\0') {
            return Err(StorageError::Invalid(
                "snapshot contains an invalid key".to_owned(),
            ));
        }
        if entry.value.len() > MAX_SNAPSHOT_RECORD_BYTES {
            return Err(StorageError::Invalid(
                "snapshot contains an oversized record".to_owned(),
            ));
        }
        validate_snapshot_value(entry.table, &entry.value)?;
        keys::validate_key(&entry.key)?;
    }
    Ok(())
}
