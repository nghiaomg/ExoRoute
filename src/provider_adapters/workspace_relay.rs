//! The admin workspace chat relay: send one prepared inference request to a
//! provider and return the decoded non-streaming response body.
//!
//! This path reuses the provider adapters' request preparation, auth, and
//! bounded-response reading, but unlike the model probes it never touches
//! credential state: no failure marks, no usage timestamps, and no quota
//! meters are written. It is the transport behind the dashboard's
//! "chat is not saved to the database" contract.

use super::*;
use crate::security::egress::ProxyEgress;
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
    /// The resolved upstream protocol. Endpoint selection, protocol-shaped
    /// headers, and adapter auth all branch on the upstream protocol exactly
    /// like the gateway's public request path, so upstream-only protocols
    /// (Google Generate Content) work here too.
    pub upstream_protocol: UpstreamProtocol,
    pub secret: Option<&'a str>,
    pub oauth_auth: Option<&'a OAuthRequestAuth>,
    pub body: Value,
    /// The provider's bound proxy endpoints: index 0 is the selected endpoint
    /// and the rest are fallbacks tried only when the connection to an
    /// endpoint fails before the upstream could have received the request.
    /// An empty slice builds the direct pinned client as before.
    pub proxy_endpoints: &'a [ProxyEgress],
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

/// Builds the authenticated upstream request builder shared by the one-shot
/// and streaming workspace relays. Egress validation, adapter headers, and
/// auth application are identical; only the pinned client's streaming shape
/// differs, so the caller picks the timeout profile. With a bound proxy the
/// provider's DNS is delegated to the validated proxy endpoint instead of
/// being pinned locally.
async fn prepare_workspace_request<A: ProviderAdapter + ?Sized>(
    adapter: &A,
    request: &AdapterWorkspaceChatRequest<'_>,
    streaming: bool,
    proxy: Option<&ProxyEgress>,
) -> Result<reqwest::RequestBuilder, AdapterRequestError> {
    let endpoint = adapter
        .upstream_endpoint(
            request.base_url,
            request.upstream_protocol,
            request.model,
            streaming,
            None,
        )
        .map_err(|message| AdapterRequestError::new(None, None, message))?;
    let upstream = request.state.operational_settings().settings.upstream;
    let connect_timeout = request
        .state
        .config
        .connect_timeout
        .min(Duration::from_secs(3));
    let request_timeout = request
        .state
        .config
        .request_timeout
        .min(Duration::from_secs(30));
    let user_agent = concat!("ExoRoute/", env!("CARGO_PKG_VERSION"));
    let (endpoint, client) = match proxy {
        Some(proxy) => egress::provider_client_with_proxy(
            endpoint.as_str(),
            request.state.config.allow_private_provider_urls,
            connect_timeout,
            request_timeout,
            streaming,
            user_agent,
            upstream,
            Some(proxy),
        )
        .await
        .map_err(|error| {
            AdapterRequestError::new(None, None, format!("provider egress check failed: {error}"))
        })?,
        None => egress::provider_client(
            endpoint.as_str(),
            request.state.config.allow_private_provider_urls,
            connect_timeout,
            request_timeout,
            streaming,
            user_agent,
            upstream,
        )
        .await
        .map_err(|error| {
            AdapterRequestError::new(None, None, format!("provider egress check failed: {error}"))
        })?,
    };
    let mut provider_request = client
        .post(endpoint)
        .json(&request.body)
        .header("x-request-id", request.provider_id);
    provider_request = apply_custom_headers(provider_request, request.custom_headers)
        .map_err(|message| AdapterRequestError::new(None, None, message))?;
    if request.upstream_protocol == UpstreamProtocol::Messages {
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
                protocol: request.upstream_protocol,
                model: None,
            },
        )
        .map_err(|message| AdapterRequestError::new(None, None, message))?;
    Ok(provider_request)
}

/// Maps a transport failure onto the shared sanitized error shape.
fn workspace_transport_error(
    request: &AdapterWorkspaceChatRequest<'_>,
    error: reqwest::Error,
) -> AdapterRequestError {
    AdapterRequestError::new(
        None,
        None,
        format!(
            "provider '{}' request {}",
            request.provider_id,
            upstream_transport_error_message(&error)
        ),
    )
}

/// Sends the prepared request, trying the remaining proxy endpoints when the
/// connection to one endpoint fails.
///
/// Only connect-class failures (`error.is_connect()`: a rejected tunnel or a
/// connect timeout) advance to the next endpoint, because those are the only
/// failures where the upstream provably never received the request — the
/// workspace relays never retry an inference the upstream may have accepted
/// or billed. Once a response arrives the outcome is final either way, and a
/// dropped caller future simply drops the in-flight send without leaving any
/// state behind (the relay persists nothing).
/// Sends one workspace chat request through an adapter-owned transport.
///
/// Local transports (Devin CLI's local ACP process, Devin Desktop, Kiro) never
/// reach the shared HTTP egress path: their endpoints are process locators such
/// as `devin://acp/stdio`, which the `http/https` URL policy must keep
/// rejecting, or HTTP protocols the shared request builder cannot express. The
/// adapter synthesizes the HTTP-shaped response itself, so the relay decodes it
/// exactly like an upstream reply; proxies are irrelevant because no shared
/// client is built.
async fn send_local_workspace_request<A: ProviderAdapter + ?Sized>(
    adapter: &A,
    request: &AdapterWorkspaceChatRequest<'_>,
    streaming: bool,
) -> Result<reqwest::Response, AdapterRequestError> {
    let request_timeout = request
        .state
        .operational_settings()
        .settings
        .request_timeout;
    let request_id = uuid::Uuid::new_v4().to_string();
    send_local_upstream_request(
        request.state,
        adapter.adapter_id(),
        &request.body,
        request.model,
        &request_id,
        streaming,
        request_timeout,
        request.secret,
        request.oauth_auth,
    )
    .await
    .map_err(|error| {
        AdapterRequestError::new(
            None,
            None,
            format!(
                "provider '{}' local transport failed: {error}",
                request.provider_id
            ),
        )
    })
}

async fn send_workspace_request_with_fallbacks<A: ProviderAdapter + ?Sized>(
    adapter: &A,
    request: &AdapterWorkspaceChatRequest<'_>,
    streaming: bool,
) -> Result<reqwest::Response, AdapterRequestError> {
    if uses_local_transport(adapter.adapter_id()) {
        return send_local_workspace_request(adapter, request, streaming).await;
    }
    let mut attempt = 0usize;
    loop {
        let provider_request = prepare_workspace_request(
            adapter,
            request,
            streaming,
            request.proxy_endpoints.get(attempt),
        )
        .await?;
        match provider_request.send().await {
            Ok(response) => return Ok(response),
            Err(error) => {
                let has_fallback = attempt + 1 < request.proxy_endpoints.len();
                if !error.is_connect() || !has_fallback {
                    return Err(workspace_transport_error(request, error));
                }
                attempt += 1;
            }
        }
    }
}

pub(super) async fn relay_workspace_inference_impl<A: ProviderAdapter + ?Sized>(
    adapter: &A,
    request: AdapterWorkspaceChatRequest<'_>,
) -> Result<AdapterWorkspaceChatOutcome, AdapterRequestError> {
    let response = send_workspace_request_with_fallbacks(adapter, &request, false).await?;
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
    // The adapter's normalization is the relay's decode contract, exactly as in
    // the gateway: a provider that wraps its completion in an adapter-specific
    // envelope (ClinePass answers `{success, data}`) must reach the response
    // decoder unwrapped, or every chat turn fails as an undecodable body.
    let value = adapter.normalize_response(value).map_err(|message| {
        AdapterRequestError::new(Some(http::StatusCode::BAD_GATEWAY), None, message)
    })?;
    Ok(AdapterWorkspaceChatOutcome { body: value })
}

/// Relays one prepared workspace chat request and returns the live upstream
/// response for incremental (streaming) consumption. Once the upstream may
/// have accepted or billed an inference the request is never retried or
/// failed over, and nothing is persisted; only a connect-class failure on a
/// bound proxy endpoint advances to the endpoint's fallback (the upstream
/// provably never received those requests). The caller owns liveness (idle
/// and overall deadlines) and must consume or drop the body; dropping it
/// releases the connection.
pub(super) async fn relay_workspace_inference_stream_impl<A: ProviderAdapter + ?Sized>(
    adapter: &A,
    request: AdapterWorkspaceChatRequest<'_>,
) -> Result<reqwest::Response, AdapterRequestError> {
    send_workspace_request_with_fallbacks(adapter, &request, true).await
}

/// A chat reply can legitimately be large, so the relay reads up to 1 MiB of
/// the upstream body instead of the probe path's 256 KiB.
pub(crate) const MAX_WORKSPACE_CHAT_RESPONSE_BYTES: usize = 1024 * 1024;
