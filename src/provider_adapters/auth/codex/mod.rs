use crate::infra::storage::{Field, Record, StorageError, Table};
use crate::security::egress;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use http::HeaderValue;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const CALLBACK_URL: &str = "http://localhost:1455/auth/callback";
pub const BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
pub const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
pub const CODEX_CLI_VERSION: &str = "0.154.0";
pub const CODEX_USER_AGENT: &str = "codex_cli_rs/0.154.0";
pub const MAX_TOKEN_BYTES: usize = 32 * 1024;
#[cfg(test)]
pub(crate) use usage::MAX_USAGE_QUOTAS;
pub(crate) use usage::parse_usage_snapshot;

mod usage;
const MAX_CREDENTIAL_BYTES: usize = MAX_TOKEN_BYTES * 3 + 4096;
pub const REFRESH_LEAD_SECONDS: i64 = 5 * 60;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodexAccount {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    pub expires_at: i64,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub chatgpt_user_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
}

impl CodexAccount {
    fn from_tokens(tokens: TokenResponse) -> Result<Self, String> {
        validate_token(&tokens.access_token)?;
        if let Some(refresh_token) = tokens.refresh_token.as_deref() {
            validate_token(refresh_token)?;
        }
        if let Some(id_token) = tokens.id_token.as_deref() {
            validate_token(id_token)?;
        }
        let expires_in = tokens.expires_in.unwrap_or(3600).clamp(60, 31_536_000);
        let mut account = Self {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            id_token: tokens.id_token,
            expires_at: unix_now().saturating_add(expires_in),
            account_id: None,
            chatgpt_user_id: None,
            email: None,
            plan: None,
        };
        account.refresh_metadata();
        Ok(account)
    }

    pub fn refresh_metadata(&mut self) {
        let claim_sources = [
            self.id_token.as_deref().and_then(decode_jwt_claims),
            decode_jwt_claims(&self.access_token),
        ];
        let mut jwt_subject = None;
        for claims in claim_sources.into_iter().flatten() {
            if self.email.is_none() {
                self.email = claims
                    .get("email")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            let auth = claims
                .get("https://api.openai.com/auth")
                .or_else(|| claims.get("https://chatgpt.com/auth"));
            if self.account_id.is_none() {
                self.account_id = auth.and_then(|value| {
                    value
                        .get("chatgpt_account_id")
                        .or_else(|| value.get("account_id"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            }
            if self.chatgpt_user_id.is_none() {
                self.chatgpt_user_id = auth.and_then(|value| {
                    value
                        .get("chatgpt_user_id")
                        .or_else(|| value.get("user_id"))
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .map(str::to_owned)
                });
            }
            if self.plan.is_none() {
                self.plan = auth.and_then(|value| {
                    value
                        .get("chatgpt_plan_type")
                        .or_else(|| value.get("plan_type"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            }
            if jwt_subject.is_none() {
                jwt_subject = claims
                    .get("sub")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned);
            }
        }
        if self.chatgpt_user_id.is_none() {
            self.chatgpt_user_id = jwt_subject;
        }
        for value in [
            &mut self.email,
            &mut self.account_id,
            &mut self.chatgpt_user_id,
            &mut self.plan,
        ] {
            if value.as_ref().is_some_and(|item| item.len() > 512) {
                *value = None;
            }
        }
    }

    pub fn display_name(&self) -> String {
        self.email
            .as_deref()
            .filter(|email| !email.trim().is_empty())
            .unwrap_or("OpenAI Codex account")
            .chars()
            .take(256)
            .collect()
    }
}

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

async fn load_account_for_use(
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

pub async fn fetch_usage(
    state: &crate::state::AppState,
    credential_id: &str,
) -> Result<crate::provider_adapters::ProviderUsageSnapshot, String> {
    let account = account_for_usage(state, credential_id).await?;
    match fetch_usage_for_account(state, &account).await {
        Err(CodexUsageError::Unauthorized) => {
            let refreshed = force_refresh_account_for_usage(state, credential_id).await?;
            fetch_usage_for_account(state, &refreshed)
                .await
                .map_err(CodexUsageError::into_message)
        }
        result => result.map_err(CodexUsageError::into_message),
    }
}

/// Loads an account for the read-only usage/quota path. Disabled credentials
/// must remain eligible for quota refresh; the enabled flag is a routing
/// decision and must not hide a valid upstream snapshot from the dashboard.
pub async fn account_for_usage(
    state: &crate::state::AppState,
    credential_id: &str,
) -> Result<CodexAccount, String> {
    load_account_for_use(state, credential_id, false, true).await
}

async fn force_refresh_account_for_usage(
    state: &crate::state::AppState,
    credential_id: &str,
) -> Result<CodexAccount, String> {
    load_account_for_use(state, credential_id, true, true).await
}

enum CodexUsageError {
    Unauthorized,
    Message(String),
}

impl CodexUsageError {
    fn into_message(self) -> String {
        match self {
            Self::Unauthorized => {
                "OpenAI Codex usage access was denied; reconnect the account".to_owned()
            }
            Self::Message(message) => message,
        }
    }
}

async fn fetch_usage_for_account(
    state: &crate::state::AppState,
    account: &CodexAccount,
) -> Result<crate::provider_adapters::ProviderUsageSnapshot, CodexUsageError> {
    let operational = state.operational_settings().settings;
    let (url, client) = egress::provider_client(
        USAGE_URL,
        false,
        operational.connect_timeout,
        operational.request_timeout,
        CODEX_USER_AGENT,
        operational.upstream,
    )
    .await
    .map_err(|_| {
        CodexUsageError::Message("Could not prepare the OpenAI Codex usage request".to_owned())
    })?;

    let mut request = client
        .get(url)
        .bearer_auth(&account.access_token)
        .header("Accept", "application/json")
        .header("originator", "codex_cli_rs")
        .header("User-Agent", CODEX_USER_AGENT)
        .header("Version", CODEX_CLI_VERSION);
    if let Some(account_id) = account.account_id.as_deref() {
        let value = HeaderValue::from_str(account_id).map_err(|_| {
            CodexUsageError::Message("OpenAI Codex account ID is invalid".to_owned())
        })?;
        request = request.header("ChatGPT-Account-ID", value);
    }

    let response = request.send().await.map_err(|_| {
        CodexUsageError::Message("Could not reach the OpenAI Codex usage service".to_owned())
    })?;
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err(CodexUsageError::Unauthorized);
    }
    if !response.status().is_success() {
        return Err(CodexUsageError::Message(format!(
            "OpenAI Codex usage API returned HTTP {}",
            response.status().as_u16()
        )));
    }

    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        CodexUsageError::Message("OpenAI Codex usage response could not be read".to_owned())
    })? {
        if body.len().saturating_add(chunk.len()) > operational.upstream.usage_response_max_bytes {
            return Err(CodexUsageError::Message(
                "OpenAI Codex usage response exceeded its size limit".to_owned(),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    let data: Value = serde_json::from_slice(&body).map_err(|_| {
        CodexUsageError::Message("OpenAI Codex usage response was invalid".to_owned())
    })?;
    Ok(parse_usage_snapshot(&data))
}

async fn oauth_client(
    connect_timeout: std::time::Duration,
    request_timeout: std::time::Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<(reqwest::Url, reqwest::Client), String> {
    let (url, client) = egress::provider_client(
        TOKEN_URL,
        false,
        connect_timeout,
        request_timeout,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        upstream,
    )
    .await?;
    Ok((url, client))
}

async fn read_token_response(response: reqwest::Response) -> Result<CodexAccount, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("OpenAI OAuth returned HTTP {}", status.as_u16()));
    }
    let mut body = Vec::new();
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "OpenAI OAuth response could not be read".to_owned())?
    {
        if body.len().saturating_add(chunk.len()) > 128 * 1024 {
            return Err("OpenAI OAuth response exceeded its size limit".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    let tokens: TokenResponse = serde_json::from_slice(&body)
        .map_err(|_| "OpenAI OAuth response did not contain valid token data".to_owned())?;
    CodexAccount::from_tokens(tokens)
}

fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty() || token.len() > MAX_TOKEN_BYTES || token.contains('\0') {
        return Err("OpenAI OAuth returned an invalid token".to_owned());
    }
    Ok(())
}

fn decode_jwt_claims(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    #[test]
    fn authorization_url_contains_pkce_and_expected_callback() {
        let url = reqwest::Url::parse(
            &authorization_url("random-state", &pkce_challenge("verifier")).unwrap(),
        )
        .unwrap();
        let query = url
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(query["state"], "random-state");
        assert_eq!(query["redirect_uri"], CALLBACK_URL);
        assert_eq!(query["code_challenge_method"], "S256");
        assert_eq!(
            query["code_challenge"],
            "iMnq5o6zALKXGivsnlom_0F5_WYda32GHkxlV7mq7hQ"
        );
    }

    #[test]
    fn account_metadata_is_derived_without_trusting_decoded_claims_for_auth() {
        let payload = URL_SAFE_NO_PAD.encode(
            br#"{"email":"user@example.com","https://api.openai.com/auth":{"chatgpt_account_id":"acct-123","chatgpt_plan_type":"plus"}}"#,
        );
        let token = format!("header.{payload}.signature");
        let mut account = CodexAccount {
            access_token: token,
            refresh_token: Some("refresh".to_owned()),
            id_token: None,
            expires_at: 123,
            account_id: None,
            chatgpt_user_id: None,
            email: None,
            plan: None,
        };
        account.refresh_metadata();
        assert_eq!(account.email.as_deref(), Some("user@example.com"));
        assert_eq!(account.account_id.as_deref(), Some("acct-123"));
        assert_eq!(account.plan.as_deref(), Some("plus"));
    }

    #[test]
    fn account_metadata_reads_per_user_identity_from_auth_claims_and_subject() {
        let access_payload = URL_SAFE_NO_PAD
            .encode(br#"{"https://api.openai.com/auth":{"user_id":"user-from-auth"}}"#);
        let access_token = format!("header.{access_payload}.signature");
        let id_payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"user-from-sub"}"#);
        let id_token = format!("header.{id_payload}.signature");
        let mut account = CodexAccount {
            access_token,
            refresh_token: None,
            id_token: Some(id_token),
            expires_at: 123,
            account_id: None,
            chatgpt_user_id: None,
            email: None,
            plan: None,
        };

        account.refresh_metadata();

        assert_eq!(account.chatgpt_user_id.as_deref(), Some("user-from-auth"));
    }

    #[test]
    fn account_metadata_falls_back_to_jwt_subject_for_per_user_identity() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"user-from-sub"}"#);
        let token = format!("header.{payload}.signature");
        let mut account = CodexAccount {
            access_token: token,
            refresh_token: None,
            id_token: None,
            expires_at: 123,
            account_id: None,
            chatgpt_user_id: None,
            email: None,
            plan: None,
        };

        account.refresh_metadata();

        assert_eq!(account.chatgpt_user_id.as_deref(), Some("user-from-sub"));
    }

    #[test]
    fn usage_parser_reads_plan_windows_and_reset_credits() {
        let data = json!({
            "plan_type": "plus",
            "rate_limit": {
                "limit_reached": false,
                "primary_window": {
                    "used_percent": 37.5,
                    "reset_at": 1_800_000_000_000_i64,
                    "limit_window_seconds": 10_800
                },
                "secondary_window": {
                    "used_percent": "92",
                    "reset_after_seconds": 3600,
                    "limit_window_seconds": 604_800
                }
            },
            "rate_limit_reset_credits": { "available_count": 2 }
        });

        let usage = parse_usage_snapshot(&data);
        assert_eq!(usage.plan.as_deref(), Some("plus"));
        assert!(!usage.limit_reached);
        assert_eq!(usage.reset_credits_available, Some(2));
        assert_eq!(usage.quotas.len(), 2);
        assert_eq!(usage.quotas[0].id, "session");
        assert_eq!(usage.quotas[0].used_percent, 37.5);
        assert_eq!(usage.quotas[0].remaining_percent, 62.5);
        assert_eq!(usage.quotas[0].reset_at, Some(1_800_000_000));
        assert_eq!(usage.quotas[1].id, "weekly");
        assert_eq!(usage.quotas[1].used_percent, 92.0);
        assert_eq!(usage.quotas[1].label, "Weekly");
        assert!(
            usage.quotas[1]
                .reset_at
                .is_some_and(|reset| reset > unix_now())
        );
    }

    #[test]
    fn usage_parser_includes_review_spark_and_additional_limits_once() {
        let data = json!({
            "rate_limit": {
                "primary_window": { "used_percent": 120 },
                "secondary_window": { "used_percent": -4 }
            },
            "additional_rate_limits": [
                {
                    "limit_name": "code_review",
                    "rate_limit": {
                        "primary_window": { "used_percent": 10 },
                        "secondary_window": { "used_percent": 20 }
                    }
                },
                {
                    "limit_name": "gpt-5.3-codex-spark",
                    "rate_limit": {
                        "primary_window": { "used_percent": 30 }
                    }
                },
                {
                    "limit_name": "image_generation",
                    "rate_limit": {
                        "primary_window": { "used_percent": 40 }
                    }
                }
            ],
            "rate_limits_by_limit_id": {
                "codex": { "primary_window": { "used_percent": 99 } },
                "code_review": { "primary_window": { "used_percent": 11 } }
            }
        });

        let usage = parse_usage_snapshot(&data);
        let by_id = usage
            .quotas
            .iter()
            .map(|quota| (quota.id.as_str(), quota))
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(by_id.len(), usage.quotas.len());
        assert_eq!(by_id["session"].used_percent, 100.0);
        assert_eq!(by_id["weekly"].used_percent, 0.0);
        assert_eq!(by_id["code_review_session"].used_percent, 11.0);
        assert_eq!(by_id["code_review_weekly"].label, "Code review · Weekly");
        assert_eq!(by_id["spark_session"].used_percent, 30.0);
        assert_eq!(by_id["image_generation_session"].used_percent, 40.0);
    }

    #[test]
    fn usage_parser_labels_long_window_as_monthly() {
        let data = json!({
            "rate_limit": {
                "secondary_window": {
                    "used_percent": 12,
                    "limit_window_seconds": 2_592_000
                }
            }
        });

        let usage = parse_usage_snapshot(&data);
        assert_eq!(usage.quotas[0].id, "monthly");
        assert_eq!(usage.quotas[0].label, "Monthly");
    }

    #[test]
    fn usage_parser_bounds_returned_quota_count() {
        let additional_rate_limits = (0..MAX_USAGE_QUOTAS + 20)
            .map(|index| {
                json!({
                    "limit_name": format!("feature_{index}"),
                    "rate_limit": { "primary_window": { "used_percent": index } }
                })
            })
            .collect::<Vec<_>>();
        let data = json!({ "additional_rate_limits": additional_rate_limits });

        let usage = parse_usage_snapshot(&data);
        assert_eq!(usage.quotas.len(), MAX_USAGE_QUOTAS);
    }
}
