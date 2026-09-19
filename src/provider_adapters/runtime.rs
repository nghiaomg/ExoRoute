use super::{
    AdapterApiKeyRequest, AdapterKeyTestOutcome, AdapterModelTestOutcome, AdapterModelTestRequest,
    AdapterOAuthAccount, ApiKeyAuthCallback, CODEX_ADAPTER_ID, GENERIC_ADAPTER_ID,
    OAuthRequestAuth, ProviderAdapter, ProviderUsageSnapshot, UpstreamAuthContext, auth, opencode,
    presets, registry,
};
use crate::{protocol::UpstreamProtocol, state::AppState};
use http::StatusCode;
use serde_json::Value;

pub fn adapter(adapter_id: &str) -> Option<&'static dyn ProviderAdapter> {
    registry::adapter(adapter_id)
}

pub fn supports_upstream_protocol(adapter_id: &str, protocol: UpstreamProtocol) -> bool {
    presets::supports_upstream_protocol(adapter_id, protocol)
}

/// Central build/runtime contract for provider authentication panels.
#[allow(dead_code)]
pub fn known_auth_panels() -> &'static [&'static str] {
    auth::panels::KNOWN_PANELS.as_slice()
}

#[allow(dead_code)]
pub fn auth_panel_is_known(panel: &str) -> bool {
    auth::panels::is_known(panel)
}

pub fn model_upstream_protocol(adapter_id: &str, model: &str) -> Option<UpstreamProtocol> {
    let capabilities = presets::capabilities(adapter_id)?;
    if !capabilities.model_protocol_routing {
        return None;
    }
    opencode::model_protocol(adapter_id, model)
        .filter(|protocol| supports_upstream_protocol(adapter_id, *protocol))
}

pub fn upstream_endpoint(
    adapter_id: &str,
    base_url: &str,
    protocol: UpstreamProtocol,
    model: &str,
    streaming: bool,
    adapter_base_url_override: Option<&str>,
) -> Result<reqwest::Url, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    if !supports_upstream_protocol(adapter_id, protocol) {
        return Err("provider adapter does not support the selected upstream protocol".to_owned());
    }
    adapter.upstream_endpoint(
        base_url,
        protocol,
        model,
        streaming,
        adapter_base_url_override,
    )
}

pub fn default_auth_type(adapter_id: &str) -> Option<&'static str> {
    adapter(adapter_id)?;
    presets::preset_for_adapter(adapter_id).map(|preset| preset.default_auth_type)
}

pub fn legacy_adapter_id(auth_type: &str) -> &'static str {
    if auth_type == "codex_oauth" {
        CODEX_ADAPTER_ID
    } else {
        GENERIC_ADAPTER_ID
    }
}

pub fn validate_adapter_config(
    adapter_id: &str,
    auth_type: &str,
    base_url: &str,
    preferred_protocol: &str,
    supported_protocols: &[String],
    api_keys_present: bool,
) -> Result<(), &'static str> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered");
    };
    let Some(preset) = presets::preset_for_adapter(adapter_id) else {
        return Err("provider adapter is not registered");
    };
    adapter.validate_config(
        preset,
        auth_type,
        base_url,
        preferred_protocol,
        supported_protocols,
        api_keys_present,
    )
}

pub async fn resolve_oauth_request_auth(
    adapter_id: &str,
    state: &AppState,
    credential_id: &str,
) -> Result<Option<OAuthRequestAuth>, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    adapter
        .resolve_oauth_request_auth(state, credential_id)
        .await
}

pub async fn fetch_oauth_usage(
    adapter_id: &str,
    state: &AppState,
    credential_id: &str,
) -> Result<ProviderUsageSnapshot, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    if !presets::capabilities(adapter_id).is_some_and(|capabilities| capabilities.usage_limits) {
        return Err("provider adapter does not support usage limits".to_owned());
    }
    adapter.fetch_oauth_usage(state, credential_id).await
}

pub async fn fetch_api_key_usage(
    adapter_id: &str,
    state: &AppState,
    base_url: &str,
    credential: &str,
) -> Result<ProviderUsageSnapshot, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    if !presets::capabilities(adapter_id).is_some_and(|capabilities| capabilities.api_key_usage) {
        return Err("provider adapter does not support API-key usage limits".to_owned());
    }
    adapter
        .fetch_api_key_usage(state, base_url, credential)
        .await
}

pub fn supports_api_key_auth_assist(adapter_id: &str) -> bool {
    adapter(adapter_id).is_some()
        && presets::capabilities(adapter_id)
            .is_some_and(|capabilities| capabilities.api_key_auth_assist)
}

pub fn api_key_auth_assist_start_url(
    adapter_id: &str,
    callback_url: &str,
    state: &str,
) -> Result<String, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    if !supports_api_key_auth_assist(adapter_id) {
        return Err("provider adapter does not support API-key sign-in assistance".to_owned());
    }
    adapter.api_key_auth_assist_start_url(callback_url, state)
}

pub fn api_key_auth_assist_origin_registered(origin: &str) -> bool {
    registry::adapters().iter().any(|adapter| {
        supports_api_key_auth_assist(adapter.adapter_id())
            && adapter.api_key_auth_assist_origins().contains(&origin)
    })
}

pub fn api_key_auth_assist_origin_allowed(adapter_id: &str, origin: &str) -> bool {
    adapter(adapter_id).is_some_and(|adapter| {
        supports_api_key_auth_assist(adapter_id)
            && adapter.api_key_auth_assist_origins().contains(&origin)
    })
}

pub fn parse_api_key_auth_callback(
    adapter_id: &str,
    body: &[u8],
) -> Result<ApiKeyAuthCallback, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    if !supports_api_key_auth_assist(adapter_id) {
        return Err("provider adapter does not support API-key sign-in assistance".to_owned());
    }
    adapter.parse_api_key_auth_callback(body)
}

pub fn model_catalog_authoritative(adapter_id: &str) -> bool {
    presets::capabilities(adapter_id)
        .is_some_and(|capabilities| capabilities.model_catalog_authoritative)
}

pub fn apply_upstream_request_auth(
    adapter_id: &str,
    request: reqwest::RequestBuilder,
    auth: UpstreamAuthContext<'_>,
) -> Result<reqwest::RequestBuilder, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    if !supports_upstream_protocol(adapter_id, auth.protocol) {
        return Err("provider adapter does not support the selected upstream protocol".to_owned());
    }
    adapter.apply_upstream_request_auth(request, auth)
}

pub fn wants_event_stream(adapter_id: &str) -> bool {
    adapter(adapter_id).is_some_and(|adapter| adapter.wants_event_stream())
}

pub fn is_key_rejection_status(adapter_id: &str, status: StatusCode) -> bool {
    adapter(adapter_id).is_some_and(|adapter| adapter.is_key_rejection_status(status))
}

pub fn accepts_embedded_oauth_callback_without_state(adapter_id: &str, code: &str) -> bool {
    adapter(adapter_id)
        .is_some_and(|adapter| adapter.accepts_embedded_oauth_callback_without_state(code))
}

pub fn normalize_response(adapter_id: &str, value: Value) -> Result<Value, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    adapter.normalize_response(value)
}

pub fn is_oauth_only_adapter(adapter_id: &str) -> bool {
    presets::capabilities(adapter_id)
        .is_some_and(|capabilities| capabilities.oauth_accounts && !capabilities.api_keys)
}

pub fn has_public_model_catalog(adapter_id: &str) -> bool {
    adapter(adapter_id).is_some_and(|adapter| adapter.has_public_model_catalog())
}

pub async fn test_api_key_credential(
    adapter_id: &str,
    request: AdapterApiKeyRequest<'_>,
) -> AdapterKeyTestOutcome {
    let failed = |message: String| AdapterKeyTestOutcome {
        test_passed: false,
        status: None,
        message,
    };
    let Some(adapter) = adapter(adapter_id) else {
        return failed("provider adapter is not registered".to_owned());
    };
    adapter.test_api_key_credential(request).await
}

pub async fn test_api_key_model(
    adapter_id: &str,
    request: AdapterModelTestRequest<'_>,
) -> AdapterModelTestOutcome {
    let failed = |message: String| AdapterModelTestOutcome {
        test_passed: false,
        status: None,
        message,
        provider_response_body: None,
    };
    let Some(adapter) = adapter(adapter_id) else {
        return failed("provider adapter is not registered".to_owned());
    };
    adapter.test_api_key_model(request).await
}

pub fn authorization_url(
    adapter_id: &str,
    state: &str,
    challenge: &str,
    redirect_uri: Option<&str>,
) -> Result<String, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    adapter.authorization_url(state, challenge, redirect_uri)
}

pub fn supports_custom_oauth_redirect_uri(adapter_id: &str) -> bool {
    adapter(adapter_id).is_some_and(ProviderAdapter::supports_custom_oauth_redirect_uri)
}

pub fn pkce_challenge(adapter_id: &str, verifier: &str) -> Result<String, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    adapter.pkce_challenge(verifier)
}

pub async fn exchange_oauth_code(
    adapter_id: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    connect_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<AdapterOAuthAccount, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    adapter
        .exchange_oauth_code(
            code,
            verifier,
            redirect_uri,
            connect_timeout,
            request_timeout,
            upstream,
        )
        .await
}

pub async fn discover_oauth_models(
    adapter_id: &str,
    state: &AppState,
    credential_id: &str,
) -> Result<Vec<String>, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    if !presets::capabilities(adapter_id).is_some_and(|capabilities| capabilities.model_discovery) {
        return Err("provider adapter does not support OAuth model discovery".to_owned());
    }
    adapter.discover_oauth_models(state, credential_id).await
}

pub async fn test_oauth_model(
    adapter_id: &str,
    state: &AppState,
    base_url: &str,
    model: &str,
    credential_id: &str,
) -> AdapterModelTestOutcome {
    let failed = |status, message: String| AdapterModelTestOutcome {
        test_passed: false,
        status,
        message,
        provider_response_body: None,
    };
    let Some(adapter) = adapter(adapter_id) else {
        return failed(None, "provider adapter is not registered".to_owned());
    };
    if !adapter.requires_oauth_accounts() {
        return failed(
            None,
            "provider adapter does not support OAuth model tests".to_owned(),
        );
    }
    adapter
        .test_oauth_model(state, base_url, model, credential_id)
        .await
}

pub async fn save_oauth_account(
    adapter_id: &str,
    state: &AppState,
    provider_id: &str,
    account: AdapterOAuthAccount,
) -> Result<(), String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    adapter
        .save_oauth_account(state, provider_id, account)
        .await
}
