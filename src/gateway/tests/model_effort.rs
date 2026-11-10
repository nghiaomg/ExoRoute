use super::support::*;
use super::*;

/// Records the full Chat Completions body the gateway dispatches upstream.
async fn capture_effort_upstream(
    State(calls): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    calls.lock().await.push(body);
    Json(json!({
        "id":"effort-alias-success",
        "model":"gpt-test",
        "choices":[{"message":{"role":"assistant","content":"served"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":1,"completion_tokens":1}
    }))
    .into_response()
}

#[tokio::test]
async fn effort_suffix_reaches_the_upstream_request() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(capture_effort_upstream))
            .with_state(calls.clone()),
    )
    .await;

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
    seed_http_provider(
        &database.db,
        format!("http://{address}/v1"),
        "chat_completions",
        &["chat_completions"],
    )
    .await;
    crate::support::test_support::seed_provider_model(&database.db, "provider", "gpt-test")
        .await
        .expect("seed model");

    // `mock/gpt-test-high` resolves provider prefix `mock`, base model
    // `gpt-test`, and effort `high`; the upstream sees the stripped model and
    // the Chat Completions effort field.
    let response = execute_with_server_retries(
        state.clone(),
        HeaderMap::new(),
        Arc::new(json!({
            "model":"mock/gpt-test-high",
            "messages":[{"role":"user","content":"hello"}]
        })),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let observed = calls.lock().await.clone();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0]["model"], json!("gpt-test"));
    assert_eq!(observed[0]["reasoning_effort"], json!("high"));

    // The history row records the effective attempt level, not just a client hint.
    let logs = database
        .db
        .read(|transaction| transaction.scan_prefix::<crate::infra::storage::Record>(
            crate::infra::storage::Table::RequestLogs,
            "",
            10,
        ))
        .await
        .expect("read request logs");
    assert_eq!(logs[0].1.optional_text("reasoning_effort").expect("effort field"), Some("high"));

    // A client-supplied reasoning option wins over the model-name suffix.
    let response = execute_with_server_retries(
        state.clone(),
        HeaderMap::new(),
        Arc::new(json!({
            "model":"mock/gpt-test-max",
            "messages":[{"role":"user","content":"hello"}],
            "reasoning_effort":"low"
        })),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let observed = calls.lock().await.clone();
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[1]["model"], json!("gpt-test"));
    assert_eq!(observed[1]["reasoning_effort"], json!("low"));
    let logs = database
        .db
        .read(|transaction| transaction.scan_prefix::<crate::infra::storage::Record>(
            crate::infra::storage::Table::RequestLogs,
            "",
            10,
        ))
        .await
        .expect("read request logs");
    assert!(logs.iter().any(|(_, row)| row.optional_text("reasoning_effort").expect("effort field") == Some("low")));

    // An unrecognized suffix is not an effort hint and stays unroutable.
    let response = execute_with_server_retries(
        state.clone(),
        HeaderMap::new(),
        Arc::new(json!({
            "model":"mock/gpt-test-turbo",
            "messages":[{"role":"user","content":"hello"}]
        })),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(calls.lock().await.len(), 2);
}
