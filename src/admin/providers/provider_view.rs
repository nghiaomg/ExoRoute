use crate::provider_adapters;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub(super) struct ProviderView {
    pub(super) id: String,
    pub(super) adapter_id: String,
    pub(super) capabilities: Option<provider_adapters::AdapterCapabilities>,
    pub(super) name: String,
    pub(super) base_url: String,
    pub(super) logo_url: Option<String>,
    pub(super) model_prefix: String,
    pub(super) enabled: bool,
    pub(super) auth_type: String,
    pub(super) auth_header: Option<String>,
    pub(super) custom_headers: Vec<String>,
    pub(super) has_api_key: bool,
    pub(super) api_key_count: i64,
    pub(super) invalid_api_key_count: i64,
    pub(super) model_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) local_rpm_target: Option<u32>,
    pub(super) preferred_protocol: String,
    pub(super) supported_protocols: Vec<String>,
    pub(super) thinking_mode: String,
    pub(super) thinking_override: Option<String>,
    pub(super) key_strategy: String,
    pub(super) created_at: String,
    pub(super) updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ProviderInput {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub logo_url: Option<String>,
    #[serde(default)]
    pub model_prefix: Option<String>,
    #[serde(default)]
    pub adapter_id: Option<String>,
    #[serde(default = "super::validation::yes")]
    pub enabled: bool,
    #[serde(default = "super::validation::no_auth")]
    pub auth_type: String,
    pub auth_header: Option<String>,
    #[serde(default)]
    pub custom_headers: Option<Vec<super::custom_headers::ProviderCustomHeaderInput>>,
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_keys: Vec<String>,
    #[serde(default)]
    pub local_rpm_target: Option<u32>,
    #[serde(default = "super::validation::default_protocol")]
    pub preferred_protocol: String,
    #[serde(default = "super::validation::default_protocols")]
    pub supported_protocols: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderThinkingSettingsInput {
    pub mode: String,
    #[serde(default)]
    pub override_text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderKeyStrategyInput {
    pub(super) strategy: String,
}
