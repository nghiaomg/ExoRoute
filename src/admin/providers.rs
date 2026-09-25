use super::*;

pub(crate) mod budgets;
pub(crate) mod crud_create;
pub(crate) mod crud_update;
pub(crate) mod custom_headers;
pub(crate) mod deletion;
pub(crate) mod indexing;
pub(crate) mod keys;
pub(crate) mod lifecycle;
pub(crate) mod models_import;
pub(crate) mod models_list;
pub(crate) mod models_manual;
pub(crate) mod models_quota;
pub(crate) mod models_routing;
pub(crate) mod models_store;
pub(crate) mod overview;
pub(crate) mod probes;
pub(crate) mod provider_keys;
pub(crate) mod provider_view;
#[cfg(test)]
mod tests;
pub(crate) mod usage;
pub(crate) mod validation;

const MAX_PROVIDER_KEYS_PER_REQUEST: usize = 8;
const MAX_PROVIDER_KEY_BYTES: usize = 4096;
const MAX_PROVIDER_NAME_BYTES: usize = 256;
const MAX_PROVIDER_ID_BYTES: usize = 256;
const MAX_PROVIDER_URL_BYTES: usize = 2048;
const MAX_PROVIDER_LOGO_URL_BYTES: usize = 2048;
const MAX_PROVIDER_MODEL_PREFIX_BYTES: usize = 64;
const DEFAULT_PROVIDER_KEY_PAGE_SIZE: usize = 50;
const MAX_PROVIDER_KEY_PAGE_SIZE: usize = 50;
const DEFAULT_PROVIDER_MODEL_PAGE_SIZE: usize = 50;
const MAX_PROVIDER_MODEL_PAGE_SIZE: usize = 50;
const MAX_PROVIDER_MODELS: usize = 10_000;
const MAX_PROVIDER_MODEL_SEARCH_BYTES: usize = 256;
const MAX_PROVIDER_SCAN_ROWS: usize = 100_000;
const PROVIDER_DELETE_BATCH_SIZE: usize = 64;
const PROVIDER_DELETE_RECOVERY_INTERVAL: Duration = Duration::from_secs(60);
const PROVIDER_DELETE_RETRY_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const DEFAULT_LOCAL_RPM_TARGET: u32 = 40;
const MAX_PROVIDER_CUSTOM_HEADER_NAME_BYTES: usize = 128;
const MAX_PROVIDER_CUSTOM_HEADER_VALUE_BYTES: usize = 4096;
const MAX_PROVIDER_CUSTOM_HEADERS_BYTES: usize = 256 * 1024;

pub(crate) use budgets::update_provider_usage_budget;
pub(crate) use crud_create::create_provider;
pub(crate) use crud_update::{
    update_provider, update_provider_key_strategy, update_provider_thinking_settings,
};
pub(crate) use custom_headers::{stored_custom_headers, validate_custom_header_entries};
pub(crate) use deletion::{delete_provider, provider_deletion_recovery_worker};
pub(crate) use indexing::{
    ProviderDeletionPhase, ensure_provider_not_deleting, provider_deletion_index_key,
    provider_is_deleting, provider_model_prefix_value, provider_name_index_key,
    provider_prefix_index_key, validate_provider_deletion_state,
};
pub(crate) use keys::{
    add_provider_key_with_id, create_provider_key, delete_provider_key, list_provider_keys,
    swap_provider_key_order, update_provider_key,
};
pub(crate) use lifecycle::{ProviderRecordData, provider_record};
pub(crate) use models_import::import_provider_models;
pub(crate) use models_list::{list_provider_models, search_provider_models};
#[cfg(test)]
pub(crate) use models_list::{parse_provider_model_page, provider_models_url};
pub(crate) use models_manual::{add_manual_provider_model, delete_provider_model};
pub(crate) use models_quota::{provider_quota, update_provider_quota_target};
pub(crate) use models_routing::{list_provider_model_routing, update_provider_model_routing};
pub(crate) use overview::{
    list_providers, notify_enabled_provider_count_changed, refresh_enabled_provider_count,
    upstream_events,
};
pub(crate) use probes::test_provider_credential;
pub(crate) use provider_keys::{
    ProviderApiKeyRecordData, provider_api_key_identity_index_key, provider_api_key_index_keys,
    provider_api_key_order_position, provider_api_key_record, store_provider_api_key,
};
pub(crate) use provider_view::ProviderInput;
// The gateway reads the stored key strategy while decoding a provider record.
pub(crate) use usage::{
    provider_key_usage, provider_usage, refresh_provider_key_usage, refresh_provider_usage,
};
#[cfg(test)]
pub(crate) use validation::stored_thinking_settings;
pub(crate) use validation::{DEFAULT_KEY_STRATEGY, KEY_STRATEGY_ROUND_ROBIN, stored_key_strategy};
#[cfg(test)]
pub(crate) use validation::{
    normalized_provider_model_prefix, normalized_provider_model_prefix_for_adapter,
    validate_provider,
};
#[cfg(not(test))]
pub(crate) use validation::{
    normalized_provider_model_prefix_for_adapter, stored_thinking_settings,
};
