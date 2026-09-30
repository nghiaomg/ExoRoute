//! The admin workspace chat relay: send one prepared inference request to a
//! provider and return the decoded non-streaming response body.
//!
//! This path reuses the provider adapters' request preparation, auth, and
//! bounded-response reading, but unlike the model probes it never touches
//! credential state: no failure marks, no usage timestamps, and no quota
//! meters are written. It is the transport behind the dashboard's
//! "chat is not saved to the database" contract.

use super::*;
use std::collections::BTreeMap;
use std::time::Duration;

pub struct AdapterWorkspaceChatRequest<'a> {
    pub state: &'a AppState,
    pub base_url: &'a str,
    pub provider_id: &'a str,
    pub model: &'a str,
    pub auth_type: &'a str,
    pub auth_header: Option<&'a str>,
    pub custom_headers: &'a BTreeMap<String, String>,
    pub protocol: Protocol,
    pub secret: Option<&'a str>,
    pub oauth_auth: Option<&'a OAuthRequestAuth>,
    pub body: Value,
}

pub struct AdapterWorkspaceChatOutcome {
    pub(super) body: Value,
}

impl AdapterWorkspaceChatOutcome {
    /// The decoded upstream JSON body. Kept behind an accessor so callers can
    /// read the reply without mutating adapter output.
    pub fn body(&self) -> &Value {
        &self.body
    }
}

pub(super) async fn relay_workspace_inference_impl<A: ProviderAdapter + ?Sized>(
    adapter: &A,
    request: AdapterWorkspaceChatRequest<'_>,
) -> Result<AdapterWorkspaceChatOutcome, AdapterRequestError> {
    let endpoint = adapter
        .endpoint(request.base_url, request.protocol, None)
        .map_err(|message| AdapterRequestError::new(None, None, message))?;
    let upstream = request.state.operational_settings().settings.upstream;
    let (endpoint, client) = egress::provider_client(
        endpoint.as_str(),
        request.state.config.allow_private_provider_urls,
        request
            .state
            .config
            .connect_timeout
            .min(Duration::from_secs(3)),
        request
            .state
            .config
            .request_timeout
            .min(Duration::from_secs(30)),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        upstream,
    )
    .await
    .map_err(|error| {
        AdapterRequestError::new(None, None, format!("provider egress check failed: {error}"))
    })?;
    let mut provider_request = client
        .post(endpoint)
        .json(&request.body)
        .header("x-request-id", request.provider_id);
    provider_request = apply_custom_headers(provider_request, request.custom_headers)
        .map_err(|message| AdapterRequestError::new(None, None, message))?;
    if request.protocol == Protocol::Messages {
        provider_request = provider_request.header("anthropic-version", "2023-06-01");
    }
    let empty_client_headers = HeaderMap::new();
    provider_request = adapter.apply_client_headers(provider_request, &empty_client_headers);
    provider_request = adapter
        .apply_upstream_request_auth(
            provider_request,
            UpstreamAuthContext {
                auth_type: request.auth_type,
                auth_header: request.auth_header,
                secret: request.secret,
                oauth_auth: request.oauth_auth,
                session_id: "workspace-chat",
                protocol: request.protocol.into(),
            },
        )
        .map_err(|message| AdapterRequestError::new(None, None, message))?;
    let response = provider_request.send().await.map_err(|error| {
        AdapterRequestError::new(
            None,
            None,
            format!(
                "provider '{}' request {}",
                request.provider_id,
                upstream_transport_error_message(&error)
            ),
        )
    })?;
    let status = response.status();
    if !status.is_success() {
        let provider_response_body = read_limited_response(response, MAX_MODEL_TEST_RESPONSE_BYTES)
            .await
            .ok()
            .and_then(|body| model_test_provider_response_body(&body));
        // Mirror the gateway's sanitized provider error shape: the provider id
        // identifies which saved provider failed, and the sanitized upstream
        // body carries the provider's own diagnostic (never request secrets;
        // the sanitizer already redacts credential-bearing fields).
        let message = match provider_response_body.as_deref() {
            Some(detail) if !detail.is_empty() => format!(
                "provider '{}' returned HTTP {}: {detail}",
                request.provider_id,
                status.as_u16()
            ),
            _ => format!(
                "provider '{}' returned HTTP {}",
                request.provider_id,
                status.as_u16()
            ),
        };
        let mut error = AdapterRequestError::new(Some(status), None, message);
        error = error.with_provider_response_body(provider_response_body);
        return Err(error);
    }
    let body = read_limited_response(response, MAX_WORKSPACE_CHAT_RESPONSE_BYTES)
        .await
        .map_err(|error| {
            AdapterRequestError::new(
                Some(http::StatusCode::BAD_GATEWAY),
                None,
                format!("provider response could not be read: {error}"),
            )
        })?;
    let value = serde_json::from_slice::<Value>(&body).map_err(|_| {
        AdapterRequestError::new(
            Some(http::StatusCode::BAD_GATEWAY),
            None,
            format!(
                "provider '{}' model '{}' returned a non-JSON inference response",
                request.provider_id, request.model
            ),
        )
    })?;
    adapter
        .normalize_response(value.clone())
        .map_err(|message| {
            AdapterRequestError::new(Some(http::StatusCode::BAD_GATEWAY), None, message)
        })?;
    Ok(AdapterWorkspaceChatOutcome { body: value })
}

/// A chat reply can legitimately be large, so the relay reads up to 1 MiB of
/// the upstream body instead of the probe path's 256 KiB.
pub(super) const MAX_WORKSPACE_CHAT_RESPONSE_BYTES: usize = 1024 * 1024;
