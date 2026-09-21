//! Provider boundary for upstream authentication, discovery, translation,
//! usage, and probing.
//!
//! Provider-specific implementations live in one directory per provider.
//! Shared registry, dispatch, protocol-independent probing, credentials, and
//! stream/image primitives remain at this level and must not be copied into a
//! provider directory.

use crate::{
    config::UpstreamSettings,
    protocol::{Protocol, UpstreamProtocol},
    security::egress,
    state::AppState,
};
use http::{HeaderMap, HeaderValue, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, future::Future, pin::Pin};

mod antigravity;
pub(crate) mod auth;
mod cline;
mod codex;
mod command_code;
mod dispatch;
mod freebuff;
mod generic;
mod images;
pub(crate) mod keys;
mod kilo;
mod model_test;
mod nvidia_nim;
mod opencode;
mod openrouter;
mod presets;
mod registry;
mod usage;

#[cfg(test)]
use crate::infra::storage::Table;
#[cfg(test)]
use crate::provider_adapters::auth::codex::{self as codex_oauth, CodexAccount};
#[cfg(test)]
pub(super) use codex::{
    CodexAccountMatchState, build_codex_model_request, parse_codex_model_list, save_codex_account,
};
#[cfg(test)]
use presets::all_presets;
pub(super) use registry::ClineAdapter;

pub(crate) use model_test::model_probe_body;
use model_test::{
    MAX_MODEL_TEST_RESPONSE_BYTES, model_test_non_sse_response, model_test_provider_response_body,
    test_api_key_credential_impl, test_api_key_model_impl,
};
pub use usage::{
    ProviderUsageCreditBalance, ProviderUsageQuota, ProviderUsageResetPeriod, ProviderUsageSnapshot,
};

#[cfg(test)]
use command_code::{
    fetch_command_code_usage, parse_command_code_usage, parse_rfc3339_epoch,
    test_command_code_api_key,
};

pub const GENERIC_ADAPTER_ID: &str = "generic";
pub const CODEX_ADAPTER_ID: &str = "openai_codex";
pub const KILO_GATEWAY_ADAPTER_ID: &str = "kilo_gateway";
pub const COMMAND_CODE_ADAPTER_ID: &str = "command_code";
pub const OPENCODE_GO_ADAPTER_ID: &str = "opencode_go";
pub const OPENCODE_ZEN_ADAPTER_ID: &str = "opencode_zen";
pub const OPENROUTER_ADAPTER_ID: &str = "openrouter";
pub const CLINE_ADAPTER_ID: &str = "cline";
pub const CLINEPASS_ADAPTER_ID: &str = "clinepass";
pub const NVIDIA_NIM_ADAPTER_ID: &str = "nvidia_nim";
pub const ANTIGRAVITY_ADAPTER_ID: &str = "antigravity";
pub const FREEBUFF_ADAPTER_ID: &str = "freebuff";
pub(super) const MAX_API_MODEL_PAGE_BYTES: usize = 4 * 1024 * 1024;
pub(super) const MAX_API_MODELS: usize = 10_000;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct AdapterCapabilities {
    pub api_keys: bool,
    pub oauth_accounts: bool,
    pub local_usage_meter: bool,
    pub local_quota_tracking: bool,
    pub model_discovery: bool,
    pub usage_limits: bool,
    pub api_key_usage: bool,
    pub api_key_usage_status: &'static str,
    pub model_protocol_routing: bool,
    pub supported_upstream_protocols: &'static [&'static str],
    pub api_key_auth_assist: bool,
    pub model_catalog_authoritative: bool,
    pub event_stream_response: bool,
    pub auth_panel: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCategory {
    Custom,
    // Reserved for a first-party provider preset; the catalog currently has none.
    #[allow(dead_code)]
    CloudApi,
    Gateway,
    Oauth,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderLabel {
    // Labels are opt-in metadata; no built-in preset is currently verified for these tags.
    #[allow(dead_code)]
    Free,
    #[allow(dead_code)]
    FreeTier,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ProviderPreset {
    pub id: &'static str,
    pub adapter_id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: ProviderCategory,
    pub labels: &'static [ProviderLabel],
    pub default_base_url: Option<&'static str>,
    pub default_logo_url: Option<&'static str>,
    pub default_model_prefix: Option<&'static str>,
    #[serde(skip_serializing)]
    pub default_models: &'static [&'static str],
    pub default_auth_type: &'static str,
    pub supported_auth_types: &'static [&'static str],
    pub default_preferred_protocol: &'static str,
    pub default_supported_protocols: &'static [&'static str],
    pub protocol_selectable: bool,
    pub capabilities: AdapterCapabilities,
}

pub enum ModelDiscoveryResult {
    Available {
        models: Vec<String>,
        truncated: bool,
    },
    Unsupported,
}

pub struct ProviderModelPage {
    pub models: Vec<String>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

type AdapterFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct AdapterOAuthAccount {
    pub payload: Value,
    pub display_name: String,
}

pub struct AdapterKeyTestOutcome {
    pub test_passed: bool,
    pub status: Option<u16>,
    pub message: String,
}

pub struct AdapterModelTestOutcome {
    pub test_passed: bool,
    pub status: Option<u16>,
    pub message: String,
    pub provider_response_body: Option<String>,
}

pub struct ApiKeyAuthCallback {
    pub api_key: String,
    pub state: String,
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    pub key_name: Option<String>,
}

pub struct AdapterModelTestRequest<'a> {
    pub state: &'a AppState,
    pub base_url: &'a str,
    pub provider_id: &'a str,
    pub model: &'a str,
    pub auth_type: &'a str,
    pub auth_header: Option<&'a str>,
    pub custom_headers: &'a BTreeMap<String, String>,
    pub protocol: Protocol,
    pub credentials: &'a [Option<String>],
}

pub struct AdapterApiKeyRequest<'a> {
    pub state: &'a AppState,
    pub base_url: &'a str,
    pub auth_type: &'a str,
    pub auth_header: Option<&'a str>,
    pub custom_headers: &'a BTreeMap<String, String>,
    pub preferred_protocol: &'a str,
    pub credential: &'a str,
}

#[derive(Clone, Debug)]
pub struct OAuthRequestAuth {
    pub access_token: String,
    pub account_id: Option<String>,
    pub project_id: Option<String>,
}

pub struct UpstreamAuthContext<'a> {
    pub auth_type: &'a str,
    pub auth_header: Option<&'a str>,
    pub secret: Option<&'a str>,
    pub oauth_auth: Option<&'a OAuthRequestAuth>,
    pub session_id: &'a str,
    pub protocol: UpstreamProtocol,
}

/// Adapter-specific work that must happen after request conversion but before
/// the upstream request body is serialized. The gateway owns the lifecycle and
/// applies the returned headers after caller headers so an adapter can protect
/// its per-request routing context.
#[derive(Default)]
pub struct AdapterRequestPreparation {
    pub(crate) headers: HeaderMap,
    pub(crate) finish_endpoint: Option<reqwest::Url>,
    pub(crate) finish_run_id: Option<String>,
}

impl AdapterRequestPreparation {
    pub(crate) fn new(
        headers: HeaderMap,
        finish_endpoint: Option<reqwest::Url>,
        finish_run_id: Option<String>,
    ) -> Self {
        Self {
            headers,
            finish_endpoint,
            finish_run_id,
        }
    }
}

#[derive(Debug)]
pub struct AdapterRequestError {
    pub(crate) status: Option<StatusCode>,
    pub(crate) retry_after: Option<HeaderValue>,
    pub(crate) message: String,
    pub(crate) provider_response_body: Option<String>,
}

impl AdapterRequestError {
    pub(crate) fn new(
        status: Option<StatusCode>,
        retry_after: Option<HeaderValue>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            status,
            retry_after,
            message: message.into(),
            provider_response_body: None,
        }
    }

    pub(crate) fn with_provider_response_body(mut self, body: Option<String>) -> Self {
        self.provider_response_body = body;
        self
    }
}

pub struct AdapterRequestContext<'a> {
    pub state: &'a AppState,
    pub base_url: &'a str,
    pub adapter_base_url_override: Option<&'a str>,
    pub model: &'a str,
    pub request_id: &'a str,
    pub streaming: bool,
    pub auth: UpstreamAuthContext<'a>,
}

pub trait ProviderAdapter: Sync {
    fn adapter_id(&self) -> &'static str;

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String>;

    fn upstream_endpoint(
        &self,
        base_url: &str,
        protocol: UpstreamProtocol,
        _model: &str,
        _streaming: bool,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let client_protocol = protocol.client_protocol().ok_or_else(|| {
            "provider adapter does not support the selected upstream protocol".to_owned()
        })?;
        self.endpoint(base_url, client_protocol, adapter_base_url_override)
    }

    fn prepare_body(&self, body: &mut Value, request_id: &str);

    fn normalize_response(&self, value: Value) -> Result<Value, String> {
        Ok(value)
    }

    fn apply_client_headers(
        &self,
        request: reqwest::RequestBuilder,
        _client_headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        request
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        provider_models_url(base_url)
    }

    fn has_public_model_catalog(&self) -> bool {
        false
    }

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        _base_url: &str,
        _preferred_protocol: &str,
        _supported_protocols: &[String],
        _api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if preset.supported_auth_types.contains(&auth_type) {
            Ok(())
        } else {
            Err("authentication type is not supported by this provider adapter")
        }
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
            return Err("provider adapter OAuth request authentication is unavailable".to_owned());
        }
        let Some(secret) = secret else {
            return Ok(request);
        };
        match auth_type {
            "none" => {}
            "bearer" => request = request.bearer_auth(secret),
            "header" => {
                let name = auth_header.ok_or("provider auth header is missing")?;
                let name = egress::provider_auth_header_name(name)?;
                let value = HeaderValue::from_str(secret)
                    .map_err(|_| "provider credential contains invalid header characters")?;
                request = request.header(name, value);
            }
            other => return Err(format!("unsupported provider auth type: {other}")),
        }
        Ok(request)
    }

    fn apply_upstream_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        auth: UpstreamAuthContext<'_>,
    ) -> Result<reqwest::RequestBuilder, String> {
        self.apply_request_auth(
            request,
            auth.auth_type,
            auth.auth_header,
            auth.secret,
            auth.oauth_auth,
            auth.session_id,
        )
    }

    fn wants_event_stream(&self) -> bool {
        capabilities(self.adapter_id())
            .is_some_and(|capabilities| capabilities.event_stream_response)
    }

    /// Some upstreams return a correctly framed event stream without the
    /// `Content-Type` header. Adapters may opt in only when their decoder
    /// validates the stream framing and terminal payload.
    fn allows_missing_event_stream_content_type(&self) -> bool {
        false
    }

    /// Some OAuth providers return a self-contained callback payload without
    /// echoing the generated OAuth state. Adapters must validate the payload
    /// before opting into the narrowly-scoped loopback fallback.
    fn accepts_embedded_oauth_callback_without_state(&self, _code: &str) -> bool {
        false
    }

    fn prepare_images<'a>(
        &'a self,
        _state: &'a AppState,
        _body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }

    fn prepare_upstream_request<'a>(
        &'a self,
        _context: AdapterRequestContext<'a>,
        _body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<AdapterRequestPreparation, AdapterRequestError>> {
        Box::pin(async { Ok(AdapterRequestPreparation::default()) })
    }

    fn retry_upstream_response_as_key_rejection(&self) -> bool {
        true
    }

    fn is_key_rejection_status(&self, status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
        )
    }

    fn finish_upstream_request<'a>(
        &'a self,
        _state: &'a AppState,
        _preparation: AdapterRequestPreparation,
        _auth: UpstreamAuthContext<'a>,
        _status: Option<StatusCode>,
    ) -> AdapterFuture<'a, ()> {
        Box::pin(async {})
    }

    fn read_event_stream<'a>(
        &'a self,
        _response: reqwest::Response,
        _max_bytes: usize,
    ) -> AdapterFuture<'a, Result<Value, AdapterSseError>> {
        Box::pin(async {
            Err(AdapterSseError::new(
                "provider adapter does not support event-stream response decoding",
                false,
            ))
        })
    }

    fn discover_api_key_models<'a>(
        &'a self,
        _request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(async { Ok(ModelDiscoveryResult::Unsupported) })
    }

    fn test_api_key_credential<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, AdapterKeyTestOutcome> {
        Box::pin(test_api_key_credential_impl(self, request))
    }

    fn test_api_key_model<'a>(
        &'a self,
        request: AdapterModelTestRequest<'a>,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(test_api_key_model_impl(self, request))
    }

    fn discover_oauth_models<'a>(
        &'a self,
        _state: &'a AppState,
        _credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async {
            Err("provider adapter does not support OAuth model discovery".to_owned())
        })
    }

    fn test_oauth_model<'a>(
        &'a self,
        _state: &'a AppState,
        _base_url: &'a str,
        _model: &'a str,
        _credential_id: &'a str,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(async {
            AdapterModelTestOutcome {
                test_passed: false,
                status: None,
                message: "provider adapter does not support OAuth model tests".to_owned(),
                provider_response_body: None,
            }
        })
    }

    fn authorization_url(
        &self,
        _state: &str,
        _challenge: &str,
        _redirect_uri: Option<&str>,
    ) -> Result<String, String> {
        Err("provider adapter does not support OAuth".to_owned())
    }

    fn supports_custom_oauth_redirect_uri(&self) -> bool {
        false
    }

    fn pkce_challenge(&self, _verifier: &str) -> Result<String, String> {
        Err("provider adapter does not support OAuth".to_owned())
    }

    fn exchange_oauth_code<'a>(
        &'a self,
        _code: &'a str,
        _verifier: &'a str,
        _redirect_uri: &'a str,
        _connect_timeout: std::time::Duration,
        _request_timeout: std::time::Duration,
        _upstream: UpstreamSettings,
    ) -> AdapterFuture<'a, Result<AdapterOAuthAccount, String>> {
        Box::pin(async { Err("provider adapter does not support OAuth".to_owned()) })
    }

    fn resolve_oauth_request_auth<'a>(
        &'a self,
        _state: &'a AppState,
        _credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Option<OAuthRequestAuth>, String>> {
        Box::pin(async { Ok(None) })
    }

    fn fetch_oauth_usage<'a>(
        &'a self,
        _state: &'a AppState,
        _credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<ProviderUsageSnapshot, String>> {
        Box::pin(async { Err("provider adapter does not support usage limits".to_owned()) })
    }

    fn fetch_api_key_usage<'a>(
        &'a self,
        _state: &'a AppState,
        _base_url: &'a str,
        _credential: &'a str,
    ) -> AdapterFuture<'a, Result<ProviderUsageSnapshot, String>> {
        Box::pin(async { Err("provider adapter does not support API-key usage limits".to_owned()) })
    }

    fn api_key_auth_assist_start_url(
        &self,
        _callback_url: &str,
        _state: &str,
    ) -> Result<String, String> {
        Err("provider adapter does not support API-key sign-in assistance".to_owned())
    }

    fn api_key_auth_assist_origins(&self) -> &'static [&'static str] {
        &[]
    }

    fn parse_api_key_auth_callback(&self, _body: &[u8]) -> Result<ApiKeyAuthCallback, String> {
        Err("provider adapter does not support API-key sign-in callbacks".to_owned())
    }

    fn save_oauth_account<'a>(
        &'a self,
        _state: &'a AppState,
        _provider_id: &'a str,
        _account: AdapterOAuthAccount,
    ) -> AdapterFuture<'a, Result<(), String>> {
        Box::pin(async { Err("provider adapter account storage is unavailable".to_owned()) })
    }

    fn requires_oauth_accounts(&self) -> bool {
        capabilities(self.adapter_id()).is_some_and(|capabilities| capabilities.oauth_accounts)
    }
}

pub use dispatch::{
    apply_custom_headers, finish_upstream_request, prepare_provider_images, prepare_request_body,
    prepare_upstream_request, retry_upstream_response_as_key_rejection,
};

mod discovery;
mod sse;
#[cfg(test)]
pub(crate) use discovery::parse_provider_model_page;
pub(crate) use discovery::{
    GenericModelDiscoveryRequest, discover_api_key_models, discover_generic_api_key_models,
    discover_generic_api_key_models_with_timeout, provider_models_url,
};
pub use presets::{
    capabilities, default_models, preset_for_adapter, presets, supported_upstream_protocols,
};
pub(crate) use sse::{
    AdapterSseError, ResponsesStreamAccumulator, accepts_event_stream_response,
    adapter_sse_error_can_fail_over, parse_adapter_event_stream, read_adapter_event_stream,
    read_codex_event_stream, read_limited_response, upstream_transport_error_message,
};
mod runtime;
pub use runtime::*;

#[cfg(test)]
mod tests;
