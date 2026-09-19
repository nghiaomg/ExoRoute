//! Freebuff credential probe: validates an Auth Token against the
//! session endpoint without creating gateway state.
//!
//! Accepts 2xx and 409 (a live session is proof the token works); surfaces
//! 401/403 as invalid-token diagnostics.

use super::catalog::{FREEBUFF_BASE_URL, FREEBUFF_SESSION_PATH, FREEBUFF_SESSION_USER_AGENT};
use super::client::{freebuff_client, read_auxiliary_body, send_bounded};
use super::{AdapterKeyTestOutcome, endpoint_from_root, validate_base_url};
use crate::state::AppState;
use http::StatusCode;
use serde_json::json;

pub(super) async fn test_freebuff_credential(
    state: &AppState,
    base_url: &str,
    auth_type: &str,
    preferred_protocol: &str,
    custom_headers: &std::collections::BTreeMap<String, String>,
    credential: &str,
) -> AdapterKeyTestOutcome {
    test_freebuff_credential_at(
        state,
        base_url,
        auth_type,
        preferred_protocol,
        custom_headers,
        credential,
        None,
    )
    .await
}

pub(super) async fn test_freebuff_credential_at(
    state: &AppState,
    base_url: &str,
    auth_type: &str,
    preferred_protocol: &str,
    custom_headers: &std::collections::BTreeMap<String, String>,
    credential: &str,
    endpoint_root: Option<&str>,
) -> AdapterKeyTestOutcome {
    let failed = |status, message: String| AdapterKeyTestOutcome {
        test_passed: false,
        status,
        message,
    };
    if credential.trim().is_empty() {
        return failed(None, "Freebuff Auth Token required".to_owned());
    }
    if auth_type != "bearer" {
        return failed(None, "Freebuff requires Bearer authentication".to_owned());
    }
    if preferred_protocol != "chat_completions" {
        return failed(
            None,
            "Freebuff supports Chat Completions upstream only".to_owned(),
        );
    }
    let endpoint = match validate_base_url(base_url).and_then(|_| {
        endpoint_from_root(
            endpoint_root.unwrap_or(FREEBUFF_BASE_URL),
            FREEBUFF_SESSION_PATH,
        )
    }) {
        Ok(endpoint) => endpoint,
        Err(error) => return failed(None, error),
    };
    let (endpoint, client) = match freebuff_client(state, &endpoint).await {
        Ok(result) => result,
        Err(_error) => return failed(None, "Freebuff validation network error".to_owned()),
    };
    let mut request =
        match crate::provider_adapters::apply_custom_headers(client.post(endpoint), custom_headers)
        {
            Ok(request) => request,
            Err(error) => return failed(None, error),
        };
    request = request
        .bearer_auth(credential)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .header("User-Agent", FREEBUFF_SESSION_USER_AGENT)
        .header("x-freebuff-model", "deepseek/deepseek-v4-flash")
        .json(&json!({}));
    let upstream = state.operational_settings().settings.upstream;
    let response =
        match send_bounded(request, "validation", upstream.freebuff_auxiliary_timeout).await {
            Ok(response) => response,
            Err(error) => {
                let message = if error.message == "Freebuff validation timed out" {
                    "Freebuff validation timed out"
                } else {
                    "Freebuff validation network error"
                };
                return failed(None, message.to_owned());
            }
        };
    let status = response.status();
    let _ = read_auxiliary_body(
        response,
        upstream.freebuff_auxiliary_timeout,
        upstream.freebuff_auxiliary_response_max_bytes,
    )
    .await;
    if status.is_success() || status == StatusCode::CONFLICT {
        return AdapterKeyTestOutcome {
            test_passed: true,
            status: Some(status.as_u16()),
            message: "Freebuff Auth Token was accepted by the session endpoint".to_owned(),
        };
    }
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return failed(
            Some(status.as_u16()),
            "Invalid or expired Freebuff Auth Token".to_owned(),
        );
    }
    failed(
        Some(status.as_u16()),
        format!("Freebuff validation returned HTTP {}", status.as_u16()),
    )
}
