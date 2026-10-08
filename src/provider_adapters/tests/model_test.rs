use super::support::*;
use super::*;

#[tokio::test]
async fn command_code_key_test_uses_whoami_and_bearer_without_inference() {
    let (address, server) =
        spawn_mock_http_server(vec![MockResponse::json(200, b"{}".to_vec())]).await;
    let base_url = format!("http://{address}");
    let (_database, state) = test_state().await;
    let outcome =
        test_command_code_api_key(&state, &base_url, &BTreeMap::new(), "test-secret").await;
    let requests = server.await.expect("mock server task");
    assert!(outcome.test_passed);
    assert_eq!(outcome.status, Some(200));
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0]
            .request_line
            .starts_with("GET /alpha/whoami HTTP/1.1")
    );
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer test-secret")
    );
}

#[tokio::test]
async fn model_test_preserves_sanitized_provider_error_body() {
    let provider_body = br#"{"error":{"message":"model does not support this request","code":"invalid_request"},"api_key":"test-secret","input":"private prompt"}"#.to_vec();
    let (address, server) =
        spawn_mock_http_server(vec![MockResponse::json(400, provider_body)]).await;
    let base_url = format!("http://{address}");
    let (_database, state) = test_state().await;
    let credential = Some("test-secret".to_owned());
    let credentials = [credential];

    let outcome = test_api_key_model(
        GENERIC_ADAPTER_ID,
        AdapterModelTestRequest {
            state: &state,
            base_url: &base_url,
            provider_id: "provider",
            model: "test-model",
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            protocol: Protocol::ChatCompletions,
            credentials: &credentials,
        },
    )
    .await;
    let requests = server.await.expect("model test mock server");

    assert!(!outcome.test_passed);
    assert_eq!(outcome.status, Some(400));
    let provider_response = outcome
        .provider_response_body
        .as_deref()
        .expect("provider error body");
    assert!(provider_response.contains("model does not support this request"));
    assert!(provider_response.contains("invalid_request"));
    assert!(!provider_response.contains("test-secret"));
    assert!(!provider_response.contains("private prompt"));
    assert_eq!(requests.len(), 1);
}

#[test]
fn non_sse_model_test_preserves_provider_diagnostics() {
    let outcome = model_test_non_sse_response(
        StatusCode::OK,
        Some("application/json; charset=utf-8"),
        br#"{"error":{"message":"streaming is unavailable for this model","api_key":"test-secret"}}"#,
    );

    assert!(!outcome.test_passed);
    assert_eq!(outcome.status, Some(StatusCode::BAD_GATEWAY.as_u16()));
    assert!(outcome.message.contains("HTTP 200"));
    assert!(outcome.message.contains("application/json; charset=utf-8"));
    let provider_response = outcome
        .provider_response_body
        .as_deref()
        .expect("provider non-SSE body");
    assert!(provider_response.contains("streaming is unavailable for this model"));
    assert!(!provider_response.contains("test-secret"));
}

/// The reference upstream refusal, byte for byte, as the OpenCode Zen free tier
/// answers a request it does not recognise as its own client.
const FREE_TIER_REFUSAL: &str = r#"{"error":{"message":"OpenCode's free tier can only be used from within OpenCode","type":"FreeTierError"},"type":"error"}"#;

async fn zen_model_probe(base_url: &str, state: &AppState) -> AdapterModelTestOutcome {
    let credential = Some("test-secret".to_owned());
    let credentials = [credential];
    test_api_key_model(
        OPENCODE_ZEN_ADAPTER_ID,
        AdapterModelTestRequest {
            state,
            base_url,
            provider_id: "provider",
            model: "exo-free",
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            protocol: Protocol::ChatCompletions,
            credentials: &credentials,
        },
    )
    .await
}

#[tokio::test]
async fn zen_model_probe_asks_for_a_stream_and_declares_the_client_fingerprint() {
    let frame = json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": {"content": "OK"}}]});
    let (address, server) = spawn_mock_http_server(vec![MockResponse::sse(
        format!("data: {frame}\n\ndata: [DONE]\n\n").into_bytes(),
    )])
    .await;
    let base_url = format!("http://{address}");
    let (_database, state) = test_state().await;

    let outcome = zen_model_probe(&base_url, &state).await;
    let requests = server.await.expect("model test mock server");

    assert!(outcome.test_passed, "{}", outcome.message);
    assert_eq!(outcome.status, Some(200));
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    // The free tier refuses a non-streaming inference, so the probe asks for an
    // event stream and must therefore accept one.
    assert_eq!(request.header("accept"), Some("text/event-stream"));
    // Without the official client's identity the same probe answers 403.
    assert_eq!(request.header("user-agent"), Some("opencode/1.18.31"));
    assert_eq!(request.header("x-opencode-client"), Some("desktop"));
    assert_eq!(request.header("x-opencode-project"), Some("global"));
    let session = request
        .header("x-opencode-session")
        .expect("session header");
    assert!(session.starts_with("ses_"), "{session}");
    let request_id = request
        .header("x-opencode-request")
        .expect("request header");
    assert!(request_id.starts_with("msg_"), "{request_id}");
}

#[tokio::test]
async fn zen_model_probe_reports_a_free_tier_refusal_from_the_event_stream() {
    let (address, server) = spawn_mock_http_server(vec![MockResponse::sse(
        format!("data: {FREE_TIER_REFUSAL}\n\n").into_bytes(),
    )])
    .await;
    let base_url = format!("http://{address}");
    let (_database, state) = test_state().await;

    let outcome = zen_model_probe(&base_url, &state).await;
    let _ = server.await.expect("model test mock server");

    assert!(!outcome.test_passed);
    assert_eq!(outcome.status, Some(StatusCode::BAD_GATEWAY.as_u16()));
    assert!(
        outcome.message.contains("free tier can only be used"),
        "{}",
        outcome.message
    );
    let provider_response = outcome
        .provider_response_body
        .as_deref()
        .expect("refusal body");
    assert!(provider_response.contains("FreeTierError"));
    assert!(!provider_response.contains("test-secret"));
}

#[test]
fn probe_event_stream_accepts_a_response_frame_and_rejects_an_empty_stream() {
    assert!(model_probe_event_stream(b"data: {\"choices\":[{\"delta\":{}}]}\n\n").is_ok());
    // A stream that only terminates carries no evidence the model ran.
    assert!(model_probe_event_stream(b"data: [DONE]\n\n").is_err());
    assert!(model_probe_event_stream(b"").is_err());
    // A JSON body never reaches this path, but a non-stream body here is not an
    // event stream and must not be reported as success.
    assert!(model_probe_event_stream(br#"{"choices":[]}"#).is_err());
}

#[test]
fn probe_event_stream_reads_the_refusal_shape_of_each_protocol() {
    assert_eq!(
        provider_event_stream_error(&json!({"error": {"type": "FreeTierError"}})).as_deref(),
        Some("Provider event stream reported an error")
    );
    assert_eq!(
        provider_event_stream_error(&json!({"type": "error", "message": "model unavailable"}))
            .as_deref(),
        Some("model unavailable")
    );
    assert_eq!(
        provider_event_stream_error(&json!({"response": {"status": "failed"}})).as_deref(),
        Some("Provider event stream reported a failed response")
    );
    assert!(
        provider_event_stream_error(&json!({"choices": [{"delta": {"content": "OK"}}]})).is_none()
    );
    assert!(provider_event_stream_error(&json!({"error": null})).is_none());
}

#[test]
fn only_a_stream_expecting_adapter_forces_the_probe_body_to_stream() {
    // The probe body itself stays non-streaming; only an adapter that declares
    // `probe_requires_event_stream` has it flipped by the probe driver.
    let body = model_probe_body("exo-free", Protocol::ChatCompletions);
    assert_eq!(body["stream"], json!(false));
}
