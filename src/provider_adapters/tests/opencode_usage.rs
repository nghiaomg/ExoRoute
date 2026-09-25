use super::support::*;
use super::*;

#[tokio::test]
async fn opencode_go_and_zen_discovery_use_their_models_endpoints_with_bearer_auth() {
    let responses = vec![
        MockResponse::json(200, br#"{"data":[{"id":"go-model"}]}"#.to_vec()),
        MockResponse::json(200, br#"{"models":[{"name":"zen-model"}]}"#.to_vec()),
    ];
    let (address, server) = spawn_mock_http_server(responses).await;
    let (_database, state) = test_state().await;
    let go = discover_api_key_models(
        OPENCODE_GO_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: &format!("http://{address}/zen/go/v1"),
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "go-secret",
        },
    )
    .await
    .expect("Go model discovery");
    let zen = discover_api_key_models(
        OPENCODE_ZEN_ADAPTER_ID,
        AdapterApiKeyRequest {
            state: &state,
            base_url: &format!("http://{address}/zen/v1"),
            auth_type: "bearer",
            auth_header: None,
            custom_headers: &BTreeMap::new(),
            preferred_protocol: "chat_completions",
            credential: "zen-secret",
        },
    )
    .await
    .expect("Zen model discovery");
    let requests = server.await.expect("model mock server");

    assert!(matches!(
        go,
        ModelDiscoveryResult::Available { ref models, truncated: false }
            if models == &["go-model"]
    ));
    assert!(matches!(
        zen,
        ModelDiscoveryResult::Available { ref models, truncated: false }
            if models == &["zen-model"]
    ));
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0]
            .request_line
            .starts_with("GET /zen/go/v1/models HTTP/1.1")
    );
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer go-secret")
    );
    assert!(
        requests[1]
            .request_line
            .starts_with("GET /zen/v1/models HTTP/1.1")
    );
    assert_eq!(
        requests[1].authorization.as_deref(),
        Some("Bearer zen-secret")
    );
    assert!(!model_catalog_authoritative(OPENCODE_GO_ADAPTER_ID));
    assert!(!model_catalog_authoritative(OPENCODE_ZEN_ADAPTER_ID));
}

#[tokio::test]
async fn opencode_go_usage_uses_bearer_limits_body_and_keeps_zen_unverified() {
    let valid = json!({"usage":{
        "rolling":{"status":"ok","percent":10,"resetsAt":"2026-09-15T01:00:00Z"},
        "weekly":{"status":"ok","percent":20,"resetsAt":"2026-09-20T00:00:00Z"},
        "monthly":{"status":"ok","percent":30,"resetsAt":"2026-10-01T00:00:00Z"}
    }});
    let (address, server) = spawn_mock_http_server(vec![MockResponse::json(
        200,
        serde_json::to_vec(&valid).expect("valid usage JSON"),
    )])
    .await;
    let (_database, state) = test_state().await;
    let snapshot = fetch_api_key_usage(
        OPENCODE_GO_ADAPTER_ID,
        &state,
        &format!("http://{address}/zen/go/v1"),
        "go-secret",
    )
    .await
    .expect("Go quota is available");
    let requests = server.await.expect("usage mock server");
    assert_eq!(snapshot.quotas.len(), 3);
    assert!(
        snapshot
            .quotas
            .iter()
            .all(|quota| quota.unit.as_deref() == Some("percent"))
    );
    assert!(
        requests[0]
            .request_line
            .starts_with("GET /zen/go/v1/usage HTTP/1.1")
    );
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer go-secret")
    );

    let unsupported = fetch_api_key_usage(
        OPENCODE_ZEN_ADAPTER_ID,
        &state,
        &format!("http://{address}/zen/v1"),
        "zen-secret",
    )
    .await
    .expect_err("Zen quota has no verified per-key source");
    assert!(unsupported.contains("does not support API-key usage"));
}

#[tokio::test]
async fn opencode_go_usage_rejects_unauthorized_and_oversized_responses() {
    let (_database, state) = test_state().await;
    let (unauthorized_address, unauthorized_server) =
        spawn_mock_http_server(vec![MockResponse::json(401, b"{}".to_vec())]).await;
    let unauthorized = fetch_api_key_usage(
        OPENCODE_GO_ADAPTER_ID,
        &state,
        &format!("http://{unauthorized_address}/zen/go/v1"),
        "bad-secret",
    )
    .await
    .expect_err("401 must be reported as a rejected key");
    assert!(unauthorized.contains("rejected this API key"));
    let _ = unauthorized_server.await.expect("unauthorized mock server");

    let oversized = vec![b' '; 256 * 1024 + 1];
    let (oversized_address, oversized_server) =
        spawn_mock_http_server(vec![MockResponse::json(200, oversized)]).await;
    let error = fetch_api_key_usage(
        OPENCODE_GO_ADAPTER_ID,
        &state,
        &format!("http://{oversized_address}/zen/go/v1"),
        "test-secret",
    )
    .await
    .expect_err("oversized quota response must be rejected");
    assert!(error.contains("256 KiB"));
    let _ = oversized_server.await.expect("oversized mock server");
}

#[tokio::test]
async fn opencode_go_usage_has_an_overall_timeout() {
    let upstream = spawn_silent_upstream().await;
    let address = upstream.address;
    let (_database, state) = test_state().await;
    let configured_timeout = state
        .operational_settings()
        .settings
        .request_timeout
        .min(Duration::from_secs(8));
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        fetch_api_key_usage(
            OPENCODE_GO_ADAPTER_ID,
            &state,
            &format!("http://{address}/zen/go/v1"),
            "test-secret",
        ),
    )
    .await
    .expect("quota fetch must return before the outer safety timeout");
    let error = result.expect_err("no upstream response should time out");
    assert!(!error.is_empty());
    assert!(started.elapsed() >= configured_timeout.saturating_sub(Duration::from_millis(500)));
    assert!(started.elapsed() < Duration::from_secs(10));
    upstream.abort();
}
