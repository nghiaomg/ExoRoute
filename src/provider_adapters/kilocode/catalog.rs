//! Kilo Code model catalog discovery.
//!
//! The catalog is read with the stored OAuth token plus the editor header the
//! gateway requires. A single bounded request is made: Kilo Code returns its
//! whole OpenRouter-style catalog in one page, and a response that declares
//! more rows is reported as truncated instead of being silently shortened.

use super::auth::kilocode as kilocode_oauth;
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
    let account = kilocode_oauth::account_for_use(state, credential_id).await?;
    // The provider row owns the endpoint, exactly like a chat request, so a
    // self-hosted or proxied Kilo deployment discovers from where it routes.
    let base_url = kilocode_oauth::provider_base_url(state, credential_id).await?;
    let endpoint = KILOCODE_ADAPTER.model_list_endpoint(&base_url)?;
    let operational = state.operational_settings().settings;
    let (endpoint, client) = egress::provider_client(
        endpoint.as_str(),
        state.config.allow_private_provider_urls,
        operational.connect_timeout.min(CATALOG_CONNECT_TIMEOUT_CAP),
        operational
            .request_timeout
            .min(operational.upstream.discovery_request_timeout),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        operational.upstream,
    )
    .await?;
    let response = client
        .get(endpoint)
        .header(http::header::ACCEPT, "application/json")
        .header(EDITOR_NAME_HEADER, EDITOR_NAME)
        .bearer_auth(&account.access_token)
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach Kilo Code while importing models: {}",
                error.without_url()
            )
        })?;
    let status = response.status();
    if status == http::StatusCode::NOT_FOUND || status == http::StatusCode::METHOD_NOT_ALLOWED {
        return Ok(ModelDiscoveryResult::Unsupported);
    }
    if !status.is_success() {
        return Err(format!(
            "Kilo Code returned HTTP {} while importing models",
            status.as_u16()
        ));
    }
    let body = read_limited_response(response, MAX_CATALOG_BODY_BYTES).await?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "Kilo Code returned an invalid JSON model list".to_owned())?;
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
