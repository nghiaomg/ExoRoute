//! OpenAI-compatible cloud providers that differ only by adapter ID and base
//! URL shape.
//!
//! Each provider here speaks the same Chat Completions / Responses wire format
//! and authenticates with a bearer token, so they share one implementation
//! instead of duplicating a pass-through adapter per vendor. A provider that
//! needs its own endpoint shape, auth scheme, or body rewrite belongs in its
//! own directory module, not in this list.

use super::*;

/// One adapter instance per provider. `adapter_id` is the only varying state,
/// so a provider is added by declaring a constant and a static below.
pub(super) struct CompatAdapter {
    pub(super) adapter_id: &'static str,
}

impl ProviderAdapter for CompatAdapter {
    fn adapter_id(&self) -> &'static str {
        self.adapter_id
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let base_url = adapter_base_url_override.unwrap_or(base_url);
        let suffix = match protocol {
            Protocol::ChatCompletions => "chat/completions",
            Protocol::Responses => "responses",
            Protocol::Messages => "messages",
        };
        cloud_api::cloud_api_endpoint(base_url, suffix)
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api::cloud_api_endpoint(base_url, "models")
    }

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        _base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        _api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if !preset.supported_auth_types.contains(&auth_type) {
            return Err("authentication type is not supported by this provider");
        }
        if supported_protocols.is_empty()
            || !supported_protocols.iter().any(|p| p == preferred_protocol)
            || supported_protocols
                .iter()
                .any(|p| !matches!(p.as_str(), "chat_completions" | "responses" | "messages"))
        {
            return Err("protocol is not supported by this provider");
        }
        Ok(())
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

pub const MOONSHOT_ADAPTER_ID: &str = "moonshot";
pub const ZHIPU_ADAPTER_ID: &str = "zhipu";
pub const MINIMAX_ADAPTER_ID: &str = "minimax";
pub const SAMBANOVA_ADAPTER_ID: &str = "sambanova";
pub const HYPERBOLIC_ADAPTER_ID: &str = "hyperbolic";
pub const DEEPINFRA_ADAPTER_ID: &str = "deepinfra";
pub const NOVITA_ADAPTER_ID: &str = "novita";
pub const GITHUB_MODELS_ADAPTER_ID: &str = "github_models";

pub(super) static MOONSHOT_ADAPTER: CompatAdapter = CompatAdapter {
    adapter_id: MOONSHOT_ADAPTER_ID,
};
pub(super) static ZHIPU_ADAPTER: CompatAdapter = CompatAdapter {
    adapter_id: ZHIPU_ADAPTER_ID,
};
pub(super) static MINIMAX_ADAPTER: CompatAdapter = CompatAdapter {
    adapter_id: MINIMAX_ADAPTER_ID,
};
pub(super) static SAMBANOVA_ADAPTER: CompatAdapter = CompatAdapter {
    adapter_id: SAMBANOVA_ADAPTER_ID,
};
pub(super) static HYPERBOLIC_ADAPTER: CompatAdapter = CompatAdapter {
    adapter_id: HYPERBOLIC_ADAPTER_ID,
};
pub(super) static DEEPINFRA_ADAPTER: CompatAdapter = CompatAdapter {
    adapter_id: DEEPINFRA_ADAPTER_ID,
};
pub(super) static NOVITA_ADAPTER: CompatAdapter = CompatAdapter {
    adapter_id: NOVITA_ADAPTER_ID,
};
pub(super) static GITHUB_MODELS_ADAPTER: CompatAdapter = CompatAdapter {
    adapter_id: GITHUB_MODELS_ADAPTER_ID,
};

#[cfg(test)]
mod tests;
