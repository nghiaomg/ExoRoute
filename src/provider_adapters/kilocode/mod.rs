//! Kilo Code (Kilocode) OAuth adapter.
//!
//! Kilo Code exposes an OpenRouter-style catalog behind its own device-code
//! sign-in. This adapter is deliberately separate from the anonymous
//! `kilo_gateway` adapter: it targets `api.kilo.ai/api/openrouter`, uses the
//! stored OAuth token as a Bearer credential, and identifies itself with the
//! editor header the gateway requires.

use super::auth::kilocode::{self as kilocode_oauth, KilocodeAccount};
use super::*;

mod catalog;
mod device;
mod probe;
#[cfg(test)]
mod tests;

use catalog::discover_models;
use probe::test_kilocode_oauth_model;

/// Fixed upstream root. The provider row stores this value as its base URL.
pub(super) const KILOCODE_BASE_URL: &str = "https://api.kilo.ai/api/openrouter";
/// Header the Kilo gateway uses to attribute the calling editor.
pub(super) const EDITOR_NAME_HEADER: &str = "x-kilocode-editorname";
pub(super) const EDITOR_NAME: &str = "ExoRoute";

pub(super) struct KilocodeAdapter;

pub(super) static KILOCODE_ADAPTER: KilocodeAdapter = KilocodeAdapter;

impl ProviderAdapter for KilocodeAdapter {
    fn adapter_id(&self) -> &'static str {
        KILOCODE_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Kilo Code requires the Chat Completions protocol".to_owned());
        }
        kilocode_url(base_url, "chat/completions")
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        kilocode_url(base_url, "models")
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn apply_client_headers(
        &self,
        request: reqwest::RequestBuilder,
        _client_headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        request.header(EDITOR_NAME_HEADER, EDITOR_NAME)
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
        if !preset.supported_auth_types.contains(&auth_type) {
            return Err("Kilo Code requires a connected Kilo Code account");
        }
        if base_url.trim_end_matches('/') != KILOCODE_BASE_URL {
            return Err("Kilo Code requires its fixed OpenRouter-compatible endpoint");
        }
        if preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err("Kilo Code requires the Chat Completions protocol");
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
        let account = oauth_auth
            .ok_or_else(|| "Kilo Code requires a connected Kilo Code account".to_owned())?;
        Ok(request.bearer_auth(&account.access_token))
    }

    fn discover_oauth_models<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async move {
            match discover_models(state, credential_id).await? {
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
        Box::pin(test_kilocode_oauth_model(
            self,
            state,
            base_url,
            model,
            credential_id,
        ))
    }

    fn uses_device_authorization(&self) -> bool {
        true
    }

    fn start_device_authorization<'a>(
        &'a self,
        state: &'a AppState,
    ) -> AdapterFuture<'a, Result<AdapterDeviceAuthorization, String>> {
        let operational = state.operational_settings().settings;
        Box::pin(device::start_authorization(
            operational.connect_timeout,
            operational.request_timeout,
            operational.upstream,
        ))
    }

    fn poll_device_authorization<'a>(
        &'a self,
        state: &'a AppState,
        device_code: &'a str,
    ) -> AdapterFuture<'a, Result<AdapterDevicePoll, String>> {
        let operational = state.operational_settings().settings;
        Box::pin(async move {
            let poll = device::poll_authorization(
                device_code,
                operational.connect_timeout,
                operational.request_timeout,
                operational.upstream,
            )
            .await?;
            Ok(match poll {
                device::DevicePoll::Pending => AdapterDevicePoll::Pending,
                device::DevicePoll::SlowDown => AdapterDevicePoll::SlowDown,
                device::DevicePoll::Denied => AdapterDevicePoll::Denied,
                device::DevicePoll::Expired => AdapterDevicePoll::Expired,
                device::DevicePoll::Approved(payload) => {
                    // The approval body carries the long-lived token; parse it
                    // here so an unusable approval never reaches storage.
                    let account = KilocodeAccount::from_device_approval(&payload)?;
                    AdapterDevicePoll::Approved(AdapterOAuthAccount {
                        payload,
                        display_name: account.display_name(),
                    })
                }
            })
        })
    }

    fn resolve_oauth_request_auth<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Option<OAuthRequestAuth>, String>> {
        Box::pin(async move {
            let account = kilocode_oauth::account_for_use(state, credential_id).await?;
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
        Box::pin(kilocode_oauth::save_kilocode_account(
            state,
            provider_id,
            account,
        ))
    }
}

/// Builds a catalog or chat URL from the stored base URL. Both suffixes are
/// stripped so a row that already stores a full endpoint still resolves.
pub(super) fn kilocode_url(base_url: &str, endpoint: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|_| "Kilo Code provider URL is invalid".to_owned())?;
    let mut path = url.path().trim_end_matches('/').to_owned();
    for suffix in ["/chat/completions", "/models"] {
        if let Some(root) = path.strip_suffix(suffix) {
            path = root.to_owned();
            break;
        }
    }
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return Err("Kilo Code provider URL must include the /api/openrouter path".to_owned());
    }
    url.set_path(&format!("{path}/{endpoint}"));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}
