//! Freebuff bounded HTTP client: egress-checked client construction,
//! timeout-wrapped sends, bounded body reads, and header helpers.
//!
//! All auxiliary session/agent/probe calls go through this module so
//! timeout and size budgets stay in one place.

use super::{AdapterRequestError, egress, read_limited_response};
use crate::state::AppState;

pub(super) fn valid_internal_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= super::catalog::MAX_FREEBUFF_ID_BYTES
        && !value.chars().any(char::is_control)
}

pub(super) fn header_value(
    name: &'static str,
    value: &str,
) -> Result<(http::HeaderName, http::HeaderValue), String> {
    let value = http::HeaderValue::from_str(value)
        .map_err(|_| format!("Freebuff response contained an invalid {name}"))?;
    let name = http::HeaderName::from_static(name);
    Ok((name, value))
}

pub(super) async fn freebuff_client(
    state: &AppState,
    endpoint: &reqwest::Url,
) -> Result<(reqwest::Url, reqwest::Client), AdapterRequestError> {
    egress::provider_client(
        endpoint.as_str(),
        state.config.allow_private_provider_urls,
        state
            .config
            .connect_timeout
            .min(std::time::Duration::from_secs(3)),
        state.config.request_timeout.min(
            state
                .operational_settings()
                .settings
                .upstream
                .freebuff_auxiliary_timeout,
        ),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        state.operational_settings().settings.upstream,
    )
    .await
    .map_err(|error| {
        AdapterRequestError::new(None, None, format!("Freebuff egress check failed: {error}"))
    })
}

pub(super) async fn send_bounded(
    request: reqwest::RequestBuilder,
    operation: &str,
    timeout: std::time::Duration,
) -> Result<reqwest::Response, AdapterRequestError> {
    tokio::time::timeout(timeout, request.send())
        .await
        .map_err(|_| {
            AdapterRequestError::new(None, None, format!("Freebuff {operation} timed out"))
        })?
        .map_err(|error| {
            AdapterRequestError::new(
                None,
                None,
                format!(
                    "Freebuff {operation} network request failed: {}",
                    error.without_url()
                ),
            )
        })
}

pub(super) async fn read_auxiliary_body(
    response: reqwest::Response,
    timeout: std::time::Duration,
    max_bytes: usize,
) -> Result<bytes::Bytes, AdapterRequestError> {
    tokio::time::timeout(timeout, read_limited_response(response, max_bytes))
        .await
        .map_err(|_| AdapterRequestError::new(None, None, "Freebuff response read timed out"))?
        .map_err(|error| {
            AdapterRequestError::new(
                None,
                None,
                format!("Freebuff response could not be read: {error}"),
            )
        })
}

pub(super) fn retry_after(response: &reqwest::Response) -> Option<http::HeaderValue> {
    response
        .headers()
        .get(http::header::RETRY_AFTER)
        .filter(|value| value.as_bytes().len() <= 128)
        .cloned()
}
