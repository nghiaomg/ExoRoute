//! Kiro token refresh and refresh-token import.
//!
//! Builder ID/IDC accounts refresh through the AWS SSO OIDC endpoint using the
//! `client_id`/`client_secret` captured at device authorization; social and
//! imported accounts refresh through the Kiro auth service, which only needs
//! the refresh token.
use super::account::{KiroAccount, email_from_jwt, unix_now};
use super::{MAX_ACCOUNT_BYTES, MAX_TOKEN_BYTES};
use crate::{security::egress, state::AppState};
use serde_json::{Value, json};
use std::time::Duration;

pub const OIDC_REGISTER_PATH: &str = "/client/register";
pub const OIDC_DEVICE_AUTH_PATH: &str = "/device_authorization";
pub const OIDC_TOKEN_PATH: &str = "/token";
pub const SOCIAL_BASE_URL: &str = "https://prod.us-east-1.auth.desktop.kiro.dev";
pub const SOCIAL_REFRESH_PATH: &str = "/refreshToken";
const DEFAULT_REGION: &str = "us-east-1";
const MAX_OAUTH_BODY_BYTES: usize = 64 * 1024;
const OIDC_CLIENT_NAME: &str = "kiro-oauth-client";
const OIDC_SCOPES: &[&str] = &[
    "codewhisperer:completions",
    "codewhisperer:analysis",
    "codewhisperer:conversations",
];
const OIDC_GRANT_TYPES: &[&str] = &[
    "urn:ietf:params:oauth:grant-type:device_code",
    "refresh_token",
];
const OIDC_ISSUER_URL: &str = "https://identitycenter.amazonaws.com/ssoins-722374e8c3c8e6c6";
const OIDC_START_URL: &str = "https://view.awsapps.com/start";

pub(crate) fn oidc_base_url(region: Option<&str>) -> String {
    format!(
        "https://oidc.{}.amazonaws.com",
        region.unwrap_or(DEFAULT_REGION)
    )
}

async fn oauth_client(url: &str, state: &AppState) -> Result<reqwest::Client, String> {
    let operational = state.operational_settings().settings;
    egress::provider_client(
        url,
        false,
        operational.connect_timeout.min(Duration::from_secs(10)),
        operational.request_timeout.min(Duration::from_secs(20)),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        operational.upstream,
    )
    .await
    .map(|(_, client)| client)
    .map_err(|error| format!("the Kiro auth endpoint was rejected: {error}"))
}

async fn post_json(
    state: &AppState,
    url: &str,
    body: &Value,
) -> Result<(reqwest::StatusCode, Value), String> {
    let client = oauth_client(url, state).await?;
    let response = client
        .post(url)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .json(body)
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach the Kiro auth service: {}",
                error.without_url()
            )
        })?;
    let status = response.status();
    let bytes = crate::provider_adapters::read_limited_response(response, MAX_OAUTH_BODY_BYTES)
        .await
        .map_err(|error| format!("Kiro auth service returned an unreadable response: {error}"))?;
    let value = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    Ok((status, value))
}

fn error_message(value: &Value, fallback: &str) -> String {
    value
        .get("error_description")
        .or_else(|| value.get("message"))
        .or_else(|| value.get("error"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|message| !message.is_empty() && message.len() <= 300)
        .unwrap_or(fallback)
        .to_owned()
}

/// Registers a public OIDC client for the device-code flow.
pub(crate) async fn register_oidc_client(state: &AppState) -> Result<(String, String), String> {
    let url = format!("{}{}", oidc_base_url(None), OIDC_REGISTER_PATH);
    let (status, value) = post_json(
        state,
        &url,
        &json!({
            "clientName": OIDC_CLIENT_NAME,
            "clientType": "public",
            "scopes": OIDC_SCOPES,
            "grantTypes": OIDC_GRANT_TYPES,
            "issuerUrl": OIDC_ISSUER_URL,
        }),
    )
    .await?;
    if !status.is_success() {
        return Err(format!(
            "Kiro client registration failed: {}",
            error_message(&value, &format!("HTTP {}", status.as_u16()))
        ));
    }
    let client_id = value
        .get("clientId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty() && id.len() <= 512)
        .ok_or("Kiro client registration did not return a client ID")?;
    let client_secret = value
        .get("clientSecret")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|secret| !secret.is_empty() && secret.len() <= 512)
        .ok_or("Kiro client registration did not return a client secret")?;
    Ok((client_id.to_owned(), client_secret.to_owned()))
}

/// Requests a device authorization for an already-registered OIDC client.
pub(crate) async fn start_device_authorization(
    state: &AppState,
    client_id: &str,
    client_secret: &str,
) -> Result<Value, String> {
    let url = format!("{}{}", oidc_base_url(None), OIDC_DEVICE_AUTH_PATH);
    let (status, value) = post_json(
        state,
        &url,
        &json!({
            "clientId": client_id,
            "clientSecret": client_secret,
            "startUrl": OIDC_START_URL,
        }),
    )
    .await?;
    if !status.is_success() {
        return Err(format!(
            "Kiro device authorization failed: {}",
            error_message(&value, &format!("HTTP {}", status.as_u16()))
        ));
    }
    Ok(value)
}

/// One device-authorization poll against the AWS SSO OIDC token endpoint.
/// Returns the raw token payload on approval.
pub(crate) async fn poll_device_token(
    state: &AppState,
    client_id: &str,
    client_secret: &str,
    device_code: &str,
) -> Result<Result<Value, &'static str>, String> {
    let url = format!("{}{}", oidc_base_url(None), OIDC_TOKEN_PATH);
    let (status, value) = post_json(
        state,
        &url,
        &json!({
            "clientId": client_id,
            "clientSecret": client_secret,
            "deviceCode": device_code,
            "grantType": "urn:ietf:params:oauth:grant-type:device_code",
        }),
    )
    .await?;
    if status.is_success() && value.get("accessToken").is_some() {
        return Ok(Ok(value));
    }
    let error = value
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match error {
        "authorization_pending" | "slow_down" => Ok(Err("pending")),
        "access_denied" | "authorization_denied" => Ok(Err("denied")),
        "expired_token" | "invalid_grant" => Ok(Err("expired")),
        _ => Err(format!(
            "Kiro device authorization poll failed: {}",
            error_message(&value, &format!("HTTP {}", status.as_u16()))
        )),
    }
}

/// Refreshes an account's access token. Builder ID accounts use the OIDC
/// endpoint with their registered client; everything else uses the Kiro auth
/// service refresh endpoint.
pub(crate) async fn refresh_account(
    state: &AppState,
    account: &mut KiroAccount,
) -> Result<(), String> {
    let Some(refresh_token) = account.refresh_token.clone() else {
        return Err("Kiro account has no refresh token; reconnect the account".to_owned());
    };
    if account.client_id.is_some() && account.client_secret.is_some() {
        let url = format!(
            "{}{}",
            oidc_base_url(account.region.as_deref()),
            OIDC_TOKEN_PATH
        );
        let (status, value) = post_json(
            state,
            &url,
            &json!({
                "clientId": account.client_id,
                "clientSecret": account.client_secret,
                "refreshToken": refresh_token,
                "grantType": "refresh_token",
            }),
        )
        .await?;
        if !status.is_success() {
            let error = error_message(&value, &format!("HTTP {}", status.as_u16()));
            if error.contains("invalid_grant") || status == reqwest::StatusCode::BAD_REQUEST {
                return Err("Kiro account must be reconnected".to_owned());
            }
            return Err(format!("Kiro token refresh failed: {error}"));
        }
        apply_token_payload(account, &value);
        return Ok(());
    }
    let url = format!("{}{}", SOCIAL_BASE_URL, SOCIAL_REFRESH_PATH);
    let (status, value) = post_json(state, &url, &json!({ "refreshToken": refresh_token })).await?;
    if !status.is_success() {
        return Err(format!(
            "Kiro token refresh failed: {}",
            error_message(&value, &format!("HTTP {}", status.as_u16()))
        ));
    }
    apply_token_payload(account, &value);
    Ok(())
}

fn apply_token_payload(account: &mut KiroAccount, value: &Value) {
    if let Some(access) = value
        .get("accessToken")
        .or_else(|| value.get("access_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty() && token.len() <= MAX_TOKEN_BYTES)
    {
        account.access_token = access.to_owned();
    }
    if let Some(refresh) = value
        .get("refreshToken")
        .or_else(|| value.get("refresh_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty() && token.len() <= MAX_TOKEN_BYTES)
    {
        account.refresh_token = Some(refresh.to_owned());
    }
    let expires_in = value
        .get("expiresIn")
        .or_else(|| value.get("expires_in"))
        .and_then(Value::as_i64)
        .unwrap_or(3600)
        .clamp(60, 31_536_000);
    account.expires_at = unix_now().saturating_add(expires_in);
    if let Some(arn) = value
        .get("profileArn")
        .or_else(|| value.get("profile_arn"))
        .and_then(Value::as_str)
    {
        account.profile_arn = Some(arn.chars().take(512).collect());
    }
    if account.email.is_none() {
        account.email = email_from_jwt(&account.access_token);
    }
}

/// Validates a pasted Kiro IDE refresh token by exchanging it once, then
/// returns the account payload ready for storage.
pub(crate) async fn import_refresh_token(
    state: &AppState,
    refresh_token: &str,
) -> Result<Value, String> {
    let refresh_token = refresh_token.trim();
    if refresh_token.is_empty()
        || refresh_token.len() > MAX_TOKEN_BYTES
        || refresh_token.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("Kiro refresh token is invalid".to_owned());
    }
    let mut account = KiroAccount {
        access_token: String::new(),
        refresh_token: Some(refresh_token.to_owned()),
        expires_at: 0,
        email: None,
        auth_method: "imported".to_owned(),
        client_id: None,
        client_secret: None,
        region: None,
        profile_arn: None,
    };
    // Social refresh is the only path that accepts a bare refresh token; an
    // OIDC refresh token without its client credentials cannot be validated.
    let url = format!("{}{}", SOCIAL_BASE_URL, SOCIAL_REFRESH_PATH);
    let (status, value) = post_json(state, &url, &json!({ "refreshToken": refresh_token })).await?;
    if !status.is_success() {
        return Err(format!(
            "Kiro refresh token was rejected: {}",
            error_message(&value, &format!("HTTP {}", status.as_u16()))
        ));
    }
    if value.get("accessToken").is_none() && value.get("access_token").is_none() {
        return Err("Kiro refresh token did not return an access token".to_owned());
    }
    apply_token_payload(&mut account, &value);
    if account.access_token.is_empty() {
        return Err("Kiro refresh token did not return an access token".to_owned());
    }
    if account.refresh_token.is_none() {
        account.refresh_token = Some(refresh_token.to_owned());
    }
    Ok(json!({
        "access_token": account.access_token,
        "refresh_token": account.refresh_token,
        "expires_in": (account.expires_at - unix_now()).max(60),
        "auth_method": "imported",
        "profile_arn": account.profile_arn,
        "email": account.email,
    }))
}

/// Account size guard shared with the store module.
#[allow(dead_code)]
pub(crate) fn account_size_limit() -> usize {
    MAX_ACCOUNT_BYTES
}
