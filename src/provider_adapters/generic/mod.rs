use super::*;

pub(super) struct GenericProviderAdapter;

pub(super) static GENERIC_ADAPTER: GenericProviderAdapter = GenericProviderAdapter;

impl ProviderAdapter for GenericProviderAdapter {
    fn adapter_id(&self) -> &'static str {
        GENERIC_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let mut url = reqwest::Url::parse(base_url)
            .map_err(|error| format!("invalid provider URL: {error}"))?;
        let suffix = match protocol {
            Protocol::ChatCompletions => "chat/completions",
            Protocol::Responses => "responses",
            Protocol::Messages => "messages",
        };
        let current = url.path().trim_end_matches('/');
        let full = if current.ends_with(suffix) {
            current.to_owned()
        } else if current.ends_with("/v1") {
            format!("{current}/{suffix}")
        } else {
            format!("{current}/v1/{suffix}")
        };
        url.set_path(&full);
        Ok(url)
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}
