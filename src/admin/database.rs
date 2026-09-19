use super::*;
use crate::{
    config::GatewayResourceLimits,
    infra::db::OperationalSettingsRecord,
    infra::storage::{Database, Field, Record, SnapshotEntry, Table},
    support::output_styles::OutputStylesSnapshot,
};
#[cfg(test)]
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

mod backup_format;
mod export;
mod import;
mod preflight;
mod restore;
mod temp_dir;
mod validate;
#[cfg(test)]
pub(super) use backup_format::{decode_backup, encode_backup};
pub(super) use backup_format::{decode_backup_file, write_backup_archive};
pub(crate) use export::export_database;
pub(crate) use import::import_database;
pub(super) use preflight::preflight_backup_reader;
#[cfg(test)]
pub(super) use restore::restore_application_data;
pub(super) use restore::spawn_restore_reconciliation_task;
pub(crate) use temp_dir::{TempDirectory, cleanup_stale_temp_directories, create_private_temp_dir};
#[cfg(test)]
pub(super) use validate::route_target_key_components;
pub(super) use validate::validate_backup_payload;

const MAX_DATABASE_IMPORT_BYTES: u64 = 512 * 1024 * 1024;
const BACKUP_FORMAT_VERSION: u32 = 1;
const BACKUP_MAGIC: &[u8] = b"EXOROUTE-LMDB-BACKUP\0";
const BACKUP_CHECKSUM_BYTES: usize = 32;
const MAX_PROVIDER_KEYS_PER_BACKUP_RECORD: usize = 8 * 1024;

#[derive(Serialize, Deserialize)]
pub(super) struct BackupPayload {
    storage_format_version: u32,
    entries: Vec<SnapshotEntry>,
}

pub(super) struct RestoredRuntimeSettings {
    gateway_resource_limits: GatewayResourceLimits,
    operational_settings: OperationalSettingsRecord,
    output_styles: Option<OutputStylesSnapshot>,
}

enum BackupExportError {
    Normalize(&'static str),
    Write(&'static str),
}

#[derive(Deserialize)]
pub(super) struct DatabaseExportQuery {
    #[serde(default)]
    include_request_logs: bool,
}

/// Materialize defaults that were introduced after the first backup format so
/// an imported legacy record is upgraded before it is committed. This keeps a
/// subsequent export self-describing instead of relying on read-time defaults.
fn normalize_provider_defaults(entries: &mut [SnapshotEntry]) -> Result<(), &'static str> {
    for entry in entries
        .iter_mut()
        .filter(|entry| entry.table == Table::Providers)
    {
        let mut record: Record = bincode::deserialize(&entry.value)
            .map_err(|_| "A provider record in the backup is invalid.")?;
        let adapter_id = record
            .text("adapter_id")
            .map_err(|_| "A provider record in the backup is invalid.")?;
        let tracks_local_quota = provider_adapters::capabilities(adapter_id)
            .is_some_and(|capabilities| capabilities.local_quota_tracking);
        let mut changed = false;
        if tracks_local_quota
            && record
                .optional_integer("local_rpm_target")
                .map_err(|_| "A provider record in the backup is invalid.")?
                .is_none()
        {
            record.insert(
                "local_rpm_target",
                Field::I64(i64::from(super::providers::DEFAULT_LOCAL_RPM_TARGET)),
            );
            changed = true;
        }
        if record
            .optional_text("key_strategy")
            .map_err(|_| "A provider record in the backup is invalid.")?
            .is_none()
        {
            record.insert(
                "key_strategy",
                Field::Text(super::providers::DEFAULT_KEY_STRATEGY.to_owned()),
            );
            changed = true;
        }
        if changed {
            entry.value = bincode::serialize(&record)
                .map_err(|_| "The backup provider record could not be normalized.")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod provider_default_tests {
    use super::*;

    #[test]
    fn legacy_provider_records_are_upgraded_before_restore() {
        let legacy = Record::new()
            .with("id", Field::Text("legacy-nim".to_owned()))
            .with("adapter_id", Field::Text("nvidia_nim".to_owned()));
        let configured = Record::new()
            .with("id", Field::Text("configured-nim".to_owned()))
            .with("adapter_id", Field::Text("nvidia_nim".to_owned()))
            .with("local_rpm_target", Field::I64(75));
        let generic = Record::new()
            .with("id", Field::Text("generic".to_owned()))
            .with("adapter_id", Field::Text("generic".to_owned()))
            .with(
                "key_strategy",
                Field::Text(crate::admin::providers::KEY_STRATEGY_ROUND_ROBIN.to_owned()),
            );
        let mut entries = [
            ("legacy-nim", legacy),
            ("configured-nim", configured),
            ("generic", generic),
        ]
        .into_iter()
        .map(|(key, record)| SnapshotEntry {
            table: Table::Providers,
            key: key.to_owned(),
            value: bincode::serialize(&record).expect("serialize provider"),
        })
        .collect::<Vec<_>>();

        normalize_provider_defaults(&mut entries).expect("normalize provider defaults");

        let decoded = entries
            .iter()
            .map(|entry| {
                (
                    entry.key.as_str(),
                    bincode::deserialize::<Record>(&entry.value).expect("decode provider"),
                )
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(
            decoded
                .get("legacy-nim")
                .expect("legacy provider")
                .integer("local_rpm_target")
                .expect("default target"),
            i64::from(crate::admin::providers::DEFAULT_LOCAL_RPM_TARGET)
        );
        assert_eq!(
            decoded
                .get("configured-nim")
                .expect("configured provider")
                .integer("local_rpm_target")
                .expect("configured target"),
            75
        );
        assert_eq!(
            decoded
                .get("generic")
                .expect("generic provider")
                .optional_integer("local_rpm_target")
                .expect("optional target"),
            None
        );
        for key in ["legacy-nim", "configured-nim"] {
            assert_eq!(
                decoded
                    .get(key)
                    .expect("provider")
                    .text("key_strategy")
                    .expect("default key strategy"),
                crate::admin::providers::DEFAULT_KEY_STRATEGY
            );
        }
        assert_eq!(
            decoded
                .get("generic")
                .expect("generic provider")
                .text("key_strategy")
                .expect("stored key strategy"),
            crate::admin::providers::KEY_STRATEGY_ROUND_ROBIN
        );
    }
}

/// Validate the fixed-width outer bincode layout before serde can reserve a
/// Vec based on an attacker-controlled length. The backup format uses
/// `with_fixint_encoding()`: u32 enum/version tags and u64 string/vector
/// lengths, all little-endian.
#[cfg(test)]
fn preflight_backup_payload(bytes: &[u8]) -> Result<(), &'static str> {
    fn take_u32(bytes: &[u8], offset: &mut usize) -> Result<u32, &'static str> {
        let end = offset
            .checked_add(4)
            .ok_or("The backup payload length is invalid.")?;
        let value = bytes
            .get(*offset..end)
            .ok_or("The backup payload is truncated.")?;
        *offset = end;
        Ok(u32::from_le_bytes(
            value
                .try_into()
                .map_err(|_| "The backup payload is truncated.")?,
        ))
    }

    fn take_u64(bytes: &[u8], offset: &mut usize) -> Result<u64, &'static str> {
        let end = offset
            .checked_add(8)
            .ok_or("The backup payload length is invalid.")?;
        let value = bytes
            .get(*offset..end)
            .ok_or("The backup payload is truncated.")?;
        *offset = end;
        Ok(u64::from_le_bytes(
            value
                .try_into()
                .map_err(|_| "The backup payload is truncated.")?,
        ))
    }

    fn skip(bytes: &[u8], offset: &mut usize, length: u64) -> Result<(), &'static str> {
        let length = usize::try_from(length).map_err(|_| "The backup payload is too large.")?;
        let end = offset
            .checked_add(length)
            .ok_or("The backup payload length is invalid.")?;
        if end > bytes.len() {
            return Err("The backup payload is truncated.");
        }
        *offset = end;
        Ok(())
    }

    let mut offset = 0;
    let storage_version = take_u32(bytes, &mut offset)?;
    if storage_version == 0 || storage_version > crate::infra::storage::STORAGE_FORMAT_VERSION {
        return Err("This LMDB storage format version is not supported by this ExoRoute build.");
    }
    let entry_count = usize::try_from(take_u64(bytes, &mut offset)?)
        .map_err(|_| "The backup contains too many records.")?;
    if entry_count > crate::infra::storage::MAX_SNAPSHOT_ENTRIES {
        return Err("The backup contains too many records.");
    }
    for _ in 0..entry_count {
        let table = usize::try_from(take_u32(bytes, &mut offset)?)
            .map_err(|_| "The backup contains an unknown record type.")?;
        if table >= Table::ALL.len() {
            return Err("The backup contains an unknown record type.");
        }
        let key_len = take_u64(bytes, &mut offset)?;
        if key_len == 0 || key_len > 500 {
            return Err("The backup contains an invalid storage key.");
        }
        skip(bytes, &mut offset, key_len)?;
        let value_len = take_u64(bytes, &mut offset)?;
        if value_len > crate::infra::storage::MAX_SNAPSHOT_RECORD_BYTES as u64 {
            return Err("The backup contains a record larger than the supported limit.");
        }
        skip(bytes, &mut offset, value_len)?;
    }
    if offset != bytes.len() {
        return Err("The backup payload contains trailing data.");
    }
    Ok(())
}

fn rebuild_backup_secondary_indexes(entries: &mut Vec<SnapshotEntry>) -> Result<(), &'static str> {
    fn put_index(
        indexes: &mut Vec<SnapshotEntry>,
        table: Table,
        key: String,
        value: String,
    ) -> Result<(), &'static str> {
        let encoded = bincode::serialize(&value).map_err(|_| "The backup indexes are invalid.")?;
        indexes.push(SnapshotEntry {
            table,
            key,
            value: encoded,
        });
        Ok(())
    }

    let is_derived_index = |table| {
        matches!(
            table,
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
                | Table::RouteTargetProviderIndex
        )
    };
    entries.retain(|entry| !is_derived_index(entry.table));
    let mut indexes = Vec::new();

    for entry in entries.iter() {
        let record: Record = match entry.table {
            Table::Providers
            | Table::ProviderApiKeys
            | Table::ProviderModels
            | Table::Routes
            | Table::RouteTargets
            | Table::ApiKeys
            | Table::RequestLogs
            | Table::UsageWindows
            | Table::ProviderUsageMeters => bincode::deserialize(&entry.value)
                .map_err(|_| "The backup contains an invalid primary record.")?,
            _ => continue,
        };
        match entry.table {
            Table::Providers => {
                let id = record
                    .text("id")
                    .map_err(|_| "The backup contains an invalid provider record.")?;
                let name = record
                    .text("name")
                    .map_err(|_| "The backup contains an invalid provider record.")?;
                let adapter_id = record
                    .optional_text("adapter_id")
                    .map_err(|_| "The backup contains an invalid provider record.")?;
                let adapter_id = adapter_id.unwrap_or(provider_adapters::GENERIC_ADAPTER_ID);
                let prefix = super::providers::normalized_provider_model_prefix_for_adapter(
                    record.optional_text("model_prefix").ok().flatten(),
                    id,
                    adapter_id,
                )
                .map_err(|_| "The backup contains an invalid provider model prefix.")?;
                if super::providers::validate_provider_deletion_state(&record)
                    .map_err(|_| "The backup contains invalid provider deletion state.")?
                {
                    put_index(
                        &mut indexes,
                        Table::Indexes,
                        super::providers::provider_deletion_index_key(id).map_err(
                            |_| "The backup contains an invalid provider deletion index.",
                        )?,
                        id.to_owned(),
                    )?;
                }
                put_index(
                    &mut indexes,
                    Table::ProviderNameIndex,
                    super::providers::provider_name_index_key(name, id)
                        .map_err(|_| "The backup contains an invalid provider index.")?,
                    id.to_owned(),
                )?;
                put_index(
                    &mut indexes,
                    Table::Indexes,
                    super::providers::provider_prefix_index_key(&prefix)
                        .map_err(|_| "The backup contains an invalid provider index.")?,
                    id.to_owned(),
                )?;
            }
            Table::ProviderApiKeys => {
                let id = record
                    .text("id")
                    .map_err(|_| "The backup contains an invalid provider API key.")?;
                let (all_key, created_key, available_key) =
                    super::providers::provider_api_key_index_keys(&record)
                        .map_err(|_| "The backup contains an invalid provider API key index.")?;
                put_index(
                    &mut indexes,
                    Table::ProviderApiKeyIndex,
                    all_key,
                    id.to_owned(),
                )?;
                put_index(
                    &mut indexes,
                    Table::ProviderApiKeyCreatedIndex,
                    created_key,
                    id.to_owned(),
                )?;
                if record
                    .boolean("enabled")
                    .map_err(|_| "The backup contains an invalid provider API key.")?
                    && !record
                        .boolean("invalid")
                        .map_err(|_| "The backup contains an invalid provider API key.")?
                {
                    put_index(
                        &mut indexes,
                        Table::ProviderApiKeyAvailabilityIndex,
                        available_key,
                        id.to_owned(),
                    )?;
                }
                if let Some(identity_key) =
                    super::providers::provider_api_key_identity_index_key(&record)
                        .map_err(|_| "The backup contains an invalid provider API key index.")?
                {
                    put_index(
                        &mut indexes,
                        Table::ProviderApiKeyIndex,
                        identity_key,
                        id.to_owned(),
                    )?;
                }
            }
            Table::ProviderModels => {
                let provider_id = record
                    .text("provider_id")
                    .map_err(|_| "The backup contains an invalid provider model.")?;
                let model = record
                    .text("model")
                    .map_err(|_| "The backup contains an invalid provider model.")?;
                put_index(
                    &mut indexes,
                    Table::ProviderModelIndex,
                    crate::infra::db::provider_model_index_key(provider_id, model)
                        .map_err(|_| "The backup contains an invalid provider model index.")?,
                    model.to_owned(),
                )?;
            }
            Table::Routes => {
                let id = record
                    .text("id")
                    .map_err(|_| "The backup contains an invalid route.")?;
                let name = record
                    .text("name")
                    .map_err(|_| "The backup contains an invalid route.")?;
                put_index(
                    &mut indexes,
                    Table::RouteNameIndex,
                    super::combos::route_name_index_key(name, id)
                        .map_err(|_| "The backup contains an invalid route index.")?,
                    id.to_owned(),
                )?;
            }
            Table::RouteTargets => {
                let provider_id = record
                    .text("provider_id")
                    .map_err(|_| "The backup contains an invalid route target.")?;
                put_index(
                    &mut indexes,
                    Table::RouteTargetProviderIndex,
                    super::combos::route_target_provider_index_key(provider_id, &entry.key)
                        .map_err(|_| "The backup contains an invalid route target index.")?,
                    entry.key.clone(),
                )?;
            }
            Table::ApiKeys => {
                let id = record
                    .text("id")
                    .map_err(|_| "The backup contains an invalid gateway API key.")?;
                let created_at = record
                    .text("created_at")
                    .map_err(|_| "The backup contains an invalid gateway API key.")?;
                let token_hash = record
                    .bytes("token_hash")
                    .map_err(|_| "The backup contains an invalid gateway API key.")?;
                put_index(
                    &mut indexes,
                    Table::ApiKeyIndex,
                    super::api_keys::api_key_index_key(created_at, id)
                        .map_err(|_| "The backup contains an invalid gateway API key index.")?,
                    id.to_owned(),
                )?;
                put_index(
                    &mut indexes,
                    Table::ApiKeyTokenIndex,
                    super::api_keys::api_key_token_index_key(token_hash),
                    id.to_owned(),
                )?;
            }
            Table::RequestLogs => {
                let id = record
                    .text("id")
                    .map_err(|_| "The backup contains an invalid request log.")?;
                let created_at = record
                    .text("created_at")
                    .map_err(|_| "The backup contains an invalid request log.")?;
                put_index(
                    &mut indexes,
                    Table::RequestLogIndex,
                    crate::infra::db::request_log_index_key(created_at, id)
                        .map_err(|_| "The backup contains an invalid request log index.")?,
                    id.to_owned(),
                )?;
                put_index(
                    &mut indexes,
                    Table::RequestLogIndex,
                    crate::infra::db::request_log_desc_index_key(created_at, id)
                        .map_err(|_| "The backup contains an invalid request log index.")?,
                    id.to_owned(),
                )?;
            }
            Table::ProviderUsageMeters => {
                let (credential_id, minute_text) = entry
                    .key
                    .rsplit_once('/')
                    .ok_or("The backup contains an invalid provider usage meter key.")?;
                if credential_id.is_empty()
                    || credential_id.len() > 256
                    || credential_id.contains('/')
                    || minute_text.len() != 20
                {
                    return Err("The backup contains an invalid provider usage meter key.");
                }
                let minute = minute_text
                    .parse::<i64>()
                    .map_err(|_| "The backup contains an invalid provider usage meter key.")?;
                if minute < 0
                    || record.text("credential_id").map_err(
                        |_| "The backup contains an invalid provider usage meter record.",
                    )? != credential_id
                    || record.integer("minute").map_err(
                        |_| "The backup contains an invalid provider usage meter record.",
                    )? != minute
                {
                    return Err("The backup contains an invalid provider usage meter record.");
                }
                for field in [
                    "request_count",
                    "cost_micro_usd",
                    "cost_events",
                    "missing_cost_events",
                    "input_tokens",
                    "output_tokens",
                ] {
                    if record.integer(field).map_err(
                        |_| "The backup contains an invalid provider usage meter record.",
                    )? < 0
                    {
                        return Err("The backup contains an invalid provider usage meter record.");
                    }
                }
            }
            Table::UsageWindows => {
                let mut parts = entry.key.splitn(3, '/');
                let window_minutes = parts
                    .next()
                    .and_then(|value| value.parse::<i64>().ok())
                    .ok_or("The backup contains an invalid statistics record.")?;
                let dimension_kind = parts
                    .next()
                    .and_then(|value| value.parse::<i64>().ok())
                    .ok_or("The backup contains an invalid statistics record.")?;
                let dimension_id = parts
                    .next()
                    .ok_or("The backup contains an invalid statistics record.")?;
                let request_count = record
                    .integer("request_count")
                    .map_err(|_| "The backup contains an invalid statistics record.")?;
                if request_count < 0 {
                    return Err("The backup contains an invalid statistics record.");
                }
                if request_count > 0 {
                    put_index(
                        &mut indexes,
                        Table::StatisticsIndex,
                        crate::infra::telemetry::top_index_key(
                            window_minutes,
                            dimension_kind,
                            request_count,
                            dimension_id,
                        )
                        .map_err(|_| "The backup contains an invalid statistics index.")?,
                        dimension_id.to_owned(),
                    )?;
                }
            }
            _ => {}
        }
    }
    entries.extend(indexes);
    Ok(())
}

#[cfg(test)]
mod tests;
