use super::support::*;
use super::*;

#[derive(Clone)]
struct ClientErrorState {
    calls: Arc<AtomicUsize>,
    status: StatusCode,
}

async fn client_error_upstream(
    State(state): State<Arc<ClientErrorState>>,
) -> axum::response::Response {
    state.calls.fetch_add(1, Ordering::Relaxed);
    (
        state.status,
        Json(json!({"error":{"message":"request rejected"}})),
    )
        .into_response()
}

#[derive(Clone)]
struct ShutdownTestState {
    calls: Arc<AtomicUsize>,
    first_call: Arc<tokio::sync::Notify>,
}

async fn server_error_and_signal_call(
    State(state): State<Arc<ShutdownTestState>>,
) -> axum::response::Response {
    state.calls.fetch_add(1, Ordering::Relaxed);
    state.first_call.notify_one();
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({"error":{"message":"upstream unavailable"}})),
    )
        .into_response()
}

async fn enable_combo_retry_until_success(database: &TestDatabase, route_id: &str) {
    let route_id = route_id.to_owned();
    database
        .db
        .write(move |transaction| {
            let mut route = transaction
                .get::<Record>(Table::Routes, &route_id)?
                .ok_or(crate::infra::storage::StorageError::NotFound)?;
            route.insert("retry_until_success", Field::Bool(true));
            transaction.put(Table::Routes, &route_id, &route)
        })
        .await
        .expect("enable combo retry mode");
}

async fn set_combo_strategy(database: &TestDatabase, route_id: &str, strategy: &str) {
    let route_id = route_id.to_owned();
    let strategy = strategy.to_owned();
    database
        .db
        .write(move |transaction| {
            let mut route = transaction
                .get::<Record>(Table::Routes, &route_id)?
                .ok_or(crate::infra::storage::StorageError::NotFound)?;
            route.insert("strategy", Field::Text(strategy.clone()));
            transaction.put(Table::Routes, &route_id, &route)
        })
        .await
        .expect("set combo strategy");
}

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
            api_key_scope: None,
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

#[tokio::test]
async fn retry_until_success_combo_restarts_at_the_first_target_after_errors() {
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
            id: "provider-retry",
            name: "Retry provider",
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
        "coding-retry",
        "Coding retry",
        &["chat_completions"],
        &[
            ("provider-retry", "model-a", "chat_completions", 0),
            ("provider-retry", "model-b", "chat_completions", 1),
        ],
    )
    .await
    .expect("seed combo");
    enable_combo_retry_until_success(&database, "coding-retry").await;
    set_combo_strategy(&database, "coding-retry", "round_robin").await;

    let response = handle_request_with_limits_and_owner(
        state,
        HeaderMap::new(),
        json!({
            "model":"coding-retry",
            "messages":[{"role":"user","content":"hello"}]
        }),
        Protocol::ChatCompletions,
        retry_execution_settings(),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        calls.lock().await.clone(),
        ["model-a", "model-b", "model-a", "model-b"]
    );
}

#[tokio::test]
async fn retry_until_success_combo_returns_http_400_and_413_after_exhaustion() {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());

    for status in [StatusCode::BAD_REQUEST, StatusCode::PAYLOAD_TOO_LARGE] {
        let calls = Arc::new(AtomicUsize::new(0));
        let upstream_state = Arc::new(ClientErrorState {
            calls: calls.clone(),
            status,
        });
        let address = spawn_upstream(
            Router::new()
                .route("/v1/chat/completions", post(client_error_upstream))
                .with_state(upstream_state),
        )
        .await;
        let provider_id = format!("provider-{}", status.as_u16());
        let combo_id = format!("coding-{}", status.as_u16());
        crate::support::test_support::seed_provider(
            &database.db,
            crate::support::test_support::ProviderSeed {
                id: &provider_id,
                name: "Client error provider",
                base_url: &format!("http://{address}/v1"),
                adapter_id: "generic",
                auth_type: "none",
                model_prefix: if status == StatusCode::BAD_REQUEST {
                    "mock-400"
                } else {
                    "mock-413"
                },
                preferred_protocol: "chat_completions",
                supported_protocols: &["chat_completions"],
            },
        )
        .await
        .expect("seed provider");
        crate::support::test_support::seed_route(
            &database.db,
            &combo_id,
            "Client error combo",
            &["chat_completions"],
            &[
                (&provider_id, "model-a", "chat_completions", 0),
                (&provider_id, "model-b", "chat_completions", 1),
            ],
        )
        .await
        .expect("seed combo");
        enable_combo_retry_until_success(&database, &combo_id).await;

        let response = handle_request_with_limits_and_owner(
            state.clone(),
            HeaderMap::new(),
            json!({
                "model":combo_id,
                "messages":[{"role":"user","content":"hello"}]
            }),
            Protocol::ChatCompletions,
            retry_execution_settings(),
        )
        .await;

        assert_eq!(response.status(), status);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }
}

#[tokio::test]
async fn retry_until_success_combo_stops_cleanly_on_gateway_shutdown() {
    let calls = Arc::new(AtomicUsize::new(0));
    let first_call = Arc::new(tokio::sync::Notify::new());
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(server_error_and_signal_call))
            .with_state(Arc::new(ShutdownTestState {
                calls: calls.clone(),
                first_call: first_call.clone(),
            })),
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
            id: "provider-shutdown",
            name: "Shutdown provider",
            base_url: &format!("http://{address}/v1"),
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "shutdown-mock",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    crate::support::test_support::seed_route(
        &database.db,
        "coding-shutdown",
        "Shutdown combo",
        &["chat_completions"],
        &[("provider-shutdown", "model-a", "chat_completions", 0)],
    )
    .await
    .expect("seed combo");
    enable_combo_retry_until_success(&database, "coding-shutdown").await;

    let first_call_notified = first_call.notified();
    let request = tokio::spawn(handle_request_with_limits_and_owner(
        state.clone(),
        HeaderMap::new(),
        json!({
            "model":"coding-shutdown",
            "messages":[{"role":"user","content":"hello"}]
        }),
        Protocol::ChatCompletions,
        retry_execution_settings(),
    ));
    tokio::time::timeout(Duration::from_secs(2), first_call_notified)
        .await
        .expect("first upstream attempt reached");
    state.request_shutdown();

    let response = tokio::time::timeout(Duration::from_secs(2), request)
        .await
        .expect("combo retry stopped after shutdown")
        .expect("gateway request task joined");
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(calls.load(Ordering::Relaxed) >= 1);
}
