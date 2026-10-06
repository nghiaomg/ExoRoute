use super::account::RefreshError;
use super::payload::decode_embedded_payload;
use super::token::unix_now;
use super::*;

pub fn authorization_url(state: &str, challenge: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(AUTHORIZE_URL)
        .map_err(|_| "Cline authorization URL is invalid".to_owned())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_type", "extension")
        .append_pair("callback_url", CALLBACK_URL)
        .append_pair("redirect_uri", CALLBACK_URL)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state);
    Ok(url.to_string())
}

pub fn pkce_challenge(verifier: &str) -> String {
    general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub async fn exchange_code(
    code: &str,
    verifier: &str,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<ClineAccount, String> {
    if code.is_empty() || code.len() > 8192 || verifier.is_empty() || verifier.len() > 256 {
        return Err("Cline authorization response is invalid".to_owned());
    }
    if let Some(payload) = decode_embedded_payload(code)
        && let Ok(account) = ClineAccount::parse(&payload)
    {
        return Ok(account);
    }
    let (url, client) = oauth_client(TOKEN_URL, connect_timeout, request_timeout, upstream).await?;
    let response = client
        .post(url)
        .header("Accept", "application/json")
        .json(&json!({
            "grantType": "authorization_code",
            "code": code,
            "codeVerifier": verifier,
            "clientType": "extension",
            "redirectUri": CALLBACK_URL,
        }))
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach Cline OAuth service: {}",
                error.without_url()
            )
        })?;
    let value = bounded_json(response, "Cline OAuth token exchange").await?;
    ClineAccount::parse(&value)
}

pub async fn account_for_use(
    state: &AppState,
    credential_id: &str,
) -> Result<ClineAccount, String> {
    let lock = state.provider_token_refresh_lock(credential_id).await;
    let _guard = lock.lock().await;
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
            if !matches!(
                provider.text("adapter_id")?,
                CLINE_ADAPTER_ID | CLINEPASS_ADAPTER_ID
            ) {
                return Ok(None);
            }
            Ok(Some((
                key.bytes("secret")?.to_vec(),
                provider.text("adapter_id")?.to_owned(),
            )))
        })
        .await
        .map_err(|_| "could not load the Cline account".to_owned())?
        .ok_or_else(|| "Cline account was removed, disabled, or needs reconnecting".to_owned())?;
    let (encrypted, adapter_id) = stored;
    if encrypted.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Cline account data exceeded its size limit".to_owned());
    }
    let serialized = security::decrypt_secret(state.config.master_key.as_ref(), Some(&encrypted))
        .map_err(|_| "could not decrypt Cline account; check EXOROUTE_MASTER_KEY".to_owned())?
        .ok_or_else(|| "Cline account credentials are empty".to_owned())?;
    if serialized.len() > MAX_ACCOUNT_BYTES {
        return Err("stored Cline account data exceeded its size limit".to_owned());
    }
    let value: Value = serde_json::from_str(&serialized)
        .map_err(|_| "stored Cline account data is invalid".to_owned())?;
    let mut account = ClineAccount::parse(&value)
        .map_err(|_| "stored Cline account data is invalid".to_owned())?;
    if account.expires_at <= unix_now().saturating_add(REFRESH_LEAD_SECONDS) {
        match refresh_account(state, &adapter_id, credential_id, &mut account).await {
            Ok(()) => {}
            Err(RefreshError::ReauthenticationRequired) => {
                mark_reauthentication_required(state, credential_id).await?;
                return Err("Cline account must be reconnected".to_owned());
            }
            Err(RefreshError::Temporary(message)) => return Err(message),
        }
    }
    Ok(account)
}

pub(super) async fn refresh_account(
    state: &AppState,
    adapter_id: &str,
    credential_id: &str,
    account: &mut ClineAccount,
) -> Result<(), RefreshError> {
    let Some(refresh_token) = account.refresh_token.as_deref() else {
        return Err(RefreshError::ReauthenticationRequired);
    };
    let operational = state.operational_settings().settings;
    let (url, client) = oauth_client(
        REFRESH_URL,
        operational.connect_timeout,
        operational.request_timeout,
        operational.upstream,
    )
    .await
    .map_err(RefreshError::Temporary)?;
    let response = client
        .post(url)
        .header("Accept", "application/json")
        .json(&json!({
            "refreshToken": refresh_token,
            "grantType": "refresh_token",
            "clientType": "extension",
        }))
        .send()
        .await
        .map_err(|error| {
            RefreshError::Temporary(format!(
                "could not reach Cline token refresh service: {}",
                error.without_url()
            ))
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = match read_limited_body(response).await {
            Ok(body) => body,
            Err(error) => return Err(RefreshError::Temporary(error)),
        };
        let text = String::from_utf8_lossy(&body).to_ascii_lowercase();
        if matches!(status.as_u16(), 401 | 403)
            || text.contains("invalid_grant")
            || text.contains("invalid_request")
        {
            return Err(RefreshError::ReauthenticationRequired);
        }
        return Err(RefreshError::Temporary(format!(
            "Cline token refresh returned HTTP {}",
            status.as_u16()
        )));
    }
    let value = bounded_json(response, "Cline token refresh")
        .await
        .map_err(RefreshError::Temporary)?;
    let mut refreshed = ClineAccount::parse(&value).map_err(RefreshError::Temporary)?;
    if refreshed.refresh_token.is_none() {
        refreshed.refresh_token = account.refresh_token.clone();
    }
    let serialized = serde_json::to_string(&refreshed).map_err(|_| {
        RefreshError::Temporary("could not encode refreshed Cline account".to_owned())
    })?;
    let encrypted = security::encrypt_secret(state.config.master_key.as_ref(), &serialized)
        .map_err(|_| {
            RefreshError::Temporary("could not encrypt refreshed Cline account".to_owned())
        })?
        .ok_or_else(|| {
            RefreshError::Temporary("Cline returned an empty refreshed account".to_owned())
        })?;
    let id = credential_id.to_owned();
    let adapter_id = adapter_id.to_owned();
    let display_name = refreshed.display_name();
    let updated_at = crate::infra::db::utc_timestamp_now().map_err(|_| {
        RefreshError::Temporary("could not save refreshed Cline account".to_owned())
    })?;
    state
        .db
        .write(move |tx| {
            let mut key = tx
                .get::<Record>(Table::ProviderApiKeys, &id)?
                .ok_or(StorageError::NotFound)?;
            if key.optional_text("credential_type")? != Some("oauth") {
                return Err(StorageError::Conflict);
            }
            let provider_id = key.text("provider_id")?.to_owned();
            let provider = tx
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            if provider.text("adapter_id")? != adapter_id {
                return Err(StorageError::Conflict);
            }
            crate::admin::providers::ensure_provider_not_deleting(&provider)?;
            let was_invalid = key.boolean("invalid")?;
            key.insert("name", Field::Text(display_name.clone()));
            key.insert("secret", Field::Bytes(encrypted.clone()));
            key.insert("invalid", Field::Bool(false));
            key.insert("last_error", Field::Null);
            key.insert("last_test_passed", Field::Bool(true));
            key.insert("last_test_status", Field::I64(200));
            key.insert("last_tested_at", Field::Text(updated_at.clone()));
            tx.put(Table::ProviderApiKeys, &id, &key)?;
            if key.boolean("enabled")?
                && (was_invalid
                    || tx
                        .get::<String>(
                            Table::ProviderApiKeyAvailabilityIndex,
                            &crate::infra::db::provider_api_key_index_key(&provider_id, &id, true)?,
                        )?
                        .is_none())
            {
                tx.put(
                    Table::ProviderApiKeyAvailabilityIndex,
                    &crate::infra::db::provider_api_key_index_key(&provider_id, &id, true)?,
                    &id,
                )?;
            }
            if was_invalid {
                let mut provider = tx
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
                tx.put(Table::Providers, &provider_id, &provider)?;
            }
            Ok(())
        })
        .await
        .map_err(|_| {
            RefreshError::Temporary("could not save refreshed Cline account".to_owned())
        })?;
    *account = refreshed;
    Ok(())
}

pub(super) async fn mark_reauthentication_required(
    state: &AppState,
    credential_id: &str,
) -> Result<(), String> {
    let id = credential_id.to_owned();
    state
        .db
        .write(move |tx| {
            let Some(mut key) = tx.get::<Record>(Table::ProviderApiKeys, &id)? else {
                return Ok(());
            };
            let provider_id = key.text("provider_id")?.to_owned();
            let mut provider = tx
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            crate::admin::providers::ensure_provider_not_deleting(&provider)?;
            let was_invalid = key.boolean("invalid")?;
            key.insert("invalid", Field::Bool(true));
            key.insert(
                "last_error",
                Field::Text("Cline account must be reconnected".to_owned()),
            );
            if key.boolean("enabled")? {
                tx.delete(
                    Table::ProviderApiKeyAvailabilityIndex,
                    &crate::infra::db::provider_api_key_index_key(&provider_id, &id, true)?,
                )?;
            }
            tx.put(Table::ProviderApiKeys, &id, &key)?;
            if !was_invalid {
                provider.insert(
                    "invalid_api_key_count",
                    Field::I64(provider.integer("invalid_api_key_count")?.saturating_add(1)),
                );
                tx.put(Table::Providers, &provider_id, &provider)?;
            }
            Ok(())
        })
        .await
        .map_err(|_| "could not mark Cline account for reconnection".to_owned())
}

pub(super) async fn oauth_client(
    endpoint: &str,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<(reqwest::Url, reqwest::Client), String> {
    egress::provider_client(
        endpoint,
        false,
        connect_timeout.min(Duration::from_secs(3)),
        request_timeout.min(Duration::from_secs(15)),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        upstream,
    )
    .await
}

pub(super) async fn bounded_json(
    response: reqwest::Response,
    operation: &str,
) -> Result<Value, String> {
    if !response.status().is_success() {
        return Err(format!(
            "{operation} returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let body = read_limited_body(response).await?;
    serde_json::from_slice(&body).map_err(|_| format!("{operation} returned invalid JSON"))
}

pub(super) async fn read_limited_body(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("OAuth response could not be read: {}", error.without_url()))?
    {
        if body.len().saturating_add(chunk.len()) > MAX_OAUTH_BODY_BYTES {
            return Err("OAuth response exceeded its size limit".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
