//! Qoder OAuth adapter.
//!
//! Qoder provides access to frontier models (Qwen, DeepSeek, GLM, Kimi, etc.)
//! via its device-authorization flow. This adapter implements both international
//! (qoder) and China (qoder-cn) regions with the official device grant.

use super::auth::qoder as qoder_oauth;
use super::*;

mod catalog;
mod device;
mod probe;
#[cfg(test)]
mod tests;

use probe::test_qoder_oauth_model;

pub(super) const QODER_IDE_VERSION_HEADER: &str = "x-ide-version";
pub(super) const QODER_IDE_VERSION: &str = "1.0.0";
pub(super) const QODER_CLIENT_TYPE_HEADER: &str = "x-client-type";
pub(super) const QODER_CLIENT_TYPE: &str = "5";
pub(super) const QODER_LOGIN_VERSION_HEADER: &str = "x-login-version";
pub(super) const QODER_LOGIN_VERSION: &str = "v2";

pub(super) struct QoderAdapter;
pub(super) struct QoderCnAdapter;

pub(super) static QODER_ADAPTER: QoderAdapter = QoderAdapter;
pub(super) static QODER_CN_ADAPTER: QoderCnAdapter = QoderCnAdapter;

impl ProviderAdapter for QoderAdapter {
    fn adapter_id(&self) -> &'static str {
        QODER_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Qoder requires the Chat Completions protocol".to_owned());
        }
        qoder_url(base_url, "chat/completions")
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        qoder_url(base_url, "models")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn apply_client_headers(
        &self,
        request: reqwest::RequestBuilder,
        _client_headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        request
            .header(QODER_IDE_VERSION_HEADER, QODER_IDE_VERSION)
            .header(QODER_CLIENT_TYPE_HEADER, QODER_CLIENT_TYPE)
            .header(QODER_LOGIN_VERSION_HEADER, QODER_LOGIN_VERSION)
    }

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        _base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        _api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if !preset.supported_auth_types.contains(&auth_type) {
            return Err("Qoder requires a connected Qoder account");
        }
        if preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err("Qoder requires the Chat Completions protocol");
        }
        Ok(())
    }

    fn apply_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        _auth_type: &str,
        _auth_header: Option<&str>,
        _secret: Option<&str>,
        oauth_auth: Option<&OAuthRequestAuth>,
        _session_id: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        let account =
            oauth_auth.ok_or_else(|| "Qoder requires a connected Qoder account".to_owned())?;
        Ok(request.bearer_auth(&account.access_token))
    }

    fn test_oauth_model<'a>(
        &'a self,
        state: &'a AppState,
        base_url: &'a str,
        model: &'a str,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(test_qoder_oauth_model(
            self,
            state,
            base_url,
            model,
            credential_id,
        ))
    }

    fn discover_oauth_models<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(catalog::discover_models(
            state,
            credential_id,
            QODER_ADAPTER_ID,
        ))
    }

    fn uses_device_authorization(&self) -> bool {
        true
    }

    fn start_device_authorization<'a>(
        &'a self,
        _state: &'a AppState,
        _base_url: &'a str,
    ) -> AdapterFuture<'a, Result<AdapterDeviceAuthorization, String>> {
        Box::pin(async move { device::start_authorization("intl") })
    }

    fn poll_device_authorization<'a>(
        &'a self,
        state: &'a AppState,
        _base_url: &'a str,
        device_code: &'a str,
    ) -> AdapterFuture<'a, Result<AdapterDevicePoll, String>> {
        let operational = state.operational_settings().settings;
        let allow_local = state.config.allow_private_provider_urls;
        Box::pin(async move {
            device::poll_authorization(
                device_code,
                allow_local,
                operational.connect_timeout,
                operational.request_timeout,
                operational.upstream,
            )
            .await
        })
    }

    fn resolve_oauth_request_auth<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Option<OAuthRequestAuth>, String>> {
        Box::pin(async move {
            let account = qoder_oauth::account_for_use(state, credential_id).await?;
            Ok(Some(OAuthRequestAuth {
                access_token: account.access_token,
                account_id: None,
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
        Box::pin(qoder_oauth::save_qoder_account(state, provider_id, account))
    }
}

impl ProviderAdapter for QoderCnAdapter {
    fn adapter_id(&self) -> &'static str {
        QODER_CN_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Qoder CN requires the Chat Completions protocol".to_owned());
        }
        qoder_url(base_url, "chat/completions")
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        qoder_url(base_url, "models")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn apply_client_headers(
        &self,
        request: reqwest::RequestBuilder,
        _client_headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        request
            .header(QODER_IDE_VERSION_HEADER, QODER_IDE_VERSION)
            .header(QODER_CLIENT_TYPE_HEADER, QODER_CLIENT_TYPE)
            .header(QODER_LOGIN_VERSION_HEADER, QODER_LOGIN_VERSION)
    }

    fn validate_config(
        &self,
        preset: &ProviderPreset,
        auth_type: &str,
        _base_url: &str,
        preferred_protocol: &str,
        supported_protocols: &[String],
        _api_keys_present: bool,
    ) -> Result<(), &'static str> {
        if !preset.supported_auth_types.contains(&auth_type) {
            return Err("Qoder CN requires a connected Qoder account");
        }
        if preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err("Qoder CN requires the Chat Completions protocol");
        }
        Ok(())
    }

    fn apply_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        _auth_type: &str,
        _auth_header: Option<&str>,
        _secret: Option<&str>,
        oauth_auth: Option<&OAuthRequestAuth>,
        _session_id: &str,
    ) -> Result<reqwest::RequestBuilder, String> {
        let account =
            oauth_auth.ok_or_else(|| "Qoder CN requires a connected Qoder account".to_owned())?;
        Ok(request.bearer_auth(&account.access_token))
    }

    fn test_oauth_model<'a>(
        &'a self,
        state: &'a AppState,
        base_url: &'a str,
        model: &'a str,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, AdapterModelTestOutcome> {
        Box::pin(test_qoder_oauth_model(
            self,
            state,
            base_url,
            model,
            credential_id,
        ))
    }

    fn discover_oauth_models<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(catalog::discover_models(
            state,
            credential_id,
            QODER_CN_ADAPTER_ID,
        ))
    }

    fn uses_device_authorization(&self) -> bool {
        true
    }

    fn start_device_authorization<'a>(
        &'a self,
        _state: &'a AppState,
        _base_url: &'a str,
    ) -> AdapterFuture<'a, Result<AdapterDeviceAuthorization, String>> {
        Box::pin(async move { device::start_authorization("cn") })
    }

    fn poll_device_authorization<'a>(
        &'a self,
        state: &'a AppState,
        _base_url: &'a str,
        device_code: &'a str,
    ) -> AdapterFuture<'a, Result<AdapterDevicePoll, String>> {
        let operational = state.operational_settings().settings;
        let allow_local = state.config.allow_private_provider_urls;
        Box::pin(async move {
            device::poll_authorization(
                device_code,
                allow_local,
                operational.connect_timeout,
                operational.request_timeout,
                operational.upstream,
            )
            .await
        })
    }

    fn resolve_oauth_request_auth<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Option<OAuthRequestAuth>, String>> {
        Box::pin(async move {
            let account = qoder_oauth::account_for_use(state, credential_id).await?;
            Ok(Some(OAuthRequestAuth {
                access_token: account.access_token,
                account_id: None,
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
        Box::pin(qoder_oauth::save_qoder_account(state, provider_id, account))
    }
}

pub(super) fn qoder_url(base_url: &str, endpoint: &str) -> Result<reqwest::Url, String> {
    let mut url =
        reqwest::Url::parse(base_url).map_err(|_| "Qoder provider URL is invalid".to_owned())?;
    let mut path = url.path().trim_end_matches('/').to_owned();
    for suffix in ["/chat/completions", "/models"] {
        if let Some(root) = path.strip_suffix(suffix) {
            path = root.to_owned();
            break;
        }
    }
    let path = path.trim_end_matches('/');
    if path.ends_with("/v1") {
        url.set_path(&format!("{path}/{endpoint}"));
    } else if path.is_empty() {
        url.set_path(&format!("/v1/{endpoint}"));
    } else {
        url.set_path(&format!("{path}/v1/{endpoint}"));
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}
