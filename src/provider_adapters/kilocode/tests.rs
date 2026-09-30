use super::device::{DevicePoll, parse_device_code_response, parse_device_poll_response};
use super::*;
use crate::infra::storage::{Field, Record, Table};
use crate::support::mock_upstream::{MockResponse, spawn_mock_http_server};
use crate::support::test_support::{ProviderSeed, TestDatabase, seed_provider};
use serde_json::json;

const TEST_MASTER_KEY: [u8; 32] = [47_u8; 32];

#[test]
fn endpoints_keep_the_openrouter_root_and_reject_other_protocols() {
    let adapter = KilocodeAdapter;
    assert_eq!(
        adapter
            .endpoint(KILOCODE_BASE_URL, Protocol::ChatCompletions, None)
            .expect("chat endpoint")
            .as_str(),
        "https://api.kilo.ai/api/openrouter/chat/completions"
    );
    // A row that already stores a full endpoint still resolves.
    assert_eq!(
        kilocode_url(
            "https://api.kilo.ai/api/openrouter/chat/completions",
            "models"
        )
        .expect("catalog endpoint")
        .as_str(),
        "https://api.kilo.ai/api/openrouter/models"
    );
    assert!(
        adapter
            .endpoint(KILOCODE_BASE_URL, Protocol::Messages, None)
            .is_err()
    );
    assert!(kilocode_url("https://api.kilo.ai", "models").is_err());
}

#[test]
fn config_validation_requires_the_fixed_endpoint_and_a_connected_account() {
    let adapter = KilocodeAdapter;
    let preset = preset_for_adapter(KILOCODE_ADAPTER_ID).expect("kilocode preset");
    let protocols = ["chat_completions".to_owned()];
    assert!(
        adapter
            .validate_config(
                preset,
                "kilocode_oauth",
                KILOCODE_BASE_URL,
                "chat_completions",
                &protocols,
                false,
            )
            .is_ok()
    );

    for (auth_type, base_url) in [
        ("bearer", KILOCODE_BASE_URL),
        ("none", KILOCODE_BASE_URL),
        ("kilocode_oauth", "https://api.kilo.ai/api/gateway"),
    ] {
        assert!(
            adapter
                .validate_config(
                    preset,
                    auth_type,
                    base_url,
                    "chat_completions",
                    &protocols,
                    false
                )
                .is_err(),
            "{auth_type} at {base_url} must be rejected"
        );
    }
}

#[test]
fn device_code_response_requires_a_secure_verification_url() {
    let authorization = parse_device_code_response(&json!({
        "code": "device-code-1",
        "verificationUrl": "https://api.kilo.ai/device?code=device-code-1",
        "expiresIn": 120,
    }))
    .expect("valid device authorization");
    assert_eq!(authorization.device_code, "device-code-1");
    assert_eq!(authorization.user_code, "device-code-1");
    assert_eq!(authorization.expires_in.as_secs(), 120);
    assert_eq!(authorization.interval.as_secs(), 3);

    // A missing expiry keeps the documented five-minute grant.
    let defaulted = parse_device_code_response(&json!({
        "code": "device-code-2",
        "verificationUrl": "https://api.kilo.ai/device",
    }))
    .expect("default expiry");
    assert_eq!(defaulted.expires_in.as_secs(), 300);

    for invalid in [
        json!({}),
        json!({"code": "", "verificationUrl": "https://api.kilo.ai/device"}),
        json!({"code": "device-code", "verificationUrl": ""}),
        json!({"code": "device-code", "verificationUrl": "http://api.kilo.ai/device"}),
        json!({"code": "device-code", "verificationUrl": "https://user:pass@api.kilo.ai/device"}),
        json!({"code": "device-code", "verificationUrl": "https://api.kilo.ai/device#fragment"}),
    ] {
        assert!(
            parse_device_code_response(&invalid).is_err(),
            "expected rejection: {invalid}"
        );
    }
}

#[test]
fn device_poll_statuses_separate_pending_denied_expired_and_approved() {
    assert!(matches!(
        parse_device_poll_response(&json!({"status": "pending"})).expect("pending"),
        DevicePoll::Pending
    ));
    assert!(matches!(
        parse_device_poll_response(&json!({"status": "approved"})).expect("approved without token"),
        DevicePoll::Pending
    ));
    let approved = parse_device_poll_response(&json!({
        "status": "approved",
        "token": "access-token",
        "userEmail": "operator@example.com",
    }))
    .expect("approved");
    let DevicePoll::Approved(payload) = approved else {
        panic!("an approved poll with a token must be approved");
    };
    let account = KilocodeAccount::from_device_approval(&payload).expect("usable approval");
    assert_eq!(account.access_token, "access-token");
}

#[tokio::test]
async fn oauth_catalog_discovery_sends_the_bearer_token_and_editor_header() {
    let (address, server) = spawn_mock_http_server(vec![MockResponse::json(
        200,
        br#"{"data":[{"id":"openai/gpt-5.6-sol"},{"id":"deepseek/deepseek-v4-pro-0813"}]}"#
            .to_vec(),
    )])
    .await;
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some(TEST_MASTER_KEY);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "kilocode-provider",
            name: "Kilo Code",
            base_url: &format!("http://{address}/api/openrouter"),
            adapter_id: KILOCODE_ADAPTER_ID,
            auth_type: "kilocode_oauth",
            model_prefix: "kc",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    save_oauth_credential(
        &database.db,
        "kilocode-provider",
        "account-1",
        "access-token",
    )
    .await;

    let models = discover_models(&state, "account-1")
        .await
        .expect("catalog discovery");
    let ModelDiscoveryResult::Available { models, truncated } = models else {
        panic!("the catalog must be discoverable");
    };
    assert_eq!(
        models,
        vec![
            "openai/gpt-5.6-sol".to_owned(),
            "deepseek/deepseek-v4-pro-0813".to_owned()
        ]
    );
    assert!(!truncated);

    let requests = server.await.expect("mock upstream");
    assert_eq!(
        requests[0].authorization.as_deref(),
        Some("Bearer access-token")
    );
    assert_eq!(
        requests[0].header("x-kilocode-editorname"),
        Some(EDITOR_NAME)
    );
    assert_eq!(
        requests[0].request_line,
        "GET /api/openrouter/models HTTP/1.1"
    );
}

#[test]
fn device_endpoint_uses_the_provider_host_and_the_fixed_path() {
    // The row stores the OpenRouter root, so the device grant must still land
    // on the same origin the provider routes requests to.
    for base_url in [
        "https://api.kilo.ai/api/openrouter",
        "https://api.kilo.ai/api/openrouter/chat/completions",
        "https://api.kilo.ai/api/openrouter/",
    ] {
        assert_eq!(
            device_auth_url(base_url).expect("device endpoint").as_str(),
            "https://api.kilo.ai/api/device-auth/codes",
            "{base_url} must resolve to the Kilo Code device endpoint"
        );
    }
    // A self-hosted deployment signs in against its own host and port.
    assert_eq!(
        device_auth_url("http://127.0.0.1:8412/api/openrouter")
            .expect("self-hosted device endpoint")
            .as_str(),
        "http://127.0.0.1:8412/api/device-auth/codes"
    );
    for invalid in ["not a url", "mailto:ops@example.com", "/api/openrouter"] {
        assert!(
            device_auth_url(invalid).is_err(),
            "{invalid} must not resolve to a device endpoint"
        );
    }
}

/// The device grant is exercised through the real HTTP path, because the
/// adapter used to hard-code the public Kilo Code endpoint and no test could
/// reach it. Both requests must go to the provider row's host.
#[tokio::test]
async fn device_sign_in_starts_and_polls_against_the_provider_endpoint() {
    let (address, server) = spawn_mock_http_server(vec![
        MockResponse::json(
            200,
            br#"{"code":"device-code-1","verificationUrl":"https://app.kilo.ai/device","expiresIn":120}"#
                .to_vec(),
        ),
        MockResponse::json(
            200,
            br#"{"status":"approved","token":"access-token","userEmail":"Operator@Example.com"}"#
                .to_vec(),
        ),
    ])
    .await;
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some(TEST_MASTER_KEY);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    let base_url = format!("http://{address}/api/openrouter");

    let authorization = start_device_authorization(KILOCODE_ADAPTER_ID, &state, &base_url)
        .await
        .expect("device authorization");
    assert_eq!(authorization.device_code, "device-code-1");
    assert_eq!(authorization.user_code, "device-code-1");
    assert_eq!(authorization.verification_uri, "https://app.kilo.ai/device");
    assert_eq!(authorization.expires_in.as_secs(), 120);
    assert_eq!(authorization.interval.as_secs(), 3);

    let poll = poll_device_authorization(KILOCODE_ADAPTER_ID, &state, &base_url, "device-code-1")
        .await
        .expect("device poll");
    let AdapterDevicePoll::Approved(account) = poll else {
        panic!("an approved poll must yield the granted account");
    };
    assert_eq!(account.display_name, "Operator@Example.com");
    let parsed = KilocodeAccount::from_device_approval(&account.payload).expect("usable account");
    assert_eq!(parsed.access_token, "access-token");

    let requests = server.await.expect("mock upstream");
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].request_line,
        "POST /api/device-auth/codes HTTP/1.1"
    );
    assert_eq!(
        requests[1].request_line,
        "GET /api/device-auth/codes/device-code-1 HTTP/1.1"
    );
    // The grant request is unauthenticated, so it must not carry a credential.
    assert_eq!(requests[0].authorization, None);
    assert!(matches!(
        requests[0].header("content-length"),
        None | Some("0")
    ));
}

/// Stores one enabled Kilo Code OAuth credential the way a completed sign-in
/// would, using the test master key.
async fn save_oauth_credential(
    db: &crate::infra::storage::Database,
    provider_id: &str,
    key_id: &str,
    access_token: &str,
) {
    let account = KilocodeAccount {
        access_token: access_token.to_owned(),
        email: Some("operator@example.com".to_owned()),
    };
    let serialized = serde_json::to_string(&account).expect("encode account");
    let encrypted = crate::security::encrypt_secret(Some(&TEST_MASTER_KEY), &serialized)
        .expect("encrypt")
        .expect("non-empty encrypted account");
    let now = crate::infra::db::utc_timestamp_now().expect("timestamp");
    let mut record = crate::admin::providers::provider_api_key_record(
        crate::admin::providers::ProviderApiKeyRecordData {
            id: key_id,
            provider_id,
            name: "Kilo Code account",
            secret: &encrypted,
            last_error: None,
            last_test_passed: true,
            last_test_status: Some(200),
            tested_at: &now,
            created_at: &now,
        },
    );
    record.insert("credential_type", Field::Text("oauth".to_owned()));
    let provider_id = provider_id.to_owned();
    db.write(move |transaction| {
        crate::admin::providers::store_provider_api_key(transaction, &record)?;
        if let Some(mut provider) = transaction.get::<Record>(Table::Providers, &provider_id)? {
            provider.insert(
                "api_key_count",
                Field::I64(provider.integer("api_key_count")?.saturating_add(1)),
            );
            transaction.put(Table::Providers, &provider_id, &provider)?;
        }
        Ok(())
    })
    .await
    .expect("store oauth credential");
}
