//! Qoder model catalog discovery.
//!
//! Tries to fetch live models from the provider's /models endpoint. If the upstream
//! requires special signing (such as COSY) or does not expose a standard models
//! endpoint, it gracefully falls back to the authoritative models catalog
//! declared in the provider preset.

use super::auth::qoder as qoder_oauth;
use super::*;
use crate::security::egress;
use std::collections::HashSet;
use std::time::Duration;

const MAX_CATALOG_BODY_BYTES: usize = 4 * 1024 * 1024;
const CATALOG_CONNECT_TIMEOUT_CAP: Duration = Duration::from_secs(3);

pub(super) async fn discover_models(
    state: &AppState,
    credential_id: &str,
    adapter_id: &str,
) -> Result<Vec<String>, String> {
    let account = qoder_oauth::account_for_use(state, credential_id).await?;
    let base_url = qoder_oauth::provider_base_url(state, credential_id)
        .await
        .unwrap_or_else(|_| {
            if adapter_id == QODER_CN_ADAPTER_ID {
                "https://gateway.qoder.com.cn".to_owned()
            } else {
                "https://api3.qoder.sh".to_owned()
            }
        });

    let default_list: Vec<String> = presets::default_models(adapter_id)
        .iter()
        .map(|&s| s.to_owned())
        .collect();

    if let Ok(live_models) = fetch_upstream_models(state, &base_url, &account.access_token).await
        && !live_models.is_empty()
    {
        let mut seen = HashSet::new();
        let mut models = Vec::new();
        for model in live_models {
            if seen.insert(model.clone()) {
                models.push(model);
                if models.len() >= MAX_API_MODELS {
                    break;
                }
            }
        }
        if !models.is_empty() {
            return Ok(models);
        }
    }

    if !default_list.is_empty() {
        return Ok(default_list);
    }

    Err("Qoder returned no supported models".to_owned())
}

pub(super) fn is_supported_qoder_model(adapter_id: &str, model: &str) -> bool {
    let key = model
        .strip_prefix("qoder/")
        .or_else(|| model.strip_prefix("qd/"))
        .or_else(|| model.strip_prefix("qdcn/"))
        .unwrap_or(model);

    if presets::default_models(adapter_id).contains(&key) {
        return true;
    }

    matches!(
        key,
        "ultimate"
            | "auto"
            | "performance"
            | "efficient"
            | "lite"
            | "qmodel_38max"
            | "qmodel_latest"
            | "qmodel"
            | "qfmodel"
            | "kmodel_latest"
            | "kmodel"
            | "gmodel"
            | "gfmodel"
            | "dmodel"
            | "dfmodel"
            | "mmodel"
            | "qwen3.8-max"
            | "qwen3.7-max"
            | "qwen3.7-plus"
            | "qwen3.8-flash"
            | "kimi-k3"
            | "kimi-k2.7-code"
            | "glm-5.3"
            | "glm-5.3-flash"
            | "deepseek-v4-pro"
            | "deepseek-v4-flash"
            | "minimax-m3"
    )
}

async fn fetch_upstream_models(
    state: &AppState,
    base_url: &str,
    access_token: &str,
) -> Result<Vec<String>, String> {
    let endpoint = qoder_url(base_url, "models")?;
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
        .header(QODER_IDE_VERSION_HEADER, QODER_IDE_VERSION)
        .header(QODER_CLIENT_TYPE_HEADER, QODER_CLIENT_TYPE)
        .header(QODER_LOGIN_VERSION_HEADER, QODER_LOGIN_VERSION)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach Qoder while importing models: {}",
                error.without_url()
            )
        })?;

    if !response.status().is_success() {
        return Err(format!(
            "Qoder returned HTTP {} while importing models",
            response.status().as_u16()
        ));
    }

    let body = read_limited_response(response, MAX_CATALOG_BODY_BYTES).await?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|_| "Qoder returned an invalid JSON model list".to_owned())?;
    let page = parse_provider_model_page(&value)?;
    Ok(page.models)
}
