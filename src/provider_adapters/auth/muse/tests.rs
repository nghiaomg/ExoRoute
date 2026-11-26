use super::*;
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    provider_adapters::{AdapterOAuthAccount, MUSE_ADAPTER_ID, save_oauth_account},
    state::AppState,
    support::test_support::{ProviderSeed, TestDatabase, seed_provider, seed_provider_key},
};
use serde_json::json;

const TEST_MASTER_KEY: [u8; 32] = [47_u8; 32];

/// Builds the payload the device flow stores, without performing any request.
fn approval(dca_token: &str, api_key: Option<&str>, email: Option<&str>) -> AdapterOAuthAccount {
    let mut account = MuseAccount::from_device_grant(&json!({ "access_token": dca_token }))
        .expect("usable device grant");
    account.email = email.map(str::to_owned);
    let account = match api_key {
        Some(api_key) => account
            .with_minted_key(api_key)
            .expect("usable inference key"),
        None => account,
    };
    AdapterOAuthAccount {
        display_name: account.display_name(),
        payload: account.to_payload().expect("account payload"),
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
            id: "muse-provider",
            name: "Meta Muse Code",
            base_url: "https://api.meta.ai/v1",
            adapter_id: MUSE_ADAPTER_ID,
            auth_type: "bearer",
            model_prefix: "muse",
            preferred_protocol: "responses",
            supported_protocols: &["responses", "chat_completions"],
        },
    )
    .await
    .expect("seed provider");
    (database, state)
}

/// Reads every credential stored for the test provider through its creation
/// index, which is the path the dashboard uses to list keys.
async fn credential_records(database: &TestDatabase) -> Vec<Record> {
    let prefix =
        crate::infra::db::provider_api_key_created_prefix("muse-provider").expect("key prefix");
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

async fn credential_id(database: &TestDatabase) -> String {
    credential_records(database).await[0]
        .text("id")
        .expect("credential id")
        .to_owned()
}

#[test]
fn a_device_grant_must_carry_a_usable_token() {
    for valid in ["dca:one.two-three_four", "opaque-token"] {
        let account = MuseAccount::from_device_grant(&json!({ "access_token": valid }))
            .expect("usable grant");
        assert_eq!(account.dca_token, valid);
        assert!(account.api_key.is_none());
        assert!(account.needs_remint());
    }
    for invalid in [
        json!({}),
        json!({ "access_token": "" }),
        json!({ "access_token": "  " }),
        json!({ "access_token": "has space" }),
        json!({ "access_token": "line\nbreak" }),
        json!({ "access_token": 42 }),
    ] {
        assert!(
            MuseAccount::from_device_grant(&invalid).is_err(),
            "expected rejection: {invalid}"
        );
    }
    let oversized = "a".repeat(MAX_TOKEN_BYTES + 1);
    assert!(
        MuseAccount::from_device_grant(&json!({ "access_token": oversized })).is_err(),
        "an oversized token must not be stored"
    );
}

#[test]
fn identity_prefers_the_email_and_never_repeats_a_secret() {
    let mut with_email = MuseAccount::from_device_grant(&json!({ "access_token": "dca:secret" }))
        .expect("usable grant");
    with_email.email = Some("Operator@Example.com".to_owned());
    assert_eq!(with_email.identity(), "email:operator@example.com");

    let without_email =
        MuseAccount::from_device_grant(&json!({ "access_token": "dca:secret" })).expect("grant");
    let identity = without_email.identity();
    assert!(identity.starts_with("token:"));
    assert!(!identity.contains("dca:secret"));
    assert!(
        !without_email.display_name().contains("dca:secret"),
        "a credential name must never contain the stored secret"
    );
}

#[test]
fn a_stored_record_round_trips_and_rejects_corruption() {
    let account = MuseAccount::from_device_grant(&json!({ "access_token": "dca:one" }))
        .expect("usable grant")
        .with_minted_key("inference-one")
        .expect("usable key");
    let serialized = serde_json::to_string(&account).expect("serialize");
    let decoded = MuseAccount::decode_str(&serialized).expect("round trip");
    assert_eq!(decoded.dca_token, "dca:one");
    assert_eq!(decoded.inference_key(), Some("inference-one"));
    assert!(!decoded.needs_remint());

    for corrupt in [
        "not json",
        "{}",
        r#"{"dca_token":""}"#,
        r#"{"dca_token":"has space"}"#,
        r#"{"dca_token":"dca:one","api_key":"has space"}"#,
        r#"{"dca_token":"dca:one","email":"line\nbreak"}"#,
    ] {
        assert!(
            MuseAccount::decode_str(corrupt).is_err(),
            "expected rejection: {corrupt}"
        );
    }
}

#[tokio::test]
async fn a_sign_in_stores_both_secrets_and_reconnecting_updates_one_credential() {
    let (database, state) = state_with_provider().await;
    save_oauth_account(
        MUSE_ADAPTER_ID,
        &state,
        "muse-provider",
        approval(
            "dca:first",
            Some("inference-one"),
            Some("operator@example.com"),
        ),
    )
    .await
    .expect("first sign-in");
    save_oauth_account(
        MUSE_ADAPTER_ID,
        &state,
        "muse-provider",
        approval(
            "dca:first",
            Some("inference-two"),
            Some("Operator@Example.com"),
        ),
    )
    .await
    .expect("reconnection");

    let records = credential_records(&database).await;
    assert_eq!(records.len(), 1, "a reconnection must not duplicate");
    assert_eq!(
        records[0]
            .optional_text("oauth_account_identity")
            .expect("identity"),
        Some("email:operator@example.com")
    );

    let loaded = load_muse_account(&state, &credential_id(&database).await)
        .await
        .expect("stored account");
    assert_eq!(loaded.account.dca_token, "dca:first");
    assert_eq!(loaded.account.inference_key(), Some("inference-two"));
    assert!(!loaded.needs_remint);
    assert_eq!(
        provider_base_url(&state, &credential_id(&database).await)
            .await
            .expect("base url"),
        "https://api.meta.ai/v1"
    );

    // A different account keeps its own credential.
    save_oauth_account(
        MUSE_ADAPTER_ID,
        &state,
        "muse-provider",
        approval(
            "dca:second",
            Some("inference-three"),
            Some("second@example.com"),
        ),
    )
    .await
    .expect("second account");
    assert_eq!(credential_records(&database).await.len(), 2);
}

#[tokio::test]
async fn an_account_without_an_inference_key_is_reported_as_needing_a_remint() {
    let (database, state) = state_with_provider().await;
    save_oauth_account(
        MUSE_ADAPTER_ID,
        &state,
        "muse-provider",
        approval("dca:only", None, Some("operator@example.com")),
    )
    .await
    .expect("sign-in without a minted key");

    let loaded = load_muse_account(&state, &credential_id(&database).await)
        .await
        .expect("stored account");
    assert!(
        loaded.needs_remint,
        "a grant stored without a minted key must be minted before use"
    );
    assert!(loaded.account.inference_key().is_none());
}

/// A rejected inference key must be repairable: the durable device token is
/// still valid, so the credential has to stay loadable while marked invalid and
/// return to rotation once a replacement key is stored.
#[tokio::test]
async fn a_repaired_account_returns_to_rotation() {
    let (database, state) = state_with_provider().await;
    save_oauth_account(
        MUSE_ADAPTER_ID,
        &state,
        "muse-provider",
        approval(
            "dca:first",
            Some("inference-one"),
            Some("operator@example.com"),
        ),
    )
    .await
    .expect("sign-in");
    let key_id = credential_id(&database).await;

    // Reproduce what the gateway does when the upstream rejects a key.
    let availability = crate::infra::db::provider_api_key_index_key("muse-provider", &key_id, true)
        .expect("availability key");
    let availability_for_write = availability.clone();
    let key_id_for_write = key_id.clone();
    database
        .db
        .write(move |transaction| {
            let mut record = transaction
                .get::<Record>(Table::ProviderApiKeys, &key_id_for_write)?
                .ok_or(StorageError::NotFound)?;
            record.insert("invalid", Field::Bool(true));
            transaction.put(Table::ProviderApiKeys, &key_id_for_write, &record)?;
            transaction.delete(
                Table::ProviderApiKeyAvailabilityIndex,
                &availability_for_write,
            )
        })
        .await
        .expect("mark rejected");

    let loaded = load_muse_account(&state, &key_id)
        .await
        .expect("a rejected account must stay loadable so it can be repaired");
    assert!(
        !loaded.needs_remint,
        "the stored key is present but rejected"
    );

    let reminted = loaded
        .account
        .with_minted_key("inference-two")
        .expect("replacement key");
    persist_account(&state, &key_id, &reminted)
        .await
        .expect("persist replacement");

    let reloaded = load_muse_account(&state, &key_id)
        .await
        .expect("repaired account");
    assert_eq!(reloaded.account.inference_key(), Some("inference-two"));
    assert_eq!(reloaded.account.dca_token, "dca:first");

    let (invalid, stored_availability) = {
        let key_id = key_id.clone();
        let availability = availability.clone();
        database
            .db
            .read(move |transaction| {
                let invalid = transaction
                    .get::<Record>(Table::ProviderApiKeys, &key_id)?
                    .ok_or(StorageError::NotFound)?
                    .boolean("invalid")?;
                let available = transaction
                    .get::<String>(Table::ProviderApiKeyAvailabilityIndex, &availability)?
                    .is_some();
                Ok((invalid, available))
            })
            .await
            .expect("read repaired credential")
    };
    assert!(
        !invalid,
        "a repaired credential must leave the invalid state"
    );
    assert!(
        stored_availability,
        "a repaired credential must return to the availability index"
    );
}

#[tokio::test]
async fn a_pasted_api_key_is_not_a_muse_account() {
    let (database, state) = state_with_provider().await;
    seed_provider_key(
        &database.db,
        "muse-provider",
        "pasted-key",
        b"a-pasted-muse-api-key",
        false,
    )
    .await
    .expect("seed pasted key");

    assert!(
        load_muse_account(&state, "pasted-key").await.is_err(),
        "a pasted API key has no device token and cannot be reminted"
    );
    assert!(
        load_muse_account(&state, "missing-credential")
            .await
            .is_err(),
        "an unknown credential must not authenticate"
    );
}

/// The remint throttle is what stops client traffic from turning into mint
/// traffic. It has to allow a first mint, refuse a second one inside the
/// cooldown, refuse every later one once the budget is spent, and then let the
/// account mint again once the window rolls — a lifetime cap would lock an
/// operator out of a working account forever.
#[test]
fn the_remint_throttle_blocks_a_cooldown_and_an_exhausted_budget() {
    const COOLDOWN: u64 = 60;
    const LIMIT: u32 = 3;
    const WINDOW: u64 = 24 * 60 * 60;
    let now = crate::infra::db::parse_utc_timestamp(
        &crate::infra::db::utc_timestamp_now().expect("clock"),
    )
    .expect("clock");

    let mut account = MuseAccount::from_device_grant(&json!({ "access_token": "dca:first" }))
        .expect("usable device grant");
    assert_eq!(account.remint_blocked(now, COOLDOWN, LIMIT, WINDOW), None);

    account
        .record_mint_attempt(now, WINDOW)
        .expect("record attempt");
    assert_eq!(account.remint_attempts, 1);
    assert!(
        account
            .remint_blocked(now, COOLDOWN, LIMIT, WINDOW)
            .is_some(),
        "a mint that just ran must not be repeated immediately"
    );

    // Past the cooldown a mint is allowed again — until the budget is spent. The
    // margin absorbs the skew between reading the clock and recording the attempt.
    let after_cooldown = now + COOLDOWN + 5;
    assert_eq!(
        account.remint_blocked(after_cooldown, COOLDOWN, LIMIT, WINDOW),
        None
    );
    account
        .record_mint_attempt(after_cooldown, WINDOW)
        .expect("record attempt");
    account
        .record_mint_attempt(after_cooldown, WINDOW)
        .expect("record attempt");
    assert_eq!(account.remint_attempts, LIMIT);
    assert!(
        account
            .remint_blocked(after_cooldown, COOLDOWN, LIMIT, WINDOW)
            .is_some(),
        "a spent budget refuses another mint inside the same window"
    );

    // Once the window has aged past the last attempt the budget rolls over, so a
    // persistently bad upstream cannot lock a working account out permanently.
    let after_window = now + WINDOW + COOLDOWN + 10;
    assert_eq!(
        account.remint_blocked(after_window, COOLDOWN, LIMIT, WINDOW),
        None,
        "a rolled window must permit a mint again"
    );
    account
        .record_mint_attempt(after_window, WINDOW)
        .expect("record attempt");
    assert_eq!(
        account.remint_attempts, 1,
        "an attempt after the window starts a fresh budget instead of inheriting a spent one"
    );
}

/// Recording a mint attempt is throttling, not repair. The caller is about to
/// hand the credential to the normal invalidation path, so this must leave the
/// credential exactly as invalid as it found it.
#[tokio::test]
async fn recording_a_failed_mint_attempt_does_not_repair_the_credential() {
    let (database, state) = state_with_provider().await;
    save_oauth_account(
        MUSE_ADAPTER_ID,
        &state,
        "muse-provider",
        approval(
            "dca:first",
            Some("inference-one"),
            Some("operator@example.com"),
        ),
    )
    .await
    .expect("sign-in");
    let key_id = credential_id(&database).await;

    // Reproduce the rejected state the gateway writes when an upstream rejects
    // the stored key: invalid, and removed from the availability index.
    let availability = crate::infra::db::provider_api_key_index_key("muse-provider", &key_id, true)
        .expect("availability key");
    let availability_for_write = availability.clone();
    let key_id_for_write = key_id.clone();
    database
        .db
        .write(move |transaction| {
            let mut record = transaction
                .get::<Record>(Table::ProviderApiKeys, &key_id_for_write)?
                .ok_or(StorageError::NotFound)?;
            record.insert("invalid", Field::Bool(true));
            transaction.put(Table::ProviderApiKeys, &key_id_for_write, &record)?;
            transaction.delete(
                Table::ProviderApiKeyAvailabilityIndex,
                &availability_for_write,
            )
        })
        .await
        .expect("mark rejected");

    let mut attempted = load_muse_account(&state, &key_id)
        .await
        .expect("a rejected account stays loadable")
        .account;
    attempted
        .record_mint_attempt(
            crate::infra::db::parse_utc_timestamp(
                &crate::infra::db::utc_timestamp_now().expect("clock"),
            )
            .expect("clock"),
            24 * 60 * 60,
        )
        .expect("record attempt");
    persist_remint_attempt(&state, &key_id, &attempted)
        .await
        .expect("persist the attempt");

    let reloaded = load_muse_account(&state, &key_id)
        .await
        .expect("still loadable after a failed mint");
    assert_eq!(
        reloaded.account.remint_attempts, 1,
        "the failed attempt must be recorded so the next request is throttled"
    );

    let (invalid, stored_availability) = {
        let key_id = key_id.clone();
        database
            .db
            .read(move |transaction| {
                let invalid = transaction
                    .get::<Record>(Table::ProviderApiKeys, &key_id)?
                    .ok_or(StorageError::NotFound)?
                    .boolean("invalid")?;
                let available = transaction
                    .get::<String>(Table::ProviderApiKeyAvailabilityIndex, &availability)?
                    .is_some();
                Ok((invalid, available))
            })
            .await
            .expect("read credential state")
    };
    assert!(
        invalid,
        "recording a failed mint must not clear the invalid flag"
    );
    assert!(
        !stored_availability,
        "recording a failed mint must not return the credential to rotation"
    );
}
