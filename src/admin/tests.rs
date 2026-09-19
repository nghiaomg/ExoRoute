//! Admin barrel regression tests: preset catalog, provider validation,
//! model discovery parsing, failed-key behavior, and password policy.
//!
//! These tests exercise cross-module contracts, so they live at the barrel
//! level instead of inside a single feature module.

use super::{
    AppState, ProviderInput, list_provider_presets, normalized_provider_model_prefix,
    normalized_provider_model_prefix_for_adapter, password_needs_change, provider_adapters,
    providers, validate_provider,
};
use super::{Argon2, Json, PasswordHasher, SaltString};
use super::{MAX_ADMIN_PASSWORD_HASH_BYTES, is_supported_admin_password_hash};
use super::{hash_admin_password, json};
use crate::admin::providers::{parse_provider_model_page, provider_models_url};
use crate::{
    infra::storage::{Record, Table},
    provider_adapters::auth::codex,
    support::test_support::TestDatabase,
};
use axum::extract::{Json as AxumJson, State as AxumState};

#[tokio::test]
async fn provider_preset_catalog_exposes_categories_labels_and_adapter_ids() {
    let Json(value) = list_provider_presets()
        .await
        .expect("provider preset catalog");
    let presets = value["presets"]
        .as_array()
        .expect("preset list in response");
    let custom = presets
        .iter()
        .find(|preset| preset["id"] == "custom")
        .expect("custom preset");
    let codex = presets
        .iter()
        .find(|preset| preset["id"] == "openai_codex")
        .expect("Codex preset");
    let kilo = presets
        .iter()
        .find(|preset| preset["id"] == "kilo_gateway")
        .expect("Kilo Gateway preset");
    let command = presets
        .iter()
        .find(|preset| preset["id"] == "command_code")
        .expect("Command Code preset");
    assert_eq!(custom["adapter_id"], provider_adapters::GENERIC_ADAPTER_ID);
    assert_eq!(custom["category"], "custom");
    assert_eq!(custom["labels"].as_array().map(Vec::len), Some(0));
    assert_eq!(custom["protocol_selectable"], true);
    assert_eq!(
        custom["default_supported_protocols"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(codex["adapter_id"], provider_adapters::CODEX_ADAPTER_ID);
    assert_eq!(codex["category"], "oauth");
    assert_eq!(codex["protocol_selectable"], false);
    assert_eq!(codex["capabilities"]["oauth_accounts"], true);
    assert_eq!(
        kilo["adapter_id"],
        provider_adapters::KILO_GATEWAY_ADAPTER_ID
    );
    assert_eq!(kilo["category"], "gateway");
    assert_eq!(kilo["default_base_url"], "https://api.kilo.ai/api/gateway");
    assert_eq!(kilo["default_preferred_protocol"], "chat_completions");
    assert_eq!(kilo["capabilities"]["model_discovery"], true);
    assert_eq!(command["category"], "gateway");
}

#[test]
fn codex_provider_requires_its_fixed_responses_endpoint_without_api_keys() {
    let mut input = ProviderInput {
        id: String::new(),
        name: "OpenAI Codex".to_owned(),
        base_url: codex::BASE_URL.to_owned(),
        logo_url: None,
        model_prefix: None,
        adapter_id: Some(provider_adapters::CODEX_ADAPTER_ID.to_owned()),
        enabled: true,
        auth_type: "codex_oauth".to_owned(),
        auth_header: None,
        custom_headers: None,
        api_key: None,
        api_keys: Vec::new(),
        local_rpm_target: None,
        preferred_protocol: "responses".to_owned(),
        supported_protocols: vec!["responses".to_owned()],
    };
    assert!(validate_provider(&input, provider_adapters::CODEX_ADAPTER_ID, false).is_ok());
    assert!(validate_provider(&input, "unregistered", false).is_err());
    input.base_url = "https://attacker.example/backend-api/codex".to_owned();
    assert!(validate_provider(&input, provider_adapters::CODEX_ADAPTER_ID, false).is_err());
    input.base_url = codex::BASE_URL.to_owned();
    input.logo_url = Some("https://cdn.example.com/provider.png".to_owned());
    assert!(validate_provider(&input, provider_adapters::CODEX_ADAPTER_ID, false).is_ok());
    input.logo_url = Some("http://cdn.example.com/provider.png".to_owned());
    assert!(validate_provider(&input, provider_adapters::CODEX_ADAPTER_ID, false).is_err());
    input.logo_url = Some("https://127.0.0.1/provider.png".to_owned());
    assert!(validate_provider(&input, provider_adapters::CODEX_ADAPTER_ID, false).is_err());
    input.logo_url = Some(format!("https://cdn.example.com/{}", "x".repeat(2048)));
    assert!(validate_provider(&input, provider_adapters::CODEX_ADAPTER_ID, false).is_err());
    input.logo_url = None;
    input.api_keys.push("not-an-oauth-token".to_owned());
    assert!(validate_provider(&input, provider_adapters::CODEX_ADAPTER_ID, false).is_err());
}

#[test]
fn provider_model_prefixes_are_normalized_and_bounded() {
    assert_eq!(
        normalized_provider_model_prefix(None, "OpenAI Compatible").unwrap(),
        "openai-compatible"
    );
    assert_eq!(
        normalized_provider_model_prefix(Some(" My.OpenAI_vendor-1 "), "provider").unwrap(),
        "my.openai_vendor-1"
    );
    assert!(normalized_provider_model_prefix(Some("/openai"), "provider").is_err());
    assert!(normalized_provider_model_prefix(Some("openai/compat"), "provider").is_err());
    assert!(normalized_provider_model_prefix(Some(&"a".repeat(65)), "provider").is_err());
    assert_eq!(
        normalized_provider_model_prefix_for_adapter(
            None,
            "command-code",
            provider_adapters::COMMAND_CODE_ADAPTER_ID,
        )
        .unwrap(),
        "cmc"
    );
    assert_eq!(
        normalized_provider_model_prefix_for_adapter(
            None,
            "openai-codex",
            provider_adapters::CODEX_ADAPTER_ID,
        )
        .unwrap(),
        "cx"
    );
    assert_eq!(
        normalized_provider_model_prefix_for_adapter(
            None,
            "kilo-ai-gateway",
            provider_adapters::KILO_GATEWAY_ADAPTER_ID,
        )
        .unwrap(),
        "kilo"
    );
}

#[test]
fn models_url_preserves_existing_v1_or_models_path() {
    assert_eq!(
        provider_models_url("https://api.example.com/v1/")
            .unwrap()
            .as_str(),
        "https://api.example.com/v1/models"
    );
    assert_eq!(
        provider_models_url("https://api.example.com/models")
            .unwrap()
            .as_str(),
        "https://api.example.com/models"
    );
    assert_eq!(
        provider_models_url("https://api.example.com")
            .unwrap()
            .as_str(),
        "https://api.example.com/v1/models"
    );
}

#[test]
fn parses_openai_and_google_style_model_identifiers() {
    let openai = parse_provider_model_page(&json!({
        "object":"list",
        "data":[{"id":"gpt-4.1"},{"id":"gpt-4.1-mini"},{"owned_by":"openai"}]
    }))
    .unwrap();
    assert_eq!(openai.models, ["gpt-4.1", "gpt-4.1-mini"]);
    assert!(!openai.has_more);

    let google = parse_provider_model_page(&json!({
        "models":[{"name":"models/gemini-2.5-pro"}]
    }))
    .unwrap();
    assert_eq!(google.models, ["models/gemini-2.5-pro"]);
}

#[test]
fn model_page_reads_anthropic_pagination_cursor_and_rejects_unknown_shapes() {
    let page = parse_provider_model_page(&json!({
        "data":[{"id":"claude-3-7-sonnet-latest"}],
        "has_more":true,
        "last_id":"claude-3-7-sonnet-latest"
    }))
    .unwrap();
    assert!(page.has_more);
    assert_eq!(
        page.next_cursor.as_deref(),
        Some("claude-3-7-sonnet-latest")
    );
    assert!(parse_provider_model_page(&json!({"items":[]})).is_err());
}

#[tokio::test]
async fn failed_provider_key_is_saved_with_warning_and_never_returned() {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([17_u8; 32]);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    let input: ProviderInput = serde_json::from_value(json!({
        "name":"Test provider",
        "base_url":"http://127.0.0.1:1/v1",
        "auth_type":"bearer",
        "api_keys":["invalid-but-saved"],
        "preferred_protocol":"chat_completions",
        "supported_protocols":["chat_completions"]
    }))
    .expect("provider request");
    let AxumJson(response) = providers::create_provider(AxumState(state.clone()), AxumJson(input))
        .await
        .expect("provider with failed credential is still saved");

    assert_eq!(response["key_tests"][0]["test_passed"], false);
    assert!(response["key_tests"][0]["warning"].as_str().is_some());
    assert!(!response.to_string().contains("invalid-but-saved"));
    let provider_id = response["id"]
        .as_str()
        .expect("created provider id")
        .to_owned();
    let rows = state
        .db
        .read(move |transaction| {
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(crate::infra::storage::StorageError::NotFound)?;
            let prefix = crate::infra::db::provider_api_key_index_prefix(&provider_id, false)?;
            let key_ids =
                transaction.scan_prefix::<String>(Table::ProviderApiKeyIndex, &prefix, 10)?;
            let (_, key_id) = key_ids
                .first()
                .ok_or(crate::infra::storage::StorageError::NotFound)?;
            let key = transaction
                .get::<Record>(Table::ProviderApiKeys, key_id)?
                .ok_or(crate::infra::storage::StorageError::NotFound)?;
            Ok((provider, key))
        })
        .await
        .expect("persisted provider and key");
    assert_eq!(rows.0.integer("api_key_count").expect("key count"), 1);
    assert!(
        !rows
            .1
            .boolean("last_test_passed")
            .expect("failed test state")
    );
    let encrypted = rows.1.bytes("secret").expect("encrypted provider key");
    assert!(
        !encrypted
            .windows(b"invalid-but-saved".len())
            .any(|window| window == b"invalid-but-saved")
    );
    assert_eq!(
        crate::security::decrypt_secret(state.config.master_key.as_ref(), Some(encrypted))
            .expect("decrypt stored key")
            .as_deref(),
        Some("invalid-but-saved")
    );
}

#[test]
fn weak_default_passwords_are_always_forced_to_change() {
    for password in ["123456", "admin123456", "aaaaaaaaaaaa", "password123456"] {
        assert!(password_needs_change(password), "{password}");
    }
    assert!(!password_needs_change("generated-random-credential-2026"));
}

#[test]
fn imported_admin_hashes_must_use_bounded_argon2id_parameters() {
    let current = hash_admin_password("strong test password").expect("hash password");
    assert!(is_supported_admin_password_hash(&current));
    assert!(!is_supported_admin_password_hash(
        &"x".repeat(MAX_ADMIN_PASSWORD_HASH_BYTES + 1)
    ));

    let weak_params = argon2::Params::new(8_192, 1, 1, None).expect("weak test parameters");
    let weak_hasher = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        weak_params,
    );
    let salt = SaltString::generate(&mut rand::rngs::OsRng);
    let weak = weak_hasher
        .hash_password(b"weak test password", &salt)
        .expect("hash with weak parameters")
        .to_string();
    assert!(!is_supported_admin_password_hash(&weak));
}
