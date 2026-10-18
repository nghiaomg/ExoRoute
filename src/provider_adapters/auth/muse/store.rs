use super::MAX_ACCOUNT_BYTES;
use super::account::MuseAccount;
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    provider_adapters::{AdapterOAuthAccount, MUSE_ADAPTER_ID},
    security,
    state::AppState,
};

/// What a gateway request or probe learned about a stored account.
pub(crate) struct LoadedMuseAccount {
    pub(crate) account: MuseAccount,
    /// The stored account has no usable inference key yet, so the durable
    /// device token must be exchanged for one before the account can serve a
    /// request.
    pub(crate) needs_remint: bool,
}

/// Encrypts and stores the account a device authorization just approved.
///
/// Reconnecting the same Muse account updates the stored credential instead of
/// adding a second one. The lookup is a single bounded secondary-index scan, so
/// no provider-wide credential scan happens here.
pub(crate) async fn save_muse_account(
    state: &AppState,
    provider_id: &str,
    account: AdapterOAuthAccount,
) -> Result<(), String> {
    let account = MuseAccount::decode(&account.payload)
        .map_err(|_| "Muse returned invalid account credentials".to_owned())?;
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
        .map_err(|_| "could not load provider for Muse sign-in".to_owned())?
        .ok_or_else(|| "provider no longer exists".to_owned())?;
    if stored_adapter != MUSE_ADAPTER_ID {
        return Err("provider settings changed during Muse sign-in".to_owned());
    }

    let serialized = serde_json::to_string(&account)
        .map_err(|_| "could not encode Muse account credentials".to_owned())?;
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt Muse account credentials".to_owned())?
        .ok_or_else(|| "Muse returned an empty account credential".to_owned())?;
    let now = crate::infra::db::utc_timestamp_now()
        .map_err(|_| "could not save Muse account credentials".to_owned())?;
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
            if provider.text("adapter_id")? != MUSE_ADAPTER_ID {
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
        .map_err(|_| "could not save Muse account credentials".to_owned())
}

/// Rewrites a stored account after the inference key was replaced, clearing the
/// rejected-key state so the credential returns to rotation.
///
/// Only the secret and the test/invalid bookkeeping change: the credential id,
/// its identity index, and its display name stay as they are, so a repaired
/// account keeps its position in the operator's key list.
pub(crate) async fn persist_account(
    state: &AppState,
    credential_id: &str,
    account: &MuseAccount,
) -> Result<(), String> {
    let serialized = serde_json::to_string(account)
        .map_err(|_| "could not encode Muse account credentials".to_owned())?;
    if serialized.len() > MAX_ACCOUNT_BYTES {
        return Err("Muse account data exceeded its size limit".to_owned());
    }
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt Muse account credentials".to_owned())?
        .ok_or_else(|| "Muse account credentials are empty".to_owned())?;
    let now = crate::infra::db::utc_timestamp_now()
        .map_err(|_| "could not save Muse account credentials".to_owned())?;
    let id = credential_id.to_owned();
    state
        .db
        .write(move |transaction| {
            let Some(mut key) = transaction.get::<Record>(Table::ProviderApiKeys, &id)? else {
                return Err(StorageError::NotFound);
            };
            if key.optional_text("credential_type")? != Some("oauth") {
                return Err(StorageError::Conflict);
            }
            let provider_id = key.text("provider_id")?.to_owned();
            let Some(mut provider) = transaction.get::<Record>(Table::Providers, &provider_id)?
            else {
                return Err(StorageError::NotFound);
            };
            crate::admin::providers::ensure_provider_not_deleting(&provider)?;
            if provider.text("adapter_id")? != MUSE_ADAPTER_ID {
                return Err(StorageError::Conflict);
            }
            let was_invalid = key.boolean("invalid")?;
            let enabled = key.boolean("enabled")?;
            key.insert("secret", Field::Bytes(encrypted.clone()));
            key.insert("invalid", Field::Bool(false));
            key.insert("last_error", Field::Null);
            key.insert("last_test_passed", Field::Bool(true));
            key.insert("last_test_status", Field::I64(200));
            key.insert("last_tested_at", Field::Text(now.clone()));
            transaction.put(Table::ProviderApiKeys, &id, &key)?;
            if enabled {
                let availability =
                    crate::infra::db::provider_api_key_index_key(&provider_id, &id, true)?;
                transaction.put(Table::ProviderApiKeyAvailabilityIndex, &availability, &id)?;
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
        .map_err(|_| "could not save Muse account credentials".to_owned())
}

/// Records the throttle state of a mint attempt that did not leave the account
/// holding a usable key.
///
/// Only the encrypted account secret changes. The caller still hands the
/// credential to the normal invalidation path, so unlike [`persist_account`]
/// this deliberately does not clear `invalid`, mark the credential tested, or
/// touch the availability index — a failed mint is not a repaired credential.
pub(crate) async fn persist_remint_attempt(
    state: &AppState,
    credential_id: &str,
    account: &MuseAccount,
) -> Result<(), String> {
    let serialized = serde_json::to_string(account)
        .map_err(|_| "could not encode Muse account credentials".to_owned())?;
    if serialized.len() > MAX_ACCOUNT_BYTES {
        return Err("Muse account data exceeded its size limit".to_owned());
    }
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt Muse account credentials".to_owned())?
        .ok_or_else(|| "Muse account credentials are empty".to_owned())?;
    let id = credential_id.to_owned();
    state
        .db
        .write(move |transaction| {
            let Some(mut key) = transaction.get::<Record>(Table::ProviderApiKeys, &id)? else {
                return Err(StorageError::NotFound);
            };
            if key.optional_text("credential_type")? != Some("oauth") {
                return Err(StorageError::Conflict);
            }
            let provider_id = key.text("provider_id")?.to_owned();
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            if provider.text("adapter_id")? != MUSE_ADAPTER_ID {
                return Err(StorageError::Conflict);
            }
            key.insert("secret", Field::Bytes(encrypted.clone()));
            transaction.put(Table::ProviderApiKeys, &id, &key)?;
            Ok(())
        })
        .await
        .map_err(|_| "could not save the Muse account credentials".to_owned())
}

/// The endpoint the credential's provider is configured with, so catalog
/// discovery and remints call the same host the provider routes to.
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
            if provider.text("adapter_id")? != MUSE_ADAPTER_ID {
                return Err(StorageError::Conflict);
            }
            Ok(provider.text("base_url")?.to_owned())
        })
        .await
        .map_err(|_| "could not load the Muse provider endpoint".to_owned())
}

/// Loads the stored account a gateway request or probe must authenticate as.
///
/// Unlike a plain credential lookup this deliberately keeps an account that was
/// marked invalid: the durable device token behind it is still valid, so the
/// account can mint a replacement inference key instead of being dropped from
/// rotation. A credential that cannot be repaired is filtered out by the
/// rotation stage, not here.
pub(crate) async fn load_muse_account(
    state: &AppState,
    credential_id: &str,
) -> Result<LoadedMuseAccount, String> {
    let id = credential_id.to_owned();
    let stored = state
        .db
        .read(move |tx| {
            let Some(key) = tx.get::<Record>(Table::ProviderApiKeys, &id)? else {
                return Ok(None);
            };
            if !key.boolean("enabled")? || key.optional_text("credential_type")? != Some("oauth") {
                return Ok(None);
            }
            let provider_id = key.text("provider_id")?.to_owned();
            let provider = tx
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            if provider.text("adapter_id")? != MUSE_ADAPTER_ID {
                return Ok(None);
            }
            Ok(Some(key.bytes("secret")?.to_vec()))
        })
        .await
        .map_err(|_| "could not load the Muse account".to_owned())?
        .ok_or_else(|| "Muse account was removed, disabled, or needs reconnecting".to_owned())?;
    if stored.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Muse account data exceeded its size limit".to_owned());
    }
    let serialized = security::decrypt_secret(state.config.master_key.as_ref(), Some(&stored))
        .map_err(|_| "could not decrypt Muse account; check EXOROUTE_MASTER_KEY".to_owned())?
        .ok_or_else(|| "Muse account credentials are empty".to_owned())?;
    if serialized.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Muse account data exceeded its size limit".to_owned());
    }
    let account = MuseAccount::decode_str(&serialized)
        .map_err(|_| "stored Muse account data is invalid".to_owned())?;
    let needs_remint = account.needs_remint();
    Ok(LoadedMuseAccount {
        account,
        needs_remint,
    })
}
