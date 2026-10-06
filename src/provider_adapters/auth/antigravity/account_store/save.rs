//! Persisting account updates and reauthentication state for Antigravity credentials.

use super::*;
use crate::infra::storage::Record;

pub(in crate::provider_adapters) async fn save_account(
    state: &AppState,
    provider_id: &str,
    account: AntigravityAccount,
) -> Result<(), String> {
    validate_account(&account)?;
    let account_identity = account
        .email
        .as_deref()
        .and_then(normalize_account_identity);
    let serialized = serde_json::to_string(&account)
        .map_err(|_| "could not encode the Antigravity account credentials".to_owned())?;
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt the Antigravity account credentials".to_owned())?
        .ok_or_else(|| "Antigravity returned empty account credentials".to_owned())?;
    let created_at = crate::infra::db::utc_timestamp_now()
        .map_err(|_| "could not save the Antigravity account".to_owned())?;
    let key_id = uuid::Uuid::new_v4().to_string();
    let display_name = account.display_name();
    let mut record = crate::admin::providers::provider_api_key_record(
        crate::admin::providers::ProviderApiKeyRecordData {
            id: &key_id,
            provider_id,
            name: &display_name,
            secret: &encrypted,
            last_error: None,
            last_test_passed: true,
            last_test_status: Some(200),
            tested_at: &created_at,
            created_at: &created_at,
        },
    );
    record.insert("credential_type", Field::Text("oauth".to_owned()));
    if let Some(identity) = account_identity.as_deref() {
        record.insert("oauth_account_identity", Field::Text(identity.to_owned()));
    }
    let provider_id_owned = provider_id.to_owned();
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id_owned)?
                .ok_or(StorageError::NotFound)?;
            crate::admin::providers::ensure_provider_not_deleting(&provider)?;
            let current_count = provider.integer("api_key_count")?;

            let mut matching = Vec::new();
            if let Some(identity) = account_identity.as_deref() {
                let identity_prefix = crate::infra::db::provider_api_key_identity_index_prefix(
                    &provider_id_owned,
                    identity,
                )?;
                let indexed_rows = transaction.scan_prefix::<String>(
                    Table::ProviderApiKeyIndex,
                    &identity_prefix,
                    usize::MAX,
                )?;
                let has_indexed_rows = !indexed_rows.is_empty();
                let candidate_ids = if has_indexed_rows {
                    indexed_rows
                } else {
                    let created_prefix =
                        crate::infra::db::provider_api_key_created_prefix(&provider_id_owned)?;
                    transaction.scan_prefix::<String>(
                        Table::ProviderApiKeyCreatedIndex,
                        &created_prefix,
                        usize::MAX,
                    )?
                };
                for (_, key_id) in candidate_ids {
                    let Some(existing) =
                        transaction.get::<Record>(Table::ProviderApiKeys, &key_id)?
                    else {
                        continue;
                    };
                    if existing.text("provider_id")? != provider_id_owned
                        || existing.optional_text("credential_type")? != Some("oauth")
                        || record_account_identity(&existing)?.is_none_or(|value| value != identity)
                    {
                        continue;
                    }
                    matching.push(existing);
                }
                if matching.is_empty() && has_indexed_rows {
                    let created_prefix =
                        crate::infra::db::provider_api_key_created_prefix(&provider_id_owned)?;
                    let legacy_rows = transaction.scan_prefix::<String>(
                        Table::ProviderApiKeyCreatedIndex,
                        &created_prefix,
                        usize::MAX,
                    )?;
                    for (_, key_id) in legacy_rows {
                        let Some(existing) =
                            transaction.get::<Record>(Table::ProviderApiKeys, &key_id)?
                        else {
                            continue;
                        };
                        if existing.text("provider_id")? != provider_id_owned
                            || existing.optional_text("credential_type")? != Some("oauth")
                            || record_account_identity(&existing)?
                                .is_none_or(|value| value != identity)
                        {
                            continue;
                        }
                        matching.push(existing);
                    }
                }
            }

            if !matching.is_empty() {
                let mut existing = matching.remove(0);
                let existing_id = existing.text("id")?.to_owned();
                let mut removed_duplicates = 0_i64;
                let mut removed_invalid = 0_i64;
                for duplicate in matching {
                    let duplicate_id = duplicate.text("id")?.to_owned();
                    let (all_index, created_index, _) =
                        crate::admin::providers::provider_api_key_index_keys(&duplicate)?;
                    transaction.delete(Table::ProviderApiKeys, &duplicate_id)?;
                    transaction.delete(Table::ProviderApiKeyIndex, &all_index)?;
                    transaction.delete(Table::ProviderApiKeyCreatedIndex, &created_index)?;
                    if let Some(identity_index) =
                        crate::admin::providers::provider_api_key_identity_index_key(&duplicate)?
                    {
                        transaction.delete(Table::ProviderApiKeyIndex, &identity_index)?;
                    }
                    if duplicate.boolean("enabled")? && !duplicate.boolean("invalid")? {
                        let available = crate::infra::db::provider_api_key_index_key(
                            &provider_id_owned,
                            &duplicate_id,
                            true,
                        )?;
                        transaction.delete(Table::ProviderApiKeyAvailabilityIndex, &available)?;
                    }
                    transaction
                        .delete_prefix(Table::ProviderUsageMeters, &format!("{duplicate_id}/"))?;
                    removed_duplicates = removed_duplicates.saturating_add(1);
                    if duplicate.boolean("invalid")? {
                        removed_invalid = removed_invalid.saturating_add(1);
                    }
                }

                let was_invalid = existing.boolean("invalid")?;
                existing.insert("name", Field::Text(record.text("name")?.to_owned()));
                existing.insert("secret", Field::Bytes(encrypted.clone()));
                existing.insert("credential_type", Field::Text("oauth".to_owned()));
                existing.insert("invalid", Field::Bool(false));
                existing.insert("last_error", Field::Null);
                existing.insert("last_test_passed", Field::Bool(true));
                existing.insert("last_test_status", Field::I64(200));
                existing.insert("last_tested_at", Field::Text(created_at.clone()));
                if let Some(identity) = account_identity.as_deref() {
                    existing.insert("oauth_account_identity", Field::Text(identity.to_owned()));
                }
                transaction.put(Table::ProviderApiKeys, &existing_id, &existing)?;
                if existing.boolean("enabled")? {
                    transaction.put(
                        Table::ProviderApiKeyAvailabilityIndex,
                        &crate::infra::db::provider_api_key_index_key(
                            &provider_id_owned,
                            &existing_id,
                            true,
                        )?,
                        &existing_id,
                    )?;
                }
                if let Some(identity) = account_identity.as_deref() {
                    transaction.put(
                        Table::ProviderApiKeyIndex,
                        &crate::infra::db::provider_api_key_identity_index_key(
                            &provider_id_owned,
                            identity,
                            &existing_id,
                        )?,
                        &existing_id,
                    )?;
                }
                provider.insert(
                    "api_key_count",
                    Field::I64(current_count.saturating_sub(removed_duplicates)),
                );
                let invalid_count = provider
                    .integer("invalid_api_key_count")?
                    .saturating_sub(removed_invalid)
                    .saturating_sub(i64::from(was_invalid));
                provider.insert("invalid_api_key_count", Field::I64(invalid_count.max(0)));
                transaction.put(Table::Providers, &provider_id_owned, &provider)?;
                return Ok(());
            }
            /*
             * The legacy fallback above scans the provider's created index only
             * when no identity index exists yet. New and reconnected accounts
             * always receive the bounded identity index below.
             */

            let count = current_count.checked_add(1).ok_or_else(|| {
                StorageError::Invalid("provider API key count overflowed".to_owned())
            })?;
            provider.insert("api_key_count", Field::I64(count));
            transaction.put(Table::Providers, &provider_id_owned, &provider)?;
            crate::admin::providers::store_provider_api_key(transaction, &record)
        })
        .await
        .map_err(|_| "could not save the Antigravity account".to_owned())
}

fn normalize_account_identity(value: &str) -> Option<String> {
    let normalized = value.trim().to_lowercase();
    if normalized.is_empty()
        || normalized.len() > 512
        || normalized.bytes().any(|byte| byte.is_ascii_control())
    {
        None
    } else {
        Some(normalized)
    }
}

fn record_account_identity(record: &Record) -> Result<Option<String>, StorageError> {
    if record.optional_text("credential_type")? != Some("oauth") {
        return Ok(None);
    }
    let identity = match record.optional_text("oauth_account_identity")? {
        Some(identity) => Some(identity),
        None => record.optional_text("name")?,
    };
    Ok(identity.and_then(normalize_account_identity))
}

pub(super) async fn persist_account_update(
    state: &AppState,
    credential_id: &str,
    account: &AntigravityAccount,
) -> Result<(), String> {
    let serialized = serde_json::to_string(account)
        .map_err(|_| "could not encode refreshed Antigravity credentials".to_owned())?;
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt refreshed Antigravity credentials".to_owned())?
        .ok_or_else(|| "Antigravity returned empty refreshed credentials".to_owned())?;
    let id = credential_id.to_owned();
    let name = account.display_name();
    let account_identity = account
        .email
        .as_deref()
        .and_then(normalize_account_identity);
    let timestamp = crate::infra::db::utc_timestamp_now()
        .map_err(|_| "could not timestamp refreshed Antigravity credentials".to_owned())?;
    state
        .db
        .write(move |transaction| {
            let mut key = transaction
                .get::<Record>(Table::ProviderApiKeys, &id)?
                .ok_or(StorageError::NotFound)?;
            let provider_id = key.text("provider_id")?.to_owned();
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            if provider.text("adapter_id")? != crate::provider_adapters::ANTIGRAVITY_ADAPTER_ID {
                return Err(StorageError::Conflict);
            }
            crate::admin::providers::ensure_provider_not_deleting(&provider)?;
            let previous_identity_index =
                crate::admin::providers::provider_api_key_identity_index_key(&key)?;
            key.insert("name", Field::Text(name.clone()));
            key.insert("secret", Field::Bytes(encrypted.clone()));
            if let Some(identity) = account_identity.as_deref() {
                key.insert("oauth_account_identity", Field::Text(identity.to_owned()));
            }
            key.insert("invalid", Field::Bool(false));
            key.insert("last_error", Field::Null);
            key.insert("last_test_passed", Field::Bool(true));
            key.insert("last_test_status", Field::I64(200));
            key.insert("last_tested_at", Field::Text(timestamp.clone()));
            transaction.put(Table::ProviderApiKeys, &id, &key)?;
            if let Some(identity) = account_identity.as_deref() {
                if let Some(identity_index) = previous_identity_index {
                    transaction.delete(Table::ProviderApiKeyIndex, &identity_index)?;
                }
                transaction.put(
                    Table::ProviderApiKeyIndex,
                    &crate::infra::db::provider_api_key_identity_index_key(
                        &provider_id,
                        identity,
                        &id,
                    )?,
                    &id,
                )?;
            }
            if key.boolean("enabled")? {
                transaction.put(
                    Table::ProviderApiKeyAvailabilityIndex,
                    &crate::infra::db::provider_api_key_index_key(&provider_id, &id, true)?,
                    &id,
                )?;
            }
            Ok(())
        })
        .await
        .map_err(|_| "could not save refreshed Antigravity credentials".to_owned())
}

pub(super) async fn mark_reauthentication_required(
    state: &AppState,
    credential_id: &str,
) -> Result<(), String> {
    let id = credential_id.to_owned();
    state
        .db
        .write(move |transaction| {
            let Some(mut key) = transaction.get::<Record>(Table::ProviderApiKeys, &id)? else {
                return Ok(());
            };
            let provider_id = key.text("provider_id")?.to_owned();
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            crate::admin::providers::ensure_provider_not_deleting(&provider)?;
            let was_invalid = key.boolean("invalid")?;
            key.insert("invalid", Field::Bool(true));
            key.insert(
                "last_error",
                Field::Text("Antigravity account must be reconnected".to_owned()),
            );
            key.insert("last_test_passed", Field::Bool(false));
            transaction.put(Table::ProviderApiKeys, &id, &key)?;
            if key.boolean("enabled")? {
                transaction.delete(
                    Table::ProviderApiKeyAvailabilityIndex,
                    &crate::infra::db::provider_api_key_index_key(&provider_id, &id, true)?,
                )?;
            }
            if !was_invalid {
                let mut provider = transaction
                    .get::<Record>(Table::Providers, &provider_id)?
                    .ok_or(StorageError::NotFound)?;
                provider.insert(
                    "invalid_api_key_count",
                    Field::I64(provider.integer("invalid_api_key_count")?.saturating_add(1)),
                );
                transaction.put(Table::Providers, &provider_id, &provider)?;
            }
            Ok(())
        })
        .await
        .map_err(|_| "could not mark Antigravity account for reconnection".to_owned())
}
