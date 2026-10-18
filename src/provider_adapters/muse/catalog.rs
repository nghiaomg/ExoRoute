//! Muse model catalog discovery.
//!
//! The catalog is read with the account's minted inference key, exactly like a
//! chat request. Muse returns its whole OpenAI-style catalog in one page, and a
//! response that declares more rows is reported as truncated instead of being
//! silently shortened.

use super::auth::muse as muse_oauth;
use super::*;
use crate::security::egress;
use std::collections::HashSet;
use std::time::Duration;

const MAX_CATALOG_BODY_BYTES: usize = 4 * 1024 * 1024;
const CATALOG_CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(3);

pub(super) async fn discover_models(
    state: &AppState,
    credential_id: &str,
) -> Result<ModelDiscoveryResult, String> {
    let loaded = muse_oauth::load_muse_account(state, credential_id).await?;
    // Discovery is authenticated with the subscription inference key, so an
    // account whose key is still missing is minted first rather than reported
    // as an empty catalog.
    let account = match loaded.needs_remint {
        true => remint_and_store(state, credential_id, &loaded.account).await?,
        false => loaded.account,
    };
    let api_key = account
        .inference_key()
        .ok_or_else(|| "Muse account has no inference key".to_owned())?;
    // The provider row owns the endpoint, so a proxied deployment discovers
    // from where it routes.
    let base_url = muse_oauth::provider_base_url(state, credential_id).await?;
    let endpoint = MUSE_ADAPTER.model_list_endpoint(&base_url)?;
    let operational = state.operational_settings().settings;
    let (endpoint, client) = egress::provider_client(
        endpoint.as_str(),
        state.config.allow_private_provider_urls,
        operational.connect_timeout.min(CATALOG_CONNECT_TIMEOUT_CAP),
        operational
            .request_timeout
            .min(operational.upstream.discovery_request_timeout),
        false,
        INFERENCE_USER_AGENT,
        operational.upstream,
    )
    .await?;
    let response = client
        .get(endpoint)
        .header(http::header::ACCEPT, "application/json")
        .header(device::API_VERSION_HEADER, device::API_VERSION)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach Muse while importing models: {}",
                error.without_url()
            )
        })?;
    let status = response.status();
    if status == http::StatusCode::NOT_FOUND || status == http::StatusCode::METHOD_NOT_ALLOWED {
        return Ok(ModelDiscoveryResult::Unsupported);
    }
    if !status.is_success() {
        return Err(format!(
            "Muse returned HTTP {} while importing models",
            status.as_u16()
        ));
    }
    let body = read_limited_response(response, MAX_CATALOG_BODY_BYTES).await?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "Muse returned an invalid JSON model list".to_owned())?;
    let page = parse_provider_model_page(&value)?;
    let mut seen = HashSet::new();
    let mut models = Vec::new();
    for model in page.models {
        if seen.insert(model.clone()) {
            models.push(model);
            if models.len() >= MAX_API_MODELS {
                return Ok(ModelDiscoveryResult::Available {
                    models,
                    truncated: true,
                });
            }
        }
    }
    Ok(ModelDiscoveryResult::Available {
        models,
        truncated: page.has_more,
    })
}
