use super::support::*;
use super::*;

#[tokio::test]
async fn server_failure_is_retried_before_returning_to_the_client() {
    let calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(transient_server_error_upstream),
            )
            .with_state(calls.clone()),
    )
    .await;

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
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

    let response = execute_with_server_retries(
        state,
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}]
        })),
        Protocol::ChatCompletions,
        None,
        GatewayExecutionSettings {
            resource_limits: GatewayResourceLimits::default(),
            operational_settings: OperationalSettings {
                request_timeout: Duration::from_secs(2),
                ..OperationalSettings::default()
            },
            api_key_id: None,
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("gateway response");
    let payload: Value = serde_json::from_slice(&body).expect("gateway JSON");
    assert_eq!(
        payload["choices"][0]["message"]["content"],
        "served after retry"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn empty_upstream_completion_is_retried_before_returning_to_the_client() {
    let calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(transient_empty_completion_upstream),
            )
            .with_state(calls.clone()),
    )
    .await;

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
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

    let response = execute_with_server_retries(
        state,
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}]
        })),
        Protocol::ChatCompletions,
        None,
        GatewayExecutionSettings {
            resource_limits: GatewayResourceLimits::default(),
            operational_settings: OperationalSettings {
                request_timeout: Duration::from_secs(2),
                ..OperationalSettings::default()
            },
            api_key_id: None,
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("gateway response");
    let payload: Value = serde_json::from_slice(&body).expect("gateway JSON");
    assert_eq!(
        payload["choices"][0]["message"]["content"],
        "served after empty response"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn empty_upstream_stream_is_retried_before_returning_to_the_client() {
    let calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(transient_empty_stream_upstream),
            )
            .with_state(calls.clone()),
    )
    .await;

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
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

    let response = execute_with_server_retries(
        state,
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true
        })),
        Protocol::ChatCompletions,
        None,
        GatewayExecutionSettings {
            resource_limits: GatewayResourceLimits::default(),
            operational_settings: OperationalSettings {
                request_timeout: Duration::from_secs(2),
                ..OperationalSettings::default()
            },
            api_key_id: None,
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("gateway stream response");
    let body = String::from_utf8(body.to_vec()).expect("gateway stream body");
    assert!(body.contains("recovered after empty stream"), "{body}");
    assert!(
        !body.contains("upstream completed the response without content"),
        "{body}"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}
