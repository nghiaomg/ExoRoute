//! Freebuff adapter barrel: trait impl plus the catalog, body, session,
//! client, errors, finish, and credential-probe modules.
//!
//! URL validation and endpoint joining stay here because the trait impl and
//! the session/probe paths share them; everything else lives in a submodule.

use super::*;
use sha2::{Digest, Sha256};

mod body;
mod catalog;
mod client;
mod credential_test;
mod errors;
mod finish;
mod session;
#[cfg(test)]
mod tests;

pub(super) static FREEBUFF_ADAPTER: FreebuffAdapter = FreebuffAdapter;

pub(super) struct FreebuffAdapter;

impl ProviderAdapter for FreebuffAdapter {
    fn adapter_id(&self) -> &'static str {
        FREEBUFF_ADAPTER_ID
    }

    fn endpoint(
        &self,
        base_url: &str,
        protocol: Protocol,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        if protocol != Protocol::ChatCompletions {
            return Err("Freebuff supports Chat Completions upstream".to_owned());
        }
        validate_base_url(base_url)?;
        endpoint_from_root(
            adapter_base_url_override.unwrap_or(catalog::FREEBUFF_BASE_URL),
            catalog::FREEBUFF_CHAT_COMPLETIONS_PATH,
        )
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        validate_base_url(base_url)?;
        endpoint_from_root(catalog::FREEBUFF_BASE_URL, catalog::FREEBUFF_MODELS_PATH)
    }

    fn has_public_model_catalog(&self) -> bool {
        true
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
        if auth_type != "bearer" || !preset.supported_auth_types.contains(&auth_type) {
            return Err("Freebuff requires Bearer authentication");
        }
        if validate_base_url(base_url).is_err() {
            return Err("Freebuff uses https://www.codebuff.com/api/v1");
        }
        if preferred_protocol != "chat_completions"
            || supported_protocols.len() != 1
            || supported_protocols[0] != "chat_completions"
        {
            return Err("Freebuff supports Chat Completions upstream only");
        }
        Ok(())
    }

    fn discover_api_key_models<'a>(
        &'a self,
        _request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(async {
            Ok(ModelDiscoveryResult::Available {
                models: catalog::catalog_models(),
                truncated: false,
            })
        })
    }

    fn prepare_upstream_request<'a>(
        &'a self,
        context: AdapterRequestContext<'a>,
        body: &'a mut Value,
    ) -> AdapterFuture<'a, Result<AdapterRequestPreparation, AdapterRequestError>> {
        Box::pin(session::prepare_freebuff_request(context, body))
    }

    fn retry_upstream_response_as_key_rejection(&self) -> bool {
        false
    }

    fn finish_upstream_request<'a>(
        &'a self,
        state: &'a AppState,
        preparation: AdapterRequestPreparation,
        auth: UpstreamAuthContext<'a>,
        status: Option<StatusCode>,
    ) -> AdapterFuture<'a, ()> {
        Box::pin(finish::finish_freebuff_agent_run(
            state,
            preparation,
            auth,
            status,
        ))
    }

    fn test_api_key_credential<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, AdapterKeyTestOutcome> {
        Box::pin(credential_test::test_freebuff_credential(
            request.state,
            request.base_url,
            request.auth_type,
            request.preferred_protocol,
            request.custom_headers,
            request.credential,
        ))
    }
}

pub(super) fn validate_base_url(base_url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(base_url.trim())
        .map_err(|_| "Freebuff uses https://www.codebuff.com/api/v1".to_owned())?;
    if parsed.scheme() != "https"
        || !parsed
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case("www.codebuff.com"))
        || parsed.port().is_some_and(|port| port != 443)
        || !matches!(parsed.path(), "/api/v1" | "/api/v1/")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Freebuff uses https://www.codebuff.com/api/v1".to_owned());
    }
    reqwest::Url::parse(catalog::FREEBUFF_BASE_URL)
        .map_err(|error| format!("invalid Freebuff base URL: {error}"))
}

pub(super) fn endpoint_from_root(root: &str, suffix: &str) -> Result<reqwest::Url, String> {
    let mut url =
        reqwest::Url::parse(root).map_err(|_| "Freebuff endpoint URL is invalid".to_owned())?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Freebuff endpoint URL is invalid".to_owned());
    }
    let root_path = url.path().trim_end_matches('/');
    let path = if root_path.is_empty() {
        format!("/{suffix}")
    } else {
        format!("{root_path}/{suffix}")
    };
    url.set_path(&path);
    Ok(url)
}

pub(super) fn freebuff_session_cache_key(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}
