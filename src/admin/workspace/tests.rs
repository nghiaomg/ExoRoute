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
async fn workspace_chat_reports_the_provider_diagnostic_for_an_undecodable_body() {
    let database = TestDatabase::open().await;
    // A provider that answers HTTP 200 with its own error payload must not
    // reach the admin as an opaque decode failure: the relay keeps the
    // decoder's summary and the provider's sanitized diagnostic.
    let upstream = spawn_upstream(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            axum::Json(json!({
                "error": {"message": "model 'sample-model' is not available on this plan"}
            }))
        }),
    ))
    .await;
    let state = seed_chat_provider(&database, upstream).await;

    let response = chat(state, workspace_input()).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let payload = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    let value: Value = serde_json::from_slice(&payload).expect("json response");
    assert_eq!(
        value["error"]["message"],
        "could not decode provider response: chat response has no choices (provider said: model 'sample-model' is not available on this plan)"
    );
}

#[tokio::test]
async fn workspace_chat_names_the_payload_shape_of_a_protocol_mismatch() {
    let database = TestDatabase::open().await;
    // The model is configured for Chat Completions but the provider answers a
    // Messages payload (the model speaks another protocol upstream). The relay
    // must say so instead of only reporting a missing field.
    let upstream = spawn_upstream(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            axum::Json(json!({
                "id": "msg-1",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "text", "text": "hello"}],
                "stop_reason": "end_turn"
            }))
        }),
    ))
    .await;
    let state = seed_chat_provider(&database, upstream).await;

    let response = chat(state, workspace_input()).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let payload = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    let value: Value = serde_json::from_slice(&payload).expect("json response");
    assert_eq!(
        value["error"]["message"],
        "could not decode provider response: chat response has no choices (provider returned a messages payload; check this model's upstream protocol)"
    );
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

#[tokio::test]
async fn workspace_chat_replays_history_and_generation_options() {
    let database = TestDatabase::open().await;
    let model = "sample-model";
    let upstream = spawn_upstream(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(move |axum::Json(body): axum::Json<Value>| async move {
            // The canonical history must arrive in order, before the turn
            // being asked, and the sampling knobs must reach the provider.
            let messages = body["messages"].as_array().expect("messages");
            assert_eq!(messages.len(), 4, "system, two history turns, current turn");
            assert_eq!(messages[0]["role"], "system");
            assert_eq!(messages[1]["role"], "user");
            assert_eq!(messages[1]["content"], "first question");
            assert_eq!(messages[2]["role"], "assistant");
            assert_eq!(messages[2]["content"], "first answer");
            assert_eq!(messages[3]["role"], "user");
            assert_eq!(messages[3]["content"], "follow up");
            // Sampling values travel as f32, so compare numerically.
            let temperature = body["temperature"].as_f64().expect("temperature");
            assert!(
                (temperature - 0.4).abs() < 1e-6,
                "temperature: {temperature}"
            );
            let top_p = body["top_p"].as_f64().expect("top_p");
            assert!((top_p - 0.9).abs() < 1e-6, "top_p: {top_p}");
            assert_eq!(body["max_tokens"], 256);
            assert_eq!(body["stream"], false);
            axum::Json(serde_json::from_str::<Value>(&upstream_ok(model)).unwrap())
        }),
    ))
    .await;
    let state = seed_chat_provider(&database, upstream).await;

    let mut input = workspace_input();
    input["message"] = json!("follow up");
    input["history"] = json!([
        {"role": "user", "text": "first question"},
        {"role": "assistant", "text": "first answer"},
    ]);
    input["temperature"] = json!(0.4);
    input["top_p"] = json!(0.9);
    input["max_tokens"] = json!(256);

    let response = chat(state, input).await;
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn workspace_chat_rejects_out_of_range_options_and_unbounded_history() {
    let database = TestDatabase::open().await;
    let state = seed_chat_provider(&database, "127.0.0.1:1".parse().unwrap()).await;
    let turn_limit = super::input::MAX_CHAT_HISTORY_TURNS;
    let too_long = "x".repeat(super::input::MAX_CHAT_HISTORY_CHARS);

    let cases: Vec<(&str, Value, &str)> = vec![
        (
            "temperature",
            json!(2.5),
            "temperature must be a number between 0 and 2",
        ),
        (
            "top_p",
            json!(1.25),
            "top_p must be a number between 0 and 1",
        ),
        (
            "max_tokens",
            json!(0),
            "max_tokens must be an integer between 1 and",
        ),
        ("history", json!("nope"), "history must be an array"),
        (
            "history",
            json!([{"role": "system", "text": "x"}]),
            "history role must be user or assistant",
        ),
        (
            "history",
            json!([{"role": "user", "text": "   "}]),
            "history text must be a non-empty string",
        ),
        (
            "history",
            Value::Array(
                (0..turn_limit + 1)
                    .map(|_| json!({"role": "user", "text": "turn"}))
                    .collect(),
            ),
            "history exceeds the 40 turn limit",
        ),
        (
            "history",
            json!([{"role": "user", "text": too_long}]),
            "history message exceeds the",
        ),
    ];

    for (key, value, expected) in cases {
        let mut input = workspace_input();
        input[key] = value;
        let response = chat(state.clone(), input).await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "case {key} must be rejected"
        );
        let payload = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("error body");
        let text = String::from_utf8_lossy(&payload);
        assert!(
            text.contains(expected),
            "case {key}: expected '{expected}' in {text}"
        );
    }
}

/// Splits one SSE body into `(event, data)` frames for assertions.
fn sse_frames(body: &str) -> Vec<(String, Value)> {
    let mut frames = Vec::new();
    for frame in body.split(
        "

",
    ) {
        let mut event = String::new();
        let mut data = String::new();
        for line in frame.lines() {
            if let Some(name) = line.strip_prefix("event: ") {
                event = name.to_owned();
            }
            if let Some(payload) = line.strip_prefix("data: ") {
                data = payload.to_owned();
            }
        }
        if !event.is_empty() {
            frames.push((event, serde_json::from_str(&data).unwrap_or(Value::Null)));
        }
    }
    frames
}

async fn stream_chat(state: AppState, body: Value) -> axum::response::Response {
    match super::stream::stream_workspace_chat(AxumState(state), Json(body)).await {
        Ok(response) => response.into_response(),
        Err((status, payload)) => (status, payload).into_response(),
    }
}

#[tokio::test]
async fn workspace_chat_stream_relays_deltas_and_done_without_persisting() {
    let database = TestDatabase::open().await;
    let model = "sample-model";
    let upstream = spawn_upstream(
        axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |axum::Json(body): axum::Json<Value>| async move {
                // The streaming relay must ask the provider for an event
                // stream, not a one-shot completion.
                assert_eq!(body["stream"], true);
                assert_eq!(body["model"], model);
                let frames_body = [
                    "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}",
                    "",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}",
                    "",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}",
                    "",
                    &format!(
                        "data: {{\"model\":\"{model}\",\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":3,\"completion_tokens\":2}}}}"
                    ),
                    "",
                    "data: [DONE]",
                    "",
                ]
                .join("
");
                crate::support::mock_upstream::sse_response(frames_body)
            }),
        ),
    )
    .await;
    let state = seed_chat_provider(&database, upstream).await;

    let response = stream_chat(state.clone(), workspace_input()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::trim),
        Some("text/event-stream")
    );
    let payload = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("stream body");
    let frames = sse_frames(std::str::from_utf8(&payload).expect("utf-8 stream"));

    let text: String = frames
        .iter()
        .filter(|(event, _)| event == "text-delta")
        .filter_map(|(_, payload)| payload["delta"].as_str())
        .collect();
    assert_eq!(text, "Hello");

    let done = frames
        .iter()
        .find(|(event, _)| event == "done")
        .map(|(_, payload)| payload.clone())
        .expect("terminal done event");
    assert_eq!(done["model"], model);
    assert_eq!(done["finish_reason"], "stop");
    assert_eq!(done["input_tokens"], 3);
    assert_eq!(done["output_tokens"], 2);
    assert!(done["duration_ms"].is_i64());
    assert!(
        !frames.iter().any(|(event, _)| event == "error"),
        "a completed stream must not carry an error event"
    );

    // The no-persistence contract covers the streaming path too.
    let request_logs = database
        .db
        .read(|transaction| transaction.scan_prefix::<Vec<u8>>(Table::RequestLogs, "", 100))
        .await
        .expect("request logs scan");
    assert!(request_logs.is_empty());
    let counters = state.telemetry.drop_counters();
    assert_eq!(counters.total(), 0);
}

#[tokio::test]
async fn workspace_chat_stream_reports_a_truncated_upstream_as_an_error_event() {
    let database = TestDatabase::open().await;
    let upstream = spawn_upstream(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            // A delta arrives, then the stream ends without any terminal
            // event: the dashboard must receive a terminal error, not a
            // silently complete turn.
            crate::support::mock_upstream::sse_response(
                "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}

",
            )
        }),
    ))
    .await;
    let state = seed_chat_provider(&database, upstream).await;

    let response = stream_chat(state, workspace_input()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let payload = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("stream body");
    let frames = sse_frames(std::str::from_utf8(&payload).expect("utf-8 stream"));
    assert!(frames.iter().any(|(event, _)| event == "text-delta"));
    let error = frames
        .iter()
        .find(|(event, _)| event == "error")
        .map(|(_, payload)| payload.clone())
        .expect("terminal error event");
    assert!(
        error["message"]
            .as_str()
            .unwrap_or_default()
            .contains("ended without a completed response"),
        "error: {error}"
    );
    assert!(
        !frames.iter().any(|(event, _)| event == "done"),
        "a failed stream must not report done"
    );
}

#[tokio::test]
async fn workspace_chat_stream_reports_the_provider_diagnostic_for_a_json_response() {
    let database = TestDatabase::open().await;
    // A provider that answers a streaming request with a plain JSON error body
    // must not hide its own diagnostic behind the framing mismatch.
    let upstream = spawn_upstream(axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            axum::Json(json!({
                "error": {"message": "streaming is not supported for this model"}
            }))
        }),
    ))
    .await;
    let state = seed_chat_provider(&database, upstream).await;

    let response = stream_chat(state, workspace_input()).await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let payload = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("response body");
    let value: Value = serde_json::from_slice(&payload).expect("json response");
    assert_eq!(
        value["error"]["message"],
        "provider 'provider' returned a non-SSE response to a streaming request (provider said: streaming is not supported for this model)"
    );
}

#[tokio::test]
async fn workspace_chat_stream_rejects_invalid_targets_before_streaming() {
    let database = TestDatabase::open().await;
    let state = seed_chat_provider(&database, "127.0.0.1:1".parse().unwrap()).await;
    let mut input = workspace_input();
    input["model"] = json!("not-saved");
    let response = stream_chat(state, input).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
