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
