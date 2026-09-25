use super::*;

use super::token::{oauth_client, read_token_response};
use super::usage::usage_core::unix_now;
pub fn authorization_url(state: &str, code_challenge: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(AUTHORIZE_URL).map_err(|_| "OAuth URL is invalid")?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", CALLBACK_URL)
        .append_pair("scope", "openid profile email offline_access")
        .append_pair("code_challenge", code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", "codex_cli_rs")
        .append_pair("state", state);
    Ok(url.to_string())
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub async fn exchange_code(
    code: &str,
    verifier: &str,
    connect_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<CodexAccount, String> {
    let (url, client) = oauth_client(connect_timeout, request_timeout, upstream).await?;
    let response = client
        .post(url)
        .header("Accept", "application/json")
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("redirect_uri", CALLBACK_URL),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|_| "Could not reach the OpenAI OAuth service".to_owned())?;
    read_token_response(response).await
}

pub async fn refresh_account(
    account: &mut CodexAccount,
    connect_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<(), RefreshError> {
    let Some(refresh_token) = account.refresh_token.as_deref() else {
        return Err(RefreshError::ReauthenticationRequired);
    };
    let (url, client) = oauth_client(connect_timeout, request_timeout, upstream)
        .await
        .map_err(RefreshError::Temporary)?;
    let response = client
        .post(url)
        .header("Accept", "application/json")
        .json(&json!({
            "client_id": CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .map_err(|_| {
            RefreshError::Temporary("Could not reach the OpenAI OAuth service".to_owned())
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let mut body = Vec::new();
        let mut response = response;
        while let Ok(Some(chunk)) = response.chunk().await {
            let remaining = (16 * 1024usize).saturating_sub(body.len());
            body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            if body.len() >= 16 * 1024 {
                break;
            }
        }
        let lower = String::from_utf8_lossy(&body).to_ascii_lowercase();
        if lower.contains("invalid_grant")
            || lower.contains("refresh token") && lower.contains("expired")
        {
            return Err(RefreshError::ReauthenticationRequired);
        }
        return Err(RefreshError::Temporary(format!(
            "OpenAI token refresh returned HTTP {}",
            status.as_u16()
        )));
    }
    let mut refreshed = read_token_response(response)
        .await
        .map_err(RefreshError::Temporary)?;
    if refreshed.refresh_token.is_none() {
        refreshed.refresh_token = account.refresh_token.clone();
    }
    *account = refreshed;
    Ok(())
}

#[derive(Debug)]
pub enum RefreshError {
    ReauthenticationRequired,
    Temporary(String),
}

pub async fn account_for_use(
    state: &crate::state::AppState,
    credential_id: &str,
) -> Result<CodexAccount, String> {
    load_account_for_use(state, credential_id, false, false).await
}

pub async fn force_refresh_account_for_use(
    state: &crate::state::AppState,
    credential_id: &str,
) -> Result<CodexAccount, String> {
    load_account_for_use(state, credential_id, true, false).await
}

pub(super) async fn load_account_for_use(
    state: &crate::state::AppState,
    credential_id: &str,
    force_refresh: bool,
    allow_disabled: bool,
) -> Result<CodexAccount, String> {
    let refresh_lock = state.codex_refresh_lock(credential_id).await;
    let _refresh_guard = refresh_lock.lock().await;
    let credential_id_owned = credential_id.to_owned();
    let encrypted = state
        .db
        .read(move |transaction| {
            let Some(record) =
                transaction.get::<Record>(Table::ProviderApiKeys, &credential_id_owned)?
            else {
                return Ok(None);
            };
            if !allow_disabled && !record.boolean("enabled")? {
                return Ok(None);
            }
            Ok(Some(record.bytes("secret")?.to_vec()))
        })
        .await
        .map_err(|_| "could not load the OpenAI Codex account".to_owned())?
        .ok_or_else(|| "OpenAI Codex account was removed or disabled".to_owned())?;
    if encrypted.len() > MAX_CREDENTIAL_BYTES {
        return Err("stored OpenAI Codex account data exceeded its size limit".to_owned());
    }
    let serialized =
        crate::security::decrypt_secret(state.config.master_key.as_ref(), Some(&encrypted))
            .map_err(|_| {
                "could not decrypt the OpenAI Codex account; check EXOROUTE_MASTER_KEY".to_owned()
            })?
            .ok_or_else(|| "OpenAI Codex account credentials are empty".to_owned())?;
    let mut account: CodexAccount = serde_json::from_str(&serialized)
        .map_err(|_| "stored OpenAI Codex account data is invalid".to_owned())?;
    if force_refresh || account.expires_at <= unix_now().saturating_add(REFRESH_LEAD_SECONDS) {
        let operational = state.operational_settings().settings;
        match refresh_account(
            &mut account,
            operational.connect_timeout,
            operational.request_timeout,
            operational.upstream,
        )
        .await
        {
            Ok(()) => {
                account.refresh_metadata();
                let serialized = serde_json::to_string(&account)
                    .map_err(|_| "could not encode the refreshed OpenAI account".to_owned())?;
                let encrypted =
                    crate::security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
                        .map_err(|_| "could not encrypt the refreshed OpenAI account".to_owned())?
                        .ok_or_else(|| "OpenAI returned an empty refreshed account".to_owned())?;
                let id = credential_id.to_owned();
                let display_name = account.display_name();
                let updated_at = crate::infra::db::utc_timestamp_now().map_err(|_| {
                    "could not save refreshed OpenAI account credentials".to_owned()
                })?;
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
                        crate::admin::providers::ensure_provider_not_deleting(&provider)?;
                        let was_invalid = key.boolean("invalid")?;
                        key.insert("name", Field::Text(display_name.clone()));
                        key.insert("secret", Field::Bytes(encrypted.clone()));
                        key.insert("invalid", Field::Bool(false));
                        key.insert("last_error", Field::Null);
                        key.insert("last_test_passed", Field::Bool(true));
                        key.insert("last_test_status", Field::I64(200));
                        key.insert("last_tested_at", Field::Text(updated_at.clone()));
                        transaction.put(Table::ProviderApiKeys, &id, &key)?;
                        if key.boolean("enabled")?
                            && (was_invalid
                                || transaction
                                    .get::<String>(
                                        Table::ProviderApiKeyAvailabilityIndex,
                                        &crate::infra::db::provider_api_key_index_key(
                                            &provider_id,
                                            &id,
                                            true,
                                        )?,
                                    )?
                                    .is_none())
                        {
                            transaction.put(
                                Table::ProviderApiKeyAvailabilityIndex,
                                &crate::infra::db::provider_api_key_index_key(
                                    &provider_id,
                                    &id,
                                    true,
                                )?,
                                &id,
                            )?;
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
                    .map_err(|_| {
                        "could not save refreshed OpenAI account credentials".to_owned()
                    })?;
            }
            Err(RefreshError::ReauthenticationRequired) => {
                let id = credential_id.to_owned();
                state
                    .db
                    .write(move |transaction| {
                        let mut key = transaction
                            .get::<Record>(Table::ProviderApiKeys, &id)?
                            .ok_or(StorageError::NotFound)?;
                        let was_invalid = key.boolean("invalid")?;
                        let provider_id = key.text("provider_id")?.to_owned();
                        let provider = transaction
                            .get::<Record>(Table::Providers, &provider_id)?
                            .ok_or(StorageError::NotFound)?;
                        crate::admin::providers::ensure_provider_not_deleting(&provider)?;
                        key.insert("invalid", Field::Bool(true));
                        key.insert(
                            "last_error",
                            Field::Text("OpenAI Codex account needs to be reconnected".to_owned()),
                        );
                        key.insert("last_test_passed", Field::Bool(false));
                        transaction.put(Table::ProviderApiKeys, &id, &key)?;
                        if key.boolean("enabled")? {
                            let available = crate::infra::db::provider_api_key_index_key(
                                &provider_id,
                                &id,
                                true,
                            )?;
                            transaction
                                .delete(Table::ProviderApiKeyAvailabilityIndex, &available)?;
                        }
                        if !was_invalid {
                            let mut provider = transaction
                                .get::<Record>(Table::Providers, &provider_id)?
                                .ok_or(StorageError::NotFound)?;
                            provider.insert(
                                "invalid_api_key_count",
                                Field::I64(
                                    provider.integer("invalid_api_key_count")?.saturating_add(1),
                                ),
                            );
                            transaction.put(Table::Providers, &provider_id, &provider)?;
                        }
                        Ok(())
                    })
                    .await
                    .map_err(|_| "could not save the OpenAI account reconnect state".to_owned())?;
                return Err("OpenAI Codex account needs to be reconnected".to_owned());
            }
            Err(RefreshError::Temporary(error))
                if force_refresh || account.expires_at <= unix_now() =>
            {
                return Err(format!("OpenAI Codex account token expired; {error}"));
            }
            Err(RefreshError::Temporary(_)) => {}
        }
    }
    Ok(account)
}
