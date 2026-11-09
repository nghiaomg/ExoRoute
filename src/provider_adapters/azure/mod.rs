//! Azure OpenAI.
//!
//! Azure addresses deployments instead of models and authenticates with an
//! `api-key` header, so it cannot share the OpenAI adapter. The deployment
//! name is the model the gateway was asked to route, and the API version is a
//! required query parameter that the operator supplies on the provider URL
//! because Azure versions differ per resource.

use super::*;
use reqwest::header::HeaderValue;

pub const AZURE_ADAPTER_ID: &str = "azure_openai";
/// Azure rejects requests without a matching `api-version`. A value is supplied
/// by the provider URL's query string rather than guessed here, because a
/// version the resource does not serve fails with an opaque upstream error.
const API_VERSION_PARAM: &str = "api-version";

/// Builds `{base}/openai/deployments/{deployment}/{suffix}` on top of the
/// operator-supplied base URL, preserving its `api-version` query parameter.
fn azure_endpoint(
    base_url: &str,
    deployment: &str,
    suffix: &str,
    require_api_version: bool,
) -> Result<reqwest::Url, String> {
    if deployment.trim().is_empty() {
        return Err("Azure OpenAI requires a deployment name as the model".to_owned());
    }
    if deployment.contains('/') || deployment.contains('?') || deployment.contains('#') {
        return Err("Azure OpenAI deployment name contains invalid characters".to_owned());
    }
    let mut url = reqwest::Url::parse(base_url.trim())
        .map_err(|error| format!("invalid provider URL: {error}"))?;
    if require_api_version && !url.query_pairs().any(|(name, _)| name == API_VERSION_PARAM) {
        return Err(
            "Azure OpenAI requires an api-version query parameter on the provider URL".to_owned(),
        );
    }
    let current = url.path().trim_end_matches('/');
    let deployment_segment = format!("/deployments/{deployment}");
    let path = if current.ends_with(&deployment_segment) {
        // Already a deployment URL: append the suffix only if it is missing.
        if current.ends_with(suffix) {
            current.to_owned()
        } else {
            format!("{current}/{}", suffix.trim_start_matches('/'))
        }
    } else if current.ends_with(&suffix) {
        current.to_owned()
    } else {
        format!("{current}/openai/deployments/{deployment}/{suffix}")
    };
    url.set_path(&path);
    Ok(url)
}

pub(super) struct AzureOpenAiAdapter;

pub(super) static AZURE_ADAPTER: AzureOpenAiAdapter = AzureOpenAiAdapter;

impl ProviderAdapter for AzureOpenAiAdapter {
    fn adapter_id(&self) -> &'static str {
        AZURE_ADAPTER_ID
    }

    /// The client API cannot name a deployment, so a bare `endpoint` call has no
    /// meaning; routing always goes through [`Self::upstream_endpoint`].
    fn endpoint(
        &self,
        _base_url: &str,
        _protocol: Protocol,
        _adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        Err("Azure OpenAI requires a deployment name; route through a model".to_owned())
    }

    fn upstream_endpoint(
        &self,
        base_url: &str,
        protocol: UpstreamProtocol,
        model: &str,
        _streaming: bool,
        adapter_base_url_override: Option<&str>,
    ) -> Result<reqwest::Url, String> {
        let suffix = match protocol {
            UpstreamProtocol::ChatCompletions => "chat/completions",
            UpstreamProtocol::Responses => "responses",
            UpstreamProtocol::Messages | UpstreamProtocol::GoogleGenerateContent => {
                return Err("Azure OpenAI supports Chat Completions and Responses only".to_owned());
            }
        };
        azure_endpoint(
            adapter_base_url_override.unwrap_or(base_url),
            model,
            suffix,
            true,
        )
    }

    fn prepare_body(&self, _body: &mut Value, _request_id: &str) {}

    fn model_list_endpoint(&self, base_url: &str) -> Result<reqwest::Url, String> {
        let mut url = reqwest::Url::parse(base_url.trim())
            .map_err(|error| format!("invalid provider URL: {error}"))?;
        let current = url.path().trim_end_matches('/');
        let path = format!("{current}/openai/deployments");
        url.set_path(&path);
        Ok(url)
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
            return Err("authentication type is not supported by this provider");
        }
        if supported_protocols.is_empty()
            || !supported_protocols.iter().any(|p| p == preferred_protocol)
            || supported_protocols
                .iter()
                .any(|p| !matches!(p.as_str(), "chat_completions" | "responses"))
        {
            return Err("Azure OpenAI supports Chat Completions and Responses only");
        }
        // The operator's URL carries the resource host and api-version; without
        // the version Azure answers with an error that names no field.
        if reqwest::Url::parse(base_url.trim())
            .map(|url| !url.query_pairs().any(|(name, _)| name == API_VERSION_PARAM))
            .unwrap_or(true)
        {
            return Err("Azure OpenAI requires an api-version query parameter on the base URL");
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
        if oauth_auth.is_some() {
            return Err("Azure OpenAI does not support OAuth accounts".to_owned());
        }
        if auth_type == "none" {
            return Ok(request);
        }
        let Some(secret) = secret else {
            return Ok(request);
        };
        let value = HeaderValue::from_str(secret)
            .map_err(|_| "provider credential contains invalid header characters")?;
        Ok(request.header("api-key", value))
    }

    fn discover_api_key_models<'a>(
        &'a self,
        request: AdapterApiKeyRequest<'a>,
    ) -> AdapterFuture<'a, Result<ModelDiscoveryResult, String>> {
        Box::pin(discover_generic_api_key_models(self, request))
    }

    /// Azure reports a spent or wrong key as 401/403 like OpenAI. A 404 means the
    /// deployment name is wrong, which retrying with another key cannot fix.
    fn is_key_rejection_status(&self, status: StatusCode) -> bool {
        matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::TOO_MANY_REQUESTS
        )
    }

    fn should_mark_api_key_invalid(&self, status: StatusCode, _error_message: &str) -> bool {
        matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
    }
}

#[cfg(test)]
mod tests;
