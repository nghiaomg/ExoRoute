use super::support::*;
use super::*;

#[tokio::test]
async fn stream_continuity_retries_rate_limit_and_server_failures_before_emitting_an_error() {
    let calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(transient_stream_retryable_upstream),
            )
            .with_state(calls.clone()),
    )
    .await;

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let resource_limits = GatewayResourceLimits {
        stream_continuity_enabled: true,
        ..GatewayResourceLimits::default()
    };
    let state =
        AppState::new_with_gateway_resource_limits(config, database.db.clone(), resource_limits);
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "provider",
            name: "Mock provider",
            base_url: &format!("http://{address}/v1"),
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "mock",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "mock-model", "chat_completions", 0)],
    )
    .await
    .expect("seed route");

    let response = start_continuous_request(
        state,
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true
        })),
        Protocol::ChatCompletions,
        GatewayExecutionSettings {
            resource_limits,
            operational_settings: OperationalSettings {
                request_timeout: Duration::from_secs(2),
                ..OperationalSettings::default()
            },
            api_key_id: Some("owner".to_owned()),
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("continuity response");
    assert!(!String::from_utf8_lossy(&body).contains("event: error"));
    assert_eq!(calls.load(Ordering::Relaxed), 4);
}

#[tokio::test]
async fn stream_continuity_retries_upstream_timeout_before_emitting_an_error() {
    let calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(transient_stream_timeout_upstream),
            )
            .with_state(calls.clone()),
    )
    .await;

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(1);
    let resource_limits = GatewayResourceLimits {
        stream_continuity_enabled: true,
        ..GatewayResourceLimits::default()
    };
    let state =
        AppState::new_with_gateway_resource_limits(config, database.db.clone(), resource_limits);
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "provider",
            name: "Mock provider",
            base_url: &format!("http://{address}/v1"),
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "mock",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "mock-model", "chat_completions", 0)],
    )
    .await
    .expect("seed route");

    let response = start_continuous_request(
        state,
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true
        })),
        Protocol::ChatCompletions,
        GatewayExecutionSettings {
            resource_limits,
            operational_settings: OperationalSettings {
                connect_timeout: Duration::from_secs(1),
                request_timeout: Duration::from_secs(1),
                ..OperationalSettings::default()
            },
            api_key_id: Some("owner".to_owned()),
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("continuity response");
    let body = String::from_utf8_lossy(&body);
    assert!(!body.contains("event: error"), "{body}");
    assert!(body.contains("recovered"), "{body}");
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}
