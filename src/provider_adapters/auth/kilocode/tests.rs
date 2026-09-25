use super::*;
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    provider_adapters::{AdapterOAuthAccount, KILOCODE_ADAPTER_ID, save_oauth_account},
    state::AppState,
    support::test_support::{ProviderSeed, TestDatabase, seed_provider},
};
use serde_json::json;

const TEST_MASTER_KEY: [u8; 32] = [47_u8; 32];

fn approval(token: &str, email: Option<&str>) -> AdapterOAuthAccount {
    AdapterOAuthAccount {
        payload: json!({"status": "approved", "token": token, "userEmail": email}),
        display_name: "Kilo Code account".to_owned(),
    }
}

async fn state_with_provider() -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some(TEST_MASTER_KEY);
    let state = AppState::new(config, database.db.clone());
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "kilocode-provider",
            name: "Kilo Code",
            base_url: "https://api.kilo.ai/api/openrouter",
            adapter_id: KILOCODE_ADAPTER_ID,
            auth_type: "kilocode_oauth",
            model_prefix: "kc",
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
    let account = KilocodeAccount::from_device_approval(&json!({
        "status": "approved",
        "token": "one.two-three_four",
        "userEmail": "operator@example.com",
    }))
    .expect("usable approval");
    assert_eq!(account.access_token, "one.two-three_four");
    assert_eq!(account.display_name(), "operator@example.com");

    for invalid in [
        json!({"status": "approved"}),
        json!({"status": "approved", "token": ""}),
        json!({"status": "approved", "token": "has space"}),
        json!({"status": "approved", "token": "line\nbreak"}),
        json!({"status": "approved", "token": 42}),
    ] {
        assert!(
            KilocodeAccount::from_device_approval(&invalid).is_err(),
            "expected rejection: {invalid}"
        );
    }
    let oversized = "a".repeat(MAX_TOKEN_BYTES + 1);
    assert!(
        KilocodeAccount::from_device_approval(&json!({"status": "approved", "token": oversized}))
            .is_err()
    );
}

#[test]
fn identity_prefers_the_email_and_never_repeats_the_token() {
    let with_email = KilocodeAccount {
        access_token: "secret-token".to_owned(),
        email: Some("  Operator@Example.com ".to_owned()),
    };
    assert_eq!(with_email.identity(), "email:operator@example.com");

    let without_email = KilocodeAccount {
        access_token: "secret-token".to_owned(),
        email: None,
    };
    let identity = without_email.identity();
    assert!(identity.starts_with("token:"));
    assert!(!identity.contains("secret-token"));
    assert_eq!(
        identity,
        KilocodeAccount {
            access_token: "secret-token".to_owned(),
            email: None,
        }
        .identity(),
        "the same token must map to the same identity"
    );
    assert_ne!(
        identity,
        KilocodeAccount {
            access_token: "other-token".to_owned(),
            email: None,
        }
        .identity()
    );
}

#[tokio::test]
async fn reconnecting_the_same_account_updates_one_credential() {
    let (database, state) = state_with_provider().await;
    save_oauth_account(
        KILOCODE_ADAPTER_ID,
        &state,
        "kilocode-provider",
        approval("first-token", Some("operator@example.com")),
    )
    .await
    .expect("first sign-in");
    save_oauth_account(
        KILOCODE_ADAPTER_ID,
        &state,
        "kilocode-provider",
        approval("second-token", Some("Operator@Example.com")),
    )
    .await
    .expect("reconnection");

    let credentials = credential_records(&database).await;
    assert_eq!(credentials.len(), 1, "a reconnection must not duplicate");
    let record = &credentials[0];
    assert_eq!(
        record.optional_text("oauth_account_identity").unwrap(),
        Some("email:operator@example.com")
    );
    let account = account_for_use(&state, record.text("id").unwrap())
        .await
        .expect("stored account");
    assert_eq!(account.access_token, "second-token");

    save_oauth_account(
        KILOCODE_ADAPTER_ID,
        &state,
        "kilocode-provider",
        approval("third-token", Some("second@example.com")),
    )
    .await
    .expect("second account");
    assert_eq!(
        credential_records(&database).await.len(),
        2,
        "a different account keeps its own credential"
    );
}

#[tokio::test]
async fn account_for_use_rejects_a_foreign_or_disabled_credential() {
    let (database, state) = state_with_provider().await;
    save_oauth_account(
        KILOCODE_ADAPTER_ID,
        &state,
        "kilocode-provider",
        approval("access-token", Some("operator@example.com")),
    )
    .await
    .expect("sign-in");
    let key_id = credential_records(&database).await[0]
        .text("id")
        .unwrap()
        .to_owned();
    assert!(account_for_use(&state, &key_id).await.is_ok());
    assert!(
        account_for_use(&state, "missing-credential").await.is_err(),
        "an unknown credential must not authenticate"
    );

    let id = key_id.clone();
    database
        .db
        .write(move |transaction| {
            let mut record = transaction
                .get::<Record>(Table::ProviderApiKeys, &id)?
                .expect("stored credential");
            record.insert("enabled", Field::Bool(false));
            transaction.put(Table::ProviderApiKeys, &id, &record)
        })
        .await
        .expect("disable credential");
    assert!(account_for_use(&state, &key_id).await.is_err());
}

#[tokio::test]
async fn provider_base_url_comes_from_the_provider_row() {
    let (database, state) = state_with_provider().await;
    save_oauth_account(
        KILOCODE_ADAPTER_ID,
        &state,
        "kilocode-provider",
        approval("access-token", Some("operator@example.com")),
    )
    .await
    .expect("sign-in");
    let key_id = credential_records(&database).await[0]
        .text("id")
        .unwrap()
        .to_owned();
    assert_eq!(
        provider_base_url(&state, &key_id).await.expect("base url"),
        "https://api.kilo.ai/api/openrouter"
    );
    assert!(
        provider_base_url(&state, "missing-credential")
            .await
            .is_err()
    );
}

/// Reads every credential stored for the test provider through its creation
/// index, which is the path the dashboard uses to list keys.
async fn credential_records(database: &TestDatabase) -> Vec<Record> {
    let prefix =
        crate::infra::db::provider_api_key_created_prefix("kilocode-provider").expect("key prefix");
    database
        .db
        .read(move |transaction| {
            let ids = transaction.scan_prefix::<String>(
                Table::ProviderApiKeyCreatedIndex,
                &prefix,
                64,
            )?;
            let mut records = Vec::with_capacity(ids.len());
            for (_, key_id) in ids {
                records.push(
                    transaction
                        .get::<Record>(Table::ProviderApiKeys, &key_id)?
                        .ok_or(StorageError::NotFound)?,
                );
            }
            Ok(records)
        })
        .await
        .expect("read credentials")
}
