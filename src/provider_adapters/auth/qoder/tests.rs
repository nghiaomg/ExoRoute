use super::*;
use crate::{
    infra::storage::{StorageError, Table},
    provider_adapters::{AdapterOAuthAccount, QODER_ADAPTER_ID, QODER_CN_ADAPTER_ID, save_oauth_account},
    state::AppState,
    support::test_support::{ProviderSeed, TestDatabase, seed_provider},
};
use serde_json::json;

const TEST_MASTER_KEY: [u8; 32] = [47_u8; 32];

fn approval(token: &str, email: Option<&str>, user_id: Option<&str>) -> AdapterOAuthAccount {
    AdapterOAuthAccount {
        payload: json!({
            "token": token,
            "email": email,
            "user_id": user_id,
            "display_name": email.or(user_id),
        }),
        display_name: email.or(user_id).unwrap_or("Qoder account").to_owned(),
    }
}

async fn state_with_provider(adapter_id: &'static str) -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some(TEST_MASTER_KEY);
    let state = AppState::new(config, database.db.clone());
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "qoder-provider",
            name: "Qoder",
            base_url: "https://api3.qoder.sh",
            adapter_id,
            auth_type: "qoder_oauth",
            model_prefix: "qd",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    (database, state)
}

#[test]
fn an_approval_must_carry_a_usable_bearer_token() {
    let account = QoderAccount::from_device_approval(&json!({
        "token": "dt-test-token-12345",
        "email": "operator@example.com",
        "user_id": "usr_9876",
    }))
    .expect("usable approval");
    assert_eq!(account.access_token, "dt-test-token-12345");
    assert_eq!(account.email.as_deref(), Some("operator@example.com"));
    assert_eq!(account.user_id.as_deref(), Some("usr_9876"));

    for invalid in [
        json!({"status": "approved"}),
        json!({"token": ""}),
        json!({"token": "has space"}),
        json!({"token": "line\nbreak"}),
        json!({"token": 42}),
    ] {
        assert!(
            QoderAccount::from_device_approval(&invalid).is_err(),
            "expected rejection: {invalid}"
        );
    }
    let oversized = "a".repeat(MAX_TOKEN_BYTES + 1);
    assert!(QoderAccount::from_device_approval(&json!({"token": oversized})).is_err());
}

#[test]
fn identity_priority_is_email_then_user_id_then_token_digest() {
    let with_email = QoderAccount {
        access_token: "secret-token".to_owned(),
        refresh_token: None,
        user_id: Some("usr_123".to_owned()),
        machine_id: None,
        organization_id: None,
        email: Some("  Operator@Example.com ".to_owned()),
        display_name: None,
        region: None,
        expires_at: None,
    };
    assert_eq!(with_email.identity(), "email:operator@example.com");

    let with_user_id = QoderAccount {
        access_token: "secret-token".to_owned(),
        refresh_token: None,
        user_id: Some(" Usr_123 ".to_owned()),
        machine_id: None,
        organization_id: None,
        email: None,
        display_name: None,
        region: None,
        expires_at: None,
    };
    assert_eq!(with_user_id.identity(), "qoder:usr_123");

    let token_only = QoderAccount {
        access_token: "secret-token".to_owned(),
        refresh_token: None,
        user_id: None,
        machine_id: None,
        organization_id: None,
        email: None,
        display_name: None,
        region: None,
        expires_at: None,
    };
    let identity = token_only.identity();
    assert!(identity.starts_with("token:"));
    assert!(!identity.contains("secret-token"));
}

#[tokio::test]
async fn saving_an_account_stores_and_encrypts_credential() {
    let (_database, state) = state_with_provider(QODER_ADAPTER_ID).await;
    save_oauth_account(
        QODER_ADAPTER_ID,
        &state,
        "qoder-provider",
        approval("dt-valid-token-1", Some("user@example.com"), Some("u1")),
    )
    .await
    .expect("save account");

    let key_id = state
        .db
        .read(|tx| {
            let index_prefix = crate::infra::db::provider_api_key_identity_index_prefix(
                "qoder-provider",
                "email:user@example.com",
            )?;
            let item = tx
                .scan_prefix::<String>(Table::ProviderApiKeyIndex, &index_prefix, 1)?
                .into_iter()
                .map(|(_, id)| id)
                .next();
            Ok::<_, StorageError>(item)
        })
        .await
        .expect("db read")
        .expect("key id");

    let loaded = account_for_use(&state, &key_id).await.expect("account for use");
    assert_eq!(loaded.access_token, "dt-valid-token-1");
    assert_eq!(loaded.email.as_deref(), Some("user@example.com"));

    // Reconnecting the same account updates the existing key
    save_oauth_account(
        QODER_ADAPTER_ID,
        &state,
        "qoder-provider",
        approval("dt-valid-token-2", Some("user@example.com"), Some("u1")),
    )
    .await
    .expect("save account reconnect");

    let updated = account_for_use(&state, &key_id).await.expect("account for use updated");
    assert_eq!(updated.access_token, "dt-valid-token-2");
}

#[tokio::test]
async fn saving_account_works_for_qoder_cn_adapter() {
    let (_database, state) = state_with_provider(QODER_CN_ADAPTER_ID).await;
    save_oauth_account(
        QODER_CN_ADAPTER_ID,
        &state,
        "qoder-provider",
        approval("dt-valid-cn-token", Some("cn_user@example.cn"), Some("cn1")),
    )
    .await
    .expect("save qoder-cn account");

    let key_id = state
        .db
        .read(|tx| {
            let index_prefix = crate::infra::db::provider_api_key_identity_index_prefix(
                "qoder-provider",
                "email:cn_user@example.cn",
            )?;
            let item = tx
                .scan_prefix::<String>(Table::ProviderApiKeyIndex, &index_prefix, 1)?
                .into_iter()
                .map(|(_, id)| id)
                .next();
            Ok::<_, StorageError>(item)
        })
        .await
        .expect("db read")
        .expect("key id");

    let loaded = account_for_use(&state, &key_id).await.expect("account for use");
    assert_eq!(loaded.access_token, "dt-valid-cn-token");
}
