use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    provider_adapters::{CLINE_ADAPTER_ID, CLINEPASS_ADAPTER_ID},
    security,
    security::egress,
    state::AppState,
};
use base64::{Engine, engine::general_purpose};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub const AUTHORIZE_URL: &str = "https://api.cline.bot/api/v1/auth/authorize";
pub const TOKEN_URL: &str = "https://api.cline.bot/api/v1/auth/token";
pub const REFRESH_URL: &str = "https://api.cline.bot/api/v1/auth/refresh";
pub const CALLBACK_URL: &str = "http://localhost:1455/auth/callback";
const MAX_TOKEN_BYTES: usize = 32 * 1024;
const MAX_ACCOUNT_BYTES: usize = MAX_TOKEN_BYTES * 2 + 4096;
const MAX_OAUTH_BODY_BYTES: usize = 64 * 1024;
const REFRESH_LEAD_SECONDS: i64 = 300;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClineAccount {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug)]
pub enum RefreshError {
    ReauthenticationRequired,
    Temporary(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenPayload {
    #[serde(alias = "access_token")]
    access_token: String,
    #[serde(default, alias = "refresh_token")]
    refresh_token: Option<String>,
    #[serde(default, alias = "expires_in")]
    expires_in: Option<i64>,
    #[serde(default, alias = "expires_at")]
    expires_at: Option<Value>,
    #[serde(default, alias = "account_id")]
    account_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

impl ClineAccount {
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        Self::parse_at(value, unix_now())
    }

    fn parse_at(value: &Value, now: i64) -> Result<Self, String> {
        let data = value
            .get("data")
            .filter(|item| item.is_object())
            .unwrap_or(value);
        let payload: TokenPayload = serde_json::from_value(data.clone())
            .map_err(|_| "Cline OAuth response did not contain valid tokens".to_owned())?;
        validate_token(&payload.access_token)?;
        if let Some(token) = payload.refresh_token.as_deref() {
            validate_token(token)?;
        }
        let expiry = payload
            .expires_at
            .as_ref()
            .and_then(parse_expiry_value)
            .or_else(|| {
                payload
                    .expires_in
                    .map(|seconds| now.saturating_add(seconds.clamp(60, 31_536_000)))
            })
            .unwrap_or_else(|| now.saturating_add(3600))
            .clamp(now.saturating_add(60), now.saturating_add(31_536_000));
        Ok(Self {
            access_token: payload.access_token,
            refresh_token: payload.refresh_token,
            expires_at: expiry,
            account_id: bounded(payload.account_id),
            email: bounded(payload.email),
            name: bounded(payload.name),
        })
    }

    pub fn display_name(&self) -> String {
        self.name
            .as_deref()
            .or(self.email.as_deref())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Cline account")
            .chars()
            .take(256)
            .collect()
    }
}

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

async fn refresh_account(
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

async fn mark_reauthentication_required(
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

async fn oauth_client(
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
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        upstream,
    )
    .await
}

async fn bounded_json(response: reqwest::Response, operation: &str) -> Result<Value, String> {
    if !response.status().is_success() {
        return Err(format!(
            "{operation} returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let body = read_limited_body(response).await?;
    serde_json::from_slice(&body).map_err(|_| format!("{operation} returned invalid JSON"))
}

async fn read_limited_body(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
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

fn decode_embedded_payload(code: &str) -> Option<Value> {
    // Cline's loopback callback contains a base64-encoded JSON token payload.
    // Some Cline versions append an opaque signature after the JSON object;
    // decode the URI component first and parse only the JSON prefix, matching
    // the behavior of the Cline OAuth client used by OmniRoute.
    let code = if code.as_bytes().contains(&b'%') {
        decode_uri_component(code).unwrap_or_else(|| code.to_owned())
    } else {
        code.to_owned()
    };
    let padding = (4 - code.len() % 4) % 4;
    let mut padded_code = code;
    if padding > 0 {
        padded_code.extend(std::iter::repeat_n('=', padding));
    }

    for decoder in [
        &general_purpose::URL_SAFE_NO_PAD,
        &general_purpose::URL_SAFE,
        &general_purpose::STANDARD_NO_PAD,
        &general_purpose::STANDARD,
    ] {
        let Ok(decoded) = decoder.decode(padded_code.as_bytes()) else {
            continue;
        };
        if decoded.len() > MAX_OAUTH_BODY_BYTES {
            return None;
        }
        let decoded = String::from_utf8_lossy(&decoded);
        let Some(last_brace) = decoded.rfind('}') else {
            continue;
        };
        let json = &decoded[..=last_brace];
        if let Ok(value) = serde_json::from_str::<Value>(json)
            && value
                .get("accessToken")
                .or_else(|| value.get("access_token"))
                .is_some()
        {
            return Some(value);
        }
    }
    None
}

fn decode_uri_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let high = bytes.get(index + 1).and_then(|byte| hex_value(*byte))?;
        let low = bytes.get(index + 2).and_then(|byte| hex_value(*byte))?;
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn is_embedded_callback_code(code: &str) -> bool {
    decode_embedded_payload(code).is_some_and(|payload| ClineAccount::parse(&payload).is_ok())
}

fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty()
        || token.len() > MAX_TOKEN_BYTES
        || token.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("Cline returned invalid token data".to_owned());
    }
    Ok(())
}

fn bounded(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.chars().take(512).collect::<String>())
        .filter(|value| !value.trim().is_empty())
}

fn parse_rfc3339_epoch(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || (bytes.get(10) != Some(&b'T') && bytes.get(10) != Some(&b't'))
    {
        return None;
    }
    let year = fixed_digits(bytes, 0, 4)? as i64;
    let month = fixed_digits(bytes, 5, 2)? as i64;
    let day = fixed_digits(bytes, 8, 2)? as i64;
    let hour = fixed_digits(bytes, 11, 2)? as i64;
    let minute = fixed_digits(bytes, 14, 2)? as i64;
    let second = fixed_digits(bytes, 17, 2)? as i64;
    if bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    }
    let offset = match bytes.get(index) {
        Some(b'Z' | b'z') if index + 1 == bytes.len() => 0,
        Some(sign @ (b'+' | b'-'))
            if index + 6 == bytes.len() && bytes.get(index + 3) == Some(&b':') =>
        {
            let hours = fixed_digits(bytes, index + 1, 2)? as i64;
            let minutes = fixed_digits(bytes, index + 4, 2)? as i64;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let offset = hours * 3600 + minutes * 60;
            if *sign == b'+' { offset } else { -offset }
        }
        _ => return None,
    };
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(
        (era * 146_097 + day_of_era - 719_468) * 86_400 + hour * 3600 + minute * 60 + second
            - offset,
    )
}

fn parse_expiry_value(value: &Value) -> Option<i64> {
    match value {
        Value::String(value) => parse_rfc3339_epoch(value)
            .or_else(|| value.trim().parse::<i64>().ok().map(normalize_epoch)),
        Value::Number(value) => value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .map(normalize_epoch),
        _ => None,
    }
}

fn normalize_epoch(value: i64) -> i64 {
    if value > 100_000_000_000 {
        value.saturating_div(1_000)
    } else {
        value
    }
}

fn fixed_digits(bytes: &[u8], start: usize, length: usize) -> Option<u32> {
    bytes
        .get(start..start.checked_add(length)?)?
        .iter()
        .try_fold(0_u32, |value, digit| {
            if !digit.is_ascii_digit() {
                return None;
            }
            value.checked_mul(10)?.checked_add(u32::from(*digit - b'0'))
        })
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authorization_url_contains_loopback_callback_state_and_pkce() {
        let url = authorization_url("state-123", "challenge-456").expect("authorization URL");
        let parsed = reqwest::Url::parse(&url).expect("parse authorization URL");
        assert_eq!(
            parsed.origin().ascii_serialization(),
            "https://api.cline.bot"
        );
        let query = parsed
            .query_pairs()
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            query.get("response_type").map(|value| value.as_ref()),
            Some("code")
        );
        assert_eq!(
            query.get("client_type").map(|value| value.as_ref()),
            Some("extension")
        );
        assert_eq!(
            query.get("redirect_uri").map(|value| value.as_ref()),
            Some(CALLBACK_URL)
        );
        assert_eq!(
            query.get("callback_url").map(|value| value.as_ref()),
            Some(CALLBACK_URL)
        );
        assert_eq!(
            query.get("code_challenge").map(|value| value.as_ref()),
            Some("challenge-456")
        );
        assert_eq!(
            query
                .get("code_challenge_method")
                .map(|value| value.as_ref()),
            Some("S256")
        );
        assert_eq!(
            query.get("state").map(|value| value.as_ref()),
            Some("state-123")
        );
    }

    #[test]
    fn pkce_challenge_matches_rfc7636_vector() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn token_payload_accepts_camel_and_snake_case_and_bounds_expiry() {
        let now = 1_700_000_000_i64;
        let account = ClineAccount::parse_at(
            &json!({
                "accessToken": "access",
                "refreshToken": "refresh",
                "expiresIn": 120,
                "accountId": "account-1",
                "email": "user@example.com"
            }),
            now,
        )
        .expect("parse Cline token payload");
        assert_eq!(account.access_token, "access");
        assert_eq!(account.refresh_token.as_deref(), Some("refresh"));
        assert_eq!(account.account_id.as_deref(), Some("account-1"));
        assert!(account.expires_at >= now + 60);
        assert!(account.expires_at <= now + 120);

        let snake = ClineAccount::parse(&json!({
            "access_token": "access-2",
            "refresh_token": "refresh-2",
            "expires_at": "2026-09-14T12:00:00Z",
            "account_id": "account-2"
        }))
        .expect("parse snake-case Cline token payload");
        assert_eq!(snake.account_id.as_deref(), Some("account-2"));
        assert_eq!(snake.refresh_token.as_deref(), Some("refresh-2"));
        assert!(snake.expires_at > 0);

        let numeric = ClineAccount::parse_at(
            &json!({
                "accessToken": "access-3",
                "expiresAt": (now + 600) * 1000
            }),
            now,
        )
        .expect("parse numeric Cline expiry");
        assert!(numeric.expires_at >= now + 599);
        assert!(numeric.expires_at <= now + 601);
    }

    #[test]
    fn token_payload_rejects_missing_or_control_character_tokens() {
        assert!(ClineAccount::parse(&json!({"refreshToken": "refresh"})).is_err());
        assert!(
            ClineAccount::parse(&json!({
                "accessToken": "access\ninjected",
                "refreshToken": "refresh"
            }))
            .is_err()
        );
    }

    #[test]
    fn embedded_authorization_payload_is_bounded_and_decoded() {
        let payload_bytes = serde_json::to_vec(&json!({
            "accessToken": "access",
            "refreshToken": "refresh",
            "expiresIn": 3600
        }))
        .expect("encode payload");
        let payload = general_purpose::URL_SAFE_NO_PAD.encode(&payload_bytes);
        let decoded = decode_embedded_payload(&payload).expect("embedded OAuth payload");
        assert_eq!(decoded["accessToken"], "access");
        assert!(is_embedded_callback_code(&payload));
        assert!(!is_embedded_callback_code("authorization-code"));
        assert!(decode_embedded_payload(&"x".repeat(MAX_OAUTH_BODY_BYTES + 1)).is_none());
    }

    #[test]
    fn embedded_authorization_payload_matches_cline_suffix_and_uri_encoding() {
        let mut payload_bytes = serde_json::to_vec(&json!({
            "accessToken": "access",
            "refreshToken": "refresh",
            "expiresAt": "2030-01-01T00:00:00.000000000Z"
        }))
        .expect("encode payload");
        payload_bytes.extend_from_slice(b"opaque-signature");
        let payload = general_purpose::STANDARD.encode(payload_bytes);
        let encoded = payload.replace('=', "%3D");

        let decoded = decode_embedded_payload(&encoded).expect("embedded OAuth payload");
        assert_eq!(decoded["accessToken"], "access");
        assert_eq!(decoded["refreshToken"], "refresh");
        assert!(is_embedded_callback_code(&encoded));
    }
}
