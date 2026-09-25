//! Credential lookup and refresh for stored Antigravity accounts.
use super::save::mark_reauthentication_required;
use super::save::persist_account_update;

use super::*;
use crate::infra::storage::Record;
use std::time::Duration;

pub(in crate::provider_adapters) async fn account_for_use(
    state: &AppState,
    credential_id: &str,
) -> Result<AntigravityAccount, String> {
    load_account_for_use(state, credential_id, false, false).await
}

pub(in crate::provider_adapters) async fn load_account_for_use(
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
