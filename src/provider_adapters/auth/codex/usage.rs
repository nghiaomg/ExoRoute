//! HTTP fetching for the OpenAI Codex usage and quota endpoint.
//!
//! The provider response parsing stays in [`usage_core`], a pure module over
//! JSON values. This module owns only the request lifecycle: egress client
//! construction, auth headers, bounded response consumption, and the
//! 401/403 refresh-and-retry.

use super::{
    CODEX_CLI_VERSION, CODEX_USER_AGENT, USAGE_URL, account::CodexAccount,
    oauth::load_account_for_use,
};
use crate::security::egress;
use http::HeaderValue;
use serde_json::Value;

#[path = "usage_core.rs"]
pub(super) mod usage_core;

pub(crate) use usage_core::parse_usage_snapshot;

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
        false,
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
