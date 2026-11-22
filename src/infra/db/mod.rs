//! Typed domain operations over the LMDB environment.
//!
//! This module stays a thin barrel: `connect` plus one submodule per concern.
//! Request-log repair/pruning lives in `request_logs`, each singleton
//! settings domain owns its load/save module, bootstrap owns first-open
//! defaults, `clock` owns timestamps, and `admin_auth` owns auth reads.

pub use crate::infra::lock::lock_database;
pub use crate::infra::settings_record::OperationalSettingsRecord;
use crate::infra::storage::{Database, StorageError, Table, WriteTxn};
use std::path::Path;

pub(crate) mod admin_auth;
pub(crate) mod bootstrap;
pub(crate) mod clock;
pub(crate) mod gateway_limits;
pub(crate) mod operational_settings;
pub(crate) mod output_styles_store;
pub(crate) mod request_logs;
#[cfg(test)]
mod tests;

pub(crate) mod keys;
mod settings_records;
#[allow(unused_imports)]
pub use keys::REQUEST_LOG_DESC_INDEX_PREFIX;
pub use keys::{
    REQUEST_LOG_INDEX_PREFIX, provider_api_key_created_prefix, provider_api_key_identity_index_key,
    provider_api_key_identity_index_prefix, provider_api_key_index_key,
    provider_api_key_index_prefix, provider_api_key_order_key, provider_model_index_key,
    provider_model_index_prefix, provider_model_key, provider_model_prefix,
    request_log_desc_index_key, request_log_index_key,
};
pub(crate) use settings_records::{
    gateway_limits_from_record, operational_settings_from_record, output_styles_from_record,
    output_styles_record,
};

pub use admin_auth::{admin_password_change_required, has_admin_auth};
pub use clock::{
    format_utc_timestamp, parse_utc_timestamp, utc_timestamp_before, utc_timestamp_now,
};
pub use gateway_limits::{load_gateway_resource_limits, save_gateway_resource_limits};
pub use operational_settings::{load_operational_settings, save_operational_settings};
pub use output_styles_store::{load_output_styles, save_output_styles};
pub use request_logs::prune_request_logs_batch;

const REQUEST_LOG_INDEX_FORMAT_KEY: &str = "request_log_index_format";
const REQUEST_LOG_INDEX_FORMAT_VERSION: u32 = 1;
const SINGLETON_KEY: &str = "singleton";
const STATISTICS_WINDOWS: [i64; 3] = [24 * 60, 7 * 24 * 60, 30 * 24 * 60];

/// `Meta` key holding the sum of `retained_bytes` over every live
/// `StreamRuns` row. Kept as a counter so a chunk append never has to scan
/// the run table to enforce the global replay budget. Startup recovery
/// recomputes it from the primary rows.
pub(crate) const STREAM_RETAINED_BYTES_KEY: &str = "stream_continuity_retained_bytes";

/// Reads the retained-byte counter. An absent or malformed counter defaults
/// to zero: it is a derived value that startup recovery always recomputes.
pub(crate) fn stream_retained_bytes(transaction: &WriteTxn<'_, '_>) -> Result<i64, StorageError> {
    read_writable_stream_retained_bytes(transaction)
}

/// Adds `delta` to the retained-byte counter inside the caller's transaction.
/// Saturates instead of failing so a corrupt counter cannot block journaling;
/// startup recovery restores the exact value.
pub(crate) fn add_stream_retained_bytes(
    transaction: &mut WriteTxn<'_, '_>,
    delta: i64,
) -> Result<i64, StorageError> {
    let current = read_writable_stream_retained_bytes(transaction)?;
    let next = current.saturating_add(delta.max(0)).max(0);
    transaction.put(Table::Meta, STREAM_RETAINED_BYTES_KEY, &next)?;
    Ok(next)
}

/// Subtracts `delta` from the retained-byte counter inside the caller's
/// transaction, clamping at zero.
pub(crate) fn deduct_stream_retained_bytes(
    transaction: &mut WriteTxn<'_, '_>,
    delta: i64,
) -> Result<i64, StorageError> {
    let current = read_writable_stream_retained_bytes(transaction)?;
    let next = current.saturating_sub(delta.max(0)).max(0);
    transaction.put(Table::Meta, STREAM_RETAINED_BYTES_KEY, &next)?;
    Ok(next)
}

/// Replaces the retained-byte counter with an exact recomputed value.
pub(crate) fn set_stream_retained_bytes(
    transaction: &mut WriteTxn<'_, '_>,
    value: i64,
) -> Result<(), StorageError> {
    transaction.put(Table::Meta, STREAM_RETAINED_BYTES_KEY, &value.max(0))
}

fn read_writable_stream_retained_bytes(
    transaction: &WriteTxn<'_, '_>,
) -> Result<i64, StorageError> {
    Ok(transaction
        .get::<i64>(Table::Meta, STREAM_RETAINED_BYTES_KEY)?
        .unwrap_or(0)
        .max(0))
}

pub async fn connect(path: &Path) -> Result<Database, StorageError> {
    let database = Database::open(path).await?;
    // Load the persisted request-log compression dictionary before any step
    // below touches that table. A request-log frame written with the
    // dictionary cannot be decoded without it, and a frame that needs it is
    // rejected as a codec failure rather than silently mis-read, so loading it
    // second made the startup index repair fail with "Dictionary mismatch" and
    // abort startup on any environment that had already trained a dictionary.
    // Test connections skip this to keep the process-global dictionary out of
    // parallel test runs.
    crate::infra::storage::load_request_log_dictionary(&database).await;
    bootstrap::initialize_storage(&database).await?;
    request_logs::repair_request_log_indexes(&database).await?;
    bootstrap::recover_abandoned_stream_runs(&database).await?;
    Ok(database)
}

#[cfg(test)]
pub async fn connect_for_test(path: &Path) -> Result<Database, StorageError> {
    let database = Database::open_for_test(path, 32 * 1024 * 1024).await?;
    bootstrap::initialize_storage(&database).await?;
    request_logs::repair_request_log_indexes(&database).await?;
    bootstrap::recover_abandoned_stream_runs(&database).await?;
    Ok(database)
}
