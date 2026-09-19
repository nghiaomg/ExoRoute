use crate::{
    infra::storage::{Field, StorageError, Table},
    provider_adapters::{ProviderUsageCreditBalance, ProviderUsageSnapshot},
    security,
    state::AppState,
};
use http::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

#[cfg(test)]
use crate::provider_adapters::ProviderUsageResetPeriod;

pub const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo";
pub const CALLBACK_URL: &str = "http://localhost:1455/auth/callback";
pub const RUNTIME_BASE_URL: &str = "https://daily-cloudcode-pa.googleapis.com";
pub const RUNTIME_FALLBACK_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";
pub const BOOTSTRAP_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";
pub const CLIENT_ID_ENV: &str = "EXOROUTE_ANTIGRAVITY_OAUTH_CLIENT_ID";
pub const CLIENT_SECRET_ENV: &str = "EXOROUTE_ANTIGRAVITY_OAUTH_CLIENT_SECRET";
pub const IDE_VERSION_ENV: &str = "EXOROUTE_ANTIGRAVITY_IDE_VERSION";
pub const ANTIGRAVITY_IDE_VERSION: &str = "2.11.0";

// Operators must provide a Google OAuth client registered for their deployment.
// Public PKCE clients may omit the optional client secret.

const MAX_TOKEN_BYTES: usize = 64 * 1024;
const MAX_ACCOUNT_BYTES: usize = 256 * 1024;
const MAX_OAUTH_BODY_BYTES: usize = 256 * 1024;
const MAX_OAUTH_REDIRECT_URI_BYTES: usize = 16 * 1024;
const MAX_QUOTA_ROWS: usize = 256;
const MAX_WEEKLY_ROWS: usize = 32;
const REFRESH_LEAD_SECONDS: i64 = 5 * 60;
const ANTIGRAVITY_ONBOARD_ATTEMPTS: usize = 2;
const ANTIGRAVITY_ONBOARD_RETRY_DELAY: Duration = Duration::from_secs(12);
const IDE_TYPE: i64 = 9;
const PLUGIN_TYPE: i64 = 2;
const WINDOWS_AMD64: i64 = 5;
const PROFILE_IDE: &str = "ide";

const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];

mod account;
mod account_store;
mod catalog;
mod oauth;
mod payload;
mod project;
#[cfg(test)]
mod tests;
mod transport;
mod usage;
pub(crate) use account::{AntigravityAccount, RefreshError};
use account_store::load_account_for_use;
pub(crate) use account_store::{account_for_use, save_account};
pub(crate) use catalog::{is_stale_unsupported_model, normalize_model_id};
use catalog::{is_supported_model, log_discovery_shape, model_id, parse_model_list};
pub(crate) use oauth::{authorization_url, exchange_code, pkce_challenge};
pub(crate) use payload::{
    extract_credit_balance, extract_onboard_project_id, extract_plan, extract_project_id,
    extract_reset_at, extract_tier, onboard_user_body,
};
use project::enrich_account;
pub(crate) use transport::{oauth_client, post_json, post_runtime_json, read_json};
use usage::{append_weekly_quotas, parse_model_quotas, summarize_antigravity_quotas};

fn default_models() -> &'static [&'static str] {
    crate::provider_adapters::default_models(crate::provider_adapters::ANTIGRAVITY_ADAPTER_ID)
}

fn with_default_models(mut models: Vec<String>) -> Vec<String> {
    for default_model in default_models() {
        if !models.iter().any(|model| model == default_model) {
            models.push((*default_model).to_owned());
        }
    }
    models
}

pub async fn discover_models(state: &AppState, credential_id: &str) -> Result<Vec<String>, String> {
    let account = account_for_use(state, credential_id).await?;
    let project = account.project_id.as_deref().ok_or_else(|| {
        "Antigravity account has no Cloud Code project; reconnect the account".to_owned()
    })?;
    let headers = runtime_headers(&account.access_token);
    let body = json!({"project": project});
    let connect_timeout = state.config.connect_timeout.min(Duration::from_secs(3));
    let upstream = state.operational_settings().settings.upstream;
    let request_timeout = state
        .config
        .request_timeout
        .min(upstream.discovery_request_timeout);
    let mut last_error = None;
    for base in [
        RUNTIME_BASE_URL,
        RUNTIME_FALLBACK_BASE_URL,
        "https://daily-cloudcode-pa.sandbox.googleapis.com",
    ] {
        match post_json(
            base,
            "/v1internal:fetchAvailableModels",
            &headers,
            &body,
            connect_timeout,
            request_timeout,
            upstream,
        )
        .await
        {
            Ok(value) => {
                let models = parse_model_list(&value);
                log_discovery_shape(&value, base, "/v1internal:fetchAvailableModels");
                if !models.is_empty() {
                    return Ok(with_default_models(models));
                }
            }
            Err(error) => last_error = Some(error.message),
        }
    }
    // The legacy `/v1internal:models` path exposes raw catalog slots (including
    // `MODEL_PLACEHOLDER_*` entries) that `fetchAvailableModels` already
    // filters. Keep it only as a best-effort fallback and parse it with the
    // same hardened filter so catalogue pollution cannot return through it.
    for base in [
        RUNTIME_BASE_URL,
        RUNTIME_FALLBACK_BASE_URL,
        "https://daily-cloudcode-pa.sandbox.googleapis.com",
    ] {
        match post_json(
            base,
            "/v1internal:models",
            &headers,
            &body,
            connect_timeout,
            request_timeout,
            upstream,
        )
        .await
        {
            Ok(value) => {
                let models = parse_model_list(&value);
                log_discovery_shape(&value, base, "/v1internal:models");
                if !models.is_empty() {
                    return Ok(with_default_models(models));
                }
            }
            Err(error) => last_error = Some(error.message),
        }
    }
    let fallback: Vec<String> = default_models()
        .iter()
        .map(|model| (*model).to_owned())
        .collect();
    if fallback.is_empty() {
        Err(last_error.unwrap_or_else(|| "Antigravity returned no supported models".to_owned()))
    } else {
        Ok(fallback)
    }
}

pub async fn fetch_usage(
    state: &AppState,
    credential_id: &str,
) -> Result<ProviderUsageSnapshot, String> {
    let account = account_for_usage(state, credential_id).await?;
    match fetch_usage_for_account(state, &account).await {
        Err(error)
            if error == "Antigravity account was rejected by Google; reconnect the account" =>
        {
            let refreshed = force_refresh_account_for_usage(state, credential_id).await?;
            fetch_usage_for_account(state, &refreshed).await
        }
        result => result,
    }
}

/// Loads an account for the read-only usage/quota path. A disabled account is
/// excluded from inference routing, but its OAuth credential may still be
/// used to refresh the quota shown on the admin dashboard.
pub async fn account_for_usage(
    state: &AppState,
    credential_id: &str,
) -> Result<AntigravityAccount, String> {
    load_account_for_use(state, credential_id, false, true).await
}

async fn force_refresh_account_for_usage(
    state: &AppState,
    credential_id: &str,
) -> Result<AntigravityAccount, String> {
    load_account_for_use(state, credential_id, true, true).await
}

async fn fetch_usage_for_account(
    state: &AppState,
    account: &AntigravityAccount,
) -> Result<ProviderUsageSnapshot, String> {
    let bootstrap = bootstrap_headers(&account.access_token);
    let upstream = state.operational_settings().settings.upstream;
    let load_result = post_json(
        BOOTSTRAP_BASE_URL,
        "/v1internal:loadCodeAssist",
        &bootstrap,
        &json!({"metadata": antigravity_metadata()}),
        state.config.connect_timeout.min(Duration::from_secs(3)),
        state.config.request_timeout.min(Duration::from_secs(10)),
        upstream,
    )
    .await;
    if load_result.as_ref().err().is_some_and(|error| {
        matches!(
            error.status,
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        )
    }) {
        return Err("Antigravity account was rejected by Google; reconnect the account".to_owned());
    }
    let loaded_project = load_result.as_ref().ok().and_then(extract_project_id);
    let project = loaded_project
        .as_deref()
        .or(account.project_id.as_deref())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            "Antigravity account has no Cloud Code project; reconnect the account".to_owned()
        })?;
    let headers = runtime_headers(&account.access_token);
    let timeout_connect = state.config.connect_timeout.min(Duration::from_secs(3));
    let timeout_request = state.config.request_timeout.min(Duration::from_secs(10));
    let project_body = json!({"project": project});
    let available_future = post_runtime_json(
        "/v1internal:fetchAvailableModels",
        &headers,
        &project_body,
        timeout_connect,
        timeout_request,
        upstream,
    );
    let quota_future = post_runtime_json(
        "/v1internal:retrieveUserQuota",
        &headers,
        &project_body,
        timeout_connect,
        timeout_request,
        upstream,
    );
    let summary_future = post_runtime_json(
        "/v1internal:retrieveUserQuotaSummary",
        &headers,
        &project_body,
        timeout_connect,
        timeout_request,
        upstream,
    );
    let (available, quota, summary) = tokio::join!(available_future, quota_future, summary_future);
    let responses = [&available, &quota, &summary];
    let all_rejected = responses.iter().all(|result| {
        result.as_ref().err().is_some_and(|error| {
            matches!(
                error.status,
                Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
            )
        })
    });
    if all_rejected {
        return Err("Antigravity account was rejected by Google; reconnect the account".to_owned());
    }
    if available.is_err() && quota.is_err() && summary.is_err() {
        return Err("Antigravity quota service is temporarily unavailable".to_owned());
    }

    let load_value = load_result.ok();
    let available_value = available.ok();
    let quota_value = quota.ok();
    let summary_value = summary.ok();
    let model_quotas = parse_model_quotas(available_value.as_ref(), quota_value.as_ref());
    let mut weekly_quotas = Vec::new();
    append_weekly_quotas(&mut weekly_quotas, summary_value.as_ref());
    let tier = load_value
        .as_ref()
        .and_then(extract_tier)
        .or_else(|| account.tier.clone());
    let quotas = summarize_antigravity_quotas(&model_quotas, &weekly_quotas, tier.as_deref());
    let plan = load_value
        .as_ref()
        .and_then(extract_plan)
        .or_else(|| available_value.as_ref().and_then(extract_plan))
        .or_else(|| quota_value.as_ref().and_then(extract_plan));
    let limit_reached = available_value
        .as_ref()
        .and_then(|value| {
            value
                .get("limitReached")
                .or_else(|| value.get("limit_reached"))
        })
        .and_then(as_bool)
        .unwrap_or_else(|| {
            !quotas.is_empty() && quotas.iter().all(|quota| quota.remaining_percent <= 0.0)
        });
    let credit_balance = extract_credit_balance(
        load_value.as_ref().or(available_value.as_ref()),
        quota_value.as_ref(),
        summary_value.as_ref(),
    );
    Ok(ProviderUsageSnapshot {
        plan,
        limit_reached,
        reset_credits_available: None,
        quotas,
        credit_balance,
    })
}

fn antigravity_metadata() -> Value {
    json!({"ideType": IDE_TYPE, "platform": WINDOWS_AMD64, "pluginType": PLUGIN_TYPE})
}

fn antigravity_ide_node_user_agent() -> String {
    format!(
        "antigravity/{} darwin/arm64 google-api-nodejs-client/10.3.0",
        antigravity_ide_version()
    )
}

pub fn antigravity_user_agent() -> String {
    format!("antigravity/ide/{} darwin/arm64", antigravity_ide_version())
}

/// IDE client version sent as `X-Client-Version` and inside `User-Agent`.
/// Defaults to the pinned native-client fingerprint and accepts an operator
/// override so discovery keeps working when Google rotates the client. The
/// value is validated as `major.minor.patch`; anything else falls back to
/// the pinned version instead of sending a malformed client identity.
pub fn antigravity_ide_version() -> String {
    resolve_ide_version(std::env::var(IDE_VERSION_ENV).ok())
}

fn resolve_ide_version(configured: Option<String>) -> String {
    let Some(configured) = configured else {
        return ANTIGRAVITY_IDE_VERSION.to_owned();
    };
    let trimmed = configured.trim().trim_start_matches(['v', 'V']);
    let valid = !trimmed.is_empty()
        && trimmed.len() <= 32
        && trimmed
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && trimmed.split('.').count() == 3;
    if valid {
        trimmed.to_owned()
    } else {
        ANTIGRAVITY_IDE_VERSION.to_owned()
    }
}

fn bootstrap_headers(access_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Authorization", format!("Bearer {access_token}")),
        ("Content-Type", "application/json".to_owned()),
        ("User-Agent", antigravity_user_agent()),
    ]
}

fn runtime_headers(access_token: &str) -> Vec<(&'static str, String)> {
    vec![
        ("Authorization", format!("Bearer {access_token}")),
        ("Content-Type", "application/json".to_owned()),
        ("Accept", "application/json".to_owned()),
        ("User-Agent", antigravity_user_agent()),
        ("X-Client-Name", "antigravity".to_owned()),
        ("X-Client-Version", antigravity_ide_version()),
    ]
}

fn oauth_client_id() -> Result<String, String> {
    resolve_required_oauth_client_value(CLIENT_ID_ENV, std::env::var(CLIENT_ID_ENV).ok())
}

fn oauth_client_secret() -> Result<Option<String>, String> {
    resolve_optional_oauth_client_value(CLIENT_SECRET_ENV, std::env::var(CLIENT_SECRET_ENV).ok())
}

fn resolve_required_oauth_client_value(
    env_name: &str,
    configured: Option<String>,
) -> Result<String, String> {
    let Some(configured) = configured else {
        return Err(format!("{env_name} is not configured"));
    };
    let configured = configured.trim();
    if configured.is_empty() {
        return Err(format!("{env_name} is not configured"));
    }
    if configured.len() > 512 || configured.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(format!("{env_name} is invalid"));
    }
    Ok(configured.to_owned())
}

fn resolve_optional_oauth_client_value(
    env_name: &str,
    configured: Option<String>,
) -> Result<Option<String>, String> {
    let Some(configured) = configured else {
        return Ok(None);
    };
    let configured = configured.trim();
    if configured.is_empty() {
        return Ok(None);
    }
    if configured.len() > 512 || configured.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(format!("{env_name} is invalid"));
    }
    Ok(Some(configured.to_owned()))
}

fn bounded_token(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_TOKEN_BYTES
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("Antigravity OAuth token is invalid".to_owned());
    }
    Ok(value.to_owned())
}

fn bounded_text(value: &str) -> String {
    value.chars().take(512).collect()
}

fn validate_account(account: &AntigravityAccount) -> Result<(), String> {
    bounded_token(&account.access_token)?;
    if let Some(refresh_token) = account.refresh_token.as_deref() {
        bounded_token(refresh_token)?;
    }
    if account.client_profile != PROFILE_IDE && account.client_profile != "cli" {
        return Err("stored Antigravity client profile is invalid".to_owned());
    }
    Ok(())
}

fn as_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<f64>().ok())
        })
        .filter(|number| number.is_finite())
}

fn as_bool(value: &Value) -> Option<bool> {
    value
        .as_bool()
        .or_else(|| value.as_str().and_then(|text| text.parse::<bool>().ok()))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}
