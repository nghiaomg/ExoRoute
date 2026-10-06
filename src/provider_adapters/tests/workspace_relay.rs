//! Workspace chat relay transport tests: the relay must hand the response
//! decoder the adapter-normalized body, exactly like the gateway does.

use super::support::*;
use super::*;
use crate::provider_adapters::workspace_relay::relay_workspace_inference_impl;
use crate::support::mock_upstream::spawn_upstream;

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
        protocol: Protocol::ChatCompletions,
        secret: None,
        oauth_auth: None,
        body: json!({
            "model": "sample-model",
            "messages": [{"role": "user", "content": "hello"}],
            "stream": false
        }),
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
