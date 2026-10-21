use super::device::{decode_device_code, encode_device_code, start_authorization, QoderDeviceContext};
use super::*;

#[test]
fn endpoint_construction_resolves_chat_and_models_urls() {
    let adapter = QoderAdapter;
    assert_eq!(
        adapter
            .endpoint("https://api3.qoder.sh", Protocol::ChatCompletions, None)
            .expect("chat endpoint")
            .as_str(),
        "https://api3.qoder.sh/v1/chat/completions"
    );
    assert_eq!(
        adapter
            .endpoint("https://api3.qoder.sh/v1", Protocol::ChatCompletions, None)
            .expect("chat endpoint")
            .as_str(),
        "https://api3.qoder.sh/v1/chat/completions"
    );
    assert_eq!(
        adapter
            .endpoint(
                "https://api3.qoder.sh/v1/chat/completions",
                Protocol::ChatCompletions,
                None
            )
            .expect("chat endpoint")
            .as_str(),
        "https://api3.qoder.sh/v1/chat/completions"
    );
    assert!(
        adapter
            .endpoint("https://api3.qoder.sh", Protocol::Messages, None)
            .is_err()
    );
    assert_eq!(
        adapter
            .model_list_endpoint("https://api3.qoder.sh")
            .expect("models endpoint")
            .as_str(),
        "https://api3.qoder.sh/v1/models"
    );
}

#[test]
fn start_authorization_intl_and_cn_generate_valid_device_grants() {
    let intl = start_authorization("intl").expect("start intl authorization");
    assert!(
        intl.verification_uri
            .starts_with("https://qoder.com/device/selectAccounts?")
    );
    assert!(intl.verification_uri.contains("challenge="));
    assert!(intl.verification_uri.contains("challenge_method=S256"));
    assert!(intl.verification_uri.contains("machine_id="));
    assert!(intl.verification_uri.contains("nonce="));
    assert_eq!(intl.user_code.len(), 8);
    assert_eq!(intl.expires_in.as_secs(), 300);
    assert_eq!(intl.interval.as_secs(), 2);

    let ctx_intl = decode_device_code(&intl.device_code).expect("decode intl device code");
    assert_eq!(ctx_intl.region, "intl");
    assert!(!ctx_intl.nonce.is_empty());
    assert!(!ctx_intl.verifier.is_empty());
    assert!(!ctx_intl.machine_id.is_empty());

    let cn = start_authorization("cn").expect("start cn authorization");
    assert!(
        cn.verification_uri
            .starts_with("https://qoder.com.cn/device/selectAccounts?")
    );
    let ctx_cn = decode_device_code(&cn.device_code).expect("decode cn device code");
    assert_eq!(ctx_cn.region, "cn");
}

#[test]
fn config_validation_requires_qoder_oauth_and_chat_completions() {
    let adapter = QoderAdapter;
    let preset = preset_for_adapter(QODER_ADAPTER_ID).expect("qoder preset");
    let protocols = ["chat_completions".to_owned()];

    assert!(
        adapter
            .validate_config(
                preset,
                "qoder_oauth",
                "https://api3.qoder.sh",
                "chat_completions",
                &protocols,
                false,
            )
            .is_ok()
    );

    for (auth_type, protocol) in [
        ("bearer", "chat_completions"),
        ("none", "chat_completions"),
        ("qoder_oauth", "messages"),
    ] {
        let protos = [protocol.to_owned()];
        assert!(
            adapter
                .validate_config(
                    preset,
                    auth_type,
                    "https://api3.qoder.sh",
                    protocol,
                    &protos,
                    false
                )
                .is_err(),
            "{auth_type} with {protocol} must be rejected"
        );
    }
}

#[test]
fn client_headers_and_auth_are_applied_correctly() {
    let adapter = QoderAdapter;
    let client = reqwest::Client::new();
    let request = client.post("https://api3.qoder.sh/v1/chat/completions");
    let request = adapter.apply_client_headers(request, &HeaderMap::new());
    let oauth = OAuthRequestAuth {
        access_token: "dt-test-access-token".to_owned(),
        account_id: None,
        project_id: None,
    };
    let request = adapter
        .apply_request_auth(
            request,
            QODER_ADAPTER_ID,
            None,
            None,
            Some(&oauth),
            "session-id",
        )
        .expect("apply request auth");
    let built = request.build().expect("build request");
    assert_eq!(
        built.headers().get("authorization").and_then(|h| h.to_str().ok()),
        Some("Bearer dt-test-access-token")
    );
    assert_eq!(
        built.headers().get(QODER_IDE_VERSION_HEADER).and_then(|h| h.to_str().ok()),
        Some(QODER_IDE_VERSION)
    );
    assert_eq!(
        built.headers().get(QODER_CLIENT_TYPE_HEADER).and_then(|h| h.to_str().ok()),
        Some(QODER_CLIENT_TYPE)
    );
    assert_eq!(
        built.headers().get(QODER_LOGIN_VERSION_HEADER).and_then(|h| h.to_str().ok()),
        Some(QODER_LOGIN_VERSION)
    );
}

#[test]
fn device_code_encoding_and_decoding_round_trips() {
    let original = QoderDeviceContext {
        nonce: "test-nonce-123".to_owned(),
        verifier: "test-verifier-abc".to_owned(),
        machine_id: "test-machine-xyz".to_owned(),
        region: "intl".to_owned(),
    };
    let encoded = encode_device_code(&original).expect("encode");
    let decoded = decode_device_code(&encoded).expect("decode");
    assert_eq!(decoded.nonce, original.nonce);
    assert_eq!(decoded.verifier, original.verifier);
    assert_eq!(decoded.machine_id, original.machine_id);
    assert_eq!(decoded.region, original.region);

    assert!(decode_device_code("").is_err());
    assert!(decode_device_code("invalid base64!").is_err());
    assert!(decode_device_code("bm90LWpzb24").is_err()); // "not-json"
}

#[tokio::test]
async fn discover_oauth_models_falls_back_to_preset_models_when_upstream_unsupported() {
    let database = crate::support::test_support::TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    let state = AppState::new(config, database.db.clone());
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "qoder-p1",
            name: "Qoder",
            base_url: "https://api3.qoder.sh",
            adapter_id: QODER_ADAPTER_ID,
            auth_type: "qoder_oauth",
            model_prefix: "qd",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");

    qoder_oauth::save_qoder_account(
        &state,
        "qoder-p1",
        AdapterOAuthAccount {
            payload: serde_json::json!({
                "token": "dt-test-token-123",
                "email": "test@qoder.com",
            }),
            display_name: "test@qoder.com".to_owned(),
        },
    )
    .await
    .expect("save account");

    let key_id = state
        .db
        .read(|tx| {
            let prefix = crate::infra::db::provider_api_key_identity_index_prefix(
                "qoder-p1",
                "email:test@qoder.com",
            )?;
            let item = tx
                .scan_prefix::<String>(crate::infra::storage::Table::ProviderApiKeyIndex, &prefix, 1)?
                .into_iter()
                .map(|(_, id)| id)
                .next();
            Ok::<_, crate::infra::storage::StorageError>(item)
        })
        .await
        .expect("read key")
        .expect("key id");

    let models = QODER_ADAPTER
        .discover_oauth_models(&state, &key_id)
        .await
        .expect("discover models");

    assert!(!models.is_empty(), "models must not be empty");
    assert!(models.contains(&"ultimate".to_owned()));
    assert!(models.contains(&"qmodel_38max".to_owned()));
}

#[tokio::test]
async fn discover_oauth_models_parses_live_models_from_upstream() {
    let (address, _server) = crate::support::mock_upstream::spawn_mock_http_server(vec![
        crate::support::mock_upstream::MockResponse::json(
            200,
            br#"{"data":[{"id":"qmodel-live-1"},{"id":"qmodel-live-2"}]}"#.to_vec(),
        ),
    ])
    .await;

    let database = crate::support::test_support::TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "qoder-p2",
            name: "Qoder",
            base_url: &format!("http://{address}"),
            adapter_id: QODER_ADAPTER_ID,
            auth_type: "qoder_oauth",
            model_prefix: "qd",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");

    qoder_oauth::save_qoder_account(
        &state,
        "qoder-p2",
        AdapterOAuthAccount {
            payload: serde_json::json!({
                "token": "dt-test-token-456",
                "email": "live@qoder.com",
            }),
            display_name: "live@qoder.com".to_owned(),
        },
    )
    .await
    .expect("save account");

    let key_id = state
        .db
        .read(|tx| {
            let prefix = crate::infra::db::provider_api_key_identity_index_prefix(
                "qoder-p2",
                "email:live@qoder.com",
            )?;
            let item = tx
                .scan_prefix::<String>(crate::infra::storage::Table::ProviderApiKeyIndex, &prefix, 1)?
                .into_iter()
                .map(|(_, id)| id)
                .next();
            Ok::<_, crate::infra::storage::StorageError>(item)
        })
        .await
        .expect("read key")
        .expect("key id");

    let models = QODER_ADAPTER
        .discover_oauth_models(&state, &key_id)
        .await
        .expect("discover models");

    assert_eq!(models, vec!["qmodel-live-1".to_owned(), "qmodel-live-2".to_owned()]);
}
