use super::*;
use crate::{
    infra::storage::{Field, Record, Table},
    protocol::Protocol,
    support::test_support::TestDatabase,
};
use axum::{
    Json, Router, body::to_bytes, extract::State, http::HeaderMap, response::IntoResponse,
    routing::post,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Mutex;

#[test]
fn codex_requests_force_safe_responses_streaming() {
    let mut body = json!({
        "model":"gpt-codex",
        "input":[],
        "instructions":"",
        "stream":false,
        "store":true,
        "temperature":0.2,
        "previous_response_id":"resp_old"
    });
    prepare_codex_request(&mut body, "session-test");
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    assert_eq!(body["prompt_cache_key"], "session-test");
    assert_eq!(body["instructions"], "You are a helpful assistant.");
    assert!(body.get("temperature").is_none());
    assert!(body.get("previous_response_id").is_none());
}

#[test]
fn codex_terminal_error_is_distinguished_from_an_incomplete_stream() {
    let terminal_failure = provider_adapters::parse_adapter_event_stream(
            b"event: response.failed\ndata: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"model at capacity\"}}}\n\n",
        ).expect_err("failed terminal event");
    assert!(provider_adapters::adapter_sse_error_can_fail_over(
        false,
        &terminal_failure
    ));
    assert!(!provider_adapters::adapter_sse_error_can_fail_over(
        true,
        &terminal_failure
    ));

    let incomplete = provider_adapters::parse_adapter_event_stream(
            b"event: response.created\ndata: {\"type\":\"response.created\",\"response\":{\"status\":\"in_progress\"}}\n\n",
        ).expect_err("stream ended before completion");
    assert!(!provider_adapters::adapter_sse_error_can_fail_over(
        false,
        &incomplete
    ));
}

#[test]
fn provider_error_details_keep_diagnostics_without_credentials_or_request_content() {
    let detail = sanitize_provider_error_body(
            br#"{"error":{"message":"key expired","code":"invalid_api_key"},"api_key":"secret-key","request":{"messages":[{"content":"private prompt"}]}}"#,
        );
    assert!(detail.contains("key expired"));
    assert!(detail.contains("invalid_api_key"));
    assert!(detail.contains("[redacted]"));
    assert!(!detail.contains("secret-key"));
    assert!(!detail.contains("private prompt"));
}

#[test]
fn endpoint_url_respects_versioned_base_url() {
    assert_eq!(
        endpoint_url("https://example.com/v1/", Protocol::ChatCompletions)
            .expect("URL")
            .as_str(),
        "https://example.com/v1/chat/completions"
    );
    assert_eq!(
        endpoint_url("https://example.com", Protocol::Responses)
            .expect("URL")
            .as_str(),
        "https://example.com/v1/responses"
    );
}

#[tokio::test]
async fn route_and_provider_model_aliases_read_lmdb_indexes() {
    let database = TestDatabase::open().await;
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "provider",
            name: "Mock provider",
            base_url: "https://provider.example/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "openai",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    crate::support::test_support::seed_provider_model(&database.db, "provider", "gpt-test")
        .await
        .expect("seed model");
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "gpt-test", "chat_completions", 0)],
    )
    .await
    .expect("seed route");

    let route = request_preparation::load_stored_route(&database.db, "coding")
        .await
        .expect("load route")
        .expect("route exists");
    assert!(route.enabled);
    assert_eq!(route.targets.len(), 1);
    assert_eq!(route.targets[0].provider_id, "provider");
    let request_preparation::ProviderModelAliasResolution::Found(target) =
        request_preparation::resolve_provider_model_alias(&database.db, "openai/gpt-test")
            .await
            .expect("resolve alias")
    else {
        panic!("saved model alias should resolve");
    };
    assert_eq!(target.provider_id, "provider");
    assert_eq!(target.model, "gpt-test");
}

async fn key_rotation_upstream(
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

async fn transient_server_error_upstream(
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

async fn rotating_priority_upstream(
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

async fn transient_empty_completion_upstream(
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

async fn transient_empty_stream_upstream(
    State(calls): State<Arc<AtomicUsize>>,
    Json(_body): Json<Value>,
) -> axum::response::Response {
    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from("data: [DONE]\n\n"))
            .expect("empty stream response");
    }
    Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(
                "data: {\"choices\":[{\"delta\":{\"content\":\"recovered after empty stream\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
            ))
            .expect("empty stream retry response")
}

async fn transient_stream_retryable_upstream(
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
    Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(
                "data: {\"choices\":[{\"delta\":{\"content\":\"recovered\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
            ))
            .expect("stream retry response")
}

async fn transient_stream_timeout_upstream(
    State(calls): State<Arc<AtomicUsize>>,
    Json(_body): Json<Value>,
) -> axum::response::Response {
    if calls.fetch_add(1, Ordering::Relaxed) == 0 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":{"message":"late upstream failure"}})),
        )
            .into_response();
    }
    Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(
                "data: {\"choices\":[{\"delta\":{\"content\":\"recovered\"},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n",
            ))
            .expect("timeout retry response")
}

#[tokio::test]
async fn rejected_provider_key_is_removed_and_next_key_serves_the_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let calls = auth_calls.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/v1/chat/completions", post(key_rotation_upstream))
                    .with_state(calls),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
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
    crate::support::test_support::seed_provider_key(
        &database.db,
        "provider",
        "01-dead",
        b"dead-key",
        false,
    )
    .await
    .expect("seed first key");
    crate::support::test_support::seed_provider_key(
        &database.db,
        "provider",
        "02-backup",
        b"backup-key",
        false,
    )
    .await
    .expect("seed backup key");
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "mock-model", "chat_completions", 0)],
    )
    .await
    .expect("seed route");

    let response = handle_request_inner_with_adapter_base_url_override(
        state.clone(),
        HeaderMap::new(),
        json!({"model":"coding","messages":[{"role":"user","content":"hello"}]}),
        Protocol::ChatCompletions,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("gateway response");
    let payload: Value = serde_json::from_slice(&body).expect("gateway JSON");
    assert_eq!(
        payload["choices"][0]["message"]["content"],
        "served by backup"
    );
    assert_eq!(auth_calls.load(Ordering::Relaxed), 2);

    let (dead_key, backup_key) = database
        .db
        .read(|transaction| {
            Ok((
                transaction
                    .get::<Record>(Table::ProviderApiKeys, "01-dead")?
                    .ok_or(crate::infra::storage::StorageError::NotFound)?,
                transaction
                    .get::<Record>(Table::ProviderApiKeys, "02-backup")?
                    .ok_or(crate::infra::storage::StorageError::NotFound)?,
            ))
        })
        .await
        .expect("read provider keys");
    assert!(dead_key.boolean("invalid").expect("invalid flag"));
    assert!(!backup_key.boolean("invalid").expect("backup invalid flag"));
    let available_prefix = crate::infra::db::provider_api_key_index_prefix("provider", true)
        .expect("availability index");
    let available = database
        .db
        .read(move |transaction| {
            transaction.scan_prefix::<String>(
                Table::ProviderApiKeyAvailabilityIndex,
                &available_prefix,
                10,
            )
        })
        .await
        .expect("read availability index");
    assert_eq!(
        available
            .iter()
            .map(|(_, id)| id.as_str())
            .collect::<Vec<_>>(),
        ["02-backup"]
    );
    state.telemetry.flush().await.expect("flush request logs");
    let request_logs = database
        .db
        .read(|transaction| transaction.scan_prefix::<Record>(Table::RequestLogs, "", 10))
        .await
        .expect("read request logs");
    assert!(request_logs.iter().any(|(_, record)| {
        record.integer("status").ok() == Some(401)
            && record
                .optional_text("provider_credential_id")
                .ok()
                .flatten()
                == Some("01-dead")
            && record
                .optional_text("error")
                .ok()
                .flatten()
                .is_some_and(|error| error.contains("key expired"))
    }));
    server.abort();
}

/// Records the credential of every upstream call so a test can assert which
/// provider key served each request.
async fn round_robin_upstream(
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
async fn round_robin_rejecting_last_key_upstream(
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

async fn store_key_strategy(
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

async fn seed_round_robin_provider(
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

async fn gateway_completion(state: &AppState) -> StatusCode {
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

#[tokio::test]
async fn round_robin_provider_keys_serve_consecutive_requests_with_the_next_key() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let credentials = Arc::new(Mutex::new(Vec::new()));
    let server = tokio::spawn({
        let credentials = credentials.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/v1/chat/completions", post(round_robin_upstream))
                    .with_state(credentials),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
    let state = seed_round_robin_provider(&database, address, Some("round_robin")).await;

    for _ in 0..4 {
        assert_eq!(gateway_completion(&state).await, StatusCode::OK);
    }

    assert_eq!(
        credentials.lock().await.clone(),
        [
            "Bearer first-key",
            "Bearer second-key",
            "Bearer third-key",
            "Bearer first-key"
        ]
    );
    server.abort();
}

#[tokio::test]
async fn round_robin_provider_keys_wrap_around_for_failover_within_one_request() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let credentials = Arc::new(Mutex::new(Vec::new()));
    let server = tokio::spawn({
        let credentials = credentials.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/v1/chat/completions",
                        post(round_robin_rejecting_last_key_upstream),
                    )
                    .with_state(credentials),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
    let state = seed_round_robin_provider(&database, address, Some("round_robin")).await;

    for _ in 0..3 {
        assert_eq!(gateway_completion(&state).await, StatusCode::OK);
    }

    // The third request starts at the rejected last key and continues with the
    // keys that precede it instead of failing the request.
    assert_eq!(
        credentials.lock().await.clone(),
        [
            "Bearer first-key",
            "Bearer second-key",
            "Bearer third-key",
            "Bearer first-key"
        ]
    );
    server.abort();
}

#[tokio::test]
async fn providers_without_a_key_strategy_always_start_with_the_preferred_key() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let credentials = Arc::new(Mutex::new(Vec::new()));
    let server = tokio::spawn({
        let credentials = credentials.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/v1/chat/completions", post(round_robin_upstream))
                    .with_state(credentials),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
    let state = seed_round_robin_provider(&database, address, None).await;

    for _ in 0..3 {
        assert_eq!(gateway_completion(&state).await, StatusCode::OK);
    }

    assert_eq!(
        credentials.lock().await.clone(),
        ["Bearer first-key", "Bearer first-key", "Bearer first-key"]
    );
    server.abort();
}

#[tokio::test]
async fn round_robin_providers_keep_the_configured_secret_fallback() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let credentials = Arc::new(Mutex::new(Vec::new()));
    let server = tokio::spawn({
        let credentials = credentials.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/v1/chat/completions", post(round_robin_upstream))
                    .with_state(credentials),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
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
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "mock-model", "chat_completions", 0)],
    )
    .await
    .expect("seed route");
    let provider_id = "provider".to_owned();
    let secret = crate::security::encrypt_secret(Some(&[53_u8; 32]), "legacy-secret")
        .expect("encrypt provider secret")
        .expect("non-empty provider secret");
    database
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(crate::infra::storage::StorageError::NotFound)?;
            provider.insert("secret", Field::Bytes(secret.clone()));
            provider.insert(
                "key_strategy",
                Field::Text(crate::admin::providers::KEY_STRATEGY_ROUND_ROBIN.to_owned()),
            );
            transaction.put(Table::Providers, &provider_id, &provider)
        })
        .await
        .expect("store provider secret");

    assert_eq!(gateway_completion(&state).await, StatusCode::OK);
    assert_eq!(credentials.lock().await.clone(), ["Bearer legacy-secret"]);
    server.abort();
}

#[tokio::test]
async fn server_failure_is_retried_before_returning_to_the_client() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let calls = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let calls = calls.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/v1/chat/completions",
                        post(transient_server_error_upstream),
                    )
                    .with_state(calls),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
    crate::support::test_support::seed_provider(
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
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}]
        })),
        Protocol::ChatCompletions,
        None,
        GatewayExecutionSettings {
            resource_limits: GatewayResourceLimits::default(),
            operational_settings: OperationalSettings {
                request_timeout: Duration::from_secs(2),
                ..OperationalSettings::default()
            },
            api_key_id: None,
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("gateway response");
    let payload: Value = serde_json::from_slice(&body).expect("gateway JSON");
    assert_eq!(
        payload["choices"][0]["message"]["content"],
        "served after retry"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    server.abort();
}

#[tokio::test]
async fn priority_combo_retries_from_the_next_target_after_backoff() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let calls = Arc::new(Mutex::new(Vec::new()));
    let server = tokio::spawn({
        let calls = calls.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/v1/chat/completions", post(rotating_priority_upstream))
                    .with_state(calls),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
    crate::support::test_support::seed_provider(
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
    crate::support::test_support::seed_route(
        &database.db,
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

    let response = execute_with_server_retries(
        state,
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}]
        })),
        Protocol::ChatCompletions,
        None,
        GatewayExecutionSettings {
            resource_limits: GatewayResourceLimits::default(),
            operational_settings: OperationalSettings {
                request_timeout: Duration::from_secs(2),
                ..OperationalSettings::default()
            },
            api_key_id: None,
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let observed_models = calls.lock().await.clone();
    assert_eq!(
        observed_models,
        ["model-a", "model-b", "model-b", "model-a"]
    );
    server.abort();
}

#[tokio::test]
async fn empty_upstream_completion_is_retried_before_returning_to_the_client() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let calls = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let calls = calls.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/v1/chat/completions",
                        post(transient_empty_completion_upstream),
                    )
                    .with_state(calls),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
    crate::support::test_support::seed_provider(
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
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}]
        })),
        Protocol::ChatCompletions,
        None,
        GatewayExecutionSettings {
            resource_limits: GatewayResourceLimits::default(),
            operational_settings: OperationalSettings {
                request_timeout: Duration::from_secs(2),
                ..OperationalSettings::default()
            },
            api_key_id: None,
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("gateway response");
    let payload: Value = serde_json::from_slice(&body).expect("gateway JSON");
    assert_eq!(
        payload["choices"][0]["message"]["content"],
        "served after empty response"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    server.abort();
}

#[tokio::test]
async fn empty_upstream_stream_is_retried_before_returning_to_the_client() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let calls = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let calls = calls.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/v1/chat/completions",
                        post(transient_empty_stream_upstream),
                    )
                    .with_state(calls),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let state = AppState::new(config, database.db.clone());
    crate::support::test_support::seed_provider(
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
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true
        })),
        Protocol::ChatCompletions,
        None,
        GatewayExecutionSettings {
            resource_limits: GatewayResourceLimits::default(),
            operational_settings: OperationalSettings {
                request_timeout: Duration::from_secs(2),
                ..OperationalSettings::default()
            },
            api_key_id: None,
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("gateway stream response");
    let body = String::from_utf8(body.to_vec()).expect("gateway stream body");
    assert!(body.contains("recovered after empty stream"), "{body}");
    assert!(
        !body.contains("upstream completed the response without content"),
        "{body}"
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    server.abort();
}

#[tokio::test]
async fn stream_continuity_retries_rate_limit_and_server_failures_before_emitting_an_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let calls = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let calls = calls.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/v1/chat/completions",
                        post(transient_stream_retryable_upstream),
                    )
                    .with_state(calls),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(2);
    let resource_limits = GatewayResourceLimits {
        stream_continuity_enabled: true,
        ..GatewayResourceLimits::default()
    };
    let state =
        AppState::new_with_gateway_resource_limits(config, database.db.clone(), resource_limits);
    crate::support::test_support::seed_provider(
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
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "mock-model", "chat_completions", 0)],
    )
    .await
    .expect("seed route");

    let response = start_continuous_request(
        state,
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true
        })),
        Protocol::ChatCompletions,
        GatewayExecutionSettings {
            resource_limits,
            operational_settings: OperationalSettings {
                request_timeout: Duration::from_secs(2),
                ..OperationalSettings::default()
            },
            api_key_id: Some("owner".to_owned()),
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("continuity response");
    assert!(!String::from_utf8_lossy(&body).contains("event: error"));
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    server.abort();
}

#[tokio::test]
async fn stream_continuity_retries_upstream_timeout_before_emitting_an_error() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock upstream listener");
    let address = listener.local_addr().expect("mock upstream address");
    let calls = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let calls = calls.clone();
        async move {
            axum::serve(
                listener,
                Router::new()
                    .route(
                        "/v1/chat/completions",
                        post(transient_stream_timeout_upstream),
                    )
                    .with_state(calls),
            )
            .await
            .expect("mock upstream server");
        }
    });

    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_millis(100);
    let resource_limits = GatewayResourceLimits {
        stream_continuity_enabled: true,
        ..GatewayResourceLimits::default()
    };
    let state =
        AppState::new_with_gateway_resource_limits(config, database.db.clone(), resource_limits);
    crate::support::test_support::seed_provider(
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
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "mock-model", "chat_completions", 0)],
    )
    .await
    .expect("seed route");

    let response = start_continuous_request(
        state,
        HeaderMap::new(),
        Arc::new(json!({
            "model":"coding",
            "messages":[{"role":"user","content":"hello"}],
            "stream":true
        })),
        Protocol::ChatCompletions,
        GatewayExecutionSettings {
            resource_limits,
            operational_settings: OperationalSettings {
                connect_timeout: Duration::from_millis(100),
                request_timeout: Duration::from_millis(100),
                ..OperationalSettings::default()
            },
            api_key_id: Some("owner".to_owned()),
            analytics: None,
            stream_continuity_retry: false,
        },
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("continuity response");
    assert!(!String::from_utf8_lossy(&body).contains("event: error"));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    server.abort();
}

#[tokio::test]
async fn analytics_marks_disconnected_response_as_failed() {
    use axum::body::Body;
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let analytics = state.telemetry.start_request("test-key".to_owned());
    let body = Body::from_stream(async_stream::stream! {
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b"partial"));
        std::future::pending::<()>().await;
    });
    let response = track_analytics_response(Response::new(body), analytics);
    let mut chunks = response.into_body().into_data_stream();
    assert!(chunks.next().await.is_some());
    drop(chunks);
    state
        .telemetry
        .flush()
        .await
        .expect("flush disconnected request");
    let stats =
        crate::infra::telemetry::statistics_snapshot(&database.db, "24h", 1440, &state.telemetry)
            .await;
    let stats = stats.expect("statistics reflect the disconnected body");
    assert_eq!(stats["total_requests"], 1);
    assert_eq!(stats["failures"], 1);
}
