use super::*;

mod auth;
mod routing;
#[cfg(test)]
mod tests;
mod usage;

use auth::{
    apply_opencode_auth, apply_opencode_session_header, inject_opencode_go_reasoning_content,
    validate_opencode_config,
};
pub(super) use routing::model_protocol;
use routing::{google_generate_content_endpoint, open_code_aux_endpoint, open_code_endpoint};
use usage::fetch_opencode_go_usage;
#[cfg(test)]
use usage::parse_opencode_go_usage;

pub(super) struct OpenCodeGoAdapter;
pub(super) struct OpenCodeZenAdapter;

pub(super) static OPENCODE_GO_ADAPTER: OpenCodeGoAdapter = OpenCodeGoAdapter;
pub(super) static OPENCODE_ZEN_ADAPTER: OpenCodeZenAdapter = OpenCodeZenAdapter;

impl ProviderAdapter for OpenCodeGoAdapter {
    fn adapter_id(&self) -> &'static str {
        OPENCODE_GO_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        self.upstream_endpoint(
            adapter_base_url_override.unwrap_or(base_url),
            protocol.into(),
            "",
            false,
            None,
        )
    }

    fn upstream_endpoint(
        &self,
        base_url: &str,
        protocol: UpstreamProtocol,
        _model: &str,
        _streaming: bool,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        open_code_endpoint(adapter_base_url_override.unwrap_or(base_url), protocol)
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        open_code_aux_endpoint(base_url, "models")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn prepare_upstream_request<'a>(
        &'a self,
        context: AdapterRequestContext<'a>,
        body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<AdapterRequestPreparation, AdapterRequestError>> {
        Box::pin(async move {
            if context.auth.protocol == UpstreamProtocol::ChatCompletions {
                inject_opencode_go_reasoning_content(context.model, body);
            }
            Ok(AdapterRequestPreparation::default())
        })
    }

    fn apply_upstream_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        auth: UpstreamAuthContext<'_>,
    ) -> Result<reqwest::RequestBuilder, String> {
        apply_opencode_auth(request, auth, false)
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
        validate_opencode_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            false,
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }

    fn fetch_api_key_usage<'a>(
        &'a self,
        state: &'a AppState,
        base_url: &'a str,
        credential: &'a str,
    ) -> AdapterFuture<'a, Result<ProviderUsageSnapshot, String>> {
        Box::pin(fetch_opencode_go_usage(state, base_url, credential))
    }
}

impl ProviderAdapter for OpenCodeZenAdapter {
    fn adapter_id(&self) -> &'static str {
        OPENCODE_ZEN_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        self.upstream_endpoint(
            adapter_base_url_override.unwrap_or(base_url),
            protocol.into(),
            "",
            false,
            None,
        )
    }

    fn upstream_endpoint(
        &self,
        base_url: &str,
        protocol: UpstreamProtocol,
        model: &str,
        streaming: bool,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let base = adapter_base_url_override.unwrap_or(base_url);
        if protocol == UpstreamProtocol::GoogleGenerateContent {
            return google_generate_content_endpoint(base, model, streaming);
        }
        open_code_endpoint(base, protocol)
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        open_code_aux_endpoint(base_url, "models")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn apply_client_headers(
        &self,
        request: reqwest::RequestBuilder,
        client_headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        apply_opencode_session_header(request, client_headers)
    }

    fn apply_upstream_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        auth: UpstreamAuthContext<'_>,
    ) -> Result<reqwest::RequestBuilder, String> {
        apply_opencode_auth(request, auth, true)
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
        validate_opencode_config(
            preset,
            auth_type,
            preferred_protocol,
            supported_protocols,
            true,
        )
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }
}
