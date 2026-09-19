use super::*;

pub(super) struct KiloGatewayAdapter;

pub(super) static KILO_GATEWAY_ADAPTER: KiloGatewayAdapter = KiloGatewayAdapter;

impl ProviderAdapter for KiloGatewayAdapter {
    fn adapter_id(&self) -> &'static str {
        KILO_GATEWAY_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Kilo AI Gateway requires the Chat Completions protocol".to_owned());
        }
        kilo_gateway_url(base_url, "chat/completions")
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        kilo_gateway_url(base_url, "models")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn apply_client_headers(
        &self,
        mut request: reqwest::RequestBuilder,
        client_headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        if let Some(mode) = kilo_mode_header(client_headers) {
            request = request.header("x-kilocode-mode", mode);
        }
        request
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
            return Err("Kilo AI Gateway supports Bearer authentication or anonymous access");
        }
        if preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err("Kilo AI Gateway requires the Chat Completions protocol");
        }
        Ok(())
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }

    fn test_api_key_credential<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, AdapterKeyTestOutcome> {
        Box::pin(async move {
            let mut outcome = test_api_key_credential_impl(self, request).await;
            if outcome.test_passed {
                outcome.message = "Kilo model catalog is reachable. Its public endpoint does not validate API keys; Kilo checks the key on model requests.".to_owned();
            }
            outcome
        })
    }
}

fn kilo_gateway_url(base_url: &str, endpoint: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|error| format!("invalid Kilo Gateway URL: {error}"))?;
    let mut base_path = url.path().trim_end_matches('/');
    for suffix in ["/chat/completions", "/models"] {
        if let Some(stripped) = base_path.strip_suffix(suffix) {
            base_path = stripped;
            break;
        }
    }
    let path = if base_path.is_empty() {
        format!("/{endpoint}")
    } else {
        format!("{base_path}/{endpoint}")
    };
    url.set_path(&path);
    Ok(url)
}

fn kilo_mode_header(headers: &HeaderMap) -> Option<&str> {
    let mode = headers.get("x-kilocode-mode")?.to_str().ok()?.trim();
    (!mode.is_empty()
        && mode.len() <= 64
        && mode
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    .then_some(mode)
}
