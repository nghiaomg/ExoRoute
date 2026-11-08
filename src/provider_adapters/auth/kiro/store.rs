use super::account::{KiroAccount, unix_now};
use super::refresh::refresh_account;
use super::{MAX_ACCOUNT_BYTES, REFRESH_LEAD_SECONDS};
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    provider_adapters::{AdapterOAuthAccount, KIRO_ADAPTER_ID},
    security,
    state::AppState,
};
use serde_json::Value;

/// Encrypts and stores the account a sign-in or import just produced.
///
/// Reconnecting the same Kiro account updates the stored credential rather
/// than adding a second one, matching the other OAuth adapters.
pub(crate) async fn save_kiro_account(
    state: &AppState,
    provider_id: &str,
    account: AdapterOAuthAccount,
) -> Result<(), String> {
    let account = KiroAccount::from_token_payload(&account.payload)
        .map_err(|_| "Kiro returned invalid account credentials".to_owned())?;
    persist_new_account(state, provider_id, account).await
}

async fn persist_new_account(
    state: &AppState,
    provider_id: &str,
    account: KiroAccount,
) -> Result<(), String> {
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
        .map_err(|_| "could not load provider for Kiro sign-in".to_owned())?
        .ok_or_else(|| "provider no longer exists".to_owned())?;
    if stored_adapter != KIRO_ADAPTER_ID {
        return Err("provider settings changed during Kiro sign-in".to_owned());
    }

    let serialized = serde_json::to_string(&account)
        .map_err(|_| "could not encode Kiro account credentials".to_owned())?;
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt Kiro account credentials".to_owned())?
        .ok_or_else(|| "Kiro returned an empty account credential".to_owned())?;
    let now = crate::infra::db::utc_timestamp_now()
        .map_err(|_| "could not save Kiro account credentials".to_owned())?;
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
            if provider.text("adapter_id")? != KIRO_ADAPTER_ID {
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
        .map_err(|_| "could not save Kiro account credentials".to_owned())
}

/// Loads the stored account a gateway request must authenticate as, refreshing
/// the access token first when it is inside the expiry lead window.
pub(crate) async fn account_for_use(
    state: &AppState,
    credential_id: &str,
) -> Result<KiroAccount, String> {
    let lock = state.provider_token_refresh_lock(credential_id).await;
    let _guard = lock.lock().await;
    let encrypted = state
        .db
        .read({
            let id = credential_id.to_owned();
            move |transaction| {
                let Some(key) = transaction.get::<Record>(Table::ProviderApiKeys, &id)? else {
                    return Ok(None);
                };
                if !key.boolean("enabled")?
                    || key.boolean("invalid")?
                    || key.optional_text("credential_type")? != Some("oauth")
                {
                    return Ok(None);
                }
                let provider_id = key.text("provider_id")?.to_owned();
                let provider = transaction
                    .get::<Record>(Table::Providers, &provider_id)?
                    .ok_or(StorageError::NotFound)?;
                if provider.text("adapter_id")? != KIRO_ADAPTER_ID {
                    return Ok(None);
                }
                Ok(Some(key.bytes("secret")?.to_vec()))
            }
        })
        .await
        .map_err(|_| "could not load the Kiro account".to_owned())?
        .ok_or_else(|| "Kiro account was removed, disabled, or needs reconnecting".to_owned())?;
    if encrypted.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Kiro account data exceeded its size limit".to_owned());
    }
    let serialized = security::decrypt_secret(state.config.master_key.as_ref(), Some(&encrypted))
        .map_err(|_| "could not decrypt Kiro account; check EXOROUTE_MASTER_KEY".to_owned())?
        .ok_or_else(|| "Kiro account credentials are empty".to_owned())?;
    if serialized.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Kiro account data exceeded its size limit".to_owned());
    }
    let value: Value = serde_json::from_str(&serialized)
        .map_err(|_| "stored Kiro account data is invalid".to_owned())?;
    let mut account = KiroAccount::decode(&value)?;
    if account.expires_at <= unix_now().saturating_add(REFRESH_LEAD_SECONDS) {
        refresh_account(state, &mut account).await?;
        persist_refresh(state, credential_id, &account).await?;
    }
    Ok(account)
}

/// Writes a refreshed account back over its existing credential record.
async fn persist_refresh(
    state: &AppState,
    credential_id: &str,
    account: &KiroAccount,
) -> Result<(), String> {
    let serialized = serde_json::to_string(account)
        .map_err(|_| "could not encode Kiro account credentials".to_owned())?;
    if serialized.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Kiro account data exceeded its size limit".to_owned());
    }
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt Kiro account credentials".to_owned())?
        .ok_or_else(|| "Kiro returned empty account credentials".to_owned())?;
    let id = credential_id.to_owned();
    state
        .db
        .write(move |transaction| {
            let mut key = transaction
                .get::<Record>(Table::ProviderApiKeys, &id)?
                .ok_or(StorageError::NotFound)?;
            key.insert("secret", Field::Bytes(encrypted.clone()));
            transaction.put(Table::ProviderApiKeys, &id, &key)
        })
        .await
        .map_err(|_| "could not save the refreshed Kiro account".to_owned())
}
