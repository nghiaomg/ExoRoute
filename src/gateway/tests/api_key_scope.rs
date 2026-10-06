use super::support::*;
use super::*;
use crate::security::api_key_scope::ApiKeyScope;
use crate::support::test_support::{
    ProviderSeed, TestDatabase, seed_api_key, seed_api_key_with_scope, seed_provider,
    seed_provider_model, seed_route,
};
use std::net::SocketAddr;
use tower::ServiceExt;

/// Seeds the two providers, the route under test, and a scoped gateway key.
/// Returns the upstream call counters so a test can prove a denied request never
/// reached a provider.
async fn scoped_harness(
    first_address: SocketAddr,
    second_address: SocketAddr,
    targets: &[(&str, &str, &str, u32)],
) -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
    // Provider model prefixes are unique, so each seeded provider gets its own.
    for (id, prefix, address) in [
        ("provider-a", "mock-a", first_address),
        ("provider-b", "mock-b", second_address),
    ] {
        seed_provider(
            &database.db,
            ProviderSeed {
                id,
                name: id,
                base_url: &format!("http://{address}/v1"),
                adapter_id: "generic",
                auth_type: "none",
                model_prefix: prefix,
                preferred_protocol: "chat_completions",
                supported_protocols: &["chat_completions"],
            },
        )
        .await
        .expect("seed provider");
    }
    seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        targets,
    )
    .await
    .expect("seed route");
    (database, state)
}

fn scope(providers: &[&str], models: &[&str]) -> ApiKeyScope {
    let providers: Vec<String> = providers.iter().map(|value| (*value).to_owned()).collect();
    let models: Vec<String> = models.iter().map(|value| (*value).to_owned()).collect();
    ApiKeyScope::from_entries(&providers, &models).expect("valid API key scope")
}

fn settings(scope: Option<ApiKeyScope>) -> GatewayExecutionSettings {
    GatewayExecutionSettings {
        resource_limits: GatewayResourceLimits::default(),
        operational_settings: OperationalSettings {
            request_timeout: Duration::from_secs(2),
            ..OperationalSettings::default()
        },
        api_key_id: Some("scoped-key".to_owned()),
        api_key_scope: scope.map(Arc::new),
        analytics: None,
        stream_continuity_retry: false,
    }
}

async fn chat_request(state: &AppState, settings: GatewayExecutionSettings) -> Response {
    handle_request_inner_with_adapter_base_url_override_and_limits(
        state.clone(),
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}]
        })),
        Protocol::ChatCompletions,
        None,
        settings,
        0,
    )
    .await
}

async fn streaming_chat_request(state: &AppState, settings: GatewayExecutionSettings) -> Response {
    handle_request_inner_with_adapter_base_url_override_and_limits(
        state.clone(),
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true
        })),
        Protocol::ChatCompletions,
        None,
        settings,
        0,
    )
    .await
}

fn bearer(token: &str) -> HeaderMap {
    HeaderMap::from_iter([(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).expect("valid test header"),
    )])
}

#[tokio::test]
async fn authenticated_key_scope_is_read_from_the_stored_record() {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    seed_api_key_with_scope(
        &database.db,
        "scoped-key",
        "Scoped",
        "scoped-token",
        &["provider-a"],
        &["coding", "gpt-4*"],
    )
    .await
    .expect("seed scoped key");
    seed_api_key(&database.db, "open-key", "Open", "open-token")
        .await
        .expect("seed unscoped key");

    let client = authenticate_client(&state, &bearer("scoped-token"))
        .await
        .expect("authenticated scoped key");
    assert_eq!(client.key_id, "scoped-key");
    assert!(client.scope.allows_provider("provider-a"));
    assert!(!client.scope.allows_provider("provider-b"));
    assert!(client.scope.allows_model("coding"));
    assert!(client.scope.allows_model("gpt-4o"));
    assert!(!client.scope.allows_model("claude-3-5-sonnet"));

    let client = authenticate_client(&state, &bearer("open-token"))
        .await
        .expect("authenticated unscoped key");
    assert!(client.scope.is_unrestricted());
}

#[tokio::test]
async fn a_corrupt_stored_scope_fails_authentication_closed() {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    seed_api_key_with_scope(
        &database.db,
        "corrupt-key",
        "Corrupt",
        "corrupt-token",
        &[],
        &["coding"],
    )
    .await
    .expect("seed scoped key");
    database
        .db
        .write(|transaction| {
            let mut record = transaction
                .get::<Record>(Table::ApiKeys, "corrupt-key")?
                .ok_or(StorageError::NotFound)?;
            record.insert("allowed_models", Field::Text("{".to_owned()));
            transaction.put(Table::ApiKeys, "corrupt-key", &record)
        })
        .await
        .expect("corrupt the stored scope");

    let response = match authenticate_client(&state, &bearer("corrupt-token")).await {
        Ok(_) => panic!("a corrupt scope must not authenticate"),
        Err(response) => response,
    };
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn provider_scope_denies_a_route_without_an_allowed_target() {
    let first_calls = Arc::new(AtomicUsize::new(0));
    let second_calls = Arc::new(AtomicUsize::new(0));
    let first_address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_completion_upstream))
            .with_state(first_calls.clone()),
    )
    .await;
    let second_address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_completion_upstream))
            .with_state(second_calls.clone()),
    )
    .await;
    let (_database, state) = scoped_harness(
        first_address,
        second_address,
        &[("provider-a", "model-a", "chat_completions", 0)],
    )
    .await;

    let response = chat_request(&state, settings(Some(scope(&["provider-b"], &[])))).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("denied body");
    let message = String::from_utf8_lossy(&body);
    assert!(
        message.contains("not allowed to use any provider"),
        "{message}"
    );
    assert_eq!(first_calls.load(Ordering::Relaxed), 0);
    assert_eq!(second_calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn provider_scope_falls_over_past_targets_it_may_not_use() {
    let first_calls = Arc::new(AtomicUsize::new(0));
    let second_calls = Arc::new(AtomicUsize::new(0));
    let first_address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_completion_upstream))
            .with_state(first_calls.clone()),
    )
    .await;
    let second_address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_completion_upstream))
            .with_state(second_calls.clone()),
    )
    .await;
    let (_database, state) = scoped_harness(
        first_address,
        second_address,
        &[
            ("provider-a", "model-a", "chat_completions", 0),
            ("provider-b", "model-b", "chat_completions", 1),
        ],
    )
    .await;

    let response = chat_request(&state, settings(Some(scope(&["provider-b"], &[])))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(first_calls.load(Ordering::Relaxed), 0);
    assert_eq!(second_calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn model_rules_match_exact_names_and_prefixes() {
    let calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_completion_upstream))
            .with_state(calls.clone()),
    )
    .await;
    let (_database, state) = scoped_harness(
        address,
        address,
        &[("provider-a", "model-a", "chat_completions", 0)],
    )
    .await;

    let allowed = chat_request(&state, settings(Some(scope(&[], &["cod*"])))).await;
    assert_eq!(allowed.status(), StatusCode::OK);
    let allowed = chat_request(&state, settings(Some(scope(&[], &["coding"])))).await;
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::Relaxed), 2);

    // Exact rules stay exact: `codi` is not a prefix rule.
    let denied = chat_request(&state, settings(Some(scope(&[], &["codi"])))).await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let body = to_bytes(denied.into_body(), 64 * 1024)
        .await
        .expect("denied body");
    assert!(
        String::from_utf8_lossy(&body).contains("not allowed to use model 'coding'"),
        "denied message should name the model"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn streaming_requests_are_scoped_before_any_upstream_call() {
    let calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_stream_upstream))
            .with_state(calls.clone()),
    )
    .await;
    let (_database, state) = scoped_harness(
        address,
        address,
        &[("provider-a", "model-a", "chat_completions", 0)],
    )
    .await;

    let allowed = streaming_chat_request(&state, settings(Some(scope(&[], &["cod*"])))).await;
    assert_eq!(allowed.status(), StatusCode::OK);
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    let denied = streaming_chat_request(&state, settings(Some(scope(&[], &["other"])))).await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn continuity_work_keeps_the_scope_of_the_authenticated_key() {
    let calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_completion_upstream))
            .with_state(calls.clone()),
    )
    .await;
    let (_database, state) = scoped_harness(
        address,
        address,
        &[("provider-a", "model-a", "chat_completions", 0)],
    )
    .await;

    // Continuity work clears the client key id because the background stream
    // owns attribution; the scope snapshot must still be enforced.
    let mut continuity = settings(Some(scope(&["provider-b"], &[])));
    continuity.api_key_id = None;
    let denied = chat_request(&state, continuity).await;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn models_listing_hides_routes_and_aliases_outside_the_key_scope() {
    let calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_completion_upstream))
            .with_state(calls.clone()),
    )
    .await;
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.admin_key = Some("a-strong-admin-password-for-tests".to_owned());
    let state = AppState::new(config, database.db.clone());
    for (id, prefix) in [("provider-a", "mock"), ("provider-b", "other")] {
        seed_provider(
            &database.db,
            ProviderSeed {
                id,
                name: id,
                base_url: &format!("http://{address}/v1"),
                adapter_id: "generic",
                auth_type: "none",
                model_prefix: prefix,
                preferred_protocol: "chat_completions",
                supported_protocols: &["chat_completions"],
            },
        )
        .await
        .expect("seed provider");
    }
    for (route_id, provider_id) in [
        ("coding-a", "provider-a"),
        ("coding-b", "provider-b"),
        ("other-a", "provider-a"),
    ] {
        seed_route(
            &database.db,
            route_id,
            route_id,
            &["chat_completions"],
            &[(provider_id, "model-a", "chat_completions", 0)],
        )
        .await
        .expect("seed route");
    }
    seed_provider_model(&database.db, "provider-a", "model-a")
        .await
        .expect("seed provider-a model");
    seed_provider_model(&database.db, "provider-b", "model-b")
        .await
        .expect("seed provider-b model");
    seed_api_key_with_scope(
        &database.db,
        "scoped-key",
        "Scoped",
        "scoped-token",
        &["provider-a"],
        &["coding*", "mock/*"],
    )
    .await
    .expect("seed scoped key");

    let response = crate::app::router(state)
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("authorization", "Bearer scoped-token")
                .body(Body::empty())
                .expect("models request"),
        )
        .await
        .expect("models response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("models body");
    let payload: Value = serde_json::from_slice(&body).expect("models JSON");
    let mut ids: Vec<String> = payload["data"]
        .as_array()
        .expect("model list")
        .iter()
        .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
        .collect();
    ids.sort();
    assert_eq!(ids, ["coding-a", "mock/model-a"]);
}
