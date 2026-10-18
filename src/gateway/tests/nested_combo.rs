use super::support::*;
use super::*;
use crate::support::test_support::{RouteTargetSeed, seed_route_with_targets};

/// A mock upstream that records the requested model of every call.
async fn recording_upstream() -> (String, Arc<Mutex<Vec<String>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(recording_failover_upstream))
            .with_state(calls.clone()),
    )
    .await;
    (format!("http://{address}/v1"), calls)
}

/// One mock provider plus the state that routes to it. The database is
/// returned so it stays alive: the state holds a clone of the handle, not the
/// environment's ownership.
async fn chat_state_with_provider(base_url: &str) -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(10);
    let state = AppState::new(config, database.db.clone());
    seed_http_provider(
        &database.db,
        base_url.to_owned(),
        "chat_completions",
        &["chat_completions"],
    )
    .await;
    (database, state)
}

/// The circuit breaker is off for these requests: these tests assert how far
/// the target loop walks, and every target shares one mock provider, so the
/// default threshold of five failures would open the circuit mid-walk. The
/// breaker's own behavior is covered by `state::tests::circuit_breaker`.
fn execution_settings() -> GatewayExecutionSettings {
    GatewayExecutionSettings {
        resource_limits: GatewayResourceLimits::default(),
        operational_settings: OperationalSettings {
            request_timeout: Duration::from_secs(10),
            circuit_breaker_enabled: false,
            ..OperationalSettings::default()
        },
        api_key_id: None,
        api_key_scope: None,
        analytics: None,
        stream_continuity_retry: false,
    }
}

async fn send_chat(state: AppState, model: &str) -> axum::response::Response {
    execute_with_server_retries(
        state,
        HeaderMap::new(),
        Arc::new(json!({
            "model":model,
            "messages":[{"role":"user","content":"hello"}]
        })),
        Protocol::ChatCompletions,
        None,
        execution_settings(),
    )
    .await
}

fn provider_targets(models: &[String]) -> Vec<RouteTargetSeed<'_>> {
    models
        .iter()
        .map(|model| RouteTargetSeed::Provider {
            provider_id: "provider",
            model: model.as_str(),
            protocol: "chat_completions",
        })
        .collect()
}

#[tokio::test]
async fn nested_combo_targets_flatten_in_place_and_fail_over() {
    let (base_url, calls) = recording_upstream().await;
    let (database, state) = chat_state_with_provider(&base_url).await;

    // The child's own fallback must run where the parent referenced it: after
    // the parent's first target and before the parent's second one.
    seed_route_with_targets(
        &database.db,
        "child",
        "Child",
        &["chat_completions"],
        true,
        &provider_targets(&["model-child-fails".to_owned(), "model-child-ok".to_owned()]),
    )
    .await
    .expect("seed child combo");
    seed_route_with_targets(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        true,
        &[
            RouteTargetSeed::Provider {
                provider_id: "provider",
                model: "model-parent-a-fails",
                protocol: "chat_completions",
            },
            RouteTargetSeed::Combo { combo_id: "child" },
            RouteTargetSeed::Provider {
                provider_id: "provider",
                model: "model-parent-b",
                protocol: "chat_completions",
            },
        ],
    )
    .await
    .expect("seed parent combo");

    let response = send_chat(state, "coding").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        calls.lock().await.clone(),
        [
            "model-parent-a-fails",
            "model-child-fails",
            "model-child-ok"
        ],
        "the nested group belongs between the parent's first and second target"
    );
}

#[tokio::test]
async fn nested_combo_disabled_subtree_is_skipped() {
    let (base_url, calls) = recording_upstream().await;
    let (database, state) = chat_state_with_provider(&base_url).await;

    seed_route_with_targets(
        &database.db,
        "child",
        "Child",
        &["chat_completions"],
        false,
        &provider_targets(&["model-child-ok".to_owned()]),
    )
    .await
    .expect("seed disabled child combo");
    seed_route_with_targets(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        true,
        &[
            RouteTargetSeed::Combo { combo_id: "child" },
            RouteTargetSeed::Provider {
                provider_id: "provider",
                model: "model-parent-ok",
                protocol: "chat_completions",
            },
        ],
    )
    .await
    .expect("seed parent combo");

    let response = send_chat(state, "coding").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.lock().await.clone(), ["model-parent-ok"]);
}

#[tokio::test]
async fn nested_combo_protocol_mismatch_is_skipped() {
    let (base_url, calls) = recording_upstream().await;
    let (database, state) = chat_state_with_provider(&base_url).await;

    // The child would serve the request if it were reached; it is skipped
    // because it does not accept the client protocol.
    seed_route_with_targets(
        &database.db,
        "child",
        "Child",
        &["messages"],
        true,
        &provider_targets(&["model-child-ok".to_owned()]),
    )
    .await
    .expect("seed messages-only child combo");
    seed_route_with_targets(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        true,
        &[
            RouteTargetSeed::Combo { combo_id: "child" },
            RouteTargetSeed::Provider {
                provider_id: "provider",
                model: "model-parent-ok",
                protocol: "chat_completions",
            },
        ],
    )
    .await
    .expect("seed parent combo");

    let response = send_chat(state, "coding").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(calls.lock().await.clone(), ["model-parent-ok"]);
}

#[tokio::test]
async fn runtime_cycle_in_nested_reference_returns_503() {
    let (base_url, calls) = recording_upstream().await;
    let (database, state) = chat_state_with_provider(&base_url).await;

    // Written directly: the save path rejects a cycle, so this is the safety
    // net for a graph that reached the database another way.
    seed_route_with_targets(
        &database.db,
        "a",
        "A",
        &["chat_completions"],
        true,
        &[
            RouteTargetSeed::Provider {
                provider_id: "provider",
                model: "model-a-ok",
                protocol: "chat_completions",
            },
            RouteTargetSeed::Combo { combo_id: "b" },
        ],
    )
    .await
    .expect("seed cyclic combo a");
    seed_route_with_targets(
        &database.db,
        "b",
        "B",
        &["chat_completions"],
        true,
        &[RouteTargetSeed::Combo { combo_id: "a" }],
    )
    .await
    .expect("seed cyclic combo b");

    let response = send_chat(state, "a").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        calls.lock().await.is_empty(),
        "an unroutable graph must not reach any provider"
    );
}

#[tokio::test]
async fn expanded_overflow_returns_503_without_truncating() {
    let (base_url, calls) = recording_upstream().await;
    let (database, state) = chat_state_with_provider(&base_url).await;

    let child_models: Vec<String> = (0..crate::config::MAX_ROUTE_TARGETS)
        .map(|index| format!("model-child-{index}"))
        .collect();
    seed_route_with_targets(
        &database.db,
        "child",
        "Child",
        &["chat_completions"],
        true,
        &provider_targets(&child_models),
    )
    .await
    .expect("seed child combo");
    seed_route_with_targets(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        true,
        &[
            RouteTargetSeed::Combo { combo_id: "child" },
            RouteTargetSeed::Provider {
                provider_id: "provider",
                model: "model-parent-ok",
                protocol: "chat_completions",
            },
        ],
    )
    .await
    .expect("seed parent combo");

    let response = send_chat(state, "coding").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        calls.lock().await.is_empty(),
        "an oversized expansion must be refused, not served in part"
    );
}

#[tokio::test]
async fn gateway_routes_thirty_two_configured_targets() {
    let (base_url, calls) = recording_upstream().await;
    let (database, state) = chat_state_with_provider(&base_url).await;

    // Every target but the last fails, so the request only succeeds if all of
    // them are dispatched. A combo this size used to be rejected with a 503.
    let mut models: Vec<String> = (0..crate::config::MAX_ROUTE_TARGETS - 1)
        .map(|index| format!("model-fails-{index}"))
        .collect();
    models.push("model-last-ok".to_owned());
    seed_route_with_targets(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        true,
        &provider_targets(&models),
    )
    .await
    .expect("seed full-size combo");

    let response = send_chat(state, "coding").await;
    assert_eq!(response.status(), StatusCode::OK);
    let observed = calls.lock().await.clone();
    assert_eq!(observed.len(), crate::config::MAX_ROUTE_TARGETS);
    assert_eq!(observed.last().map(String::as_str), Some("model-last-ok"));
}
