//! Typed domain operations over the LMDB environment.
//!
//! This module stays a thin barrel: `connect` plus one submodule per concern.
//! Request-log repair/pruning lives in `request_logs`, each singleton
//! settings domain owns its load/save module, bootstrap owns first-open
//! defaults, `clock` owns timestamps, and `admin_auth` owns auth reads.

pub use crate::infra::lock::lock_database;
pub use crate::infra::settings_record::OperationalSettingsRecord;
use crate::infra::storage::{Database, StorageError};
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
pub use clock::{utc_timestamp_before, utc_timestamp_now};
pub use gateway_limits::{load_gateway_resource_limits, save_gateway_resource_limits};
pub use operational_settings::{load_operational_settings, save_operational_settings};
pub use output_styles_store::{load_output_styles, save_output_styles};
pub use request_logs::prune_request_logs_batch;

const REQUEST_LOG_INDEX_FORMAT_KEY: &str = "request_log_index_format";
const REQUEST_LOG_INDEX_FORMAT_VERSION: u32 = 1;
const SINGLETON_KEY: &str = "singleton";
const STATISTICS_WINDOWS: [i64; 3] = [24 * 60, 7 * 24 * 60, 30 * 24 * 60];

pub async fn connect(path: &Path) -> Result<Database, StorageError> {
    let database = Database::open(path).await?;
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
