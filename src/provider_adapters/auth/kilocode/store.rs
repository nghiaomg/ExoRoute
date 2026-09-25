use super::account::KilocodeAccount;
use super::{MAX_ACCOUNT_BYTES, MAX_TOKEN_BYTES};
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    provider_adapters::{AdapterOAuthAccount, KILOCODE_ADAPTER_ID},
    security,
    state::AppState,
};
use serde_json::Value;

/// Encrypts and stores the account a device authorization just approved.
///
/// Reconnecting the same Kilo Code account updates the stored credential rather
/// than adding a second one. The lookup is a single bounded secondary-index
/// scan, so no provider-wide credential scan happens here.
pub(crate) async fn save_kilocode_account(
    state: &AppState,
    provider_id: &str,
    account: AdapterOAuthAccount,
) -> Result<(), String> {
    let account = KilocodeAccount::from_device_approval(&account.payload)
        .map_err(|_| "Kilo Code returned invalid account credentials".to_owned())?;
    let provider_id_owned = provider_id.to_owned();
    let stored_adapter = state
        .db
        .read(move |transaction| {
            transaction
                .get::<Record>(Table::Providers, &provider_id_owned)?
                .map(|record| record.text("adapter_id").map(str::to_owned))
                .transpose()
        })
        .await
        .map_err(|_| "could not load provider for Kilo Code sign-in".to_owned())?
        .ok_or_else(|| "provider no longer exists".to_owned())?;
    if stored_adapter != KILOCODE_ADAPTER_ID {
        return Err("provider settings changed during Kilo Code sign-in".to_owned());
    }

    let serialized = serde_json::to_string(&account)
        .map_err(|_| "could not encode Kilo Code account credentials".to_owned())?;
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt Kilo Code account credentials".to_owned())?
        .ok_or_else(|| "Kilo Code returned an empty account credential".to_owned())?;
    let now = crate::infra::db::utc_timestamp_now()
        .map_err(|_| "could not save Kilo Code account credentials".to_owned())?;
    let identity = account.identity();
    let display_name = account.display_name();
    let key_id = uuid::Uuid::new_v4().to_string();
    let provider_id = provider_id.to_owned();
    let mut record = crate::admin::providers::provider_api_key_record(
        crate::admin::providers::ProviderApiKeyRecordData {
            id: &key_id,
            provider_id: &provider_id,
            name: &display_name,
            secret: &encrypted,
            last_error: None,
            last_test_passed: true,
            last_test_status: Some(200),
            tested_at: &now,
            created_at: &now,
        },
    );
    record.insert("credential_type", Field::Text("oauth".to_owned()));
    record.insert("oauth_account_identity", Field::Text(identity.clone()));
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            crate::admin::providers::ensure_provider_not_deleting(&provider)?;
            if provider.text("adapter_id")? != KILOCODE_ADAPTER_ID {
                return Err(StorageError::Conflict);
            }

            let index_prefix =
                crate::infra::db::provider_api_key_identity_index_prefix(&provider_id, &identity)?;
            let existing = transaction
                .scan_prefix::<String>(Table::ProviderApiKeyIndex, &index_prefix, 1)?
                .into_iter()
                .map(|(_, key_id)| key_id)
                .next();

            let Some(existing_id) = existing else {
                let count = provider
                    .integer("api_key_count")?
                    .checked_add(1)
                    .ok_or_else(|| {
                        StorageError::Invalid("provider credential count overflowed".to_owned())
                    })?;
                provider.insert("api_key_count", Field::I64(count));
                transaction.put(Table::Providers, &provider_id, &provider)?;
                return crate::admin::providers::store_provider_api_key(transaction, &record);
            };

            let mut existing = transaction
                .get::<Record>(Table::ProviderApiKeys, &existing_id)?
                .ok_or(StorageError::NotFound)?;
            if existing.text("provider_id")? != provider_id
                || existing.optional_text("credential_type")? != Some("oauth")
            {
                return Err(StorageError::Conflict);
            }
            let was_invalid = existing.boolean("invalid")?;
            existing.insert("name", Field::Text(display_name.clone()));
            existing.insert("secret", Field::Bytes(encrypted.clone()));
            existing.insert("credential_type", Field::Text("oauth".to_owned()));
            existing.insert("oauth_account_identity", Field::Text(identity.clone()));
            existing.insert("invalid", Field::Bool(false));
            existing.insert("last_error", Field::Null);
            existing.insert("last_test_passed", Field::Bool(true));
            existing.insert("last_test_status", Field::I64(200));
            existing.insert("last_tested_at", Field::Text(now.clone()));
            transaction.put(Table::ProviderApiKeys, &existing_id, &existing)?;
            if existing.boolean("enabled")? {
                let availability =
                    crate::infra::db::provider_api_key_index_key(&provider_id, &existing_id, true)?;
                transaction.put(
                    Table::ProviderApiKeyAvailabilityIndex,
                    &availability,
                    &existing_id,
                )?;
            }
            if was_invalid {
                provider.insert(
                    "invalid_api_key_count",
                    Field::I64(
                        provider
                            .integer("invalid_api_key_count")?
                            .saturating_sub(1)
                            .max(0),
                    ),
                );
                transaction.put(Table::Providers, &provider_id, &provider)?;
            }
            Ok(())
        })
        .await
        .map_err(|_| "could not save Kilo Code account credentials".to_owned())
}

/// The endpoint the credential's provider is configured with, so catalog
/// discovery calls the same host the provider routes to.
pub(crate) async fn provider_base_url(
    state: &AppState,
    credential_id: &str,
) -> Result<String, String> {
    let id = credential_id.to_owned();
    state
        .db
        .read(move |tx| {
            let key = tx
                .get::<Record>(Table::ProviderApiKeys, &id)?
                .ok_or(StorageError::NotFound)?;
            let provider_id = key.text("provider_id")?.to_owned();
            let provider = tx
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            if provider.text("adapter_id")? != KILOCODE_ADAPTER_ID {
                return Err(StorageError::Conflict);
            }
            Ok(provider.text("base_url")?.to_owned())
        })
        .await
        .map_err(|_| "could not load the Kilo Code provider endpoint".to_owned())
}

/// Loads the stored account a gateway request or probe must authenticate as.
pub(crate) async fn account_for_use(
    state: &AppState,
    credential_id: &str,
) -> Result<KilocodeAccount, String> {
    let id = credential_id.to_owned();
    let stored = state
        .db
        .read(move |tx| {
            let Some(key) = tx.get::<Record>(Table::ProviderApiKeys, &id)? else {
                return Ok(None);
            };
            if !key.boolean("enabled")?
                || key.boolean("invalid")?
                || key.optional_text("credential_type")? != Some("oauth")
            {
                return Ok(None);
            }
            let provider_id = key.text("provider_id")?.to_owned();
            let provider = tx
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            if provider.text("adapter_id")? != KILOCODE_ADAPTER_ID {
                return Ok(None);
            }
            Ok(Some(key.bytes("secret")?.to_vec()))
        })
        .await
        .map_err(|_| "could not load the Kilo Code account".to_owned())?
        .ok_or_else(|| {
            "Kilo Code account was removed, disabled, or needs reconnecting".to_owned()
        })?;
    if stored.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Kilo Code account data exceeded its size limit".to_owned());
    }
    let serialized = security::decrypt_secret(state.config.master_key.as_ref(), Some(&stored))
        .map_err(|_| "could not decrypt Kilo Code account; check EXOROUTE_MASTER_KEY".to_owned())?
        .ok_or_else(|| "Kilo Code account credentials are empty".to_owned())?;
    if serialized.len() > MAX_ACCOUNT_BYTES || serialized.len() > MAX_TOKEN_BYTES + 1024 {
        return Err("stored Kilo Code account data exceeded its size limit".to_owned());
    }
    let value: Value = serde_json::from_str(&serialized)
        .map_err(|_| "stored Kilo Code account data is invalid".to_owned())?;
    KilocodeAccount::decode(&value)
        .map_err(|_| "stored Kilo Code account data is invalid".to_owned())
}
