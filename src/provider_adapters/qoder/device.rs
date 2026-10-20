//! Qoder device authorization.
//!
//! Qoder implements a PKCE + device-token flow:
//! 1. ExoRoute generates a PKCE pair, nonce (UUID), and machine_id (UUID) locally.
//! 2. The user visits https://qoder.com/device/selectAccounts (or .cn) in their browser.
//! 3. ExoRoute polls openapi.qoder.sh/api/v1/deviceToken/poll (or .cn) until authorized.
//! 4. Upstream issues a `dt-...` device token along with user details.

use super::auth::qoder::QoderAccount;
use crate::{
    provider_adapters::{AdapterDeviceAuthorization, AdapterDevicePoll, AdapterOAuthAccount},
    security::egress,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub(super) const QODER_INTL_LOGIN_URL: &str = "https://qoder.com/device/selectAccounts";
pub(super) const QODER_INTL_DEVICE_TOKEN_URL: &str =
    "https://openapi.qoder.sh/api/v1/deviceToken/poll";
pub(super) const QODER_INTL_USERINFO_URL: &str = "https://openapi.qoder.sh/api/v1/userinfo";

pub(super) const QODER_CN_LOGIN_URL: &str = "https://qoder.com.cn/device/selectAccounts";
pub(super) const QODER_CN_DEVICE_TOKEN_URL: &str =
    "https://openapi.qoder.com.cn/api/v1/deviceToken/poll";
pub(super) const QODER_CN_USERINFO_URL: &str = "https://openapi.qoder.com.cn/api/v1/userinfo";

const DEFAULT_CODE_TTL_SECONDS: u64 = 300;
const POLL_INTERVAL_SECONDS: u64 = 2;
const MAX_DEVICE_BODY_BYTES: usize = 64 * 1024;
const MAX_DEVICE_CODE_INPUT_BYTES: usize = 512;
const DEVICE_CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(3);
const DEVICE_REQUEST_TIMEOUT_CAP: Duration = Duration::from_secs(15);
const USER_AGENT: &str = "Go-http-client/2.0";

#[derive(Serialize, Deserialize)]
pub(crate) struct QoderDeviceContext {
    pub(crate) nonce: String,
    pub(crate) verifier: String,
    pub(crate) machine_id: String,
    pub(crate) region: String,
}

pub(crate) fn encode_device_code(ctx: &QoderDeviceContext) -> Result<String, String> {
    let json = serde_json::to_string(ctx).map_err(|e| e.to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(json.as_bytes()))
}

pub(crate) fn decode_device_code(device_code: &str) -> Result<QoderDeviceContext, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(device_code)
        .map_err(|_| "Qoder device code encoding is invalid".to_owned())?;
    serde_json::from_slice(&bytes).map_err(|_| "Qoder device code payload is invalid".to_owned())
}

pub(super) fn login_url(region: &str) -> &'static str {
    if region == "cn" {
        QODER_CN_LOGIN_URL
    } else {
        QODER_INTL_LOGIN_URL
    }
}

pub(super) fn device_token_url(region: &str) -> &'static str {
    if region == "cn" {
        QODER_CN_DEVICE_TOKEN_URL
    } else {
        QODER_INTL_DEVICE_TOKEN_URL
    }
}

pub(super) fn userinfo_url(region: &str) -> &'static str {
    if region == "cn" {
        QODER_CN_USERINFO_URL
    } else {
        QODER_INTL_USERINFO_URL
    }
}

/// Generates a new device authorization session locally.
pub(super) fn start_authorization(region: &str) -> Result<AdapterDeviceAuthorization, String> {
    let mut verifier_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut verifier_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);

    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let nonce = uuid::Uuid::new_v4().to_string();
    let machine_id = uuid::Uuid::new_v4().to_string();

    let user_code = nonce.chars().take(8).collect::<String>().to_uppercase();

    let base_login = login_url(region);
    let verification_uri = format!(
        "{base_login}?challenge={challenge}&challenge_method=S256&machine_id={machine_id}&nonce={nonce}"
    );

    let ctx = QoderDeviceContext {
        nonce,
        verifier,
        machine_id,
        region: region.to_owned(),
    };
    let device_code = encode_device_code(&ctx)?;

    Ok(AdapterDeviceAuthorization {
        device_code,
        user_code,
        verification_uri,
        expires_in: Duration::from_secs(DEFAULT_CODE_TTL_SECONDS),
        interval: Duration::from_secs(POLL_INTERVAL_SECONDS),
    })
}

/// Polls the Qoder device token endpoint.
pub(super) async fn poll_authorization(
    device_code: &str,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<AdapterDevicePoll, String> {
    if device_code.is_empty() || device_code.len() > MAX_DEVICE_CODE_INPUT_BYTES {
        return Err("Qoder device code is invalid".to_owned());
    }

    let ctx = decode_device_code(device_code)?;
    let endpoint = format!(
        "{}?nonce={}&verifier={}&challenge_method=S256",
        device_token_url(&ctx.region),
        ctx.nonce,
        ctx.verifier
    );

    let (url, client) = device_client(
        &endpoint,
        allow_local,
        connect_timeout,
        request_timeout,
        upstream.clone(),
    )
    .await?;

    let response = client
        .get(url)
        .header(http::header::ACCEPT, "application/json")
        .header(http::header::USER_AGENT, USER_AGENT)
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach the Qoder device authorization service: {}",
                error.without_url()
            )
        })?;

    let status = response.status();
    match status.as_u16() {
        202 | 404 => return Ok(AdapterDevicePoll::Pending),
        429 => return Ok(AdapterDevicePoll::SlowDown),
        403 => return Ok(AdapterDevicePoll::Denied),
        410 => return Ok(AdapterDevicePoll::Expired),
        _ => {}
    }

    if !status.is_success() {
        return Err(format!(
            "Qoder device token poll failed with HTTP {}",
            status.as_u16()
        ));
    }

    let body = super::super::read_limited_response(response, MAX_DEVICE_BODY_BYTES).await?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "Qoder returned an invalid device token poll response".to_owned())?;

    let token = match value.get("token").and_then(Value::as_str) {
        Some(t) if !t.trim().is_empty() => t.trim().to_owned(),
        _ => return Ok(AdapterDevicePoll::Pending),
    };

    let user_info = fetch_user_info(
        &token,
        &ctx.region,
        allow_local,
        connect_timeout,
        request_timeout,
        upstream,
    )
    .await;

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let expires_at = value
        .get("expires_at")
        .and_then(Value::as_u64)
        .or_else(|| {
            value
                .get("expires_in")
                .and_then(Value::as_u64)
                .map(|s| now_ms.saturating_add(s.saturating_mul(1000)))
        })
        .unwrap_or_else(|| now_ms.saturating_add(30 * 24 * 3600 * 1000));

    let user_id = value
        .get("user_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let refresh_token = value
        .get("refresh_token")
        .and_then(Value::as_str)
        .map(str::to_owned);

    let payload = json!({
        "access_token": token,
        "refresh_token": refresh_token,
        "user_id": user_id,
        "machine_id": ctx.machine_id,
        "organization_id": user_info.organization_id,
        "email": user_info.email,
        "display_name": user_info.name,
        "region": ctx.region,
        "expires_at": expires_at,
    });

    let account = QoderAccount::from_device_approval(&payload)?;
    Ok(AdapterDevicePoll::Approved(AdapterOAuthAccount {
        payload,
        display_name: account.display_name(),
    }))
}

#[derive(Default)]
struct QoderUserInfo {
    name: Option<String>,
    email: Option<String>,
    organization_id: Option<String>,
}

async fn fetch_user_info(
    token: &str,
    region: &str,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> QoderUserInfo {
    let endpoint = userinfo_url(region);
    let (url, client) = match device_client(
        endpoint,
        allow_local,
        connect_timeout,
        request_timeout,
        upstream,
    )
    .await
    {
        Ok(c) => c,
        Err(_) => return QoderUserInfo::default(),
    };

    let response = match client
        .get(url)
        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
        .header(http::header::ACCEPT, "application/json")
        .header(http::header::USER_AGENT, USER_AGENT)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return QoderUserInfo::default(),
    };

    let body = match super::super::read_limited_response(response, MAX_DEVICE_BODY_BYTES).await {
        Ok(b) => b,
        Err(_) => return QoderUserInfo::default(),
    };

    let value: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return QoderUserInfo::default(),
    };

    let name = value
        .get("name")
        .or_else(|| value.get("username"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let email = value
        .get("email")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    let organization_id = value
        .get("organization_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    QoderUserInfo {
        name,
        email,
        organization_id,
    }
}

async fn device_client(
    endpoint: &str,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<(reqwest::Url, reqwest::Client), String> {
    egress::provider_client(
        endpoint,
        allow_local,
        connect_timeout.min(DEVICE_CONNECT_TIMEOUT_CAP),
        request_timeout.min(DEVICE_REQUEST_TIMEOUT_CAP),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        upstream,
    )
    .await
}
