use super::*;
use crate::infra::storage::Record;
use crate::provider_adapters::ANTIGRAVITY_ADAPTER_ID;
use crate::support::test_support::{ProviderSeed, TestDatabase, seed_provider};

fn test_account(access_token: &str, email: &str) -> AntigravityAccount {
    AntigravityAccount {
        access_token: access_token.to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
        expires_at: unix_now().saturating_add(3_600),
        scope: Some("scope".to_owned()),
        email: Some(email.to_owned()),
        project_id: Some("project-id".to_owned()),
        tier: Some("tier".to_owned()),
        client_profile: "ide".to_owned(),
    }
}

#[tokio::test]
async fn reconnecting_the_same_google_account_updates_one_credential() {
    let database = TestDatabase::open().await;
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "antigravity-provider",
            name: "Antigravity",
            base_url: super::super::RUNTIME_BASE_URL,
            adapter_id: ANTIGRAVITY_ADAPTER_ID,
            auth_type: "oauth",
            model_prefix: "ag",
            preferred_protocol: "google_generate_content",
            supported_protocols: &["google_generate_content"],
        },
    )
    .await
    .expect("seed Antigravity provider");

    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    let state = AppState::new(config, database.db.clone());

    save_account(
        &state,
        "antigravity-provider",
        test_account("access-token-1", "Account@example.com"),
    )
    .await
    .expect("save first Antigravity account");

    let first = state
        .db
        .read(|transaction| {
            let prefix = crate::infra::db::provider_api_key_created_prefix("antigravity-provider")?;
            let (_, id) = transaction
                .scan_prefix::<String>(Table::ProviderApiKeyCreatedIndex, &prefix, 1)?
                .into_iter()
                .next()
                .ok_or(StorageError::NotFound)?;
            transaction
                .get::<Record>(Table::ProviderApiKeys, &id)?
                .ok_or(StorageError::NotFound)
        })
        .await
        .expect("read first Antigravity account");
    state
        .db
        .write(move |transaction| {
            let mut duplicate = first.clone();
            duplicate.insert(
                "id",
                Field::Text("duplicate-antigravity-account".to_owned()),
            );
            crate::admin::providers::store_provider_api_key(transaction, &duplicate)?;
            let mut provider = transaction
                .get::<Record>(Table::Providers, "antigravity-provider")?
                .ok_or(StorageError::NotFound)?;
            provider.insert(
                "api_key_count",
                Field::I64(provider.integer("api_key_count")?.saturating_add(1)),
            );
            transaction.put(Table::Providers, "antigravity-provider", &provider)
        })
        .await
        .expect("seed duplicate Antigravity account");

    save_account(
        &state,
        "antigravity-provider",
        test_account("access-token-2", "account@example.com"),
    )
    .await
    .expect("reconnect Antigravity account");

    let (provider, accounts) = state
        .db
        .read(|transaction| {
            let provider = transaction
                .get::<Record>(Table::Providers, "antigravity-provider")?
                .ok_or(StorageError::NotFound)?;
            let prefix = crate::infra::db::provider_api_key_created_prefix("antigravity-provider")?;
            let indexes = transaction.scan_prefix::<String>(
                Table::ProviderApiKeyCreatedIndex,
                &prefix,
                usize::MAX,
            )?;
            let mut accounts = Vec::new();
            for (_, id) in indexes {
                let Some(record) = transaction.get::<Record>(Table::ProviderApiKeys, &id)? else {
                    continue;
                };
                accounts.push(record);
            }
            Ok((provider, accounts))
        })
        .await
        .expect("read saved Antigravity account");

    assert_eq!(provider.integer("api_key_count").expect("key count"), 1);
    assert_eq!(accounts.len(), 1);
    assert_eq!(
        accounts[0].text("name").expect("account name"),
        "account@example.com"
    );
    assert_eq!(
        accounts[0]
            .optional_text("oauth_account_identity")
            .expect("account identity"),
        Some("account@example.com")
    );
    let encrypted = accounts[0].bytes("secret").expect("account secret");
    let serialized = security::decrypt_secret(state.config.master_key.as_ref(), Some(encrypted))
        .expect("decrypt account secret")
        .expect("account secret payload");
    let account: AntigravityAccount =
        serde_json::from_str(&serialized).expect("decode account secret");
    assert_eq!(account.access_token, "access-token-2");
}
