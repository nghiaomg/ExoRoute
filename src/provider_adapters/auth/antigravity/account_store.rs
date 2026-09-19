use super::{
    AntigravityAccount, AppState, Field, MAX_ACCOUNT_BYTES, MAX_OAUTH_BODY_BYTES,
    REFRESH_LEAD_SECONDS, RefreshError, StorageError, TOKEN_URL, Table, oauth_client,
    oauth_client_id, oauth_client_secret, read_json, security, unix_now, validate_account,
};
use crate::infra::storage::Record;
use std::time::Duration;

pub(crate) async fn account_for_use(
    state: &AppState,
    credential_id: &str,
) -> Result<AntigravityAccount, String> {
    load_account_for_use(state, credential_id, false, false).await
}

pub(super) async fn load_account_for_use(
    state: &AppState,
    credential_id: &str,
    force_refresh: bool,
    allow_disabled: bool,
) -> Result<AntigravityAccount, String> {
    let lock = state.provider_token_refresh_lock(credential_id).await;
    let _guard = lock.lock().await;
    let id = credential_id.to_owned();
    let encrypted = state
        .db
        .read(move |transaction| {
            let Some(key) = transaction.get::<Record>(Table::ProviderApiKeys, &id)? else {
                return Ok(None);
            };
            if (!allow_disabled && !key.boolean("enabled")?)
                || key.boolean("invalid")?
                || key.optional_text("credential_type")? != Some("oauth")
            {
                return Ok(None);
            }
            let provider_id = key.text("provider_id")?.to_owned();
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            if provider.text("adapter_id")? != crate::provider_adapters::ANTIGRAVITY_ADAPTER_ID {
                return Ok(None);
            }
            Ok(Some(key.bytes("secret")?.to_vec()))
        })
        .await
        .map_err(|_| "could not load the Antigravity account".to_owned())?
        .ok_or_else(|| {
            "Antigravity account was removed, disabled, or needs reconnecting".to_owned()
        })?;
    if encrypted.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Antigravity account data exceeded its size limit".to_owned());
    }
    let serialized = security::decrypt_secret(state.config.master_key.as_ref(), Some(&encrypted))
        .map_err(|_| "could not decrypt Antigravity account; check EXOROUTE_MASTER_KEY".to_owned())?
        .ok_or_else(|| "Antigravity account credentials are empty".to_owned())?;
    if serialized.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Antigravity account data exceeded its size limit".to_owned());
    }
    let mut account: AntigravityAccount = serde_json::from_str(&serialized)
        .map_err(|_| "stored Antigravity account data is invalid".to_owned())?;
    validate_account(&account)?;
    if force_refresh || account.expires_at <= unix_now().saturating_add(REFRESH_LEAD_SECONDS) {
        let operational = state.operational_settings().settings;
        match refresh_account(
            state,
            credential_id,
            &mut account,
            operational.connect_timeout,
            operational.request_timeout,
            operational.upstream,
        )
        .await
        {
            Ok(()) => {}
            Err(RefreshError::ReauthenticationRequired) => {
                mark_reauthentication_required(state, credential_id).await?;
                return Err("Antigravity account must be reconnected".to_owned());
            }
            Err(RefreshError::Temporary(message))
                if force_refresh || account.expires_at <= unix_now() =>
            {
                return Err(format!("Antigravity account token expired; {message}"));
            }
            Err(RefreshError::Temporary(_)) => {}
        }
    }
    Ok(account)
}

async fn refresh_account(
    state: &AppState,
    credential_id: &str,
    account: &mut AntigravityAccount,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<(), RefreshError> {
    let Some(refresh_token) = account.refresh_token.as_deref() else {
        return Err(RefreshError::ReauthenticationRequired);
    };
    let client_id = oauth_client_id().map_err(RefreshError::Temporary)?;
    let client_secret = oauth_client_secret().map_err(RefreshError::Temporary)?;
    let (url, client) = oauth_client(TOKEN_URL, connect_timeout, request_timeout, upstream)
        .await
        .map_err(RefreshError::Temporary)?;
    let mut form = vec![
        ("grant_type", "refresh_token".to_owned()),
        ("client_id", client_id),
        ("refresh_token", refresh_token.to_owned()),
    ];
    if let Some(secret) = client_secret {
        form.push(("client_secret", secret));
    }
    let response = client
        .post(url)
        .header("Accept", "application/json")
        .form(&form)
        .send()
        .await
        .map_err(|error| {
            RefreshError::Temporary(format!(
                "could not reach Google OAuth service: {}",
                error.without_url()
            ))
        })?;
    let status = response.status();
    let value = read_json(response, "Google OAuth token refresh", MAX_OAUTH_BODY_BYTES)
        .await
        .map_err(|error| {
            if matches!(
                error.status,
                Some(http::StatusCode::UNAUTHORIZED | http::StatusCode::FORBIDDEN)
            ) || error.message.to_ascii_lowercase().contains("invalid_grant")
            {
                RefreshError::ReauthenticationRequired
            } else {
                RefreshError::Temporary(format!(
                    "Google OAuth token refresh returned HTTP {}",
                    status.as_u16()
                ))
            }
        })?;
    let mut refreshed =
        AntigravityAccount::from_token_response(&value).map_err(RefreshError::Temporary)?;
    if refreshed.refresh_token.is_none() {
        refreshed.refresh_token = account.refresh_token.clone();
    }
    refreshed.email = account.email.clone();
    refreshed.project_id = account.project_id.clone();
    refreshed.tier = account.tier.clone();
    refreshed.client_profile = account.client_profile.clone();
    persist_account_update(state, credential_id, &refreshed)
        .await
        .map_err(RefreshError::Temporary)?;
    *account = refreshed;
    Ok(())
}

pub(crate) async fn save_account(
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

async fn persist_account_update(
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

async fn mark_reauthentication_required(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_adapters::ANTIGRAVITY_ADAPTER_ID;
    use crate::support::test_support::{ProviderSeed, TestDatabase, seed_provider};

    fn test_account(access_token: &str, email: &str) -> AntigravityAccount {
        AntigravityAccount {
            access_token: access_token.to_owned(),
            refresh_token: Some("refresh-token".to_owned()),
            expires_at: unix_now().saturating_add(3_600),
            scope: Some("scope".to_owned()),
            email: Some(email.to_owned()),
            project_id: Some("project-id".to_owned()),
            tier: Some("tier".to_owned()),
            client_profile: "ide".to_owned(),
        }
    }

    #[tokio::test]
    async fn reconnecting_the_same_google_account_updates_one_credential() {
        let database = TestDatabase::open().await;
        seed_provider(
            &database.db,
            ProviderSeed {
                id: "antigravity-provider",
                name: "Antigravity",
                base_url: super::super::RUNTIME_BASE_URL,
                adapter_id: ANTIGRAVITY_ADAPTER_ID,
                auth_type: "oauth",
                model_prefix: "ag",
                preferred_protocol: "google_generate_content",
                supported_protocols: &["google_generate_content"],
            },
        )
        .await
        .expect("seed Antigravity provider");

        let mut config = database.config();
        config.master_key = Some([47_u8; 32]);
        let state = AppState::new(config, database.db.clone());

        save_account(
            &state,
            "antigravity-provider",
            test_account("access-token-1", "Account@example.com"),
        )
        .await
        .expect("save first Antigravity account");

        let first = state
            .db
            .read(|transaction| {
                let prefix =
                    crate::infra::db::provider_api_key_created_prefix("antigravity-provider")?;
                let (_, id) = transaction
                    .scan_prefix::<String>(Table::ProviderApiKeyCreatedIndex, &prefix, 1)?
                    .into_iter()
                    .next()
                    .ok_or(StorageError::NotFound)?;
                transaction
                    .get::<Record>(Table::ProviderApiKeys, &id)?
                    .ok_or(StorageError::NotFound)
            })
            .await
            .expect("read first Antigravity account");
        state
            .db
            .write(move |transaction| {
                let mut duplicate = first.clone();
                duplicate.insert(
                    "id",
                    Field::Text("duplicate-antigravity-account".to_owned()),
                );
                crate::admin::providers::store_provider_api_key(transaction, &duplicate)?;
                let mut provider = transaction
                    .get::<Record>(Table::Providers, "antigravity-provider")?
                    .ok_or(StorageError::NotFound)?;
                provider.insert(
                    "api_key_count",
                    Field::I64(provider.integer("api_key_count")?.saturating_add(1)),
                );
                transaction.put(Table::Providers, "antigravity-provider", &provider)
            })
            .await
            .expect("seed duplicate Antigravity account");

        save_account(
            &state,
            "antigravity-provider",
            test_account("access-token-2", "account@example.com"),
        )
        .await
        .expect("reconnect Antigravity account");

        let (provider, accounts) = state
            .db
            .read(|transaction| {
                let provider = transaction
                    .get::<Record>(Table::Providers, "antigravity-provider")?
                    .ok_or(StorageError::NotFound)?;
                let prefix =
                    crate::infra::db::provider_api_key_created_prefix("antigravity-provider")?;
                let indexes = transaction.scan_prefix::<String>(
                    Table::ProviderApiKeyCreatedIndex,
                    &prefix,
                    usize::MAX,
                )?;
                let mut accounts = Vec::new();
                for (_, id) in indexes {
                    let Some(record) = transaction.get::<Record>(Table::ProviderApiKeys, &id)?
                    else {
                        continue;
                    };
                    accounts.push(record);
                }
                Ok((provider, accounts))
            })
            .await
            .expect("read saved Antigravity account");

        assert_eq!(provider.integer("api_key_count").expect("key count"), 1);
        assert_eq!(accounts.len(), 1);
        assert_eq!(
            accounts[0].text("name").expect("account name"),
            "account@example.com"
        );
        assert_eq!(
            accounts[0]
                .optional_text("oauth_account_identity")
                .expect("account identity"),
            Some("account@example.com")
        );
        let encrypted = accounts[0].bytes("secret").expect("account secret");
        let serialized =
            security::decrypt_secret(state.config.master_key.as_ref(), Some(encrypted))
                .expect("decrypt account secret")
                .expect("account secret payload");
        let account: AntigravityAccount =
            serde_json::from_str(&serialized).expect("decode account secret");
        assert_eq!(account.access_token, "access-token-2");
    }
}
