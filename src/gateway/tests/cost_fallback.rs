use super::support::{retry_execution_settings, seed_http_provider, spawn_upstream, sse_response};
use super::*;
use crate::config::provider_catalog::{ModelPricingConfig, estimate_cost_micro_usd};
use crate::protocol::Usage;
use crate::support::test_support::{TestDatabase, seed_route};
use axum::{Router, routing::post};

#[tokio::test]
async fn upstream_cost_is_kept_and_missing_cost_is_estimated_from_catalog() {
    let database = TestDatabase::open().await;
    let path = database.config().app_dir.join("providers.yaml");
    std::fs::write(
        &path,
        "providers:\n  p:\n    model_overrides:\n      m:\n        pricing: { input_per_million: 2.5, output_per_million: 10.0 }\n",
    )
    .expect("write catalog");
    let state = AppState::new(database.config(), database.db.clone());

    let upstream_cost = Some(42_i64);
    let usage_with_cost = Usage {
        input_tokens: 1_000_000,
        output_tokens: 500_000,
        cached_tokens: 0,
        cost_micro_usd: upstream_cost,
        cache_input_tokens: 0,
    };
    assert_eq!(usage_with_cost.cost_micro_usd, upstream_cost);

    let mut usage_without_cost = usage_with_cost.clone();
    usage_without_cost.cost_micro_usd = None;
    let pricing = state
        .resolve_model_pricing("p", "generic", "m")
        .expect("catalog pricing");
    let estimated = estimate_cost_micro_usd(
        &pricing,
        usage_without_cost.input_tokens,
        usage_without_cost.output_tokens,
        usage_without_cost.cached_tokens,
        usage_without_cost
            .cache_input_tokens
            .saturating_sub(usage_without_cost.cached_tokens),
        0,
    );
    assert_eq!(estimated, Some(7_500_000));

    // No matching entry preserves the missing-cost telemetry semantics.
    assert!(
        state
            .resolve_model_pricing("unknown", "unknown", "m")
            .is_none()
    );
    let free = ModelPricingConfig {
        input_per_million: Some(0.0),
        ..ModelPricingConfig::default()
    };
    assert_eq!(estimate_cost_micro_usd(&free, 10, 10, 0, 0, 0), Some(0));
}

async fn cost_state(upstream: Router<()>, with_pricing: bool) -> (TestDatabase, AppState) {
    let address = spawn_upstream(upstream).await;
    let database = TestDatabase::open().await;
    let yaml = if with_pricing {
        "providers:\n  provider:\n    model_overrides:\n      mock-model:\n        pricing: { input_per_million: 2.5, output_per_million: 10.0 }\n"
    } else {
        "providers: {}\n"
    };
    std::fs::write(database.config().app_dir.join("providers.yaml"), yaml)
        .expect("write provider catalog");
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    seed_http_provider(
        &database.db,
        format!("http://{address}/v1"),
        "chat_completions",
        &["chat_completions"],
    )
    .await;
    seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "mock-model", "chat_completions", 0)],
    )
    .await
    .expect("seed route");
    (database, state)
}

async fn no_cost_upstream() -> axum::response::Response {
    axum::Json(json!({
        "id":"cost-test", "model":"mock-model",
        "choices":[{"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":1_000_000,"completion_tokens":500_000}
    }))
    .into_response()
}

async fn upstream_cost_response() -> axum::response::Response {
    axum::Json(json!({
        "id":"cost-test", "model":"mock-model",
        "choices":[{"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":1_000_000,"completion_tokens":500_000,"cost":0.042}
    }))
    .into_response()
}

async fn no_cost_stream_upstream() -> axum::response::Response {
    sse_response(concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":1000000,\"completion_tokens\":500000}}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    ))
}

async fn logged_cost(database: &TestDatabase, state: &AppState) -> Option<i64> {
    state.telemetry.flush().await.expect("flush telemetry");
    let logs = database
        .db
        .read(|transaction| transaction.scan_prefix::<Record>(Table::RequestLogs, "", 16))
        .await
        .expect("read logs");
    let (_, record) = logs.first().expect("one request log");
    record
        .optional_integer("cost_micro_usd")
        .expect("cost field")
}

#[tokio::test]
async fn fallback_cost_paths_preserve_upstream_and_fill_missing() {
    non_streaming_cost_assertions().await;
    streaming_cost_assertions().await;
}

async fn non_streaming_cost_assertions() {
    let (database, state) = cost_state(
        Router::new().route("/v1/chat/completions", post(no_cost_upstream)),
        true,
    )
    .await;
    let response = execute_with_server_retries(
        state.clone(),
        HeaderMap::new(),
        Arc::new(json!({"model":"coding","messages":[{"role":"user","content":"hello"}]})),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(logged_cost(&database, &state).await, Some(7_500_000));

    let (database2, state2) = cost_state(
        Router::new().route("/v1/chat/completions", post(upstream_cost_response)),
        true,
    )
    .await;
    let response2 = execute_with_server_retries(
        state2.clone(),
        HeaderMap::new(),
        Arc::new(json!({"model":"coding","messages":[{"role":"user","content":"hello"}]})),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response2.status(), StatusCode::OK);
    assert_eq!(logged_cost(&database2, &state2).await, Some(42_000));
}

async fn streaming_cost_assertions() {
    let (database, state) = cost_state(
        Router::new().route("/v1/chat/completions", post(no_cost_stream_upstream)),
        true,
    )
    .await;
    let response = execute_with_server_retries(
        state.clone(),
        HeaderMap::new(),
        Arc::new(
            json!({"model":"coding","messages":[{"role":"user","content":"hello"}],"stream":true}),
        ),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("stream body");
    assert!(String::from_utf8_lossy(&body).contains("hi"));
    assert_eq!(logged_cost(&database, &state).await, Some(7_500_000));

    let (database2, state2) = cost_state(
        Router::new().route("/v1/chat/completions", post(no_cost_stream_upstream)),
        false,
    )
    .await;
    let response2 = execute_with_server_retries(
        state2.clone(),
        HeaderMap::new(),
        Arc::new(
            json!({"model":"coding","messages":[{"role":"user","content":"hello"}],"stream":true}),
        ),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response2.status(), StatusCode::OK);
    let _ = axum::body::to_bytes(response2.into_body(), 1024 * 1024).await;
    assert_eq!(logged_cost(&database2, &state2).await, None);
}
