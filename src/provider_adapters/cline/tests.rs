use super::auth::cline::ClineAccount;
use super::*;
use crate::support::test_support::{ProviderSeed, TestDatabase, seed_provider};
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    security,
};
use serde_json::json;

fn test_account(access_token: &str, account_id: &str, email: &str) -> ClineAccount {
    ClineAccount {
        access_token: access_token.to_owned(),
        refresh_token: Some("refresh-token".to_owned()),
        expires_at: 2_000_000_000,
        account_id: Some(account_id.to_owned()),
        email: Some(email.to_owned()),
        name: Some("Account A".to_owned()),
    }
}

async fn save_test_account(
    state: &AppState,
    provider_id: &str,
    account: ClineAccount,
) -> Result<(), String> {
    save_cline_account(
        CLINE_ADAPTER_ID,
        state,
        provider_id,
        AdapterOAuthAccount {
            payload: serde_json::to_value(account).map_err(|error| error.to_string())?,
            display_name: "Account A".to_owned(),
        },
    )
    .await
}

#[tokio::test]
async fn reconnecting_cline_account_overwrites_usable_and_invalid_rows() {
    let database = TestDatabase::open().await;
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "cline-provider",
            name: "Cline",
            base_url: COMPLETIONS_URL,
            adapter_id: CLINE_ADAPTER_ID,
            auth_type: "oauth",
            model_prefix: "cline-test",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed Cline provider");

    let mut config = database.config();
    config.master_key = Some([47_u8; 32]);
    let state = AppState::new(config, database.db.clone());
    save_test_account(
        &state,
        "cline-provider",
        test_account("access-token-1", "account-a", "Account@example.com"),
    )
    .await
    .expect("save first Cline account");

    let first_id = database
        .db
        .read(|transaction| {
            let prefix = crate::infra::db::provider_api_key_created_prefix("cline-provider")?;
            transaction
                .scan_prefix::<String>(Table::ProviderApiKeyCreatedIndex, &prefix, 2)?
                .into_iter()
                .next()
                .map(|(_, id)| id)
                .ok_or(StorageError::NotFound)
        })
        .await
        .expect("read first Cline account");

    save_test_account(
        &state,
        "cline-provider",
        test_account("access-token-2", "account-a", "account@example.com"),
    )
    .await
    .expect("reconnect usable Cline account");

    database
        .db
        .write({
            let first_id = first_id.clone();
            move |transaction| {
                let mut key = transaction
                    .get::<Record>(Table::ProviderApiKeys, &first_id)?
                    .ok_or(StorageError::NotFound)?;
                let identity_index =
                    crate::admin::providers::provider_api_key_identity_index_key(&key)?
                        .ok_or(StorageError::NotFound)?;
                transaction.delete(Table::ProviderApiKeyIndex, &identity_index)?;
                key.insert("oauth_account_identity", Field::Null);
                key.insert("invalid", Field::Bool(true));
                key.insert(
                    "last_error",
                    Field::Text("Cline account must be reconnected".to_owned()),
                );
                key.insert("last_test_passed", Field::Bool(false));
                let provider_id = key.text("provider_id")?.to_owned();
                transaction.delete(
                    Table::ProviderApiKeyAvailabilityIndex,
                    &crate::infra::db::provider_api_key_index_key(&provider_id, &first_id, true)?,
                )?;
                transaction.put(Table::ProviderApiKeys, &first_id, &key)?;
                let mut provider = transaction
                    .get::<Record>(Table::Providers, &provider_id)?
                    .ok_or(StorageError::NotFound)?;
                provider.insert(
                    "invalid_api_key_count",
                    Field::I64(provider.integer("invalid_api_key_count")?.saturating_add(1)),
                );
                transaction.put(Table::Providers, &provider_id, &provider)
            }
        })
        .await
        .expect("mark Cline account invalid");

    save_test_account(
        &state,
        "cline-provider",
        test_account("access-token-3", "account-a", "ACCOUNT@example.com"),
    )
    .await
    .expect("reconnect invalid Cline account");

    let key_id_for_read = first_id.clone();
    let (provider, key, key_count) = database
        .db
        .read(move |transaction| {
            let provider = transaction
                .get::<Record>(Table::Providers, "cline-provider")?
                .ok_or(StorageError::NotFound)?;
            let prefix = crate::infra::db::provider_api_key_created_prefix("cline-provider")?;
            let keys =
                transaction.scan_prefix::<String>(Table::ProviderApiKeyCreatedIndex, &prefix, 2)?;
            let key = transaction
                .get::<Record>(Table::ProviderApiKeys, &key_id_for_read)?
                .ok_or(StorageError::NotFound)?;
            Ok((provider, key, keys.len()))
        })
        .await
        .expect("read reconnected Cline account");

    assert_eq!(key_count, 1);
    assert_eq!(provider.integer("api_key_count").expect("key count"), 1);
    assert_eq!(
        provider
            .integer("invalid_api_key_count")
            .expect("invalid key count"),
        0
    );
    assert_eq!(key.text("id").expect("key id"), first_id);
    assert!(!key.boolean("invalid").expect("invalid flag"));
    assert_eq!(key.optional_text("last_error").expect("last error"), None);
    assert_eq!(
        key.optional_text("oauth_account_identity")
            .expect("account identity"),
        Some("account:account-a")
    );
    let available = crate::infra::db::provider_api_key_index_key("cline-provider", &first_id, true)
        .expect("availability index");
    assert_eq!(
        database
            .db
            .read(move |transaction| transaction
                .get::<String>(Table::ProviderApiKeyAvailabilityIndex, &available))
            .await
            .expect("read availability index")
            .as_deref(),
        Some(first_id.as_str())
    );
    let serialized = security::decrypt_secret(
        state.config.master_key.as_ref(),
        Some(key.bytes("secret").expect("secret")),
    )
    .expect("decrypt Cline account")
    .expect("Cline account payload");
    let saved: ClineAccount = serde_json::from_str(&serialized).expect("decode Cline account");
    assert_eq!(saved.access_token, "access-token-3");
}

#[test]
fn endpoints_are_fixed_and_model_namespaces_are_isolated() {
    let cline = ClineAdapter {
        adapter_id: CLINE_ADAPTER_ID,
    };
    assert_eq!(
        cline
            .endpoint("https://attacker.invalid", Protocol::ChatCompletions, None)
            .expect("Cline endpoint")
            .as_str(),
        COMPLETIONS_URL
    );
    assert!(
        cline
            .endpoint("https://example.invalid", Protocol::Responses, None)
            .is_err()
    );
    assert!(
        cline
            .upstream_endpoint(
                "https://example.invalid",
                UpstreamProtocol::ChatCompletions,
                "cline-pass/model",
                false,
                None,
            )
            .is_err()
    );

    let pass = ClineAdapter {
        adapter_id: CLINEPASS_ADAPTER_ID,
    };
    assert_eq!(
        pass.upstream_endpoint(
            "https://attacker.invalid",
            UpstreamProtocol::ChatCompletions,
            "cline-pass/model",
            true,
            None,
        )
        .expect("ClinePass endpoint")
        .as_str(),
        COMPLETIONS_URL
    );
    assert!(
        pass.upstream_endpoint(
            "https://example.invalid",
            UpstreamProtocol::ChatCompletions,
            "provider/model",
            false,
            None,
        )
        .is_err()
    );
}

#[test]
fn clinepass_envelope_is_normalized_only_when_valid() {
    let pass = ClineAdapter {
        adapter_id: CLINEPASS_ADAPTER_ID,
    };
    assert_eq!(
        pass.normalize_response(json!({
            "success": true,
            "data": {"id": "response-1"},
            "requestId": "redacted"
        }))
        .expect("valid envelope"),
        json!({"id": "response-1"})
    );
    assert!(pass.normalize_response(json!({"success": false})).is_err());
    assert!(
        pass.normalize_response(json!({"success": true, "data": []}))
            .is_err()
    );
    assert_eq!(
        pass.normalize_response(json!({"id": "plain"}))
            .expect("plain response"),
        json!({"id": "plain"})
    );
}

#[test]
fn request_headers_and_oauth_token_shape_match_cline_contract() {
    let cline = ClineAdapter {
        adapter_id: CLINE_ADAPTER_ID,
    };
    let client = reqwest::Client::new();
    let mut inbound = http::HeaderMap::new();
    inbound.insert("x-task-id", http::HeaderValue::from_static("task-42"));
    let request = cline
        .apply_client_headers(client.post(COMPLETIONS_URL), &inbound)
        .build()
        .expect("build Cline request");
    assert_eq!(
        request
            .headers()
            .get("x-task-id")
            .and_then(|value| value.to_str().ok()),
        Some("task-42")
    );
    assert_eq!(
        request
            .headers()
            .get("x-client-type")
            .and_then(|value| value.to_str().ok()),
        Some("omniroute")
    );
    let request = cline
        .apply_request_auth(
            client.post(COMPLETIONS_URL),
            "bearer",
            None,
            None,
            Some(&OAuthRequestAuth {
                access_token: "oauth-token".to_owned(),
                account_id: None,
                project_id: None,
            }),
            "request",
        )
        .expect("apply OAuth auth")
        .build()
        .expect("build OAuth request");
    assert_eq!(
        request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer workos:oauth-token")
    );
    let request = cline
        .apply_request_auth(
            client.post(COMPLETIONS_URL),
            "bearer",
            None,
            Some("sk-cline"),
            None,
            "request",
        )
        .expect("apply API key auth")
        .build()
        .expect("build API key request");
    assert_eq!(
        request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer sk-cline")
    );
}

#[test]
fn model_catalog_filters_free_and_cross_namespace_entries() {
    let recommended = parse_cline_recommended_models(&json!({
        "recommended": [
            {"id": "anthropic/claude"},
            {"id": "openrouter/free:free"},
            {"id": "cline-pass/other"}
        ]
    }))
    .expect("recommended catalog");
    assert_eq!(recommended, ["anthropic/claude"]);

    let models = parse_cline_models(&json!({
        "data": [
            {"id": "anthropic/claude", "architecture": {"modality": "text->text"}},
            {"id": "openrouter/free:free", "architecture": {"modality": "text->text"}},
            {"id": "anthropic/image", "architecture": {"modality": "text->image"}},
            {"id": "cline-pass/other", "architecture": {"modality": "text->text"}}
        ]
    }))
    .expect("model catalog");
    assert_eq!(models, ["anthropic/claude"]);

    let pass = parse_clinepass_models(&json!({
        "data": {"clinePass": [{"id": "cline-pass/model"}, {"id": "anthropic/model"}]}
    }))
    .expect("ClinePass catalog");
    assert_eq!(pass, ["cline-pass/model"]);
}
