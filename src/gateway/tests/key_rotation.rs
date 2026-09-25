use super::support::*;
use super::*;

#[tokio::test]
async fn rejected_provider_key_is_removed_and_next_key_serves_the_request() {
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(key_rotation_upstream))
            .with_state(auth_calls.clone()),
    )
    .await;

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
}

#[tokio::test]
async fn round_robin_provider_keys_serve_consecutive_requests_with_the_next_key() {
    let credentials = Arc::new(Mutex::new(Vec::new()));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(round_robin_upstream))
            .with_state(credentials.clone()),
    )
    .await;

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
}

#[tokio::test]
async fn round_robin_provider_keys_wrap_around_for_failover_within_one_request() {
    let credentials = Arc::new(Mutex::new(Vec::new()));
    let address = spawn_upstream(
        Router::new()
            .route(
                "/v1/chat/completions",
                post(round_robin_rejecting_last_key_upstream),
            )
            .with_state(credentials.clone()),
    )
    .await;

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
}

#[tokio::test]
async fn providers_without_a_key_strategy_always_start_with_the_preferred_key() {
    let credentials = Arc::new(Mutex::new(Vec::new()));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(round_robin_upstream))
            .with_state(credentials.clone()),
    )
    .await;

    let database = TestDatabase::open().await;
    let state = seed_round_robin_provider(&database, address, None).await;

    for _ in 0..3 {
        assert_eq!(gateway_completion(&state).await, StatusCode::OK);
    }

    assert_eq!(
        credentials.lock().await.clone(),
        ["Bearer first-key", "Bearer first-key", "Bearer first-key"]
    );
}

#[tokio::test]
async fn round_robin_providers_keep_the_configured_secret_fallback() {
    let credentials = Arc::new(Mutex::new(Vec::new()));
    let address = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(round_robin_upstream))
            .with_state(credentials.clone()),
    )
    .await;

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
}
