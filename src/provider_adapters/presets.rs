use super::{AdapterCapabilities, ProviderCategory, ProviderLabel, ProviderPreset};
use serde::Serialize;

/// Owned mirror of [`AdapterCapabilities`] for runtime-loaded catalogs.
/// Field names and order intentionally match the static type so JSON responses
/// remain identical.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeAdapterCapabilities {
    pub api_keys: bool,
    pub oauth_accounts: bool,
    pub local_usage_meter: bool,
    pub local_quota_tracking: bool,
    pub model_discovery: bool,
    pub usage_limits: bool,
    pub api_key_usage: bool,
    pub api_key_usage_status: String,
    pub model_protocol_routing: bool,
    pub supported_upstream_protocols: Vec<String>,
    pub api_key_auth_assist: bool,
    pub model_catalog_authoritative: bool,
    pub event_stream_response: bool,
    pub auth_panel: Option<String>,
}

impl From<&AdapterCapabilities> for RuntimeAdapterCapabilities {
    fn from(value: &AdapterCapabilities) -> Self {
        Self {
            api_keys: value.api_keys,
            oauth_accounts: value.oauth_accounts,
            local_usage_meter: value.local_usage_meter,
            local_quota_tracking: value.local_quota_tracking,
            model_discovery: value.model_discovery,
            usage_limits: value.usage_limits,
            api_key_usage: value.api_key_usage,
            api_key_usage_status: value.api_key_usage_status.to_owned(),
            model_protocol_routing: value.model_protocol_routing,
            supported_upstream_protocols: value
                .supported_upstream_protocols
                .iter()
                .map(|entry| (*entry).to_owned())
                .collect(),
            api_key_auth_assist: value.api_key_auth_assist,
            model_catalog_authoritative: value.model_catalog_authoritative,
            event_stream_response: value.event_stream_response,
            auth_panel: value.auth_panel.map(str::to_owned),
        }
    }
}

/// Owned, runtime-mutable preset. Serialization matches [`ProviderPreset`]
/// byte-for-byte, including skipping `default_models` from the public admin API.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProviderPreset {
    pub id: String,
    pub adapter_id: String,
    pub name: String,
    pub description: String,
    pub category: ProviderCategory,
    pub labels: Vec<ProviderLabel>,
    pub default_base_url: Option<String>,
    pub default_logo_url: Option<String>,
    pub default_model_prefix: Option<String>,
    #[serde(skip_serializing)]
    pub default_models: Vec<String>,
    pub default_auth_type: String,
    pub supported_auth_types: Vec<String>,
    pub default_preferred_protocol: String,
    pub default_supported_protocols: Vec<String>,
    pub protocol_selectable: bool,
    pub capabilities: RuntimeAdapterCapabilities,
}

impl From<&'static ProviderPreset> for RuntimeProviderPreset {
    fn from(preset: &'static ProviderPreset) -> Self {
        let to_owned = |values: &[&str]| -> Vec<String> {
            values.iter().map(|entry| (*entry).to_owned()).collect()
        };
        Self {
            id: preset.id.to_owned(),
            adapter_id: preset.adapter_id.to_owned(),
            name: preset.name.to_owned(),
            description: preset.description.to_owned(),
            category: preset.category,
            labels: preset.labels.to_vec(),
            default_base_url: preset.default_base_url.map(str::to_owned),
            default_logo_url: preset.default_logo_url.map(str::to_owned),
            default_model_prefix: preset.default_model_prefix.map(str::to_owned),
            default_models: to_owned(preset.default_models),
            default_auth_type: preset.default_auth_type.to_owned(),
            supported_auth_types: to_owned(preset.supported_auth_types),
            default_preferred_protocol: preset.default_preferred_protocol.to_owned(),
            default_supported_protocols: to_owned(preset.default_supported_protocols),
            protocol_selectable: preset.protocol_selectable,
            capabilities: RuntimeAdapterCapabilities::from(&preset.capabilities),
        }
    }
}

/// Runtime-owned snapshot of all built-in presets, overridable by a user catalog.
#[derive(Debug, Clone)]
pub struct ProviderPresetCatalog {
    presets: Vec<RuntimeProviderPreset>,
}

impl ProviderPresetCatalog {
    pub fn built_in() -> Self {
        Self {
            presets: all_presets().map(RuntimeProviderPreset::from).collect(),
        }
    }

    pub fn all(&self) -> &[RuntimeProviderPreset] {
        &self.presets
    }

    #[cfg(test)]
    pub fn find(&self, id: &str) -> Option<&RuntimeProviderPreset> {
        self.presets.iter().find(|preset| preset.id == id)
    }

    #[cfg(test)]
    pub fn default_models(&self, id: &str) -> Vec<String> {
        self.find(id)
            .map(|preset| preset.default_models.clone())
            .unwrap_or_default()
    }

    pub fn with_model_overlay<F>(&self, merge_fn: F) -> Self
    where
        F: Fn(&str, &str, &[String]) -> Vec<String>,
    {
        Self {
            presets: self
                .presets
                .iter()
                .map(|preset| {
                    let mut overlaid = preset.clone();
                    overlaid.default_models =
                        merge_fn(&overlaid.id, &overlaid.adapter_id, &overlaid.default_models);
                    overlaid
                })
                .collect(),
        }
    }
}

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

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_preset_serializes_identically_to_static_preset() {
        for preset in all_presets() {
            let expected = serde_json::to_value(preset).expect("static preset JSON");
            let actual = serde_json::to_value(RuntimeProviderPreset::from(preset))
                .expect("runtime preset JSON");
            assert_eq!(actual, expected, "preset {} schema drift", preset.id);
        }
    }

    #[test]
    fn built_in_catalog_contains_opencode_presets() {
        let catalog = ProviderPresetCatalog::built_in();
        assert_eq!(catalog.all().len(), all_presets().count());
        assert!(catalog.find("opencode_go").is_some());
        assert!(catalog.find("opencode_zen").is_some());
        assert!(catalog.find("definitely-not-a-preset").is_none());
    }

    #[test]
    fn model_overlay_adds_and_removes_without_mutating_baseline() {
        let catalog = ProviderPresetCatalog::built_in();
        let removed = catalog
            .find("openai")
            .expect("openai preset")
            .default_models
            .first()
            .cloned()
            .expect("openai has default models");
        let overlaid = catalog.with_model_overlay(|id, _adapter, base| {
            if id == "opencode_go" || id == "opencode_zen" {
                let mut models = base.to_vec();
                models.push("user-added-model".to_owned());
                models
            } else if id == "openai" {
                base.iter()
                    .filter(|model| *model != &removed)
                    .map(|model| (*model).clone())
                    .collect()
            } else {
                base.to_vec()
            }
        });

        assert_eq!(
            overlaid
                .find("opencode_go")
                .expect("go preset")
                .default_models
                .last()
                .map(String::as_str),
            Some("user-added-model")
        );
        assert_eq!(
            overlaid
                .find("opencode_zen")
                .expect("zen preset")
                .default_models
                .last()
                .map(String::as_str),
            Some("user-added-model")
        );
        assert!(!overlaid.default_models("openai").contains(&removed));
        assert!(catalog.default_models("openai").contains(&removed));
        assert_eq!(catalog.default_models("opencode_go").len(), 0);
        assert_eq!(catalog.default_models("opencode_zen").len(), 0);
    }
}
