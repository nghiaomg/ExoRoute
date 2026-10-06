use super::*;

const NVIDIA_NIM_BASE_URL: &str = "https://integrate.api.nvidia.com/v1";
const NVIDIA_NIM_CHAT_COMPLETIONS_URL: &str =
    "https://integrate.api.nvidia.com/v1/chat/completions";
const NVIDIA_NIM_MODELS_URL: &str = "https://integrate.api.nvidia.com/v1/models";

pub(super) static NVIDIA_NIM_ADAPTER: NvidiaNimAdapter = NvidiaNimAdapter;

pub(super) struct NvidiaNimAdapter;

impl ProviderAdapter for NvidiaNimAdapter {
    fn adapter_id(&self) -> &'static str {
        NVIDIA_NIM_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("NVIDIA NIM hosted supports Chat Completions upstream".to_owned());
        }
        validate_base_url(base_url)?;
        reqwest::Url::parse(NVIDIA_NIM_CHAT_COMPLETIONS_URL)
            .map_err(|error| format!("invalid NVIDIA NIM URL: {error}"))
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        validate_base_url(base_url)?;
        reqwest::Url::parse(NVIDIA_NIM_MODELS_URL)
            .map_err(|error| format!("invalid NVIDIA NIM model-list URL: {error}"))
    }

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        _api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if auth_type != "bearer" || !preset.supported_auth_types.contains(&auth_type) {
            return Err("NVIDIA NIM hosted requires Bearer authentication");
        }
        if validate_base_url(base_url).is_err() {
            return Err("NVIDIA NIM hosted uses https://integrate.api.nvidia.com/v1");
        }
        if preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err("NVIDIA NIM hosted requires Chat Completions upstream");
        }
        Ok(())
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_nvidia_models(self, request))
    }
}

fn validate_base_url(base_url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(base_url.trim())
        .map_err(|_| "NVIDIA NIM hosted uses https://integrate.api.nvidia.com/v1".to_owned())?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("integrate.api.nvidia.com")
        || parsed.port().is_some_and(|port| port != 443)
        || !matches!(parsed.path(), "/v1" | "/v1/")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("NVIDIA NIM hosted uses https://integrate.api.nvidia.com/v1".to_owned());
    }
    reqwest::Url::parse(NVIDIA_NIM_BASE_URL)
        .map_err(|error| format!("invalid NVIDIA NIM base URL: {error}"))
}

async fn discover_nvidia_models(
    adapter: &NvidiaNimAdapter,
    request: AdapterApiKeyRequest<'_>,
) -> Result<ModelDiscoveryResult, String> {
    let endpoint = adapter.model_list_endpoint(request.base_url)?;
    let operational = request.state.operational_settings().settings;
    let (endpoint, client) = egress::provider_client(
        endpoint.as_str(),
        request.state.config.allow_private_provider_urls,
        request
            .state
            .config
            .connect_timeout
            .min(std::time::Duration::from_secs(3)),
        request
            .state
            .config
            .request_timeout
            .min(operational.upstream.discovery_request_timeout),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        operational.upstream,
    )
    .await?;
    let request_builder = crate::provider_adapters::apply_custom_headers(
        client.get(endpoint),
        request.custom_headers,
    )?;
    let request_builder = adapter.apply_request_auth(
        request_builder,
        request.auth_type,
        request.auth_header,
        Some(request.credential),
        None,
        "nvidia-nim-model-discovery",
    )?;
    let mut response = request_builder.send().await.map_err(|error| {
        format!(
            "could not reach NVIDIA NIM while importing models: {}",
            error.without_url()
        )
    })?;
    let status = response.status();
    if matches!(
        status,
        http::StatusCode::NOT_FOUND
            | http::StatusCode::METHOD_NOT_ALLOWED
            | http::StatusCode::NOT_IMPLEMENTED
    ) {
        return Ok(ModelDiscoveryResult::Unsupported);
    }
    if !status.is_success() {
        return Err(format!(
            "NVIDIA NIM returned HTTP {} while importing models",
            status.as_u16()
        ));
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        format!(
            "could not read NVIDIA NIM model list: {}",
            error.without_url()
        )
    })? {
        if body.len().saturating_add(chunk.len()) > MAX_API_MODEL_PAGE_BYTES {
            return Err("NVIDIA NIM model list exceeded the 4 MiB page limit".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "NVIDIA NIM returned an invalid JSON model list".to_owned())?;
    parse_nvidia_model_list(&value)
}

fn parse_nvidia_model_list(value: &Value) -> Result<ModelDiscoveryResult, String> {
    let entries = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.get("models").and_then(Value::as_array))
        .ok_or_else(|| "NVIDIA NIM model list did not contain a data or models array".to_owned())?;
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::with_capacity(entries.len().min(MAX_API_MODELS));
    let mut truncated = false;
    for entry in entries {
        let model = entry
            .as_str()
            .or_else(|| entry.get("id").and_then(Value::as_str))
            .ok_or_else(|| "NVIDIA NIM returned a model without a string ID".to_owned())?;
        let model = model.trim();
        if model.is_empty() || model.len() > 256 || model.chars().any(char::is_control) {
            return Err("NVIDIA NIM returned an invalid model ID".to_owned());
        }
        if seen.insert(model.to_owned()) {
            if models.len() == MAX_API_MODELS {
                truncated = true;
                break;
            }
            models.push(model.to_owned());
        }
    }
    Ok(ModelDiscoveryResult::Available { models, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosted_endpoint_is_fixed_to_catalog_chat_completions() {
        assert_eq!(
            NVIDIA_NIM_ADAPTER
                .endpoint(NVIDIA_NIM_BASE_URL, Protocol::ChatCompletions, None)
                .expect("NVIDIA NIM endpoint")
                .as_str(),
            NVIDIA_NIM_CHAT_COMPLETIONS_URL
        );
        assert!(
            NVIDIA_NIM_ADAPTER
                .endpoint(NVIDIA_NIM_BASE_URL, Protocol::Responses, None)
                .is_err()
        );
        assert!(
            NVIDIA_NIM_ADAPTER
                .endpoint("https://example.com/v1", Protocol::ChatCompletions, None)
                .is_err()
        );
    }

    #[test]
    fn hosted_config_requires_bearer_and_the_fixed_catalog_origin() {
        let preset = crate::provider_adapters::preset_for_adapter(NVIDIA_NIM_ADAPTER_ID)
            .expect("NVIDIA NIM preset");
        assert!(
            NVIDIA_NIM_ADAPTER
                .validate_config(
                    preset,
                    "bearer",
                    NVIDIA_NIM_BASE_URL,
                    "chat_completions",
                    &["chat_completions".to_owned()],
                    true,
                )
                .is_ok()
        );
        for (auth_type, base_url, protocol, supported) in [
            (
                "header",
                NVIDIA_NIM_BASE_URL,
                "chat_completions",
                vec!["chat_completions"],
            ),
            (
                "bearer",
                "https://integrate.api.nvidia.com/v2",
                "chat_completions",
                vec!["chat_completions"],
            ),
            (
                "bearer",
                NVIDIA_NIM_BASE_URL,
                "responses",
                vec!["responses"],
            ),
        ] {
            assert!(
                NVIDIA_NIM_ADAPTER
                    .validate_config(
                        preset,
                        auth_type,
                        base_url,
                        protocol,
                        &supported
                            .iter()
                            .map(|value| (*value).to_owned())
                            .collect::<Vec<_>>(),
                        true
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn hosted_auth_uses_a_bearer_header() {
        let client = reqwest::Client::new();
        let request = NVIDIA_NIM_ADAPTER
            .apply_request_auth(
                client.get(NVIDIA_NIM_MODELS_URL),
                "bearer",
                None,
                Some("test-api-key"),
                None,
                "credential-test",
            )
            .expect("bearer auth");
        let request = request.build().expect("build bearer request");
        assert_eq!(
            request
                .headers()
                .get("authorization")
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-api-key")
        );
    }

    #[test]
    fn model_list_accepts_data_and_models_shapes_and_deduplicates() {
        let data = json!({"data":[{"id":"meta/llama-3.1-70b-instruct"},{"id":"meta/llama-3.1-70b-instruct"}]});
        let ModelDiscoveryResult::Available { models, truncated } =
            parse_nvidia_model_list(&data).expect("valid data model list")
        else {
            panic!("model list should be available");
        };
        assert_eq!(models, ["meta/llama-3.1-70b-instruct"]);
        assert!(!truncated);

        let models_value = json!({"models":["nvidia/model-a"]});
        assert!(matches!(
            parse_nvidia_model_list(&models_value),
            Ok(ModelDiscoveryResult::Available { models, .. }) if models == ["nvidia/model-a"]
        ));
        assert!(matches!(
            parse_nvidia_model_list(&json!({"data": null, "models": ["nvidia/model-b"]})),
            Ok(ModelDiscoveryResult::Available { models, .. }) if models == ["nvidia/model-b"]
        ));
    }

    #[test]
    fn model_list_rejects_invalid_and_oversized_ids() {
        assert!(parse_nvidia_model_list(&json!({"data":[{"id":""}]})).is_err());
        assert!(parse_nvidia_model_list(&json!({"data":[{"id":"bad\nmodel"}]})).is_err());
        assert!(parse_nvidia_model_list(&json!({"data":[{"id":"x".repeat(257)}]})).is_err());
    }
}
