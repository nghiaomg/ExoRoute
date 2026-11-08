//! Kiro AI adapter.
//!
//! Kiro exposes Claude models through AWS CodeWhisperer's
//! `GenerateAssistantResponse` endpoint, which speaks the binary AWS
//! EventStream protocol rather than SSE. This adapter owns its transport
//! (`uses_local_transport`): it translates the OpenAI Chat Completions body
//! into Kiro's `conversationState` shape, parses EventStream frames, and
//! synthesizes the SSE/JSON response the gateway pipeline expects. Sign-in
//! supports the AWS Builder ID device flow plus pasted refresh tokens
//! (Kiro IDE / social accounts).

use super::auth::kiro as kiro_oauth;
use super::*;

mod catalog;
mod device;
mod eventstream;
#[cfg(test)]
mod tests;
mod transform;
pub(crate) mod transport;

pub(super) struct KiroAdapter;

pub(super) static KIRO_ADAPTER: KiroAdapter = KiroAdapter;

pub(super) const KIRO_CHAT_URL: &str =
    "https://codewhisperer.us-east-1.amazonaws.com/generateAssistantResponse";

impl ProviderAdapter for KiroAdapter {
    fn adapter_id(&self) -> &'static str {
        KIRO_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Kiro requires the Chat Completions protocol".to_owned());
        }
        kiro_chat_url(base_url)
    }

    fn upstream_endpoint(
        &self,
        base_url: &str,
        protocol: UpstreamProtocol,
        _model: &str,
        _streaming: bool,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != UpstreamProtocol::ChatCompletions {
            return Err("Kiro requires the Chat Completions protocol".to_owned());
        }
        kiro_chat_url(base_url)
    }

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        kiro_chat_url(base_url)
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn apply_client_headers(
        &self,
        request: reqwest::RequestBuilder,
        _client_headers: &HeaderMap,
    ) -> reqwest::RequestBuilder {
        request
            .header("User-Agent", "AWS-SDK-JS/3.0.0 kiro-ide/1.0.0")
            .header("X-Amz-User-Agent", "aws-sdk-js/3.0.0 kiro-ide/1.0.0")
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
            return Err("Kiro requires a connected Kiro account");
        }
        if preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err("Kiro requires the Chat Completions protocol");
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
            oauth_auth.ok_or_else(|| "Kiro requires a connected Kiro account".to_owned())?;
        Ok(request.bearer_auth(&account.access_token))
    }

    fn apply_upstream_request_auth(
        &self,
        request: reqwest::RequestBuilder,
        auth: UpstreamAuthContext<'_>,
    ) -> Result<reqwest::RequestBuilder, String> {
        self.apply_request_auth(
            request,
            auth.auth_type,
            auth.auth_header,
            auth.secret,
            auth.oauth_auth,
            auth.session_id,
        )
    }

    fn uses_device_authorization(&self) -> bool {
        true
    }

    fn start_device_authorization<'a>(
        &'a self,
        state: &'a AppState,
        _base_url: &'a str,
    ) -> AdapterFuture<'a, Result<AdapterDeviceAuthorization, String>> {
        Box::pin(async move { device::start_authorization(state).await })
    }

    fn poll_device_authorization<'a>(
        &'a self,
        state: &'a AppState,
        _base_url: &'a str,
        device_code: &'a str,
    ) -> AdapterFuture<'a, Result<AdapterDevicePoll, String>> {
        Box::pin(async move { device::poll_authorization(state, device_code).await })
    }

    fn resolve_oauth_request_auth<'a>(
        &'a self,
        state: &'a AppState,
        credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Option<OAuthRequestAuth>, String>> {
        Box::pin(async move {
            let account = kiro_oauth::account_for_use(state, credential_id).await?;
            Ok(Some(OAuthRequestAuth {
                access_token: account.access_token.clone(),
                account_id: Some(account.request_extras().to_string()),
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
        Box::pin(kiro_oauth::save_kiro_account(state, provider_id, account))
    }

    fn discover_oauth_models<'a>(
        &'a self,
        _state: &'a AppState,
        _credential_id: &'a str,
    ) -> AdapterFuture<'a, Result<Vec<String>, String>> {
        Box::pin(async move { Ok(catalog::default_models().to_vec()) })
    }
}

/// Builds the CodeWhisperer endpoint from the provider's configured base URL.
/// The preset pins the AWS host; the path is fixed by the API.
fn kiro_chat_url(base_url: &str) -> Result<reqwest::Url, String> {
    let mut url =
        reqwest::Url::parse(base_url).map_err(|_| "Kiro provider URL is invalid".to_owned())?;
    let path = url.path().trim_end_matches('/');
    if path.is_empty() || path == "/" {
        url.set_path("/generateAssistantResponse");
    } else if path.ends_with("/generateAssistantResponse") {
        // already canonical
    } else {
        url.set_path(&format!("{path}/generateAssistantResponse"));
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

/// Validates a Kiro refresh token import for the admin endpoint.
pub(crate) async fn import_token(
    state: &AppState,
    provider_id: &str,
    refresh_token: &str,
) -> Result<String, String> {
    let payload = kiro_oauth::import_refresh_token(state, refresh_token).await?;
    let account = kiro_oauth::KiroAccount::from_token_payload(&payload)
        .map_err(|_| "Kiro returned invalid account credentials".to_owned())?;
    let display_name = account.display_name();
    kiro_oauth::save_kiro_account(
        state,
        provider_id,
        AdapterOAuthAccount {
            payload,
            display_name: display_name.clone(),
        },
    )
    .await?;
    Ok(display_name)
}
