//! Request-history coverage for the gateway.
//!
//! Statistics count a request when its analytics guard completes, which also
//! happens when a client disconnects and the response body is dropped. The
//! request history must record the same requests, or the dashboard totals and
//! the model breakdown describe different amounts of traffic.

use super::support::*;
use super::*;

async fn completed_stream_upstream() -> axum::response::Response {
    sse_response(COMPLETE_CHAT_STREAM)
}

async fn unfinished_stream_upstream() -> axum::response::Response {
    unfinished_sse_response(CHAT_USAGE_FRAME)
}

/// Seeds the `coding` route in front of `upstream`, with one generic provider.
async fn logging_state(upstream: Router<()>) -> (TestDatabase, AppState) {
    let address = spawn_upstream(upstream).await;
    let database = TestDatabase::open().await;
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
    crate::support::test_support::seed_route(
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

/// Runs one streaming gateway request the way the HTTP layer does: the response
/// body is wrapped so the analytics guard completes when the client finishes or
/// drops it.
async fn request_stream(state: &AppState) -> Response {
    let analytics = state.telemetry.start_request("logging-key".to_owned());
    let response = handle_request_inner_with_adapter_base_url_override_and_limits(
        state.clone(),
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
            operational_settings: OperationalSettings::default(),
            api_key_id: Some("logging-key".to_owned()),
            api_key_scope: None,
            analytics: Some(analytics.clone()),
            stream_continuity_retry: false,
        },
        0,
    )
    .await;
    track_analytics_response(response, analytics)
}

async fn stored_request_logs(database: &TestDatabase) -> Vec<Record> {
    database
        .db
        .read(|transaction| transaction.scan_prefix::<Record>(Table::RequestLogs, "", 64))
        .await
        .expect("read request logs")
        .into_iter()
        .map(|(_, record)| record)
        .collect()
}

#[tokio::test]
async fn completed_stream_records_one_request_log_with_its_usage() {
    let (database, state) =
        logging_state(Router::new().route("/v1/chat/completions", post(completed_stream_upstream)))
            .await;
    let response = request_stream(&state).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("streaming response body");
    assert!(String::from_utf8_lossy(&body).contains("hi"));
    state.telemetry.flush().await.expect("flush request logs");

    let logs = stored_request_logs(&database).await;
    assert_eq!(logs.len(), 1, "one completed stream records one log");
    assert_eq!(logs[0].integer("status").expect("log status"), 200);
    assert_eq!(logs[0].text("model").expect("log model"), "mock-model");
    assert_eq!(
        logs[0].optional_integer("input_tokens").expect("input"),
        Some(11)
    );
    assert_eq!(
        logs[0].optional_integer("output_tokens").expect("output"),
        Some(5)
    );
}

#[tokio::test]
async fn abandoned_stream_records_one_request_log_with_observed_usage() {
    let (database, state) = logging_state(
        Router::new().route("/v1/chat/completions", post(unfinished_stream_upstream)),
    )
    .await;
    let response = request_stream(&state).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut chunks = response.into_body().into_data_stream();
    assert!(
        chunks.next().await.is_some(),
        "the client reads the first translated frame"
    );
    // The client disconnects mid-stream: the translated stream is dropped
    // without ever reaching its terminal event.
    drop(chunks);
    state.telemetry.flush().await.expect("flush request logs");

    let logs = stored_request_logs(&database).await;
    assert_eq!(
        logs.len(),
        1,
        "an abandoned stream still records one log entry"
    );
    assert_eq!(
        logs[0].integer("status").expect("log status"),
        499,
        "a client disconnect is recorded as 499 Client Closed Request"
    );
    assert!(
        logs[0]
            .optional_text("error")
            .expect("log error")
            .is_some_and(|error| error.contains("client disconnected")),
        "the log explains why the request ended early"
    );
    assert_eq!(
        logs[0].optional_integer("input_tokens").expect("input"),
        Some(11)
    );
    assert_eq!(
        logs[0].optional_integer("output_tokens").expect("output"),
        Some(5)
    );
}

#[tokio::test]
async fn shutdown_interrupted_stream_records_one_unavailable_request_log() {
    let (database, state) = logging_state(
        Router::new().route("/v1/chat/completions", post(unfinished_stream_upstream)),
    )
    .await;
    let response = request_stream(&state).await;
    assert_eq!(response.status(), StatusCode::OK);
    state.request_shutdown();
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("shutdown response body");
    assert!(!String::from_utf8_lossy(&body).contains("event: error"));
    state.telemetry.flush().await.expect("flush request logs");

    let logs = stored_request_logs(&database).await;
    assert_eq!(logs.len(), 1, "a shutdown still records one log entry");
    assert_eq!(
        logs[0].integer("status").expect("log status"),
        503,
        "a shutdown-interrupted stream is recorded as unavailable"
    );
    assert!(
        logs[0]
            .optional_text("error")
            .expect("log error")
            .is_some_and(|error| error.contains("shutdown")),
        "the log explains the interruption"
    );
}

/// The invariant the dashboard depends on: rolled-up totals and the
/// request-history rows must describe the same requests for the same range.
#[tokio::test]
async fn statistics_totals_and_request_history_describe_the_same_requests() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (database, state) = logging_state(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(complete_then_unfinished_chat_stream),
            )
            .with_state(calls),
    )
    .await;

    let completed = request_stream(&state).await;
    to_bytes(completed.into_body(), 1024 * 1024)
        .await
        .expect("completed stream body");

    let abandoned = request_stream(&state).await;
    let mut chunks = abandoned.into_body().into_data_stream();
    assert!(chunks.next().await.is_some());
    drop(chunks);

    state.telemetry.flush().await.expect("flush telemetry");

    let snapshot =
        crate::infra::telemetry::statistics_snapshot(&database.db, "1d", 1440, &state.telemetry)
            .await
            .expect("statistics snapshot");
    assert_eq!(snapshot["total_requests"], 2);
    assert_eq!(snapshot["successes"], 1);
    assert_eq!(snapshot["failures"], 1, "the abandoned stream failed");
    assert_eq!(
        snapshot["logged_requests"], 2,
        "history covers every counted request"
    );
    assert_eq!(snapshot["model_breakdown_incomplete"], false);
    assert_eq!(snapshot["model_breakdown_truncated"], false);

    let rows = snapshot["model_breakdown"]
        .as_array()
        .expect("model breakdown rows");
    let input: i64 = rows
        .iter()
        .map(|row| row["input_tokens"].as_i64().expect("input tokens"))
        .sum();
    let output: i64 = rows
        .iter()
        .map(|row| row["output_tokens"].as_i64().expect("output tokens"))
        .sum();
    let requests: i64 = rows
        .iter()
        .map(|row| row["requests"].as_i64().expect("requests"))
        .sum();
    assert_eq!(
        requests,
        snapshot["total_requests"].as_i64().expect("total requests")
    );
    assert_eq!(
        input,
        snapshot["input_tokens"].as_i64().expect("input tokens")
    );
    assert_eq!(
        output,
        snapshot["output_tokens"].as_i64().expect("output tokens")
    );
    assert_eq!(input, 22, "both requests reported their usage");
    assert_eq!(output, 10);
}
