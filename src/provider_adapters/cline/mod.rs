use super::auth::cline::{self as cline_oauth};
use super::*;
use crate::protocol::Protocol;
use serde_json::Value;
use std::time::Duration;

mod account;
mod catalog;
mod probe;
#[cfg(test)]
mod tests;
use account::save_cline_account;
pub(super) use catalog::discover_models;
#[cfg(test)]
pub(super) use catalog::{
    parse_cline_models, parse_cline_recommended_models, parse_clinepass_models,
};
use probe::test_cline_oauth_model;

pub(super) const COMPLETIONS_URL: &str = "https://api.cline.bot/api/v1/chat/completions";
const MODELS_URL: &str = "https://api.cline.bot/api/v1/ai/cline/models";
const RECOMMENDED_MODELS_URL: &str = "https://api.cline.bot/api/v1/ai/cline/recommended-models";
const MAX_CLINE_CATALOG_BYTES: usize = 2 * 1024 * 1024;
const MAX_CLINE_MODELS: usize = 10_000;
const CLINE_ACCOUNT_MATCH_BATCH_SIZE: usize = 128;

const CLINE_FALLBACK_MODELS: &[&str] = &[
    "z-ai/glm-5.2",
    "x-ai/grok-4.5",
    "openai/gpt-5.6-sol",
    "moonshotai/kimi-k3",
    "anthropic/claude-opus-4.8",
];

const CLINEPASS_FALLBACK_MODELS: &[&str] = &[
    "cline-pass/glm-5.2",
    "cline-pass/minimax-m3",
    "cline-pass/deepseek-v4-pro",
    "cline-pass/deepseek-v4-flash",
    "cline-pass/kimi-k3",
];

impl ProviderAdapter for ClineAdapter {
    fn adapter_id(&self) -> &'static str {
        self.adapter_id
    }

    fn endpoint(
        &self,
        _base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Cline requires the Chat Completions upstream protocol".to_owned());
        }
        reqwest::Url::parse(COMPLETIONS_URL).map_err(|_| "Cline API endpoint is invalid".to_owned())
    }

    fn upstream_endpoint(
        &self,
        base_url: &str,
        _protocol: UpstreamProtocol,
        model: &str,
        _stream: bool,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let is_pass_model = model.starts_with("cline-pass/");
        if (self.adapter_id == CLINEPASS_ADAPTER_ID) != is_pass_model {
            return Err(if self.adapter_id == CLINEPASS_ADAPTER_ID {
                "ClinePass models must use the cline-pass/ namespace".to_owned()
            } else {
                "Cline models cannot use the cline-pass/ namespace".to_owned()
            });
        }
        self.endpoint(base_url, Protocol::ChatCompletions, None)
    }

    fn model_list_endpoint(&self, _base_url: &str) -> Result<reqwest::Url, String> {
        reqwest::Url::parse(if self.adapter_id == CLINEPASS_ADAPTER_ID {
            RECOMMENDED_MODELS_URL
        } else {
            MODELS_URL
        })
        .map_err(|_| "Cline model catalog endpoint is invalid".to_owned())
    }

    fn has_public_model_catalog(&self) -> bool {
        true
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn apply_client_headers(
        &self,
        mut request: reqwest::RequestBuilder,
        client_headers: &http::HeaderMap,
    ) -> reqwest::RequestBuilder {
        // Cline's gateway uses these headers for client identification and
        // billing attribution. Keep task identity request-scoped: only copy a
        // validated value supplied by the caller and never invent one here.
        request = request
            .header("HTTP-Referer", "https://cline.bot")
            .header("X-Title", "Cline")
            .header("User-Agent", concat!("Cline/", env!("CARGO_PKG_VERSION")))
            .header("X-IS-MULTIROOT", "false")
            .header("X-CLIENT-TYPE", cline_client_type(client_headers))
            .header("X-CLIENT-VERSION", env!("CARGO_PKG_VERSION"))
            .header("X-PLATFORM", cline_platform())
            .header("X-PLATFORM-VERSION", cline_platform_version())
            .header("X-CORE-VERSION", env!("CARGO_PKG_VERSION"));
        if let Some(task_id) = cline_task_id(client_headers) {
            request = request.header("X-Task-ID", task_id);
        }
        request
    }

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        _api_keys_present: bool,
    ) -> Result<(), &'static str> {
        let Some(expected_base_url) = preset.default_base_url else {
            return Err("Cline preset is missing its fixed endpoint");
        };
        if auth_type != "bearer"
            || base_url.trim_end_matches('/') != expected_base_url
            || preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err(
                "Cline requires its fixed Chat Completions endpoint and Bearer authentication",
            );
        }
        Ok(())
    }

    fn apply_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        auth_type: &str,
        _auth_header: Option<&str>,
        secret: Option<&str>,
        oauth_auth: Option<&OAuthRequestAuth>,
        _session_id: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        if let Some(account) = oauth_auth {
            let token = if account.access_token.starts_with("workos:") {
                account.access_token.clone()
            } else {
                format!("workos:{}", account.access_token)
            };
            return Ok(request.bearer_auth(token));
        }
        if auth_type != "bearer" {
            return Err("Cline supports Bearer authentication only".to_owned());
        }
        let secret = secret.ok_or_else(|| "Cline credential is missing".to_owned())?;
        Ok(request.bearer_auth(secret))
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(async move { discover_models(self.adapter_id, request.state).await })
    }

    fn discover_oauth_models<'a>(
        &'a self,
        state: &'a AppState,
        _credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async move {
            match discover_models(self.adapter_id, state).await? {
                ModelDiscoveryResult::Available { models, .. } => Ok(models),
                ModelDiscoveryResult::Unsupported => Ok(Vec::new()),
            }
        })
    }

    fn test_oauth_model<'a>(
        &'a self,
        state: &'a AppState,
        base_url: &'a str,
        model: &'a str,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(test_cline_oauth_model(
            self,
            state,
            base_url,
            model,
            credential_id,
        ))
    }

    fn authorization_url(
        &self,
        state: &str,
        challenge: &str,
        _redirect_uri: Option<&str>,
    ) -> Result<String, String> {
        cline_oauth::authorization_url(state, challenge)
    }

    fn accepts_embedded_oauth_callback_without_state(&self, code: &str) -> bool {
        cline_oauth::is_embedded_callback_code(code)
    }

    fn pkce_challenge(&self, verifier: &str) -> Result<String, String> {
        Ok(cline_oauth::pkce_challenge(verifier))
    }

    fn exchange_oauth_code<'a>(
        &'a self,
        code: &'a str,
        verifier: &'a str,
        _redirect_uri: &'a str,
        connect_timeout: Duration,
        request_timeout: Duration,
        upstream: crate::config::UpstreamSettings,
    ) -> AdapterFuture<'a, Result<AdapterOAuthAccount, String>> {
        Box::pin(async move {
            let account = cline_oauth::exchange_code(
                code,
                verifier,
                connect_timeout,
                request_timeout,
                upstream,
            )
            .await?;
            let display_name = account.display_name();
            let payload = serde_json::to_value(account)
                .map_err(|_| "could not encode the Cline account credentials".to_owned())?;
            Ok(AdapterOAuthAccount {
                payload,
                display_name,
            })
        })
    }

    fn resolve_oauth_request_auth<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Option<OAuthRequestAuth>, String>> {
        Box::pin(async move {
            let account = cline_oauth::account_for_use(state, credential_id).await?;
            Ok(Some(OAuthRequestAuth {
                access_token: account.access_token,
                account_id: account.account_id,
                project_id: None,
            }))
        })
    }

    fn save_oauth_account<'a>(
        &'a self,
        state: &'a AppState,
        provider_id: &'a str,
        account: AdapterOAuthAccount,
    ) -> AdapterFuture<'a, Result<(), String>> {
        Box::pin(save_cline_account(
            self.adapter_id,
            state,
            provider_id,
            account,
        ))
    }

    fn normalize_response(&self, value: Value) -> Result<Value, String> {
        if self.adapter_id != CLINEPASS_ADAPTER_ID {
            return Ok(value);
        }
        let Some(object) = value.as_object() else {
            return Ok(value);
        };
        let Some(success) = object.get("success") else {
            return Ok(value);
        };
        if success.as_bool() == Some(false) {
            return Err("ClinePass returned a failed response envelope".to_owned());
        }
        if success.as_bool() == Some(true)
            && let Some(data) = object.get("data")
            && data.is_object()
        {
            return Ok(data.clone());
        }
        Err("ClinePass returned an invalid response envelope".to_owned())
    }
}

fn cline_task_id(headers: &http::HeaderMap) -> Option<&str> {
    let value = headers.get("x-task-id")?.to_str().ok()?.trim();
    (!value.is_empty()
        && value.len() <= 256
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b'\0'))
    .then_some(value)
}

fn cline_client_type(headers: &http::HeaderMap) -> &'static str {
    if headers
        .get("x-internal-test")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("combo-health-check"))
    {
        "omniroute-internal-health-check"
    } else {
        "omniroute"
    }
}

fn cline_platform() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        platform => platform,
    }
}

fn cline_platform_version() -> &'static str {
    // Cline expects a non-empty platform version. An explicit override lets a
    // packaged deployment report its embedded runtime version; the crate
    // version is a stable, truthful fallback when no runtime is available.
    option_env!("EXOROUTE_CLINE_PLATFORM_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}
