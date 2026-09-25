use super::support::*;
use super::*;

#[tokio::test]
async fn priority_combo_retries_from_the_next_target_after_backoff() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(rotating_priority_upstream))
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
        &[
            ("provider", "model-a", "chat_completions", 0),
            ("provider", "model-b", "chat_completions", 1),
        ],
    )
    .await
    .expect("seed priority combo");

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
    let observed_models = calls.lock().await.clone();
    assert_eq!(
        observed_models,
        ["model-a", "model-b", "model-b", "model-a"]
    );
}
