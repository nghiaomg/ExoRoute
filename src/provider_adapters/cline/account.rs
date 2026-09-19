use super::auth::cline::ClineAccount;
use super::*;
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    security,
};
use serde_json::Value;
pub(super) async fn save_cline_account(
    adapter_id: &str,
    state: &AppState,
    provider_id: &str,
    account: AdapterOAuthAccount,
) -> Result<(), String> {
    let account = ClineAccount::parse(&account.payload)
        .map_err(|_| "Cline returned invalid account credentials".to_owned())?;
    let account_identity = cline_account_identity(&account);
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
        .map_err(|_| "could not load provider for Cline sign-in".to_owned())?
        .ok_or_else(|| "provider no longer exists".to_owned())?;
    if stored_adapter != adapter_id {
        return Err("provider settings changed during Cline sign-in".to_owned());
    }
    let serialized = serde_json::to_string(&account)
        .map_err(|_| "could not encode Cline account credentials".to_owned())?;
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt Cline account credentials".to_owned())?
        .ok_or_else(|| "Cline returned an empty account credential".to_owned())?;
    let now = crate::infra::db::utc_timestamp_now()
        .map_err(|_| "could not save Cline account credentials".to_owned())?;
    let key_id = uuid::Uuid::new_v4().to_string();
    let mut record = crate::admin::providers::provider_api_key_record(
        crate::admin::providers::ProviderApiKeyRecordData {
            id: &key_id,
            provider_id,
            name: &account.display_name(),
            secret: &encrypted,
            last_error: None,
            last_test_passed: true,
            last_test_status: Some(200),
            tested_at: &now,
            created_at: &now,
        },
    );
    record.insert("credential_type", Field::Text("oauth".to_owned()));
    if let Some(identity) = account_identity.as_deref() {
        record.insert("oauth_account_identity", Field::Text(identity.to_owned()));
    }
    let provider_id = provider_id.to_owned();
    let adapter_id = adapter_id.to_owned();
    let incoming_account = account.clone();
    let master_key = state.config.master_key;
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            crate::admin::providers::ensure_provider_not_deleting(&provider)?;
            if provider.text("adapter_id")? != adapter_id {
                return Err(StorageError::Conflict);
            }

            let mut identity_index_active = account_identity.is_some();
            let mut index_prefix = if let Some(identity) = account_identity.as_deref() {
                crate::infra::db::provider_api_key_identity_index_prefix(&provider_id, identity)?
            } else {
                crate::infra::db::provider_api_key_created_prefix(&provider_id)?
            };
            let mut after_index = None;
            let matching_id: Option<String> = loop {
                let entries = if identity_index_active {
                    transaction.scan_prefix_after::<String>(
                        Table::ProviderApiKeyIndex,
                        &index_prefix,
                        after_index.as_deref(),
                        CLINE_ACCOUNT_MATCH_BATCH_SIZE,
                    )?
                } else {
                    transaction.scan_prefix_after::<String>(
                        Table::ProviderApiKeyCreatedIndex,
                        &index_prefix,
                        after_index.as_deref(),
                        CLINE_ACCOUNT_MATCH_BATCH_SIZE,
                    )?
                };
                if entries.is_empty() {
                    if identity_index_active {
                        identity_index_active = false;
                        index_prefix =
                            crate::infra::db::provider_api_key_created_prefix(&provider_id)?;
                        after_index = None;
                        continue;
                    }
                    break None;
                }
                after_index = entries.last().map(|(index, _)| index.clone());
                let mut found = None;
                for (_, candidate_id) in entries {
                    let Some(existing) =
                        transaction.get::<Record>(Table::ProviderApiKeys, &candidate_id)?
                    else {
                        continue;
                    };
                    if existing.text("provider_id")? != provider_id
                        || existing
                            .optional_text("credential_type")?
                            .is_some_and(|value| value != "oauth")
                    {
                        continue;
                    }
                    if stored_cline_account_matches(
                        &existing,
                        &incoming_account,
                        account_identity.as_deref(),
                        master_key.as_ref(),
                    ) {
                        found = Some(candidate_id);
                        break;
                    }
                }
                if let Some(found) = found {
                    break Some(found);
                }
                if after_index.is_some() && identity_index_active {
                    continue;
                }
                if identity_index_active {
                    identity_index_active = false;
                    index_prefix = crate::infra::db::provider_api_key_created_prefix(&provider_id)?;
                    after_index = None;
                } else if after_index.is_some() {
                    continue;
                } else {
                    break None;
                }
            };

            if let Some(existing_id) = matching_id {
                let mut existing = transaction
                    .get::<Record>(Table::ProviderApiKeys, &existing_id)?
                    .ok_or(StorageError::NotFound)?;
                let previous_identity_index =
                    crate::admin::providers::provider_api_key_identity_index_key(&existing)?;
                let was_invalid = existing.boolean("invalid")?;
                existing.insert("name", Field::Text(record.text("name")?.to_owned()));
                existing.insert("secret", Field::Bytes(encrypted.clone()));
                existing.insert("credential_type", Field::Text("oauth".to_owned()));
                if let Some(identity) = account_identity.as_deref() {
                    existing.insert("oauth_account_identity", Field::Text(identity.to_owned()));
                }
                existing.insert("invalid", Field::Bool(false));
                existing.insert("last_error", Field::Null);
                existing.insert("last_test_passed", Field::Bool(true));
                existing.insert("last_test_status", Field::I64(200));
                existing.insert("last_tested_at", Field::Text(now.clone()));
                transaction.put(Table::ProviderApiKeys, &existing_id, &existing)?;
                if let Some(previous_index) = previous_identity_index {
                    transaction.delete(Table::ProviderApiKeyIndex, &previous_index)?;
                }
                if let Some(identity) = account_identity.as_deref() {
                    transaction.put(
                        Table::ProviderApiKeyIndex,
                        &crate::infra::db::provider_api_key_identity_index_key(
                            &provider_id,
                            identity,
                            &existing_id,
                        )?,
                        &existing_id,
                    )?;
                }
                if existing.boolean("enabled")? {
                    transaction.put(
                        Table::ProviderApiKeyAvailabilityIndex,
                        &crate::infra::db::provider_api_key_index_key(
                            &provider_id,
                            &existing_id,
                            true,
                        )?,
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
                return Ok(());
            }

            let count = provider
                .integer("api_key_count")?
                .checked_add(1)
                .ok_or_else(|| {
                    StorageError::Invalid("provider credential count overflowed".to_owned())
                })?;
            provider.insert("api_key_count", Field::I64(count));
            transaction.put(Table::Providers, &provider_id, &provider)?;
            crate::admin::providers::store_provider_api_key(transaction, &record)
        })
        .await
        .map_err(|_| "could not save Cline account credentials".to_owned())
}

fn normalize_cline_identity(value: &str) -> Option<String> {
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

fn cline_account_identity(account: &ClineAccount) -> Option<String> {
    account
        .account_id
        .as_deref()
        .and_then(normalize_cline_identity)
        .map(|value| format!("account:{value}"))
        .or_else(|| {
            account
                .email
                .as_deref()
                .and_then(normalize_cline_identity)
                .map(|value| format!("email:{value}"))
        })
}

fn cline_accounts_match(previous: &ClineAccount, incoming: &ClineAccount) -> bool {
    match (
        previous
            .account_id
            .as_deref()
            .and_then(normalize_cline_identity),
        incoming
            .account_id
            .as_deref()
            .and_then(normalize_cline_identity),
    ) {
        (Some(previous_id), Some(incoming_id)) => previous_id == incoming_id,
        _ => previous
            .email
            .as_deref()
            .and_then(normalize_cline_identity)
            .zip(incoming.email.as_deref().and_then(normalize_cline_identity))
            .is_some_and(|(previous_email, incoming_email)| previous_email == incoming_email),
    }
}

fn stored_cline_account_matches(
    record: &Record,
    incoming: &ClineAccount,
    identity: Option<&str>,
    master_key: Option<&[u8; 32]>,
) -> bool {
    if identity.is_some()
        && record
            .optional_text("oauth_account_identity")
            .ok()
            .flatten()
            == identity
    {
        return true;
    }
    let Ok(secret) = record.bytes("secret") else {
        return false;
    };
    let Ok(Some(serialized)) = security::decrypt_secret(master_key, Some(secret)) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&serialized) else {
        return false;
    };
    let Ok(previous) = ClineAccount::parse(&value) else {
        return false;
    };
    cline_accounts_match(&previous, incoming)
}
