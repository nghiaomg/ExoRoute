//! Muse device authorization and inference-key minting.
//!
//! Muse signs in with the RFC 8628 device grant and then exchanges the durable
//! device token it returns for a separate subscription inference key. Two
//! different hosts are involved: the grant lives on Meta's OIDC host and the
//! mint and inference calls live on the API host. Both are fixed by Meta, so
//! unlike an adapter whose sign-in shares its API host this one does not derive
//! the authorization endpoint from the provider row's base URL.
//!
//! Every call still goes through [`egress::provider_client`], so DNS pinning,
//! private-address rejection, redirect suppression, and bounded timeouts apply
//! exactly as they do for a chat request.

use super::super::read_limited_response;
use super::*;
use crate::security::egress;
use serde_json::Value;
use std::time::Duration;

/// RFC 8628 device-authorization endpoint of Meta's OIDC provider.
pub(super) const DEVICE_CODE_URL: &str = "https://auth.meta.com/oidc/device/authorization/";
/// RFC 8628 token endpoint of Meta's OIDC provider.
pub(super) const DEVICE_TOKEN_URL: &str = "https://auth.meta.com/oidc/device/token/";
/// Exchanges a durable device token for the subscription inference key.
pub(super) const MINT_URL: &str = "https://api.meta.ai/muse-code/key";

/// Public client id of the Muse Code CLI. It identifies the client, not the
/// operator, and is not a secret: it is the same value shipped as a public
/// credential by both reference implementations, and an operator can override
/// it when Meta rotates the registration.
const DEFAULT_CLIENT_ID: &str = "1031625952748946";
const CLIENT_ID_ENV: &str = "EXOROUTE_MUSE_CLIENT_ID";

const DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// User agent of the Muse Code CLI, which is what both the device grant and the
/// mint expect to see from a first-party client.
pub(super) const MUSE_USER_AGENT: &str = "muse-code/1.0.2";
/// API-version header the Muse Code CLI sends with mint and inference calls.
pub(super) const API_VERSION_HEADER: &str = "x-api-version";
pub(super) const API_VERSION: &str = "1.0.0";

const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 5;
const DEFAULT_CODE_TTL_SECONDS: u64 = 900;
const MAX_DEVICE_CODE_INPUT_BYTES: usize = 512;
const MAX_DEVICE_BODY_BYTES: usize = 64 * 1024;
const DEVICE_CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(3);
const DEVICE_REQUEST_TIMEOUT_CAP: Duration = Duration::from_secs(15);
const MINT_CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(3);
/// A mint also runs on the request path when an upstream rejects a stored key,
/// so its ceiling has to stay small enough not to hold a failing request open.
const MINT_REQUEST_TIMEOUT_CAP: Duration = Duration::from_secs(10);

/// One device-authorization poll result.
pub(super) enum DevicePoll {
    Pending,
    SlowDown,
    Denied,
    Expired,
    Approved(Value),
}

/// The inference key and account details a successful mint returned.
pub(super) struct MintedKey {
    pub(super) api_key: String,
    /// The raw mint response, used for optional account metadata.
    pub(super) metadata: Value,
}

/// Requests a device-authorization grant.
pub(super) async fn start_authorization(
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<AdapterDeviceAuthorization, String> {
    let client_id = muse_client_id()?;
    let (url, client) = device_client(
        DEVICE_CODE_URL,
        allow_local,
        connect_timeout,
        request_timeout,
        upstream,
    )
    .await?;
    let response = client
        .post(url)
        .header(http::header::ACCEPT, "application/json")
        .form(&[("client_id", client_id.as_str())])
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach the Muse device authorization service: {}",
                error.without_url()
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Muse device authorization failed with HTTP {}",
            status.as_u16()
        ));
    }
    let body = read_limited_response(response, MAX_DEVICE_BODY_BYTES).await?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "Muse returned an invalid device authorization response".to_owned())?;
    parse_device_code_response(&value)
}

/// Reads the device grant a start response describes.
pub(super) fn parse_device_code_response(
    value: &Value,
) -> Result<AdapterDeviceAuthorization, String> {
    let device_code = bounded_text(value, "device_code", MAX_DEVICE_CODE_INPUT_BYTES)
        .ok_or_else(|| "Muse did not return a device code".to_owned())?;
    let user_code = bounded_text(value, "user_code", MAX_DEVICE_CODE_INPUT_BYTES)
        .ok_or_else(|| "Muse did not return a user code".to_owned())?;
    let verification_uri = verification_uri(value)?;
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_CODE_TTL_SECONDS)
        .clamp(30, 3600);
    // Meta publishes its own cadence; honouring it keeps the sign-in inside the
    // service's rate limits instead of polling on the gateway's own schedule.
    let interval = value
        .get("interval")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_POLL_INTERVAL_SECONDS)
        .clamp(1, 60);
    Ok(AdapterDeviceAuthorization {
        device_code,
        user_code,
        verification_uri,
        expires_in: Duration::from_secs(expires_in),
        interval: Duration::from_secs(interval),
    })
}

/// Polls one device grant.
pub(super) async fn poll_authorization(
    device_code: &str,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<DevicePoll, String> {
    if device_code.is_empty() || device_code.len() > MAX_DEVICE_CODE_INPUT_BYTES {
        return Err("Muse device code is invalid; start sign-in again".to_owned());
    }
    let client_id = muse_client_id()?;
    let (url, client) = device_client(
        DEVICE_TOKEN_URL,
        allow_local,
        connect_timeout,
        request_timeout,
        upstream,
    )
    .await?;
    let response = client
        .post(url)
        .header(http::header::ACCEPT, "application/json")
        .form(&[
            ("grant_type", DEVICE_GRANT_TYPE),
            ("device_code", device_code),
            ("client_id", client_id.as_str()),
        ])
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach the Muse token service: {}",
                error.without_url()
            )
        })?;
    let status = response.status();
    let body = read_limited_response(response, MAX_DEVICE_BODY_BYTES).await?;
    parse_token_response(status, &body)
}

/// Classifies one device-token response.
pub(super) fn parse_token_response(
    status: http::StatusCode,
    body: &[u8],
) -> Result<DevicePoll, String> {
    if status.is_success() {
        let value: Value = serde_json::from_slice(body)
            .map_err(|_| "Muse returned an invalid token response".to_owned())?;
        return Ok(DevicePoll::Approved(value));
    }
    // The error body is diagnostics only: an unparseable one still leaves the
    // HTTP status to classify on, so a parse failure is not reported as a
    // sign-in failure.
    let error_code = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    Ok(match error_code.as_deref() {
        Some("authorization_pending") => DevicePoll::Pending,
        Some("slow_down") => DevicePoll::SlowDown,
        Some("access_denied") => DevicePoll::Denied,
        Some("expired_token") => DevicePoll::Expired,
        _ if status == http::StatusCode::TOO_MANY_REQUESTS => DevicePoll::SlowDown,
        // An unrecognized rejection is a real failure: the operator needs to
        // see it instead of a sign-in that polls until it expires.
        _ => {
            return Err(format!(
                "Muse token request failed with HTTP {}",
                status.as_u16()
            ));
        }
    })
}

/// The body the mint call sends.
///
/// UNVERIFIED UPSTREAM CONTRACT. The two reference implementations disagree
/// about this body: one sends `{"onboard": true}` and the other sends
/// `{"dca_token": …}` while both authenticate the same way with
/// `Authorization: Bearer <device token>`. This gateway follows the `onboard`
/// form. It is a named function so the choice is pinned by a test and changing
/// it is a deliberate edit rather than a silent drift, and so this note travels
/// with the value. If minting starts failing against the real service, this is
/// the first suspect.
pub(super) fn mint_request_body() -> Value {
    serde_json::json!({ "onboard": true })
}

/// Exchanges the durable device token for the subscription inference key.
///
/// `endpoint` is a parameter rather than the [`MINT_URL`] constant so the real
/// HTTP path can be exercised against a scripted mock upstream; production
/// callers pass `MINT_URL`.
pub(super) async fn mint_api_key(
    endpoint: &str,
    dca_token: &str,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<MintedKey, String> {
    let (url, client) = egress::provider_client(
        endpoint,
        allow_local,
        connect_timeout.min(MINT_CONNECT_TIMEOUT_CAP),
        request_timeout.min(MINT_REQUEST_TIMEOUT_CAP),
        false,
        MUSE_USER_AGENT,
        upstream,
    )
    .await?;
    let response = client
        .post(url)
        .header(http::header::ACCEPT, "application/json")
        .header(API_VERSION_HEADER, API_VERSION)
        .bearer_auth(dca_token)
        // The Muse Code CLI asks the service to run its onboarding step, which
        // is what provisions the subscription key for the device token.
        .json(&mint_request_body())
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach the Muse key service: {}",
                error.without_url()
            )
        })?;
    let status = response.status();
    let body = read_limited_response(response, MAX_DEVICE_BODY_BYTES).await?;
    parse_mint_response(status, &body)
}

/// Reads one mint response into a usable inference key.
pub(super) fn parse_mint_response(
    status: http::StatusCode,
    body: &[u8],
) -> Result<MintedKey, String> {
    // A rejection body is diagnostics; the HTTP status still classifies the
    // failure, while a successful body is parsed strictly below.
    let value = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(mint_failure_message(status, &value));
    }
    if value.get("is_subs_active").and_then(Value::as_bool) == Some(false) {
        return Err(mint_failure_message(status, &value));
    }
    let api_key = value
        .get("api_key")
        .or_else(|| value.get("apiKey"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| "Muse did not return an inference key".to_owned())?;
    Ok(MintedKey {
        api_key: api_key.to_owned(),
        metadata: value,
    })
}

/// The configured Muse client id, or the pinned first-party one.
pub(super) fn muse_client_id() -> Result<String, String> {
    let Some(configured) = std::env::var(CLIENT_ID_ENV).ok() else {
        return Ok(DEFAULT_CLIENT_ID.to_owned());
    };
    let trimmed = configured.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(format!(
            "{CLIENT_ID_ENV} must be an alphanumeric client id of at most 64 characters"
        ));
    }
    if !trimmed.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(format!(
            "{CLIENT_ID_ENV} must be an alphanumeric client id of at most 64 characters"
        ));
    }
    Ok(trimmed.to_owned())
}

fn mint_failure_message(status: http::StatusCode, value: &Value) -> String {
    if value.get("is_subs_active").and_then(Value::as_bool) == Some(false) {
        return "Muse Code subscription is inactive; activate it on muse.ai first".to_owned();
    }
    match status {
        http::StatusCode::UNAUTHORIZED | http::StatusCode::FORBIDDEN => {
            "Muse rejected the stored device token; reconnect the Muse account".to_owned()
        }
        // Any other status is the shape a wrong request body shows up as, and the
        // mint contract is inferred rather than documented, so the operator is
        // pointed at the one thing that cannot be settled without a real account.
        _ => format!(
            "Muse key service failed with HTTP {}; the mint request shape is inferred, so verify it against a working Muse client if this persists",
            status.as_u16()
        ),
    }
}

/// Reads a non-empty, bounded string field.
fn bounded_text(value: &Value, key: &str, max_bytes: usize) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    if text.is_empty() || text.len() > max_bytes {
        return None;
    }
    Some(text.to_owned())
}

/// The URL the operator opens to approve the code. Only an absolute HTTPS URL
/// without credentials or a fragment is accepted, so a hostile or broken
/// response cannot turn the dashboard's approval link into something else.
fn verification_uri(value: &Value) -> Result<String, String> {
    let raw = [
        "verification_uri",
        "verification_uri_complete",
        "verification_url",
    ]
    .into_iter()
    .find_map(|key| bounded_text(value, key, 2048))
    .ok_or_else(|| "Muse did not return a verification URL".to_owned())?;
    let parsed = reqwest::Url::parse(&raw)
        .map_err(|_| "Muse returned an invalid verification URL".to_owned())?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none_or(str::is_empty)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Muse returned an invalid verification URL".to_owned());
    }
    Ok(parsed.to_string())
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
        MUSE_USER_AGENT,
        upstream,
    )
    .await
}
