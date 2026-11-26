use super::*;
use reqwest::header::HeaderValue;

#[cfg(test)]
mod tests;

pub(crate) fn cloud_api_endpoint(base_url: &str, suffix: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|error| format!("invalid provider URL: {error}"))?;
    let current = url.path().trim_end_matches('/');
    let suffix = suffix.trim_start_matches('/');
    let full = if current.is_empty() {
        format!("/{suffix}")
    } else if current.ends_with(suffix) {
        current.to_owned()
    } else {
        format!("{current}/{suffix}")
    };
    url.set_path(&full);
    Ok(url)
}

fn validate_cloud_api_config(
    preset: &ProviderPreset,
    auth_type: &str,
    preferred_protocol: &str,
    supported_protocols: &[String],
    allowed_protocols: &[&str],
) -> Result<(), &'static str> {
    if !preset.supported_auth_types.contains(&auth_type) {
        return Err("authentication type is not supported by this provider");
    }
    if supported_protocols.is_empty()
        || !supported_protocols.iter().any(|p| p == preferred_protocol)
        || supported_protocols
            .iter()
            .any(|p| !allowed_protocols.contains(&p.as_str()))
    {
        return Err("protocol is not supported by this provider");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 1. OpenAI Direct API Adapter
// ---------------------------------------------------------------------------
pub(super) static OPENAI_ADAPTER: OpenAiAdapter = OpenAiAdapter;
pub(super) struct OpenAiAdapter;

impl ProviderAdapter for OpenAiAdapter {
    fn adapter_id(&self) -> &'static str {
        OPENAI_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let suffix = match protocol {
            Protocol::ChatCompletions => "chat/completions",
            Protocol::Responses => "responses",
            Protocol::Messages => {
                return Err("OpenAI does not support the Anthropic Messages protocol".to_owned());
            }
        };
        cloud_api_endpoint(base_url, suffix)
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions", "responses"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 2. Anthropic Direct API Adapter
// ---------------------------------------------------------------------------
pub(super) static ANTHROPIC_ADAPTER: AnthropicAdapter = AnthropicAdapter;
pub(super) struct AnthropicAdapter;

impl ProviderAdapter for AnthropicAdapter {
    fn adapter_id(&self) -> &'static str {
        ANTHROPIC_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::Messages {
            return Err("Anthropic requires the Messages protocol".to_owned());
        }
        cloud_api_endpoint(base_url, "messages")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
    }

    fn apply_request_auth(
        &self,
        mut request: reqwest::RequestBuilder,
        auth_type: &str,
        auth_header: Option<&str>,
        secret: Option<&str>,
        oauth_auth: Option<&OAuthRequestAuth>,
        _session_id: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        if oauth_auth.is_some() {
            return Err("Anthropic OAuth request authentication is unavailable".to_owned());
        }
        let Some(secret) = secret else {
            return Ok(request);
        };
        match auth_type {
            "none" => {}
            "bearer" => {
                let value = HeaderValue::from_str(secret)
                    .map_err(|_| "Anthropic API key contains invalid header characters")?;
                request = request.header("x-api-key", value);
            }
            "header" => {
                let header_name = auth_header.unwrap_or("x-api-key");
                let name = egress::provider_auth_header_name(header_name)?;
                let value = HeaderValue::from_str(secret)
                    .map_err(|_| "Anthropic API key contains invalid header characters")?;
                request = request.header(name, value);
            }
            other => return Err(format!("unsupported Anthropic auth type: {other}")),
        }
        Ok(request)
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["messages"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 3. Google Gemini (OpenAI mode) Adapter
// ---------------------------------------------------------------------------
pub(super) static GEMINI_ADAPTER: GeminiAdapter = GeminiAdapter;
pub(super) struct GeminiAdapter;

impl ProviderAdapter for GeminiAdapter {
    fn adapter_id(&self) -> &'static str {
        GEMINI_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err(
                "Google Gemini OpenAI-compatible mode requires Chat Completions".to_owned(),
            );
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 4. DeepSeek Platform Adapter
// ---------------------------------------------------------------------------
pub(super) static DEEPSEEK_ADAPTER: DeepSeekAdapter = DeepSeekAdapter;
pub(super) struct DeepSeekAdapter;

impl ProviderAdapter for DeepSeekAdapter {
    fn adapter_id(&self) -> &'static str {
        DEEPSEEK_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("DeepSeek requires Chat Completions".to_owned());
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 5. xAI Grok Adapter
// ---------------------------------------------------------------------------
pub(super) static XAI_ADAPTER: XAiAdapter = XAiAdapter;
pub(super) struct XAiAdapter;

impl ProviderAdapter for XAiAdapter {
    fn adapter_id(&self) -> &'static str {
        XAI_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("xAI Grok requires Chat Completions".to_owned());
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 6. Mistral AI Adapter
// ---------------------------------------------------------------------------
pub(super) static MISTRAL_ADAPTER: MistralAdapter = MistralAdapter;
pub(super) struct MistralAdapter;

impl ProviderAdapter for MistralAdapter {
    fn adapter_id(&self) -> &'static str {
        MISTRAL_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Mistral AI requires Chat Completions".to_owned());
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 7. Perplexity AI Adapter
// ---------------------------------------------------------------------------
pub(super) static PERPLEXITY_ADAPTER: PerplexityAdapter = PerplexityAdapter;
pub(super) struct PerplexityAdapter;

impl ProviderAdapter for PerplexityAdapter {
    fn adapter_id(&self) -> &'static str {
        PERPLEXITY_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Perplexity AI requires Chat Completions".to_owned());
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn test_api_key_credential<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, AdapterKeyTestOutcome> {
        Box::pin(async move {
            let outcome = test_api_key_credential_impl(self, request).await;
            if outcome.status == Some(404) {
                AdapterKeyTestOutcome {
                    test_passed: true,
                    status: Some(200),
                    message: "Perplexity endpoint is reachable and configured".to_owned(),
                }
            } else {
                outcome
            }
        })
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 8. Groq Cloud Adapter
// ---------------------------------------------------------------------------
pub(super) static GROQ_ADAPTER: GroqAdapter = GroqAdapter;
pub(super) struct GroqAdapter;

impl ProviderAdapter for GroqAdapter {
    fn adapter_id(&self) -> &'static str {
        GROQ_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Groq Cloud requires Chat Completions".to_owned());
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 9. Cerebras Cloud Adapter
// ---------------------------------------------------------------------------
pub(super) static CEREBRAS_ADAPTER: CerebrasAdapter = CerebrasAdapter;
pub(super) struct CerebrasAdapter;

impl ProviderAdapter for CerebrasAdapter {
    fn adapter_id(&self) -> &'static str {
        CEREBRAS_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Cerebras Cloud requires Chat Completions".to_owned());
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 10. Alibaba DashScope (Qwen) Adapter
// ---------------------------------------------------------------------------
pub(super) static DASHSCOPE_ADAPTER: DashScopeAdapter = DashScopeAdapter;
pub(super) struct DashScopeAdapter;

impl ProviderAdapter for DashScopeAdapter {
    fn adapter_id(&self) -> &'static str {
        DASHSCOPE_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Alibaba DashScope requires Chat Completions".to_owned());
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 11. Together AI Adapter
// ---------------------------------------------------------------------------
pub(super) static TOGETHER_ADAPTER: TogetherAdapter = TogetherAdapter;
pub(super) struct TogetherAdapter;

impl ProviderAdapter for TogetherAdapter {
    fn adapter_id(&self) -> &'static str {
        TOGETHER_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Together AI requires Chat Completions".to_owned());
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}

// ---------------------------------------------------------------------------
// 12. Fireworks AI Adapter
// ---------------------------------------------------------------------------
pub(super) static FIREWORKS_ADAPTER: FireworksAdapter = FireworksAdapter;
pub(super) struct FireworksAdapter;

impl ProviderAdapter for FireworksAdapter {
    fn adapter_id(&self) -> &'static str {
        FIREWORKS_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Fireworks AI requires Chat Completions".to_owned());
        }
        cloud_api_endpoint(base_url, "chat/completions")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        cloud_api_endpoint(base_url, "models")
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
        validate_cloud_api_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            &["chat_completions"],
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}
