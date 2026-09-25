use super::support::*;
use super::*;

#[test]
fn codex_model_parser_accepts_live_slug_catalog_and_filters_hidden_models() {
    let models = parse_codex_model_list(&json!({
        "models": [
            {
                "slug": "gpt-5.6-luna",
                "display_name": "GPT 5.6 Luna",
                "visibility": "list",
                "supported_in_api": true
            },
            {
                "slug": "hidden-model",
                "visibility": "hide",
                "supported_in_api": true
            },
            {
                "slug": "internal-model",
                "visibility": "list",
                "supported_in_api": false
            },
            { "id": "gpt-5.5" },
            { "name": "gpt-5.5" }
        ]
    }))
    .expect("Codex live catalog");

    assert_eq!(models, ["gpt-5.6-luna", "gpt-5.5"]);
}

#[test]
fn codex_model_parser_accepts_root_arrays_and_model_maps() {
    let root_array = parse_codex_model_list(&json!([
        { "model": "gpt-5.5" },
        { "id": "gpt-5.4" }
    ]))
    .expect("Codex root array");
    assert_eq!(root_array, ["gpt-5.5", "gpt-5.4"]);

    let model_map = parse_codex_model_list(&json!({
        "gpt-5.6-luna": { "title": "GPT 5.6 Luna" },
        "gpt-5.5": { "title": "GPT 5.5" }
    }))
    .expect("Codex model map");
    assert_eq!(model_map, ["gpt-5.5", "gpt-5.6-luna"]);
}

#[test]
fn codex_model_request_uses_backend_identity_headers() {
    let account = CodexAccount {
        access_token: "access-token".to_owned(),
        refresh_token: None,
        id_token: None,
        expires_at: 0,
        account_id: Some("account-123".to_owned()),
        chatgpt_user_id: None,
        email: None,
        plan: None,
    };
    let url = reqwest::Url::parse("https://chatgpt.com/backend-api/codex/models").unwrap();
    let request = build_codex_model_request(&reqwest::Client::new(), &url, &account)
        .expect("Codex model request")
        .build()
        .expect("build Codex model request");

    assert_eq!(request.headers()["authorization"], "Bearer access-token");
    assert_eq!(request.headers()["originator"], "codex_cli_rs");
    assert_eq!(request.headers()["version"], codex_oauth::CODEX_CLI_VERSION);
    assert_eq!(request.headers()["openai-beta"], "responses=experimental");
    assert_eq!(
        request.headers()["x-codex-beta-features"],
        "responses_websockets"
    );
    assert_eq!(request.headers()["chatgpt-account-id"], "account-123");
}

#[test]
fn codex_account_match_requires_the_same_workspace_and_user() {
    let incoming = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let same_user = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let different_user = codex_account_for_match(
        Some("workspace-1"),
        Some("user-2"),
        Some("user@example.com"),
    );

    let mut same_state = CodexAccountMatchState::default();
    same_state.consider("same", &same_user, &incoming);
    assert_eq!(same_state.selected_id().as_deref(), Some("same"));

    let mut different_state = CodexAccountMatchState::default();
    different_state.consider("different", &different_user, &incoming);
    assert_eq!(different_state.selected_id(), None);
}

#[test]
fn codex_account_match_rejects_ambiguous_legacy_rows() {
    let incoming = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let legacy = codex_account_for_match(Some("workspace-1"), None, Some("user@example.com"));
    let mut state = CodexAccountMatchState::default();
    state.consider("legacy-1", &legacy, &incoming);
    state.consider("legacy-2", &legacy, &incoming);

    assert_eq!(state.selected_id(), None);
}

#[test]
fn codex_account_match_can_upgrade_one_legacy_row() {
    let incoming = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let legacy = codex_account_for_match(Some("workspace-1"), None, Some("user@example.com"));
    let mut state = CodexAccountMatchState::default();
    state.consider("legacy", &legacy, &incoming);

    assert_eq!(state.selected_id().as_deref(), Some("legacy"));
}

#[test]
fn codex_account_match_accepts_a_callback_with_partial_metadata() {
    let previous = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("user@example.com"),
    );
    let incoming = codex_account_for_match(Some("workspace-1"), None, Some("USER@example.com"));
    let mut state = CodexAccountMatchState::default();
    state.consider("existing", &previous, &incoming);

    assert_eq!(state.selected_id().as_deref(), Some("existing"));
}

#[test]
fn codex_account_match_can_use_email_when_a_legacy_row_lacks_account_id() {
    let previous = codex_account_for_match(None, Some("user-1"), Some("user@example.com"));
    let incoming = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("USER@example.com"),
    );
    let mut state = CodexAccountMatchState::default();
    state.consider("legacy", &previous, &incoming);

    assert_eq!(state.selected_id().as_deref(), Some("legacy"));
}

#[tokio::test]
async fn saving_distinct_and_reconnected_codex_accounts_keeps_existing_credentials() {
    let (database, state) = test_state().await;
    seed_provider(
        &database.db,
        ProviderSeed {
            id: "codex-provider",
            name: "Codex",
            base_url: codex_oauth::BASE_URL,
            adapter_id: CODEX_ADAPTER_ID,
            auth_type: "oauth",
            model_prefix: "codex-test",
            preferred_protocol: "responses",
            supported_protocols: &["responses"],
        },
    )
    .await
    .expect("seed Codex provider");

    for (user_id, access_token) in [("user-1", "token-1"), ("user-2", "token-2")] {
        let account = codex_account_for_match(
            Some("workspace-1"),
            Some(user_id),
            Some("shared@example.com"),
        );
        save_codex_account(
            &state,
            "codex-provider",
            AdapterOAuthAccount {
                payload: serde_json::to_value(CodexAccount {
                    access_token: access_token.to_owned(),
                    ..account
                })
                .expect("serialize Codex account"),
                display_name: user_id.to_owned(),
            },
        )
        .await
        .expect("save Codex account");
    }

    let account = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("shared@example.com"),
    );
    save_codex_account(
        &state,
        "codex-provider",
        AdapterOAuthAccount {
            payload: serde_json::to_value(CodexAccount {
                access_token: "token-1-refreshed".to_owned(),
                ..account
            })
            .expect("serialize refreshed Codex account"),
            display_name: "user-1 refreshed".to_owned(),
        },
    )
    .await
    .expect("refresh Codex account");

    let master_key = state.config.master_key;
    let keys = database
        .db
        .read(move |transaction| {
            let keys = transaction
                .scan_prefix::<Record>(Table::ProviderApiKeys, "", 128)?
                .into_iter()
                .map(|(_, record)| {
                    let secret = record.bytes("secret").expect("Codex key secret");
                    let serialized = security::decrypt_secret(master_key.as_ref(), Some(secret))
                        .expect("decrypt Codex key")
                        .expect("Codex key payload");
                    let account: CodexAccount =
                        serde_json::from_str(&serialized).expect("decode Codex key");
                    (record.text("id").expect("key id").to_owned(), account)
                })
                .collect::<Vec<_>>();
            Ok(keys)
        })
        .await
        .expect("read Codex key index");

    assert_eq!(keys.len(), 2);
    let (reconnected_id, _user_1) = keys
        .iter()
        .find(|(_, account)| account.chatgpt_user_id.as_deref() == Some("user-1"))
        .expect("user-1 Codex key")
        .clone();
    let (other_id, _user_2) = keys
        .iter()
        .find(|(_, account)| account.chatgpt_user_id.as_deref() == Some("user-2"))
        .expect("user-2 Codex key")
        .clone();
    assert_ne!(reconnected_id, other_id);
    database
        .db
        .write({
            let reconnected_id = reconnected_id.clone();
            move |transaction| {
                let mut key = transaction
                    .get::<Record>(Table::ProviderApiKeys, &reconnected_id)?
                    .ok_or(StorageError::NotFound)?;
                key.insert("invalid", Field::Bool(true));
                key.insert(
                    "last_error",
                    Field::Text("OpenAI Codex account needs to be reconnected".to_owned()),
                );
                key.insert("last_test_passed", Field::Bool(false));
                let provider_id = key.text("provider_id")?.to_owned();
                transaction.delete(
                    Table::ProviderApiKeyAvailabilityIndex,
                    &crate::infra::db::provider_api_key_index_key(
                        &provider_id,
                        &reconnected_id,
                        true,
                    )?,
                )?;
                transaction.put(Table::ProviderApiKeys, &reconnected_id, &key)?;
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
        .expect("mark Codex account invalid");

    let account = codex_account_for_match(
        Some("workspace-1"),
        Some("user-1"),
        Some("shared@example.com"),
    );
    save_codex_account(
        &state,
        "codex-provider",
        AdapterOAuthAccount {
            payload: serde_json::to_value(CodexAccount {
                access_token: "token-1-reconnected".to_owned(),
                ..account
            })
            .expect("serialize reconnected Codex account"),
            display_name: "user-1 reconnected".to_owned(),
        },
    )
    .await
    .expect("reconnect invalid Codex account");

    let (provider, saved) = database
        .db
        .read(move |transaction| {
            let provider = transaction
                .get::<Record>(Table::Providers, "codex-provider")?
                .ok_or(StorageError::NotFound)?;
            let key = transaction
                .get::<Record>(Table::ProviderApiKeys, &reconnected_id)?
                .ok_or(StorageError::NotFound)?;
            Ok((provider, key))
        })
        .await
        .expect("read reconnected Codex account");
    assert_eq!(provider.integer("api_key_count").expect("key count"), 2);
    assert_eq!(
        provider
            .integer("invalid_api_key_count")
            .expect("invalid key count"),
        0
    );
    assert!(!saved.boolean("invalid").expect("invalid flag"));
    let serialized = security::decrypt_secret(
        state.config.master_key.as_ref(),
        Some(saved.bytes("secret").expect("secret")),
    )
    .expect("decrypt reconnected Codex account")
    .expect("Codex account payload");
    let saved_account: CodexAccount =
        serde_json::from_str(&serialized).expect("decode reconnected Codex account");
    assert_eq!(saved_account.access_token, "token-1-reconnected");
}
