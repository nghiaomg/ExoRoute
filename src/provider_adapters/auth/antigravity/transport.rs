use crate::security::egress;
use http::StatusCode;
use serde_json::Value;
use std::time::Duration;

use super::{RUNTIME_BASE_URL, RUNTIME_FALLBACK_BASE_URL};
#[derive(Debug)]
pub(crate) struct UpstreamJsonError {
    pub(crate) status: Option<StatusCode>,
    pub(crate) message: String,
}

pub(crate) async fn post_json(
    base_url: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &Value,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<Value, UpstreamJsonError> {
    let mut url = reqwest::Url::parse(base_url).map_err(|_| UpstreamJsonError {
        status: None,
        message: "Antigravity endpoint is invalid".to_owned(),
    })?;
    url.set_path(path);
    let (url, client) = egress::provider_client(
        url.as_str(),
        false,
        connect_timeout,
        request_timeout,
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        upstream,
    )
    .await
    .map_err(|error| UpstreamJsonError {
        status: None,
        message: error,
    })?;
    let mut request = client.post(url).json(body);
    for (name, value) in headers {
        request = request.header(*name, value);
    }
    let response = request.send().await.map_err(|error| UpstreamJsonError {
        status: None,
        message: format!("Antigravity request failed: {}", error.without_url()),
    })?;
    read_json(
        response,
        "Antigravity response",
        upstream.usage_response_max_bytes,
    )
    .await
    .map_err(|error| UpstreamJsonError {
        status: error.status,
        message: error.message,
    })
}

pub(crate) async fn post_runtime_json(
    path: &str,
    headers: &[(&str, String)],
    body: &Value,
    connect_timeout: Duration,
    request_timeout: Duration,
    upstream: crate::config::UpstreamSettings,
) -> Result<Value, UpstreamJsonError> {
    let mut last_error = None;
    for base_url in [RUNTIME_BASE_URL, RUNTIME_FALLBACK_BASE_URL] {
        match post_json(
            base_url,
            path,
            headers,
            body,
            connect_timeout,
            request_timeout,
            upstream,
        )
        .await
        {
            Ok(value) => return Ok(value),
            Err(error)
                if matches!(
                    error.status,
                    Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
                ) =>
            {
                return Err(error);
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| UpstreamJsonError {
        status: None,
        message: "Antigravity runtime service is unavailable".to_owned(),
    }))
}

pub(crate) async fn oauth_client(
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

pub(crate) async fn read_json(
    response: reqwest::Response,
    operation: &str,
    max_bytes: usize,
) -> Result<Value, UpstreamJsonError> {
    let status = response.status();
    let body = read_limited_body(response, max_bytes).await?;
    if !status.is_success() {
        return Err(UpstreamJsonError {
            status: Some(status),
            message: format!("{operation} returned HTTP {}", status.as_u16()),
        });
    }
    serde_json::from_slice(&body).map_err(|_| UpstreamJsonError {
        status: Some(status),
        message: format!("{operation} returned invalid JSON"),
    })
}

pub(crate) async fn read_limited_body(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, UpstreamJsonError> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(UpstreamJsonError {
            status: Some(response.status()),
            message: "Antigravity response exceeded its size limit".to_owned(),
        });
    }
    let status = response.status();
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|_| UpstreamJsonError {
            status: Some(status),
            message: "Antigravity response could not be read".to_owned(),
        })?;
        if bytes.len().saturating_add(chunk.len()) > max_bytes {
            return Err(UpstreamJsonError {
                status: Some(status),
                message: "Antigravity response exceeded its size limit".to_owned(),
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
