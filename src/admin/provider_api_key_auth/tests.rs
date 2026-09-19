use super::*;
use crate::{
    infra::storage::{Record, Table},
    security::{decrypt_secret, encrypt_secret, token_hash},
    state::AppState,
    support::test_support::TestDatabase,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, HeaderValue, Request, header},
    routing::post,
};
use std::net::SocketAddr;
use tower::ServiceExt;

async fn test_state() -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([37_u8; 32]);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    (database, state)
}

fn pending_flow(state_value: &str, expires_at: Instant) -> PendingProviderApiKeyAuthFlow {
    PendingProviderApiKeyAuthFlow {
        adapter_id: provider_adapters::COMMAND_CODE_ADAPTER_ID.to_owned(),
        provider_id: "command-code".to_owned(),
        state_hash: token_hash(state_value),
        encrypted_api_key: None,
        provider_key_id: None,
        user_id: None,
        key_name: None,
        user_name: None,
        expires_at,
        applying_since: None,
        status: ProviderApiKeyAuthFlowStatus::Pending,
        message: None,
    }
}

fn callback_request(payload: Value, origin: &'static str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/callback")
        .header(header::ORIGIN, origin)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(payload.to_string()))
        .expect("callback request")
}

#[test]
fn callback_origin_allowlist_is_exact() {
    for origin in [
        "https://commandcode.ai",
        "https://staging.commandcode.ai",
        "http://localhost:5173",
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, HeaderValue::from_static(origin));
        assert_eq!(callback_origin(&headers).as_deref(), Some(origin));
    }
    for origin in [
        "https://evil.commandcode.ai",
        "null",
        "http://localhost:5959",
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, HeaderValue::from_static(origin));
        assert!(callback_origin(&headers).is_none());
    }
    assert!(callback_origin(&HeaderMap::new()).is_none());
}

#[test]
fn loopback_address_selection_excludes_non_loopback_addresses() {
    let addresses = loopback_addresses([
        SocketAddr::from(([127, 0, 0, 1], 5959)),
        SocketAddr::from(([192, 0, 2, 1], 5959)),
        SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 5959)),
    ]);
    assert_eq!(addresses.len(), 2);
    assert!(addresses.iter().all(|address| address.ip().is_loopback()));
}

#[test]
fn callback_state_is_bound_to_a_pending_flow_and_cannot_be_replayed() {
    let expected_state = URL_SAFE_NO_PAD.encode([41_u8; 32]);
    let now = Instant::now();
    let mut flows = std::collections::HashMap::from([(
        "flow-1".to_owned(),
        pending_flow(&expected_state, now + Duration::from_secs(30)),
    )]);

    assert!(
        accept_callback_state(
            &mut flows,
            ApiKeyCallbackAcceptance {
                adapter_id: provider_adapters::COMMAND_CODE_ADAPTER_ID,
                submitted_state_hash: &token_hash("wrong-state"),
                encrypted_api_key: vec![1, 2, 3],
                user_id: None,
                key_name: None,
                user_name: None,
                now,
            },
        )
        .is_none()
    );
    let metadata = accept_callback_state(
        &mut flows,
        ApiKeyCallbackAcceptance {
            adapter_id: provider_adapters::COMMAND_CODE_ADAPTER_ID,
            submitted_state_hash: &token_hash(&expected_state),
            encrypted_api_key: vec![4, 5, 6],
            user_id: Some(" user-1 ".to_owned()),
            key_name: Some(" key-1 ".to_owned()),
            user_name: Some(" User One ".to_owned()),
            now,
        },
    )
    .expect("valid flow accepts callback");
    assert_eq!(metadata["key_name"], "key-1");
    let flow = flows.get("flow-1").expect("flow retained");
    assert_eq!(flow.status, ProviderApiKeyAuthFlowStatus::Received);
    assert_eq!(
        flow.encrypted_api_key.as_deref(),
        Some([4, 5, 6].as_slice())
    );
    assert!(
        accept_callback_state(
            &mut flows,
            ApiKeyCallbackAcceptance {
                adapter_id: provider_adapters::COMMAND_CODE_ADAPTER_ID,
                submitted_state_hash: &token_hash(&expected_state),
                encrypted_api_key: vec![7, 8, 9],
                user_id: None,
                key_name: None,
                user_name: None,
                now,
            },
        )
        .is_none()
    );
}

#[test]
fn callback_state_rejects_expired_flows() {
    let expected_state = URL_SAFE_NO_PAD.encode([42_u8; 32]);
    let now = Instant::now();
    let mut flows = std::collections::HashMap::from([(
        "expired".to_owned(),
        pending_flow(&expected_state, now - Duration::from_secs(1)),
    )]);
    assert!(
        accept_callback_state(
            &mut flows,
            ApiKeyCallbackAcceptance {
                adapter_id: provider_adapters::COMMAND_CODE_ADAPTER_ID,
                submitted_state_hash: &token_hash(&expected_state),
                encrypted_api_key: vec![1],
                user_id: None,
                key_name: None,
                user_name: None,
                now,
            },
        )
        .is_none()
    );
    assert!(flows.is_empty());
}

#[tokio::test]
async fn callback_accepts_a_valid_key_once_and_never_returns_the_secret() {
    let (_database, state) = test_state().await;
    let callback_state = URL_SAFE_NO_PAD.encode([43_u8; 32]);
    state
        .register_provider_api_key_auth_flow(
            "flow-2".to_owned(),
            pending_flow(&callback_state, Instant::now() + Duration::from_secs(60)),
        )
        .await
        .expect("register callback flow");
    let app = Router::new()
        .route("/callback", post(callback).options(preflight))
        .with_state(state.clone());
    let payload = json!({
        "apiKey":"cc-secret-test-value",
        "state":callback_state,
        "userId":"user-2",
        "userName":"Studio User",
        "keyName":"Studio key"
    });
    let response = app
        .clone()
        .oneshot(callback_request(payload.clone(), "https://commandcode.ai"))
        .await
        .expect("callback response");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("https://commandcode.ai")
    );
    let body = to_bytes(response.into_body(), CALLBACK_BODY_LIMIT)
        .await
        .expect("callback response body");
    assert!(!String::from_utf8_lossy(&body).contains("cc-secret-test-value"));
    {
        let flows = state.admin.provider_api_key_auth_flows.lock().await;
        let flow = flows.get("flow-2").expect("flow state");
        assert_eq!(flow.status, ProviderApiKeyAuthFlowStatus::Received);
        let decrypted = decrypt_secret(
            state.config.master_key.as_ref(),
            flow.encrypted_api_key.as_deref(),
        )
        .expect("decrypt test key")
        .expect("stored key");
        assert_eq!(decrypted, "cc-secret-test-value");
    }

    let replay = app
        .clone()
        .oneshot(callback_request(payload, "https://commandcode.ai"))
        .await
        .expect("replayed callback response");
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);

    let wrong_origin = app
        .clone()
        .oneshot(callback_request(
            json!({"apiKey":"cc-secret-test-value","state":callback_state}),
            "https://evil.commandcode.ai",
        ))
        .await
        .expect("wrong origin response");
    assert_eq!(wrong_origin.status(), StatusCode::FORBIDDEN);

    let oversized = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/callback")
                .header(header::ORIGIN, "https://commandcode.ai")
                .body(Body::from(vec![b'x'; CALLBACK_BODY_LIMIT + 1]))
                .expect("oversized callback request"),
        )
        .await
        .expect("oversized callback response");
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn apply_persists_the_encrypted_key_once_and_never_returns_it() {
    let (_database, state) = test_state().await;
    crate::support::test_support::seed_provider(
        &state.db,
        crate::support::test_support::ProviderSeed {
            id: "command-code",
            name: "Command Code",
            base_url: "http://127.0.0.1:1",
            adapter_id: "command_code",
            auth_type: "bearer",
            model_prefix: "command-code",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions", "messages"],
        },
    )
    .await
    .expect("Command Code provider fixture");

    let flow_id = "flow-apply";
    let callback_state = URL_SAFE_NO_PAD.encode([44_u8; 32]);
    let mut flow = pending_flow(&callback_state, Instant::now() + Duration::from_secs(60));
    flow.status = ProviderApiKeyAuthFlowStatus::Received;
    flow.key_name = Some("Studio key".to_owned());
    flow.encrypted_api_key = Some(
        encrypt_secret(state.config.master_key.as_ref(), "cc-secret-test-value")
            .expect("encrypt pending API key")
            .expect("non-empty API key"),
    );
    state
        .register_provider_api_key_auth_flow(flow_id.to_owned(), flow)
        .await
        .expect("register received flow");

    let Json(result) = apply(
        State(state.clone()),
        Path(("command-code".to_owned(), flow_id.to_owned())),
    )
    .await
    .expect("apply received API key");
    assert_eq!(result["status"], "applied");
    assert!(!result.to_string().contains("cc-secret-test-value"));

    let key_prefix = crate::infra::db::provider_api_key_index_prefix("command-code", false)
        .expect("provider API key index prefix");
    let saved_secret = state
        .db
        .read(move |transaction| {
            transaction
                .scan_prefix::<String>(Table::ProviderApiKeyIndex, &key_prefix, 10)?
                .into_iter()
                .next()
                .ok_or(crate::infra::storage::StorageError::NotFound)
                .and_then(|(_, id)| {
                    transaction
                        .get::<Record>(Table::ProviderApiKeys, &id)?
                        .ok_or(crate::infra::storage::StorageError::NotFound)
                })
                .and_then(|record| Ok(record.bytes("secret")?.to_vec()))
        })
        .await
        .expect("saved provider key");
    let decrypted = decrypt_secret(state.config.master_key.as_ref(), Some(&saved_secret))
        .expect("decrypt saved key")
        .expect("stored key");
    assert_eq!(decrypted, "cc-secret-test-value");
    let key_prefix = crate::infra::db::provider_api_key_index_prefix("command-code", false)
        .expect("provider API key index prefix");
    let key_count = state
        .db
        .read(move |transaction| {
            Ok(transaction
                .scan_prefix::<String>(Table::ProviderApiKeyIndex, &key_prefix, 10)?
                .len())
        })
        .await
        .expect("provider key count");
    assert_eq!(key_count, 1_usize);

    let Json(replayed) = apply(
        State(state.clone()),
        Path(("command-code".to_owned(), flow_id.to_owned())),
    )
    .await
    .expect("repeated apply is idempotent");
    assert_eq!(replayed["status"], "applied");
    let key_prefix = crate::infra::db::provider_api_key_index_prefix("command-code", false)
        .expect("provider API key index prefix");
    let key_count = state
        .db
        .read(move |transaction| {
            Ok(transaction
                .scan_prefix::<String>(Table::ProviderApiKeyIndex, &key_prefix, 10)?
                .len())
        })
        .await
        .expect("provider key count after replay");
    assert_eq!(key_count, 1_usize);
    let flows = state.admin.provider_api_key_auth_flows.lock().await;
    let flow = flows.get(flow_id).expect("applied flow");
    assert_eq!(flow.status, ProviderApiKeyAuthFlowStatus::Applied);
    assert!(flow.encrypted_api_key.is_none());
}

#[tokio::test]
async fn concurrent_apply_requests_claim_a_flow_only_once() {
    let (_database, state) = test_state().await;
    let callback_state = URL_SAFE_NO_PAD.encode([45_u8; 32]);
    let mut flow = pending_flow(&callback_state, Instant::now() + Duration::from_secs(60));
    flow.status = ProviderApiKeyAuthFlowStatus::Received;
    flow.encrypted_api_key = Some(vec![1, 2, 3]);
    state
        .register_provider_api_key_auth_flow("flow-concurrent".to_owned(), flow)
        .await
        .expect("register received flow");

    let (first, second) = tokio::join!(
        claim_api_key_auth_apply(&state, "command-code", "flow-concurrent"),
        claim_api_key_auth_apply(&state, "command-code", "flow-concurrent"),
    );
    let first_claimed = matches!(&first, Ok(ApiKeyAuthApplyClaim::Claimed { .. }));
    let second_claimed = matches!(&second, Ok(ApiKeyAuthApplyClaim::Claimed { .. }));
    assert_ne!(first_claimed, second_claimed);
    let conflict_count = [&first, &second]
        .into_iter()
        .filter(|result| matches!(result, Err((StatusCode::CONFLICT, _))))
        .count();
    assert_eq!(conflict_count, 1);
    let flows = state.admin.provider_api_key_auth_flows.lock().await;
    assert_eq!(
        flows.get("flow-concurrent").expect("flow retained").status,
        ProviderApiKeyAuthFlowStatus::Applying
    );
}

#[tokio::test]
async fn abandoned_apply_claims_can_be_retried_after_the_recovery_window() {
    let (_database, state) = test_state().await;
    let callback_state = URL_SAFE_NO_PAD.encode([46_u8; 32]);
    let mut flow = pending_flow(&callback_state, Instant::now() + Duration::from_secs(60));
    flow.status = ProviderApiKeyAuthFlowStatus::Applying;
    flow.applying_since =
        Some(Instant::now() - crate::state::PROVIDER_API_KEY_AUTH_APPLY_RECOVERY_TIMEOUT);
    flow.encrypted_api_key = Some(vec![1, 2, 3]);
    state
        .register_provider_api_key_auth_flow("flow-retry".to_owned(), flow)
        .await
        .expect("register stale apply flow");

    let claim = claim_api_key_auth_apply(&state, "command-code", "flow-retry")
        .await
        .expect("stale apply claim is recoverable");
    assert!(matches!(claim, ApiKeyAuthApplyClaim::Claimed { .. }));
    let flows = state.admin.provider_api_key_auth_flows.lock().await;
    let flow = flows.get("flow-retry").expect("flow retained");
    assert_eq!(flow.status, ProviderApiKeyAuthFlowStatus::Applying);
    assert!(flow.applying_since.is_some());
}

#[tokio::test]
async fn apply_failure_releases_the_claim_for_retry() {
    let (_database, state) = test_state().await;
    let callback_state = URL_SAFE_NO_PAD.encode([47_u8; 32]);
    let mut flow = pending_flow(&callback_state, Instant::now() + Duration::from_secs(60));
    flow.status = ProviderApiKeyAuthFlowStatus::Received;
    flow.encrypted_api_key = Some(vec![1, 2, 3]);
    state
        .register_provider_api_key_auth_flow("flow-failure".to_owned(), flow)
        .await
        .expect("register received flow");

    let result = apply(
        State(state.clone()),
        Path(("command-code".to_owned(), "flow-failure".to_owned())),
    )
    .await;
    assert!(matches!(result, Err((StatusCode::NOT_FOUND, _))));
    let flows = state.admin.provider_api_key_auth_flows.lock().await;
    let flow = flows.get("flow-failure").expect("flow retained");
    assert_eq!(flow.status, ProviderApiKeyAuthFlowStatus::Received);
    assert!(flow.encrypted_api_key.is_some());
    assert!(flow.applying_since.is_none());
}
