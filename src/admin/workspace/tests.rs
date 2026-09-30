//! Workspace chat relay tests: picker shape, relay success, and the
//! no-persistence contract.

use super::*;
use crate::infra::storage::Table;
use crate::support::mock_upstream::{spawn_upstream, sse_response};
use crate::support::test_support::{TestDatabase, seed_provider, seed_provider_model};
use axum::extract::State as AxumState;
use axum::http::StatusCode;
use axum::response::IntoResponse;

fn workspace_input() -> Value {
    json!({
        "provider_id": "provider",
        "model": "sample-model",
        "message": "hello",
        "system_prompt": "you are concise",
        "thinking_mode": "default",
    })
}

async fn chat(state: AppState, body: Value) -> axum::response::Response {
    send_workspace_chat(AxumState(state), Json(body))
        .await
        .into_response()
}

fn upstream_ok(model: &str) -> String {
    format!(
        r#"{{"id":"chatcmpl-1","model":"{model}","choices":[{{"message":{{"role":"assistant","content":"workspace reply"}},"finish_reason":"stop"}}],"usage":{{"prompt_tokens":3,"completion_tokens":5}}}}"#
    )
}

/// Seeds one enabled generic provider with the mock upstream base URL and one
/// saved model, the minimal configuration the relay reads.
async fn seed_chat_provider(database: &TestDatabase, address: std::net::SocketAddr) -> AppState {
    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    seed_provider(
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
    seed_provider_model(&database.db, "provider", "sample-model")
        .await
        .expect("seed provider model");
    state
}

#[tokio::test]
async fn workspace_chat_validation_rejects_empty_and_unsaved_models() {
    let database = TestDatabase::open().await;
    let state = seed_chat_provider(&database, "127.0.0.1:1".parse().unwrap()).await;

    // Missing message.
    let response = chat(
        state.clone(),
        json!({"provider_id": "provider", "model": "sample-model"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Empty message.
    let response = chat(
        state.clone(),
        json!({"provider_id": "provider", "model": "sample-model", "message": "   "}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Unknown thinking mode.
    let response = chat(
        state.clone(),
        json!({
            "provider_id": "provider",
            "model": "sample-model",
            "message": "hello",
            "thinking_mode": "aggressive",
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Override without override text.
    let response = chat(
        state.clone(),
        json!({
            "provider_id": "provider",
            "model": "sample-model",
            "message": "hello",
            "thinking_mode": "override",
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // A model that is not saved for the provider must be rejected before any
    // upstream call.
    let response = chat(
        state.clone(),
        json!({
            "provider_id": "provider",
            "model": "not-saved",
            "message": "hello",
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // An unknown provider is a 404 as well.
    let response = chat(
        state,
        json!({
            "provider_id": "missing",
            "model": "sample-model",
            "message": "hello",
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workspace_chat_relays_a_completion_without_persisting_anything() {
    let database = TestDatabase::open().await;
    let model = "sample-model";
    let upstream = spawn_upstream(
        axum::Router::new()
            .route(
                "/v1/chat/completions",
                axum::routing::post(move |axum::Json(body): axum::Json<Value>| async move {
                    // The relay must send the system prompt and the plain
                    // user text through the generic adapter untouched.
                    assert_eq!(body["model"], model);
                    let messages = body["messages"].as_array().unwrap();
                    assert_eq!(messages[0]["role"], "system");
                    assert_eq!(messages[0]["content"], "you are concise");
                    assert_eq!(messages[1]["role"], "user");
                    assert_eq!(messages[1]["content"], "hello");
                    axum::Json(serde_json::from_str::<Value>(&upstream_ok(model)).unwrap())
                }),
            )
            .route(
                "/v1/nonexistent",
                axum::routing::post(|| async { sse_response("data: [DONE]\n\n") }),
            ),
    )
    .await;
    let state = seed_chat_provider(&database, upstream).await;

    let response = chat(state.clone(), workspace_input()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    let value: Value = serde_json::from_slice(&payload).expect("json response");
    assert_eq!(value["reply"], "workspace reply");
    assert_eq!(value["finish_reason"], "stop");
    assert_eq!(value["usage"]["input_tokens"], 3);
    assert_eq!(value["usage"]["output_tokens"], 5);
    assert!(value["reasoning"].is_null());

    // The no-persistence contract: a relayed chat writes no request log, no
    // telemetry counters, and no provider credential state.
    let request_logs = database
        .db
        .read(|transaction| transaction.scan_prefix::<Vec<u8>>(Table::RequestLogs, "", 100))
        .await
        .expect("request logs scan");
    assert!(
        request_logs.is_empty(),
        "workspace chat must not create request log records"
    );
    let counters = state.telemetry.drop_counters();
    assert_eq!(
        counters.total(),
        0,
        "workspace chat must not enqueue telemetry"
    );
    let provider = database
        .db
        .read(|transaction| {
            transaction.get::<crate::infra::storage::Record>(Table::Providers, "provider")
        })
        .await
        .expect("provider read")
        .expect("provider row");
    assert_eq!(provider.integer("invalid_api_key_count").unwrap(), 0);
}

#[tokio::test]
async fn workspace_chat_models_lists_prefix_model_ids_and_skips_disabled() {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    let state = AppState::new(config, database.db.clone());
    seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "enabled",
            name: "Enabled provider",
            base_url: "https://upstream.example/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "ocg",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed enabled provider");
    seed_provider_model(&database.db, "enabled", "model-b")
        .await
        .expect("seed model-b");
    seed_provider_model(&database.db, "enabled", "model-a")
        .await
        .expect("seed model-a");
    // A disabled provider whose models must not be offered.
    seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "disabled",
            name: "Disabled provider",
            base_url: "https://upstream.example/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "off",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed disabled provider");
    seed_provider_model(&database.db, "disabled", "hidden-model")
        .await
        .expect("seed hidden-model");
    let disabled_prefix = "off";
    database
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<crate::infra::storage::Record>(Table::Providers, "disabled")?
                .ok_or(crate::infra::storage::StorageError::NotFound)?;
            provider.insert("enabled", crate::infra::storage::Field::Bool(false));
            transaction.put(Table::Providers, "disabled", &provider)
        })
        .await
        .expect("disable provider");
    let _ = disabled_prefix;

    let ApiResult::Ok(Json(value)) = workspace_chat_models(AxumState(state)).await else {
        panic!("model picker request should succeed");
    };
    let models = value["models"].as_array().expect("models array");
    assert_eq!(models.len(), 2, "disabled providers must be skipped");
    assert_eq!(models[0]["id"], "ocg/model-a");
    assert_eq!(models[1]["id"], "ocg/model-b");
    assert_eq!(models[0]["provider_id"], "enabled");
    assert_eq!(value["truncated"], false);
}

#[tokio::test]
async fn workspace_chat_rejects_unsupported_document_kinds_for_images() {
    let database = TestDatabase::open().await;
    let state = seed_chat_provider(&database, "127.0.0.1:1".parse().unwrap()).await;
    let response = chat(
        state,
        json!({
            "provider_id": "provider",
            "model": "sample-model",
            "message": "describe",
            "attachments": [{
                "kind": "image",
                "media_type": "application/pdf",
                "data": "aGVsbG8=",
            }],
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
