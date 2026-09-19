use super::auth::codex::CodexAccount;
use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};
pub(crate) async fn save_codex_account(
    state: &AppState,
    provider_id: &str,
    account: AdapterOAuthAccount,
) -> Result<(), String> {
    let AdapterOAuthAccount {
        payload,
        display_name,
    } = account;
    let mut account: CodexAccount = serde_json::from_value(payload)
        .map_err(|_| "OpenAI returned invalid account credentials".to_owned())?;
    account.refresh_metadata();
    let provider_key = provider_id.to_owned();
    let stored_adapter = state
        .db
        .read(move |transaction| {
            transaction
                .get::<Record>(Table::Providers, &provider_key)?
                .map(|record| record.text("adapter_id").map(str::to_owned))
                .transpose()
        })
        .await
        .map_err(|_| "could not load the provider for OAuth sign-in".to_owned())?
        .ok_or_else(|| "provider no longer exists".to_owned())?;
    if stored_adapter != CODEX_ADAPTER_ID {
        return Err("provider settings changed during sign-in".to_owned());
    }

    let save_lock = state.codex_account_save_lock();
    let _save_guard = save_lock.lock().await;
    let serialized = serde_json::to_string(&account)
        .map_err(|_| "could not encode the OpenAI account credentials".to_owned())?;
    let encrypted = crate::security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| "could not encrypt the OpenAI account credentials".to_owned())?
        .ok_or_else(|| "OpenAI returned an empty account credential".to_owned())?;
    let index_prefix = crate::infra::db::provider_api_key_index_prefix(provider_id, false)
        .map_err(|_| "could not inspect existing OpenAI accounts".to_owned())?;
    let mut after_index: Option<String> = None;
    let mut match_state = CodexAccountMatchState::default();
    loop {
        let scan_prefix = index_prefix.clone();
        let after = after_index.clone();
        let batch = state
            .db
            .read(move |transaction| {
                let ids = transaction.scan_prefix_after::<String>(
                    Table::ProviderApiKeyIndex,
                    &scan_prefix,
                    after.as_deref(),
                    128,
                )?;
                let mut records = Vec::with_capacity(ids.len());
                for (index, id) in ids {
                    if let Some(record) = transaction.get::<Record>(Table::ProviderApiKeys, &id)? {
                        records.push((index, record));
                    }
                }
                Ok(records)
            })
            .await
            .map_err(|_| "could not inspect existing OpenAI accounts".to_owned())?;
        if batch.is_empty() {
            break;
        }

        after_index = batch.last().map(|(index, _)| index.clone());
        for (_, record) in batch {
            let id = record
                .text("id")
                .map_err(|_| "could not inspect an existing OpenAI account".to_owned())?
                .to_owned();
            let secret = record
                .bytes("secret")
                .map_err(|_| "could not inspect an existing OpenAI account".to_owned())?;
            let Some(secret) =
                crate::security::decrypt_secret(state.config.master_key.as_ref(), Some(secret))
                    .map_err(|_| "could not decrypt an existing OpenAI account".to_owned())?
            else {
                continue;
            };
            let Ok(mut previous) = serde_json::from_str::<CodexAccount>(&secret) else {
                continue;
            };
            previous.refresh_metadata();
            match_state.consider(&id, &previous, &account);
        }
    }
    if let Some(id) = match_state.selected_id() {
        let refreshed_at = crate::infra::db::utc_timestamp_now()
            .map_err(|_| "could not refresh the saved OpenAI account".to_owned())?;
        let provider_id = provider_id.to_owned();
        let display_name = display_name.clone();
        let encrypted = encrypted.clone();
        state
            .db
            .write(move |transaction| {
                let mut key = transaction
                    .get::<Record>(Table::ProviderApiKeys, &id)?
                    .ok_or(StorageError::NotFound)?;
                if key.text("provider_id")? != provider_id {
                    return Err(StorageError::Conflict);
                }
                let provider = transaction
                    .get::<Record>(Table::Providers, &provider_id)?
                    .ok_or(StorageError::NotFound)?;
                crate::admin::providers::ensure_provider_not_deleting(&provider)?;
                let was_invalid = key.boolean("invalid")?;
                let was_available = key.boolean("enabled")? && !was_invalid;
                key.insert("name", Field::Text(display_name.clone()));
                key.insert("secret", Field::Bytes(encrypted.clone()));
                key.insert("enabled", Field::Bool(true));
                key.insert("invalid", Field::Bool(false));
                key.insert("last_error", Field::Null);
                key.insert("last_test_passed", Field::Bool(true));
                key.insert("last_test_status", Field::I64(200));
                key.insert("last_tested_at", Field::Text(refreshed_at.clone()));
                transaction.put(Table::ProviderApiKeys, &id, &key)?;
                if !was_available {
                    let available =
                        crate::infra::db::provider_api_key_index_key(&provider_id, &id, true)?;
                    transaction.put(Table::ProviderApiKeyAvailabilityIndex, &available, &id)?;
                }
                if was_invalid {
                    let mut provider = transaction
                        .get::<Record>(Table::Providers, &provider_id)?
                        .ok_or(StorageError::NotFound)?;
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
            .map_err(|_| "could not refresh the saved OpenAI account".to_owned())?;
        return Ok(());
    }

    let created_at = crate::infra::db::utc_timestamp_now()
        .map_err(|_| "could not save the OpenAI account".to_owned())?;
    let key_id = uuid::Uuid::new_v4().to_string();
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
    let provider_id = provider_id.to_owned();
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            crate::admin::providers::ensure_provider_not_deleting(&provider)?;
            let count = provider
                .integer("api_key_count")?
                .checked_add(1)
                .ok_or_else(|| {
                    StorageError::Invalid("provider API key count overflowed".to_owned())
                })?;
            provider.insert("api_key_count", Field::I64(count));
            transaction.put(Table::Providers, &provider_id, &provider)?;
            crate::admin::providers::store_provider_api_key(transaction, &record)
        })
        .await
        .map_err(|_| "could not save the OpenAI account".to_owned())?;
    Ok(())
}

#[derive(Default)]
pub(crate) struct CodexAccountMatchState {
    exact_match_id: Option<String>,
    legacy_match_id: Option<String>,
    legacy_match_count: usize,
    strong_identity_conflict: bool,
}

impl CodexAccountMatchState {
    pub(crate) fn consider(&mut self, id: &str, previous: &CodexAccount, incoming: &CodexAccount) {
        let previous_account_id = non_empty_codex_identity(previous.account_id.as_ref());
        let incoming_account_id = non_empty_codex_identity(incoming.account_id.as_ref());
        let same_account_scope = match (previous_account_id, incoming_account_id) {
            (Some(previous_id), Some(incoming_id)) => previous_id == incoming_id,
            (None, None) => true,
            _ => codex_legacy_identity_matches(previous, incoming),
        };
        if !same_account_scope {
            return;
        }

        match (
            non_empty_codex_identity(previous.chatgpt_user_id.as_ref()),
            non_empty_codex_identity(incoming.chatgpt_user_id.as_ref()),
        ) {
            (Some(previous_user_id), Some(incoming_user_id))
                if previous_user_id == incoming_user_id && self.exact_match_id.is_none() =>
            {
                self.exact_match_id = Some(id.to_owned());
            }
            (Some(_), Some(_)) => {
                self.strong_identity_conflict = true;
            }
            (_, _) if codex_legacy_identity_matches(previous, incoming) => {
                self.legacy_match_count = self.legacy_match_count.saturating_add(1);
                if self.legacy_match_id.is_none() {
                    self.legacy_match_id = Some(id.to_owned());
                }
            }
            _ => {}
        }
    }

    pub(crate) fn selected_id(self) -> Option<String> {
        self.exact_match_id.or_else(|| {
            (!self.strong_identity_conflict && self.legacy_match_count == 1)
                .then_some(self.legacy_match_id)
                .flatten()
        })
    }
}

fn non_empty_codex_identity(value: Option<&String>) -> Option<&str> {
    value
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn codex_legacy_identity_matches(previous: &CodexAccount, incoming: &CodexAccount) -> bool {
    match (
        previous
            .email
            .as_deref()
            .filter(|email| !email.trim().is_empty()),
        incoming
            .email
            .as_deref()
            .filter(|email| !email.trim().is_empty()),
    ) {
        (Some(previous_email), Some(incoming_email)) => {
            previous_email.eq_ignore_ascii_case(incoming_email)
        }
        (None, None) => true,
        _ => false,
    }
}
