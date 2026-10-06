//! Provider responses that cannot be decoded must reach the client with the
//! provider's own diagnostic, not only the decoder's shape summary.

use super::support::*;
use super::*;

#[tokio::test]
async fn undecodable_provider_body_reaches_the_client_with_its_diagnostic() {
    // A provider that answers with HTTP 200 and its own error payload used to
    // surface to clients as "chat response has no choices" with no hint about
    // what the provider reported.
    let address = spawn_upstream(Router::new().route(
        "/v1/chat/completions",
        post(|| async {
            Json(json!({"error": {"message": "model is not available on this plan"}}))
        }),
    ))
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
        system_prompt_request("coding", false),
        Protocol::ChatCompletions,
        None,
        retry_execution_settings(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("gateway response body");
    let payload: Value = serde_json::from_slice(&body).expect("gateway JSON");
    assert_eq!(
        payload["error"]["message"],
        "could not decode provider response: chat response has no choices (provider said: model is not available on this plan)"
    );
}
