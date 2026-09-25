use super::support::*;
use super::*;

#[tokio::test]
async fn fallback_to_a_messages_target_translates_the_system_prompt() {
    // Cross-protocol combo: the Chat Completions client system prompt must be
    // translated into the Anthropic `system` field of the fallback target.
    let upstream = Arc::new(CapturingUpstream::messages(false));
    let address = spawn_upstream(
        Router::new()
            .route(upstream.route_path(), post(capturing_fallback_upstream))
            .with_state(upstream.clone()),
    )
    .await;

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
    seed_http_provider(
        &database.db,
        format!("http://{address}"),
        "messages",
        &["messages"],
    )
    .await;
    seed_messages_combo(&database.db).await;

    let response = execute_with_server_retries(
        state,
        HeaderMap::new(),
        system_prompt_request("coding", false),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let observed = upstream.calls.lock().await.clone();
    assert_eq!(observed.len(), 2, "first target failed, the second served");
    assert_eq!(observed[0]["system"], "you are a concise assistant");
    assert_eq!(observed[1]["system"], "you are a concise assistant");
}

#[tokio::test]
async fn fallback_to_a_messages_stream_translates_the_system_prompt() {
    // Streaming cross-protocol fallback: the Chat Completions system prompt
    // must reach the Anthropic fallback target even when the request that
    // finally serves the client is a translated SSE stream.
    let upstream = Arc::new(CapturingUpstream::messages(true));
    let address = spawn_upstream(
        Router::new()
            .route(upstream.route_path(), post(capturing_fallback_upstream))
            .with_state(upstream.clone()),
    )
    .await;

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
    seed_http_provider(
        &database.db,
        format!("http://{address}"),
        "messages",
        &["messages"],
    )
    .await;
    seed_messages_combo(&database.db).await;

    let response = execute_with_server_retries(
        state,
        HeaderMap::new(),
        system_prompt_request("coding", true),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let observed = upstream.calls.lock().await.clone();
    assert_eq!(observed.len(), 2, "first target failed, the second served");
    assert_eq!(observed[0]["system"], "you are a concise assistant");
    assert_eq!(observed[1]["system"], "you are a concise assistant");
}
