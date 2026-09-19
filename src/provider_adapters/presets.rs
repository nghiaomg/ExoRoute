use super::{AdapterCapabilities, ProviderCategory, ProviderLabel, ProviderPreset};

static CUSTOM_PROVIDER_PRESET: ProviderPreset = ProviderPreset {
    id: "custom",
    adapter_id: super::GENERIC_ADAPTER_ID,
    name: "Custom provider",
    description: "Connect an OpenAI-compatible API using an API key or custom header.",
    category: ProviderCategory::Custom,
    labels: &[],
    default_base_url: None,
    default_logo_url: None,
    default_model_prefix: None,
    default_models: &[],
    default_auth_type: "bearer",
    supported_auth_types: &["none", "bearer", "header"],
    default_preferred_protocol: "chat_completions",
    default_supported_protocols: &["chat_completions", "responses", "messages"],
    protocol_selectable: true,
    capabilities: AdapterCapabilities {
        api_keys: true,
        oauth_accounts: false,
        local_usage_meter: false,
        local_quota_tracking: false,
        model_discovery: true,
        usage_limits: false,
        api_key_usage: false,
        api_key_usage_status: "unsupported",
        model_protocol_routing: false,
        supported_upstream_protocols: &["chat_completions", "responses", "messages"],
        api_key_auth_assist: false,
        model_catalog_authoritative: true,
        event_stream_response: false,
        auth_panel: None,
    },
};

include!(concat!(env!("OUT_DIR"), "/provider_presets.rs"));

pub(super) fn all_presets() -> impl Iterator<Item = &'static ProviderPreset> {
    std::iter::once(&CUSTOM_PROVIDER_PRESET).chain(YAML_PROVIDER_PRESETS.iter())
}

pub fn preset_for_adapter(adapter_id: &str) -> Option<&'static ProviderPreset> {
    // Provider rows persist the adapter ID, not the selected preset ID. Presets that share an
    // adapter therefore share its capabilities and accepted authentication modes.
    all_presets().find(|preset| preset.adapter_id == adapter_id)
}

pub fn default_models(adapter_id: &str) -> &'static [&'static str] {
    preset_for_adapter(adapter_id)
        .map(|preset| preset.default_models)
        .unwrap_or(&[])
}

pub fn presets() -> Vec<ProviderPreset> {
    all_presets().copied().collect()
}

pub fn capabilities(adapter_id: &str) -> Option<AdapterCapabilities> {
    preset_for_adapter(adapter_id).map(|preset| preset.capabilities)
}

pub fn supported_upstream_protocols(adapter_id: &str) -> &'static [&'static str] {
    capabilities(adapter_id)
        .map(|capabilities| capabilities.supported_upstream_protocols)
        .unwrap_or(&[])
}

pub fn supports_upstream_protocol(adapter_id: &str, protocol: super::UpstreamProtocol) -> bool {
    super::adapter(adapter_id).is_some()
        && supported_upstream_protocols(adapter_id).contains(&protocol.as_str())
}
