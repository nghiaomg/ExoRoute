use super::auth::antigravity as antigravity_oauth;
use super::*;
use crate::{protocol::UpstreamProtocol, state::AppState};
use http::StatusCode;

mod probes;

#[path = "tests.rs"]
#[cfg(test)]
mod tests;

use probes::{
    apply_tiered_thinking_config, normalize_model_id, parse_antigravity_event_stream,
    strip_enum_descriptions, test_antigravity_model,
};

const ANTIGRAVITY_MODEL_TEST_MAX_REQUEST_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);
const ANTIGRAVITY_MODEL_TEST_MAX_OUTPUT_TOKENS: u64 = 128;

pub(in crate::provider_adapters) struct AntigravityAdapter;

pub(in crate::provider_adapters) static ANTIGRAVITY_ADAPTER: AntigravityAdapter =
    AntigravityAdapter;

impl ProviderAdapter for AntigravityAdapter {
    fn adapter_id(&self) -> &'static str {
        ANTIGRAVITY_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        _protocol: Protocol,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        self.chat_endpoint(
            adapter_base_url_override.unwrap_or(base_url),
            adapter_base_url_override.is_some(),
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
        if protocol != UpstreamProtocol::GoogleGenerateContent {
            return Err(
                "Antigravity requires Google Generate Content upstream protocol".to_owned(),
            );
        }
        self.chat_endpoint(
            adapter_base_url_override.unwrap_or(base_url),
            adapter_base_url_override.is_some(),
        )
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if auth_type != preset.default_auth_type
            || base_url.trim_end_matches('/') != preset.default_base_url.unwrap_or_default()
            || preferred_protocol != preset.default_preferred_protocol
            || supported_protocols.len() != preset.default_supported_protocols.len()
            || !supported_protocols
                .iter()
                .zip(preset.default_supported_protocols)
                .all(|(actual, expected)| actual == expected)
            || api_keys_present
        {
            return Err(
                "Antigravity requires its fixed Cloud Code endpoint, Google Generate Content protocol, and OAuth accounts",
            );
        }
        Ok(())
    }

    fn apply_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        _auth_type: &str,
        _auth_header: Option<&str>,
        _secret: Option<&str>,
        oauth_auth: Option<&OAuthRequestAuth>,
        _session_id: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        let auth = oauth_auth.ok_or("Antigravity OAuth account is unavailable")?;
        Ok(request
            .bearer_auth(&auth.access_token)
            .header("User-Agent", antigravity_oauth::antigravity_user_agent())
            .header("Accept", "text/event-stream")
            .header("X-Client-Name", "antigravity")
            .header(
                "X-Client-Version",
                antigravity_oauth::antigravity_ide_version(),
            ))
    }

    fn apply_client_headers(
        &self,
        request: reqwest::RequestBuilder,
        _client_headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        // Prevent forwarding proxy tracing, Chromium/Electron headers (e.g. sec-ch-ua-*),
        // or SDK headers (e.g. x-stainless-*) to Google Cloud Code API to avoid fingerprint detection.
        request
    }

    fn wants_event_stream(&self) -> bool {
        true
    }

    fn allows_missing_event_stream_content_type(&self) -> bool {
        true
    }

    fn is_key_rejection_status(&self, status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::UNAUTHORIZED
                | StatusCode::FORBIDDEN
                | StatusCode::CONFLICT
                | StatusCode::TOO_MANY_REQUESTS
        )
    }

    fn prepare_upstream_request<'a>(
        &'a self,
        context: AdapterRequestContext<'a>,
        body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<AdapterRequestPreparation, AdapterRequestError>> {
        Box::pin(async move {
            let Some(auth) = context.auth.oauth_auth else {
                return Err(AdapterRequestError::new(
                    Some(StatusCode::UNAUTHORIZED),
                    None,
                    "Antigravity OAuth account is unavailable",
                ));
            };
            let Some(project_id) = auth
                .project_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            else {
                return Err(AdapterRequestError::new(
                    Some(StatusCode::UNPROCESSABLE_ENTITY),
                    None,
                    "Antigravity account has no Cloud Code project; reconnect the account after completing Gemini Code Assist onboarding",
                ));
            };
            let mut request = std::mem::take(body);
            let request_object = request.as_object_mut().ok_or_else(|| {
                AdapterRequestError::new(
                    Some(StatusCode::BAD_REQUEST),
                    None,
                    "Antigravity Google request is invalid",
                )
            })?;
            let formatted_session = format_antigravity_session_id(context.auth.session_id);
            request_object.insert("sessionId".to_owned(), Value::String(formatted_session));
            if let Some(tools) = request_object.get_mut("tools") {
                strip_enum_descriptions(tools);
            }
            let model = normalize_model_id(context.model);
            *body = json!({
                "project": project_id,
                "model": model,
                "userAgent": "antigravity",
                "requestType": "agent",
                "requestId": context.request_id,
                "request": request,
            });
            apply_tiered_thinking_config(&mut body["request"], context.model);
            Ok(AdapterRequestPreparation::new(HeaderMap::new(), None, None))
        })
    }

    fn read_event_stream<'a>(
        &'a self,
        response: reqwest::Response,
        max_bytes: usize,
    ) -> AdapterFuture<'a, Result<Value, AdapterSseError>> {
        Box::pin(async move {
            let bytes = read_limited_response(response, max_bytes)
                .await
                .map_err(|error| AdapterSseError::new(error, true))?;
            parse_antigravity_event_stream(&bytes)
        })
    }

    fn discover_oauth_models<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(antigravity_oauth::discover_models(state, credential_id))
    }

    fn test_oauth_model<'a>(
        &'a self,
        state: &'a AppState,
        base_url: &'a str,
        model: &'a str,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(test_antigravity_model(
            state,
            base_url,
            model,
            credential_id,
        ))
    }

    fn authorization_url(
        &self,
        state: &str,
        challenge: &str,
        redirect_uri: Option<&str>,
    ) -> Result<String, String> {
        antigravity_oauth::authorization_url(state, challenge, redirect_uri)
    }

    fn supports_custom_oauth_redirect_uri(&self) -> bool {
        true
    }

    fn pkce_challenge(&self, verifier: &str) -> Result<String, String> {
        Ok(antigravity_oauth::pkce_challenge(verifier))
    }

    fn exchange_oauth_code<'a>(
        &'a self,
        code: &'a str,
        verifier: &'a str,
        redirect_uri: &'a str,
        connect_timeout: std::time::Duration,
        request_timeout: std::time::Duration,
        upstream: crate::config::UpstreamSettings,
    ) -> AdapterFuture<'a, Result<AdapterOAuthAccount, String>> {
        Box::pin(async move {
            let account = antigravity_oauth::exchange_code(
                code,
                verifier,
                redirect_uri,
                connect_timeout,
                request_timeout,
                upstream,
            )
            .await?;
            let display_name = account.display_name();
            let payload = serde_json::to_value(account)
                .map_err(|_| "could not encode the Antigravity account credentials".to_owned())?;
            Ok(AdapterOAuthAccount {
                payload,
                display_name,
            })
        })
    }

    fn resolve_oauth_request_auth<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Option<OAuthRequestAuth>, String>> {
        Box::pin(async move {
            let account = antigravity_oauth::account_for_use(state, credential_id).await?;
            Ok(Some(OAuthRequestAuth {
                access_token: account.access_token,
                account_id: None,
                project_id: account.project_id,
            }))
        })
    }

    fn fetch_oauth_usage<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<ProviderUsageSnapshot, String>> {
        Box::pin(antigravity_oauth::fetch_usage(state, credential_id))
    }

    fn save_oauth_account<'a>(
        &'a self,
        state: &'a AppState,
        provider_id: &'a str,
        account: AdapterOAuthAccount,
    ) -> AdapterFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let account: antigravity_oauth::AntigravityAccount =
                serde_json::from_value(account.payload).map_err(|_| {
                    "Google returned invalid Antigravity account credentials".to_owned()
                })?;
            antigravity_oauth::save_account(state, provider_id, account).await
        })
    }
}

impl AntigravityAdapter {
    fn chat_endpoint(&self, base_url: &str, allow_override: bool) -> Result<reqwest::Url, String> {
        if !allow_override && base_url.trim_end_matches('/') != antigravity_oauth::RUNTIME_BASE_URL
        {
            return Err(
                "Antigravity endpoint must be https://daily-cloudcode-pa.googleapis.com".to_owned(),
            );
        }
        let mut url = reqwest::Url::parse(base_url)
            .map_err(|_| "Antigravity endpoint is invalid".to_owned())?;
        if url.scheme() != "https" {
            return Err("Antigravity endpoint must use HTTPS".to_owned());
        }
        url.set_path("/v1internal:streamGenerateContent");
        url.query_pairs_mut().append_pair("alt", "sse");
        Ok(url)
    }
}

/// Identifies whether an upstream Google Cloud Code 403 error represents a true
/// permanent account ban versus a recoverable/operator-actionable condition.
///
/// Mirrors OmniRoute/9Router: Google returns 403 for verification prompts and
/// project misconfigurations which must NOT permanently invalidate the account.
pub fn is_permanent_antigravity_ban(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    // "verify your account to continue" is an operator prompt, NOT a ban!
    if lower.contains("verify your account to continue") {
        return false;
    }
    // Recoverable project routing / API enablement errors are not account bans:
    if lower.contains("has not been used in project")
        || lower.contains("service_disabled")
        || lower.contains("accessnotconfigured")
        || lower.contains("permission_denied")
        || lower.contains("it is disabled")
    {
        return false;
    }
    // Real permanent ban signals from Google Cloud Code:
    lower.contains("this service has been disabled in this account")
        || lower.contains("account_deactivated")
        || lower.contains("account has been deactivated")
        || lower.contains("account has been disabled")
        || lower.contains("your account has been suspended")
        || lower.contains("this account is deactivated")
}

pub(super) fn format_antigravity_session_id(session_id: &str) -> String {
    let trimmed = session_id.trim();
    if trimmed.starts_with('-') && trimmed[1..].chars().all(|c| c.is_ascii_digit()) {
        return trimmed.to_owned();
    }
    // Native Antigravity clients use negative 64-bit integers for session IDs.
    let mut hash: i64 = -3750763034362895579;
    for byte in trimmed.as_bytes() {
        hash ^= i64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    if hash > 0 {
        hash = -hash;
    }
    hash.to_string()
}
