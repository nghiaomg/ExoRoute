use super::auth::codex::{self as codex_oauth};
use super::*;
use crate::state::AppState;

mod account;
mod models;
mod probe;
#[cfg(test)]
pub(crate) use account::CodexAccountMatchState;
pub(crate) use account::save_codex_account;
use models::discover_codex_models;
#[cfg(test)]
pub(crate) use models::{build_codex_model_request, parse_codex_model_list};
use probe::test_oauth_model_impl;

pub(super) struct OpenAiCodexAdapter;

pub(super) static OPENAI_CODEX_ADAPTER: OpenAiCodexAdapter = OpenAiCodexAdapter;

impl ProviderAdapter for OpenAiCodexAdapter {
    fn adapter_id(&self) -> &'static str {
        CODEX_ADAPTER_ID
    }

    fn endpoint(
        &self,
        _base_url: &str,
        _protocol: Protocol,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        reqwest::Url::parse(adapter_base_url_override.unwrap_or(codex_oauth::BASE_URL))
            .map_err(|_| "OpenAI Codex endpoint is invalid".to_owned())
            .map(|mut url| {
                url.set_path(&format!("{}/responses", url.path().trim_end_matches('/')));
                url
            })
    }

    fn prepare_body(&self, body: &mut Value, request_id: &str) {
        body["stream"] = json!(true);
        body["store"] = json!(false);
        if body
            .get("instructions")
            .and_then(Value::as_str)
            .is_none_or(|instructions| instructions.trim().is_empty())
        {
            body["instructions"] = json!("You are a helpful assistant.");
        }
        if body.get("input").is_none_or(Value::is_null)
            || body
                .get("input")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        {
            body["input"] = json!([{"type":"message","role":"user","content":[{"type":"input_text","text":"..."}]}]);
        }
        if body
            .get("prompt_cache_key")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            body["prompt_cache_key"] = json!(request_id);
        }
        if body
            .get("reasoning")
            .and_then(|value| value.get("effort"))
            .and_then(Value::as_str)
            .is_some_and(|effort| effort != "none")
        {
            body["include"] = json!(["reasoning.encrypted_content"]);
        }
        for field in [
            "temperature",
            "top_p",
            "frequency_penalty",
            "presence_penalty",
            "logprobs",
            "top_logprobs",
            "n",
            "seed",
            "max_tokens",
            "max_completion_tokens",
            "max_output_tokens",
            "user",
            "metadata",
            "stream_options",
            "safety_identifier",
            "previous_response_id",
        ] {
            if let Some(object) = body.as_object_mut() {
                object.remove(field);
            }
        }
    }

    fn allows_missing_event_stream_content_type(&self) -> bool {
        true
    }

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        api_keys_present: bool,
    ) -> Result<(), &'static str> {
        validate_codex_config(
            preset,
            auth_type,
            base_url,
            preferred_protocol,
            supported_protocols,
            api_keys_present,
        )
    }

    fn apply_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        _auth_type: &str,
        _auth_header: Option<&str>,
        _secret: Option<&str>,
        oauth_auth: Option<&OAuthRequestAuth>,
        session_id: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        apply_codex_request_auth(request, oauth_auth, session_id)
    }

    fn prepare_images<'a>(
        &'a self,
        state: &'a AppState,
        body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<(), String>> {
        Box::pin(prepare_codex_images(
            body,
            state.operational_settings().settings.upstream,
        ))
    }

    fn read_event_stream<'a>(
        &'a self,
        response: reqwest::Response,
        max_bytes: usize,
    ) -> AdapterFuture<'a, Result<Value, AdapterSseError>> {
        Box::pin(read_codex_event_stream(response, max_bytes))
    }

    fn authorization_url(
        &self,
        state: &str,
        challenge: &str,
        _redirect_uri: Option<&str>,
    ) -> Result<String, String> {
        codex_oauth::authorization_url(state, challenge)
            .map_err(|_| "could not build the OpenAI authorization URL".to_owned())
    }

    fn pkce_challenge(&self, verifier: &str) -> Result<String, String> {
        Ok(codex_oauth::pkce_challenge(verifier))
    }

    fn exchange_oauth_code<'a>(
        &'a self,
        code: &'a str,
        verifier: &'a str,
        _redirect_uri: &'a str,
        connect_timeout: std::time::Duration,
        request_timeout: std::time::Duration,
        upstream: crate::config::UpstreamSettings,
    ) -> AdapterFuture<'a, Result<AdapterOAuthAccount, String>> {
        Box::pin(async move {
            let account = codex_oauth::exchange_code(
                code,
                verifier,
                connect_timeout,
                request_timeout,
                upstream,
            )
            .await?;
            let display_name = account.display_name();
            let payload = serde_json::to_value(account)
                .map_err(|_| "could not encode the OpenAI account credentials".to_owned())?;
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
            let account = codex_oauth::account_for_use(state, credential_id).await?;
            Ok(Some(OAuthRequestAuth {
                access_token: account.access_token,
                account_id: account.account_id,
                project_id: None,
            }))
        })
    }

    fn fetch_oauth_usage<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<ProviderUsageSnapshot, String>> {
        Box::pin(codex_oauth::fetch_usage(state, credential_id))
    }

    fn discover_oauth_models<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(discover_codex_models(state, credential_id))
    }

    fn test_oauth_model<'a>(
        &'a self,
        state: &'a AppState,
        base_url: &'a str,
        model: &'a str,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(test_oauth_model_impl(
            self,
            state,
            base_url,
            model,
            credential_id,
        ))
    }

    fn save_oauth_account<'a>(
        &'a self,
        state: &'a AppState,
        provider_id: &'a str,
        account: AdapterOAuthAccount,
    ) -> AdapterFuture<'a, Result<(), String>> {
        Box::pin(save_codex_account(state, provider_id, account))
    }
}

fn validate_codex_config(
    preset: &ProviderPreset,
    auth_type: &str,
    base_url: &str,
    preferred_protocol: &str,
    supported_protocols: &[String],
    api_keys_present: bool,
) -> Result<(), &'static str> {
    if auth_type != preset.default_auth_type
        || base_url.trim_end_matches('/') != preset.default_base_url.unwrap_or_default()
        || preferred_protocol != preset.default_preferred_protocol
        || supported_protocols.len() != preset.default_supported_protocols.len()
        || !supported_protocols
            .iter()
            .zip(preset.default_supported_protocols)
            .all(|(actual, expected)| actual == expected)
        || api_keys_present
    {
        return Err(
            "OpenAI Codex requires its fixed endpoint, Responses protocol, and OAuth accounts instead of API keys",
        );
    }
    Ok(())
}

fn apply_codex_request_auth(
    request: reqwest::RequestBuilder,
    oauth_auth: Option<&OAuthRequestAuth>,
    session_id: &str,
) -> Result<reqwest::RequestBuilder, String> {
    let auth = oauth_auth.ok_or("provider OAuth account is unavailable")?;
    let mut request = request
        .bearer_auth(&auth.access_token)
        .header("originator", "codex_cli_rs")
        .header("User-Agent", codex_oauth::CODEX_USER_AGENT)
        .header("session_id", session_id);
    if let Some(account_id) = auth.account_id.as_deref() {
        request = request.header("ChatGPT-Account-ID", account_id);
    }
    Ok(request)
}

async fn prepare_codex_images(
    body: &mut Value,
    upstream: crate::config::UpstreamSettings,
) -> Result<(), String> {
    tokio::time::timeout(
        upstream.codex_image_preparation_timeout,
        crate::provider_adapters::images::inline_remote_images(body, upstream),
    )
    .await
    .map_err(|_| "Codex image preparation timed out".to_owned())??;
    Ok(())
}
