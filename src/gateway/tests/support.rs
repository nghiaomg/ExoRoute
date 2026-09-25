use super::*;

pub(super) use crate::support::mock_upstream::{spawn_upstream, sse_response};

pub(super) async fn key_rotation_upstream(
    State(calls): State<Arc<AtomicUsize>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let authorization = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok());
    if authorization == Some("Bearer dead-key") {
        calls.fetch_add(1, Ordering::Relaxed);
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":{"message":"key expired"}})),
        )
            .into_response();
    }
    if authorization == Some("Bearer backup-key") {
        calls.fetch_add(1, Ordering::Relaxed);
        return Json(json!({
                "id":"upstream-id",
                "model":body["model"],
                "choices":[{"message":{"role":"assistant","content":"served by backup"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":2,"completion_tokens":3}
            })).into_response();
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":{"message":"missing credential"}})),
    )
        .into_response()
}

pub(super) async fn transient_server_error_upstream(
    State(calls): State<Arc<AtomicUsize>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"message":"temporary upstream failure"}})),
        )
            .into_response();
    }
    Json(json!({
            "id":"retry-success",
            "model":body["model"],
            "choices":[{"message":{"role":"assistant","content":"served after retry"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":1,"completion_tokens":2}
        }))
        .into_response()
}

pub(super) async fn rotating_priority_upstream(
    State(calls): State<Arc<Mutex<Vec<String>>>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let call_number = {
        let mut calls = calls.lock().await;
        calls.push(model.clone());
        calls.len()
    };
    if call_number <= 3 {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":{"message":"temporary upstream failure"}})),
        )
            .into_response();
    }
    Json(json!({
            "id":"rotated-retry-success",
            "model":model,
            "choices":[{"message":{"role":"assistant","content":"served after target rotation"},"finish_reason":"stop"}]
        }))
        .into_response()
}

pub(super) async fn transient_empty_completion_upstream(
    State(calls): State<Arc<AtomicUsize>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
        return Json(json!({
            "id":"empty-response",
            "model":body["model"],
            "choices":[{"message":{"role":"assistant","content":null},"finish_reason":"stop"}]
        }))
        .into_response();
    }
    Json(json!({
            "id":"empty-retry-success",
            "model":body["model"],
            "choices":[{"message":{"role":"assistant","content":"served after empty response"},"finish_reason":"stop"}]
        }))
        .into_response()
}

pub(super) async fn transient_empty_stream_upstream(
    State(calls): State<Arc<AtomicUsize>>,
    Json(_body): Json<Value>,
) -> axum::response::Response {
    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
        return sse_response("data: [DONE]\n\n");
    }
    sse_response(
        "data: {\"choices\":[{\"delta\":{\"content\":\"recovered after empty stream\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
    )
}

pub(super) async fn transient_stream_retryable_upstream(
    State(calls): State<Arc<AtomicUsize>>,
    Json(_body): Json<Value>,
) -> axum::response::Response {
    match calls.fetch_add(1, Ordering::Relaxed) {
        0 => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({"error":{"message":"rate limited"}})),
            )
                .into_response();
        }
        1 => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({"error":{"message":"temporary bad gateway"}})),
            )
                .into_response();
        }
        2 => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":{"message":"temporary unavailable"}})),
            )
                .into_response();
        }
        _ => {}
    }
    sse_response(
        "data: {\"choices\":[{\"delta\":{\"content\":\"recovered\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
    )
}

pub(super) async fn transient_stream_timeout_upstream(
    State(calls): State<Arc<AtomicUsize>>,
    Json(_body): Json<Value>,
) -> axum::response::Response {
    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"message":"late upstream failure"}})),
        )
            .into_response();
    }
    sse_response(
        "data: {\"choices\":[{\"delta\":{\"content\":\"recovered\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
    )
}

/// Records the credential of every upstream call so a test can assert which
/// provider key served each request.
pub(super) async fn round_robin_upstream(
    State(calls): State<Arc<Mutex<Vec<String>>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let credential = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing credential")
        .to_owned();
    calls.lock().await.push(credential);
    Json(json!({
        "id":"round-robin",
        "model":body["model"],
        "choices":[{"message":{"role":"assistant","content":"served"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":1,"completion_tokens":1}
    }))
    .into_response()
}

/// Rejects the credential of the provider's last key, so a rotation has to
/// continue past the end of the fallback order to serve the request.
pub(super) async fn round_robin_rejecting_last_key_upstream(
    State(calls): State<Arc<Mutex<Vec<String>>>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let credential = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("missing credential")
        .to_owned();
    calls.lock().await.push(credential.clone());
    if credential == "Bearer third-key" {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error":{"message":"key expired"}})),
        )
            .into_response();
    }
    Json(json!({
            "id":"round-robin-fallback",
            "model":body["model"],
            "choices":[{"message":{"role":"assistant","content":"served after wrap"},"finish_reason":"stop"}]
        }))
    .into_response()
}

pub(super) async fn store_key_strategy(
    state: &AppState,
    provider_id: &str,
    strategy: &str,
) -> Result<(), crate::infra::storage::StorageError> {
    let provider_id = provider_id.to_owned();
    let strategy = strategy.to_owned();
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(crate::infra::storage::StorageError::NotFound)?;
            provider.insert("key_strategy", Field::Text(strategy.clone()));
            transaction.put(Table::Providers, &provider_id, &provider)
        })
        .await
}

pub(super) async fn seed_round_robin_provider(
    database: &TestDatabase,
    address: std::net::SocketAddr,
    strategy: Option<&str>,
) -> AppState {
    let mut config = database.config();
    config.master_key = Some([53_u8; 32]);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "provider",
            name: "Mock provider",
            base_url: &format!("http://{address}/v1"),
            adapter_id: "generic",
            auth_type: "bearer",
            model_prefix: "mock",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    for (key_id, secret) in [
        ("key-1", "first-key"),
        ("key-2", "second-key"),
        ("key-3", "third-key"),
    ] {
        crate::support::test_support::seed_provider_key(
            &database.db,
            "provider",
            key_id,
            secret.as_bytes(),
            false,
        )
        .await
        .expect("seed provider key");
    }
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "mock-model", "chat_completions", 0)],
    )
    .await
    .expect("seed route");
    if let Some(strategy) = strategy {
        store_key_strategy(&state, "provider", strategy)
            .await
            .expect("store key strategy");
    }
    state
}

pub(super) async fn gateway_completion(state: &AppState) -> StatusCode {
    let response = handle_request_inner_with_adapter_base_url_override(
        state.clone(),
        HeaderMap::new(),
        json!({"model":"coding","messages":[{"role":"user","content":"hello"}]}),
        Protocol::ChatCompletions,
        None,
    )
    .await;
    response.status()
}

/// Configures resources and timeouts shared by the retry-oriented tests.
pub(super) fn retry_execution_settings() -> GatewayExecutionSettings {
    GatewayExecutionSettings {
        resource_limits: GatewayResourceLimits::default(),
        operational_settings: OperationalSettings {
            request_timeout: Duration::from_secs(2),
            ..OperationalSettings::default()
        },
        api_key_id: None,
        analytics: None,
        stream_continuity_retry: false,
    }
}

/// The canonical client request used by the system-prompt fallback tests.
pub(super) fn system_prompt_request(model: &str, streaming: bool) -> Arc<Value> {
    let mut body = json!({
        "model": model,
        "messages": [
            {"role":"system","content":"you are a concise assistant"},
            {"role":"user","content":"hello"}
        ]
    });
    if streaming {
        body["stream"] = json!(true);
    }
    Arc::new(body)
}

/// Seeds the single generic provider every fallback test talks to.
pub(super) async fn seed_http_provider(
    db: &crate::infra::storage::Database,
    base_url: String,
    preferred_protocol: &str,
    supported_protocols: &[&str],
) {
    crate::support::test_support::seed_provider(
        db,
        crate::support::test_support::ProviderSeed {
            id: "provider",
            name: "Mock provider",
            base_url: &base_url,
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "mock",
            preferred_protocol,
            supported_protocols,
        },
    )
    .await
    .expect("seed provider");
}

/// Two Chat Completions targets, so a failing first target rotates to the second.
pub(super) async fn seed_chat_priority_combo(db: &crate::infra::storage::Database) {
    crate::support::test_support::seed_route(
        db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[
            ("provider", "model-a", "chat_completions", 0),
            ("provider", "model-b", "chat_completions", 1),
        ],
    )
    .await
    .expect("seed priority combo");
}

/// One Anthropic Messages target behind a Chat Completions client route.
pub(super) async fn seed_messages_combo(db: &crate::infra::storage::Database) {
    crate::support::test_support::seed_route(
        db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "claude-model", "messages", 0)],
    )
    .await
    .expect("seed combo");
}

/// Protocol shape served by [`CapturingUpstream`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FallbackProtocol {
    ChatCompletions,
    Messages,
}

/// Mock fallback target shared by the system-prompt tests: records every
/// request body, fails the first `fail_first` calls with a retryable 502 so
/// the gateway rotates to the next combo target, then serves `protocol` in
/// either JSON or SSE form.
pub(super) struct CapturingUpstream {
    pub(super) calls: Mutex<Vec<Value>>,
    protocol: FallbackProtocol,
    streaming: bool,
    fail_first: usize,
}

impl CapturingUpstream {
    pub(super) fn chat(streaming: bool) -> Self {
        Self::new(FallbackProtocol::ChatCompletions, streaming, 2)
    }

    pub(super) fn messages(streaming: bool) -> Self {
        Self::new(FallbackProtocol::Messages, streaming, 1)
    }

    fn new(protocol: FallbackProtocol, streaming: bool, fail_first: usize) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            protocol,
            streaming,
            fail_first,
        }
    }

    pub(super) fn route_path(&self) -> &'static str {
        match self.protocol {
            FallbackProtocol::ChatCompletions => "/v1/chat/completions",
            FallbackProtocol::Messages => "/v1/messages",
        }
    }
}

const MESSAGES_FALLBACK_STREAM: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg-1\",\"model\":\"claude-model\",\"content\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"served by messages fallback\"}}\n\n",
    "event: content_block_stop\n",
    "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n",
);

/// Serves the fallback target for both protocol shapes and streaming modes.
pub(super) async fn capturing_fallback_upstream(
    State(upstream): State<Arc<CapturingUpstream>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    let call_number = {
        let mut calls = upstream.calls.lock().await;
        calls.push(body.clone());
        calls.len()
    };
    if call_number <= upstream.fail_first {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error":{"message":"temporary upstream failure"}})),
        )
            .into_response();
    }
    match (upstream.protocol, upstream.streaming) {
        (FallbackProtocol::ChatCompletions, false) => Json(json!({
            "id":"fallback-success",
            "model":body["model"],
            "choices":[{"message":{"role":"assistant","content":"served by fallback"},"finish_reason":"stop"}]
        }))
        .into_response(),
        (FallbackProtocol::ChatCompletions, true) => sse_response(
            "data: {\"choices\":[{\"delta\":{\"content\":\"served by fallback\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
        ),
        (FallbackProtocol::Messages, false) => Json(json!({
            "id":"messages-fallback-success",
            "model":body["model"],
            "content":[{"type":"text","text":"served by messages fallback"}],
            "stop_reason":"end_turn",
            "usage":{"input_tokens":1,"output_tokens":1}
        }))
        .into_response(),
        (FallbackProtocol::Messages, true) => sse_response(MESSAGES_FALLBACK_STREAM),
    }
}
