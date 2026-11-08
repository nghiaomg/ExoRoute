//! Kiro device authorization (AWS Builder ID via AWS SSO OIDC).
//!
//! The flow has three upstream calls: register a public OIDC client, request a
//! device authorization, then poll the token endpoint until the operator
//! approves the code in their browser. The registered `client_id`/`client_secret`
//! are required for later token refreshes, so they travel inside the opaque
//! `device_code` the gateway hands back to the poll step (base64url JSON,
//! the same convention the Qoder adapter uses).
use super::auth::kiro as kiro_oauth;
use crate::{
    provider_adapters::{AdapterDeviceAuthorization, AdapterDevicePoll, AdapterOAuthAccount},
    state::AppState,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

const DEFAULT_CODE_TTL_SECONDS: u64 = 600;
const DEFAULT_POLL_SECONDS: u64 = 5;
const MAX_DEVICE_CODE_INPUT_BYTES: usize = 4096;
const MAX_ENCODED_CONTEXT_BYTES: usize = 2048;

#[derive(Serialize, Deserialize)]
pub(crate) struct KiroDeviceContext {
    pub(crate) device_code: String,
    pub(crate) client_id: String,
    pub(crate) client_secret: String,
    #[serde(default = "default_region")]
    pub(crate) region: String,
}

fn default_region() -> String {
    "us-east-1".to_owned()
}

pub(crate) fn encode_device_code(ctx: &KiroDeviceContext) -> Result<String, String> {
    let json = serde_json::to_vec(ctx).map_err(|error| error.to_string())?;
    if json.len() > MAX_ENCODED_CONTEXT_BYTES {
        return Err("Kiro device context exceeded its size limit".to_owned());
    }
    Ok(URL_SAFE_NO_PAD.encode(json))
}

pub(crate) fn decode_device_code(device_code: &str) -> Result<KiroDeviceContext, String> {
    if device_code.is_empty() || device_code.len() > MAX_DEVICE_CODE_INPUT_BYTES {
        return Err("Kiro device code is invalid".to_owned());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(device_code)
        .map_err(|_| "Kiro device code encoding is invalid".to_owned())?;
    if bytes.len() > MAX_ENCODED_CONTEXT_BYTES {
        return Err("Kiro device code exceeded its size limit".to_owned());
    }
    let ctx: KiroDeviceContext = serde_json::from_slice(&bytes)
        .map_err(|_| "Kiro device code payload is invalid".to_owned())?;
    if ctx.device_code.is_empty()
        || ctx.client_id.is_empty()
        || ctx.client_secret.is_empty()
        || ctx.device_code.len() > 1024
        || ctx.client_id.len() > 512
        || ctx.client_secret.len() > 512
    {
        return Err("Kiro device code payload is invalid".to_owned());
    }
    Ok(ctx)
}

pub(crate) async fn start_authorization(
    state: &AppState,
) -> Result<AdapterDeviceAuthorization, String> {
    let (client_id, client_secret) = kiro_oauth::register_oidc_client(state).await?;
    let value = kiro_oauth::start_device_authorization(state, &client_id, &client_secret).await?;
    parse_device_code_response(&value, &client_id, &client_secret)
}

fn parse_device_code_response(
    value: &Value,
    client_id: &str,
    client_secret: &str,
) -> Result<AdapterDeviceAuthorization, String> {
    let device_code = value
        .get("deviceCode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty() && code.len() <= 1024)
        .ok_or("Kiro did not return a device code")?;
    let user_code = value
        .get("userCode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty() && code.len() <= 64)
        .ok_or("Kiro did not return a user code")?;
    let verification_uri = value
        .get("verificationUriComplete")
        .or_else(|| value.get("verificationUri"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or("Kiro did not return a verification URL")?;
    let verification_uri = reqwest::Url::parse(verification_uri)
        .map_err(|_| "Kiro returned an invalid verification URL".to_owned())?;
    if verification_uri.scheme() != "https"
        || !verification_uri.username().is_empty()
        || verification_uri.password().is_some()
        || verification_uri.fragment().is_some()
    {
        return Err("Kiro returned an invalid verification URL".to_owned());
    }
    let expires_in = value
        .get("expiresIn")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_CODE_TTL_SECONDS)
        .clamp(30, 3600);
    let interval = value
        .get("interval")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_POLL_SECONDS)
        .clamp(1, 60);
    let context = KiroDeviceContext {
        device_code: device_code.to_owned(),
        client_id: client_id.to_owned(),
        client_secret: client_secret.to_owned(),
        region: default_region(),
    };
    Ok(AdapterDeviceAuthorization {
        device_code: encode_device_code(&context)?,
        user_code: user_code.to_owned(),
        verification_uri: verification_uri.to_string(),
        expires_in: Duration::from_secs(expires_in),
        interval: Duration::from_secs(interval),
    })
}

pub(crate) async fn poll_authorization(
    state: &AppState,
    device_code: &str,
) -> Result<AdapterDevicePoll, String> {
    let context = decode_device_code(device_code)?;
    match kiro_oauth::poll_device_token(
        state,
        &context.client_id,
        &context.client_secret,
        &context.device_code,
    )
    .await?
    {
        Ok(value) => {
            let mut payload = json!({
                "accessToken": value.get("accessToken"),
                "refreshToken": value.get("refreshToken"),
                "expiresIn": value.get("expiresIn"),
                "clientId": context.client_id,
                "clientSecret": context.client_secret,
                "auth_method": "builder-id",
                "region": context.region,
            });
            if let Some(email) = value
                .get("accessToken")
                .and_then(Value::as_str)
                .and_then(kiro_oauth::email_from_jwt)
            {
                payload["email"] = json!(email);
            }
            Ok(AdapterDevicePoll::Approved(AdapterOAuthAccount {
                payload,
                display_name: "Kiro account".to_owned(),
            }))
        }
        Err("pending") => Ok(AdapterDevicePoll::Pending),
        Err("denied") => Ok(AdapterDevicePoll::Denied),
        Err("expired") => Ok(AdapterDevicePoll::Expired),
        Err(_) => Ok(AdapterDevicePoll::Pending),
    }
}
