//! Workspace chat relay transport tests: the relay must hand the response
//! decoder the adapter-normalized body, exactly like the gateway does.

use super::support::*;
use super::*;
use crate::provider_adapters::workspace_relay::relay_workspace_inference_impl;
use crate::support::mock_upstream::spawn_upstream;

use std::sync::Arc;

/// Stands in for an adapter whose upstream wraps every completion in its own
/// envelope, the shape the ClinePass adapter normalizes away.
struct EnvelopeAdapter;

impl ProviderAdapter for EnvelopeAdapter {
    fn adapter_id(&self) -> &'static str {
        "envelope"
    }

    fn endpoint(
        &self,
        base_url: &str,
        _protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        reqwest::Url::parse(&format!(
            "{}/chat/completions",
            base_url.trim_end_matches('/')
        ))
        .map_err(|_| "envelope adapter URL is invalid".to_owned())
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn normalize_response(&self, value: Value) -> Result<Value, String> {
        if value.get("success").and_then(Value::as_bool) == Some(false) {
            return Err("envelope provider returned a failed response envelope".to_owned());
        }
        match value.get("data") {
            Some(data) if data.is_object() => Ok(data.clone()),
            _ => Ok(value),
        }
    }
}

struct PreparingAdapter;

impl ProviderAdapter for PreparingAdapter {
    fn adapter_id(&self) -> &'static str {
        "preparing"
    }

    fn endpoint(
        &self,
        base_url: &str,
        _protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        reqwest::Url::parse(&format!(
            "{}/chat/completions",
            base_url.trim_end_matches('/')
        ))
        .map_err(|_| "preparing adapter URL is invalid".to_owned())
    }

    fn upstream_endpoint(
        &self,
        base_url: &str,
        _protocol: UpstreamProtocol,
        _model: &str,
        _streaming: bool,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        reqwest::Url::parse(&format!(
            "{}/chat/completions",
            base_url.trim_end_matches('/')
        ))
        .map_err(|_| "preparing adapter URL is invalid".to_owned())
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn prepare_upstream_request<'a>(
        &'a self,
        _context: AdapterRequestContext<'a>,
        body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<AdapterRequestPreparation, AdapterRequestError>> {
        Box::pin(async move {
            let inner = std::mem::take(body);
            *body = json!({
                "wrapped": true,
                "request": inner,
            });
            let mut headers = HeaderMap::new();
            headers.insert("x-prepared-header", HeaderValue::from_static("true"));
            Ok(AdapterRequestPreparation::new(headers, None, None))
        })
    }
}

fn chat_request<'a>(
    state: &'a AppState,
    base_url: &'a str,
    custom_headers: &'a BTreeMap<String, String>,
) -> AdapterWorkspaceChatRequest<'a> {
    AdapterWorkspaceChatRequest {
        state,
        base_url,
        provider_id: "provider",
        model: "sample-model",
        auth_type: "none",
        auth_header: None,
        custom_headers,
        upstream_protocol: UpstreamProtocol::ChatCompletions,
        secret: None,
        oauth_auth: None,
        body: json!({
            "model": "sample-model",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": false
        }),
        proxy_endpoints: &[],
    }
}

#[tokio::test]
async fn workspace_relay_decodes_the_adapter_normalized_body() {
    let upstream = spawn_upstream(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            axum::Json(json!({
                "success": true,
                "data": {
                    "id": "chatcmpl-1",
                    "model": "sample-model",
                    "choices": [{
                        "message": {"role": "assistant", "content": "inner reply"},
                        "finish_reason": "stop"
                    }]
                },
                "requestId": "redacted"
            }))
        }),
    ))
    .await;
    let (_database, state) = test_state().await;
    let headers = BTreeMap::new();

    let outcome = relay_workspace_inference_impl(
        &EnvelopeAdapter,
        chat_request(&state, &format!("http://{upstream}/v1"), &headers),
    )
    .await
    .expect("relay succeeds");

    // The envelope must not reach the decoder: the relayed body is the
    // adapter-normalized completion, so the response decoder finds its choices.
    assert!(
        outcome.body().get("success").is_none(),
        "the provider envelope must not be relayed: {}",
        outcome.body()
    );
    assert_eq!(
        outcome.body()["choices"][0]["message"]["content"],
        "inner reply"
    );
    let decoded = crate::protocol::decode_upstream_response(
        UpstreamProtocol::ChatCompletions,
        outcome.body(),
        "sample-model",
    )
    .expect("the normalized body decodes");
    assert_eq!(decoded.finish_reason, "stop");
}

#[tokio::test]
async fn workspace_relay_runs_prepare_upstream_request_and_attaches_headers() {
    let captured = Arc::new(tokio::sync::Mutex::new(None));
    let captured_clone = captured.clone();
    let upstream = spawn_upstream(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(
            |headers: HeaderMap, axum::Json(body): axum::Json<Value>| async move {
                let has_header = headers
                    .get("x-prepared-header")
                    .and_then(|v| v.to_str().ok())
                    == Some("true");
                *captured_clone.lock().await = Some((has_header, body));
                axum::Json(json!({
                    "id": "chatcmpl-1",
                    "model": "sample-model",
                    "choices": [{
                        "message": {"role": "assistant", "content": "reply"},
                        "finish_reason": "stop"
                    }]
                }))
            },
        ),
    ))
    .await;
    let (_database, state) = test_state().await;
    let headers = BTreeMap::new();

    let outcome = relay_workspace_inference_impl(
        &PreparingAdapter,
        chat_request(&state, &format!("http://{upstream}/v1"), &headers),
    )
    .await
    .expect("relay succeeds");

    assert_eq!(outcome.body()["choices"][0]["message"]["content"], "reply");
    let captured_data = captured
        .lock()
        .await
        .take()
        .expect("request was received upstream");
    assert!(
        captured_data.0,
        "x-prepared-header was not forwarded to upstream"
    );
    assert_eq!(
        captured_data.1["wrapped"],
        json!(true),
        "body was not prepared by adapter"
    );
    assert_eq!(captured_data.1["request"]["model"], json!("sample-model"));
}

#[tokio::test]
async fn workspace_relay_reports_a_rejected_adapter_envelope_as_a_provider_error() {
    let upstream = spawn_upstream(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            axum::Json(json!({"success": false, "requestId": "redacted"}))
        }),
    ))
    .await;
    let (_database, state) = test_state().await;
    let headers = BTreeMap::new();

    let error = match relay_workspace_inference_impl(
        &EnvelopeAdapter,
        chat_request(&state, &format!("http://{upstream}/v1"), &headers),
    )
    .await
    {
        Ok(_) => panic!("a failed provider envelope must not be relayed as a reply"),
        Err(error) => error,
    };

    assert_eq!(error.status, Some(axum::http::StatusCode::BAD_GATEWAY));
    assert_eq!(
        error.message,
        "envelope provider returned a failed response envelope"
    );
}

/// Stands in for the Devin CLI adapter: the same adapter id (the
/// local-transport dispatch key) with the process locator endpoint the real
/// adapter keeps.
struct LocalTransportAdapter;

impl ProviderAdapter for LocalTransportAdapter {
    fn adapter_id(&self) -> &'static str {
        "devin_cli"
    }

    fn endpoint(
        &self,
        _base_url: &str,
        _protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        reqwest::Url::parse("devin://acp/stdio").map_err(|error| error.to_string())
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn normalize_response(&self, value: Value) -> Result<Value, String> {
        Ok(value)
    }
}

/// A local-transport endpoint is a process locator (`devin://acp/stdio`), not
/// an HTTP URL: the relay must dispatch the adapter-owned transport instead of
/// running the shared `http/https` egress validation over the endpoint. The
/// body carries unsupported media content so the local send fails fast in its own
/// preflight — deterministically, without spawning a real CLI — which proves
/// the failure happened after the egress boundary, not inside it.
#[tokio::test]
async fn workspace_relay_routes_local_transport_adapters_without_http_egress() {
    let (_database, state) = test_state().await;
    let headers = BTreeMap::new();
    let mut request = chat_request(&state, "devin://acp/stdio", &headers);
    request.provider_id = "devin cli";
    request.body = json!({
        "model": "swe-1-7",
        "messages": [{
            "role": "user",
            "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,x"}}]
        }],
        "stream": false,
    });

    let error = match relay_workspace_inference_impl(&LocalTransportAdapter, request).await {
        Ok(_) => panic!("an unsupported media body must fail the Devin CLI preflight"),
        Err(error) => error,
    };

    assert!(
        error.message.contains("local transport failed"),
        "expected a local transport failure, got: {}",
        error.message
    );
    assert!(
        !error.message.contains("egress check"),
        "local transports must skip the HTTP egress check: {}",
        error.message
    );
}
