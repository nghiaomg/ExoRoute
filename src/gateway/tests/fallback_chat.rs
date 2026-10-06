use super::support::*;
use super::*;

#[tokio::test]
async fn fallback_targets_receive_the_client_system_prompt() {
    // The bug report: when a combo falls back from the primary model to a
    // fallback model, the system prompt was reported missing on the target
    // that served the request. Every fallback target must receive the same
    // canonical messages, including the system prompt.
    let upstream = Arc::new(CapturingUpstream::chat(false));
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
        format!("http://{address}/v1"),
        "chat_completions",
        &["chat_completions"],
    )
    .await;
    seed_chat_priority_combo(&database.db).await;

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
    assert_eq!(observed.len(), 3, "two targets failed, the third served");
    for (attempt, body) in observed.iter().enumerate() {
        let messages = body["messages"].as_array().expect("upstream messages");
        assert_eq!(
            messages.first().and_then(|message| message.get("role")),
            Some(&json!("system")),
            "attempt {attempt} lost the system message: {body}"
        );
        assert_eq!(
            messages.first().and_then(|message| message.get("content")),
            Some(&json!("you are a concise assistant")),
            "attempt {attempt} lost the system prompt text: {body}"
        );
    }
}

#[tokio::test]
async fn fallback_targets_receive_the_client_system_prompt_on_streams() {
    // Streaming variant of the fallback system-prompt guarantee: the target
    // that finally serves a retried stream must still receive the system
    // prompt from the original client request.
    let upstream = Arc::new(CapturingUpstream::chat(true));
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
        format!("http://{address}/v1"),
        "chat_completions",
        &["chat_completions"],
    )
    .await;
    seed_chat_priority_combo(&database.db).await;

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
    assert_eq!(observed.len(), 3, "two targets failed, the third served");
    for (attempt, body) in observed.iter().enumerate() {
        let messages = body["messages"].as_array().expect("upstream messages");
        assert_eq!(
            messages.first().and_then(|message| message.get("content")),
            Some(&json!("you are a concise assistant")),
            "streaming attempt {attempt} lost the system prompt: {body}"
        );
    }
}
