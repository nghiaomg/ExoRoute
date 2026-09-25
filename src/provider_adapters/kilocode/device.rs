//! Kilo Code device authorization.
//!
//! Kilo Code has no redirect URI and no PKCE: the gateway issues a pending
//! code, the operator approves it in Kilo Code's own page, and the gateway
//! hands back a long-lived token that this adapter stores as an OAuth account.

use super::*;
use crate::security::egress;
use serde_json::Value;
use std::time::Duration;

const DEVICE_AUTH_URL: &str = "https://api.kilo.ai/api/device-auth/codes";
const DEFAULT_CODE_TTL_SECONDS: u64 = 300;
const POLL_INTERVAL_SECONDS: u64 = 3;
const MAX_DEVICE_BODY_BYTES: usize = 64 * 1024;
const MAX_DEVICE_CODE_INPUT_BYTES: usize = 512;
const DEVICE_CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(3);
const DEVICE_REQUEST_TIMEOUT_CAP: Duration = Duration::from_secs(15);

/// The outcome of one poll. Transient upstream failures are reported as errors
/// so the caller keeps the flow pending and retries.
pub(crate) enum DevicePoll {
    Pending,
    SlowDown,
    Denied,
    Expired,
    /// The approval body, so the caller can parse the granted account.
    Approved(Value),
}

/// Requests a new device-authorization grant.
pub(super) async fn start_authorization(
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<AdapterDeviceAuthorization, String> {
    let (url, client) =
        device_client(DEVICE_AUTH_URL, connect_timeout, request_timeout, upstream).await?;
    // The gateway expects a JSON content type and no body, so no payload is sent.
    let response = client
        .post(url)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach the Kilo Code device authorization service: {}",
                error.without_url()
            )
        })?;
    let status = response.status();
    if status == http::StatusCode::TOO_MANY_REQUESTS {
        return Err("Too many pending Kilo Code sign-in requests; try again shortly".to_owned());
    }
    if !status.is_success() {
        return Err(format!(
            "Kilo Code device authorization failed with HTTP {}",
            status.as_u16()
        ));
    }
    let body = super::super::read_limited_response(response, MAX_DEVICE_BODY_BYTES).await?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "Kilo Code returned an invalid device authorization response".to_owned())?;
    parse_device_code_response(&value)
}

/// Polls a device-authorization grant the operator may have approved already.
pub(super) async fn poll_authorization(
    device_code: &str,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<DevicePoll, String> {
    if device_code.is_empty() || device_code.len() > MAX_DEVICE_CODE_INPUT_BYTES {
        return Err("Kilo Code device code is invalid".to_owned());
    }
    let mut url = reqwest::Url::parse(DEVICE_AUTH_URL)
        .map_err(|_| "Kilo Code device authorization URL is invalid".to_owned())?;
    // Pushing a path segment percent-encodes the code, so an unexpected
    // character can never change the polled endpoint.
    url.path_segments_mut()
        .map_err(|_| "Kilo Code device authorization URL is invalid".to_owned())?
        .push(device_code);
    let (url, client) =
        device_client(url.as_str(), connect_timeout, request_timeout, upstream).await?;
    let response = client
        .get(url)
        .header(http::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach the Kilo Code device authorization service: {}",
                error.without_url()
            )
        })?;
    let status = response.status();
    match status.as_u16() {
        202 => return Ok(DevicePoll::Pending),
        403 => return Ok(DevicePoll::Denied),
        410 => return Ok(DevicePoll::Expired),
        429 => return Ok(DevicePoll::SlowDown),
        _ => {}
    }
    if !status.is_success() {
        return Err(format!(
            "Kilo Code device authorization poll failed with HTTP {}",
            status.as_u16()
        ));
    }
    let body = super::super::read_limited_response(response, MAX_DEVICE_BODY_BYTES).await?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "Kilo Code returned an invalid device authorization response".to_owned())?;
    parse_device_poll_response(&value)
}

pub(crate) fn parse_device_code_response(
    value: &Value,
) -> Result<AdapterDeviceAuthorization, String> {
    let device_code = value
        .get("code")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|code| !code.is_empty() && code.len() <= MAX_DEVICE_CODE_INPUT_BYTES)
        .ok_or_else(|| "Kilo Code did not return a device code".to_owned())?;
    let verification_uri = value
        .get("verificationUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| "Kilo Code did not return a verification URL".to_owned())?;
    let verification_uri = reqwest::Url::parse(verification_uri)
        .map_err(|_| "Kilo Code returned an invalid verification URL".to_owned())?;
    if verification_uri.scheme() != "https"
        || !verification_uri.username().is_empty()
        || verification_uri.password().is_some()
        || verification_uri.fragment().is_some()
    {
        return Err("Kilo Code returned an invalid verification URL".to_owned());
    }
    let expires_in = value
        .get("expiresIn")
        .and_then(Value::as_u64)
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_CODE_TTL_SECONDS)
        .clamp(30, 3600);
    Ok(AdapterDeviceAuthorization {
        device_code: device_code.to_owned(),
        // Kilo Code returns a single opaque code that the operator both reads
        // and approves, so the user code is the device code.
        user_code: device_code.to_owned(),
        verification_uri: verification_uri.to_string(),
        expires_in: Duration::from_secs(expires_in),
        interval: Duration::from_secs(POLL_INTERVAL_SECONDS),
    })
}

pub(crate) fn parse_device_poll_response(value: &Value) -> Result<DevicePoll, String> {
    let approved = value
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| status.eq_ignore_ascii_case("approved"));
    let has_token = value
        .get("token")
        .and_then(Value::as_str)
        .is_some_and(|token| !token.is_empty());
    // An approval without a token is not usable yet, so it stays pending
    // instead of failing a sign-in the operator already completed.
    Ok(if approved && has_token {
        DevicePoll::Approved(value.clone())
    } else {
        DevicePoll::Pending
    })
}

async fn device_client(
    endpoint: &str,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<(reqwest::Url, reqwest::Client), String> {
    egress::provider_client(
        endpoint,
        false,
        connect_timeout.min(DEVICE_CONNECT_TIMEOUT_CAP),
        request_timeout.min(DEVICE_REQUEST_TIMEOUT_CAP),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        upstream,
    )
    .await
}
