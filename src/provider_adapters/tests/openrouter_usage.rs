use super::support::*;
use super::*;

#[tokio::test]
async fn openrouter_key_test_quota_and_discovery_use_only_documented_get_endpoints() {
    let key_response = serde_json::to_vec(&json!({"data": {
        "limit": 100.0,
        "limit_remaining": 74.5,
        "usage": 25.5,
        "limit_reset": "monthly",
        "is_free_tier": false
    }}))
    .expect("OpenRouter key response JSON");
    let models_response =
        br#"{"data":[{"id":"anthropic/claude-test"},{"id":"vendor/model:free"}]}"#.to_vec();
    let (address, server) = spawn_mock_http_server(vec![
        MockResponse::json(200, key_response.clone()),
        MockResponse::json(200, key_response),
        MockResponse::json(200, models_response),
    ])
    .await;
    let (_database, state) = test_state().await;
    let base_url = format!("http://{address}/api/v1");

    let key_test = test_api_key_credential(
        OPENROUTER_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: &base_url,
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "test-secret",
        },
    )
    .await;
    assert!(key_test.test_passed);
    assert_eq!(key_test.status, Some(200));
    let usage = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret")
        .await
        .expect("OpenRouter key usage");
    assert_eq!(usage.quotas[0].used_amount, Some(25.5));
    let discovery = discover_api_key_models(
        OPENROUTER_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: &base_url,
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "test-secret",
        },
    )
    .await
    .expect("OpenRouter model catalog");
    assert!(matches!(
        discovery,
        ModelDiscoveryResult::Available { ref models, truncated: false }
            if models == &["anthropic/claude-test", "vendor/model:free"]
    ));

    let requests = server.await.expect("OpenRouter mock server");
    assert_eq!(requests.len(), 3);
    assert!(
        requests[0]
            .request_line
            .starts_with("GET /api/v1/key HTTP/1.1")
    );
    assert!(
        requests[1]
            .request_line
            .starts_with("GET /api/v1/key HTTP/1.1")
    );
    assert!(
        requests[2]
            .request_line
            .starts_with("GET /api/v1/models HTTP/1.1")
    );
    assert!(
        requests
            .iter()
            .all(|request| request.authorization.as_deref() == Some("Bearer test-secret"))
    );
}

#[tokio::test]
async fn openrouter_usage_errors_are_redacted_and_return_http_statuses() {
    let the_body = br#"{"error":{"message":"secret upstream response"}}"#.to_vec();
    let (address, server) = spawn_mock_http_server(vec![
        MockResponse::json(401, the_body.clone()),
        MockResponse::json(403, the_body.clone()),
        MockResponse::json(429, the_body.clone()),
        MockResponse::json(503, the_body),
    ])
    .await;
    let (_database, state) = test_state().await;
    let base_url = format!("http://{address}/api/v1");

    for (status, expected) in [
        (401, "rejected this API key"),
        (403, "rejected this API key"),
        (429, "HTTP 429"),
        (503, "HTTP 503"),
    ] {
        let error = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret")
            .await
            .expect_err("non-success status must preserve an unavailable quota state");
        assert!(error.contains(expected));
        assert!(!error.contains("secret upstream response"));
        assert!(!error.contains("test-secret"));
        if status == 401 || status == 403 {
            assert!(error.contains(&status.to_string()));
        }
    }
    let requests = server.await.expect("OpenRouter error mock server");
    assert_eq!(requests.len(), 4);
    assert!(
        requests
            .iter()
            .all(|request| request.request_line.starts_with("GET /api/v1/key HTTP/1.1"))
    );
}

#[tokio::test]
async fn openrouter_usage_rejects_invalid_json_and_oversized_bodies() {
    let oversized = vec![b' '; 256 * 1024 + 1];
    let (address, server) = spawn_mock_http_server(vec![
        MockResponse::json(200, b"not-json".to_vec()),
        MockResponse::json(200, oversized),
    ])
    .await;
    let (_database, state) = test_state().await;
    let base_url = format!("http://{address}/api/v1");

    let invalid = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret")
        .await
        .expect_err("invalid JSON should be rejected");
    assert!(invalid.contains("invalid API key usage response"));
    let too_large = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret")
        .await
        .expect_err("oversized usage responses must be rejected");
    assert!(too_large.contains("configured size limit"));
    let requests = server.await.expect("OpenRouter response-limit mock server");
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.request_line.starts_with("GET /api/v1/key HTTP/1.1"))
    );
}

#[tokio::test]
async fn openrouter_egress_validation_error_is_sanitized() {
    let (_database, state) = test_state().await;
    let error = fetch_api_key_usage(
        OPENROUTER_ADAPTER_ID,
        &state,
        "file:///not-a-provider",
        "test-secret",
    )
    .await
    .expect_err("unsupported provider URL schemes are rejected");
    assert_eq!(error, "OpenRouter provider egress validation failed");
    assert!(!error.contains("test-secret"));
}

#[tokio::test]
async fn openrouter_usage_timeout_is_bounded_and_sanitized() {
    let upstream = spawn_silent_upstream().await;
    let address = upstream.address;
    let (_database, state) = test_state().await;
    let started = Instant::now();
    let configured_timeout = state
        .operational_settings()
        .settings
        .request_timeout
        .min(Duration::from_secs(8));
    let error = fetch_api_key_usage(
        OPENROUTER_ADAPTER_ID,
        &state,
        &format!("http://{address}/api/v1"),
        "test-secret",
    )
    .await
    .expect_err("silent upstream must time out");
    assert_eq!(error, "OpenRouter usage request timed out");
    assert!(started.elapsed() >= configured_timeout.saturating_sub(Duration::from_millis(500)));
    assert!(started.elapsed() < configured_timeout + Duration::from_secs(2));
    upstream.abort();
}

#[tokio::test]
async fn cancelling_openrouter_usage_read_drops_the_in_flight_request() {
    let mut upstream = spawn_silent_upstream().await;
    let address = upstream.address;
    let (_database, state) = test_state().await;
    let base_url = format!("http://{address}/api/v1");
    {
        let usage = fetch_api_key_usage(OPENROUTER_ADAPTER_ID, &state, &base_url, "test-secret");
        tokio::pin!(usage);
        tokio::select! {
            request_line = &mut upstream.request_line => {
                assert!(request_line.expect("request arrives").starts_with("GET /api/v1/key HTTP/1.1"));
            }
            result = &mut usage => {
                let _ = result;
                panic!("usage request unexpectedly completed before cancellation");
            }
        }
    }
    upstream.abort();
}
