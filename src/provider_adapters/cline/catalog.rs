//! Cline and ClinePass model catalog discovery.
//!
//! Catalog transport, shape parsing, filtering, and fallback policy are kept
//! apart from request authentication and OAuth credential persistence.

use super::*;
use serde_json::Value;
use std::time::Duration;

pub(crate) async fn discover_models(
    adapter_id: &str,
    state: &AppState,
) -> Result<ModelDiscoveryResult, String> {
    let models = if adapter_id == CLINEPASS_ADAPTER_ID {
        fetch_json_models(state, RECOMMENDED_MODELS_URL)
            .await
            .and_then(|value| parse_clinepass_models(&value))
    } else {
        match fetch_json_models(state, RECOMMENDED_MODELS_URL)
            .await
            .and_then(|value| parse_cline_recommended_models(&value))
        {
            Ok(models) if !models.is_empty() => Ok(models),
            _ => async_fetch_cline_models(state).await,
        }
    };
    let models = models.unwrap_or_else(|_| {
        if adapter_id == CLINEPASS_ADAPTER_ID {
            CLINEPASS_FALLBACK_MODELS
        } else {
            CLINE_FALLBACK_MODELS
        }
        .iter()
        .map(|model| (*model).to_owned())
        .collect()
    });
    Ok(ModelDiscoveryResult::Available {
        truncated: models.len() >= MAX_CLINE_MODELS,
        models: models.into_iter().take(MAX_CLINE_MODELS).collect(),
    })
}

async fn async_fetch_cline_models(state: &AppState) -> Result<Vec<String>, String> {
    let value = fetch_json_models(state, MODELS_URL).await?;
    parse_cline_models(&value)
}

async fn fetch_json_models(state: &AppState, endpoint: &str) -> Result<Value, String> {
    let operational = state.operational_settings().settings;
    let (url, client) = egress::provider_client(
        endpoint,
        false,
        operational.connect_timeout.min(Duration::from_secs(3)),
        operational
            .request_timeout
            .min(operational.upstream.discovery_request_timeout),
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        operational.upstream,
    )
    .await?;
    let mut response = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| {
            format!(
                "could not reach Cline model catalog: {}",
                error.without_url()
            )
        })?;
    if !response.status().is_success() {
        return Err(format!(
            "Cline model catalog returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        format!(
            "could not read Cline model catalog: {}",
            error.without_url()
        )
    })? {
        if body.len().saturating_add(chunk.len()) > MAX_CLINE_CATALOG_BYTES {
            return Err("Cline model catalog response exceeded its size limit".to_owned());
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body)
        .map_err(|_| "Cline model catalog returned invalid JSON".to_owned())
}

pub(crate) fn parse_clinepass_models(value: &Value) -> Result<Vec<String>, String> {
    let bucket = value
        .get("clinePass")
        .or_else(|| value.get("data").and_then(|data| data.get("clinePass")))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "ClinePass model catalog did not include its subscription bucket".to_owned()
        })?;
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::new();
    for entry in bucket.iter().take(MAX_CLINE_MODELS) {
        let Some(model) = entry.get("id").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if model.starts_with("cline-pass/") && seen.insert(model.to_owned()) {
            models.push(model.to_owned());
        }
    }
    if models.is_empty() {
        return Err("ClinePass subscription catalog had no usable models".to_owned());
    }
    Ok(models)
}

pub(crate) fn parse_cline_recommended_models(value: &Value) -> Result<Vec<String>, String> {
    let bucket = value
        .get("recommended")
        .or_else(|| value.get("data").and_then(|data| data.get("recommended")))
        .and_then(Value::as_array)
        .ok_or_else(|| "Cline model catalog did not include its recommended bucket".to_owned())?;
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::new();
    for entry in bucket.iter().take(MAX_CLINE_MODELS) {
        let Some(model) = entry.get("id").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        let promotional_free = entry.get("isFree").and_then(Value::as_bool) == Some(true)
            || entry.get("free").and_then(Value::as_bool) == Some(true)
            || is_promotional_free_model(model);
        if is_cline_model(model) && !promotional_free && seen.insert(model.to_owned()) {
            models.push(model.to_owned());
        }
    }
    Ok(models)
}

pub(crate) fn parse_cline_models(value: &Value) -> Result<Vec<String>, String> {
    let rows = value
        .as_array()
        .or_else(|| value.get("data").and_then(Value::as_array))
        .or_else(|| value.get("models").and_then(Value::as_array))
        .ok_or_else(|| "Cline model catalog had an unsupported response shape".to_owned())?;
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::new();
    for row in rows.iter().take(MAX_CLINE_MODELS) {
        let Some(model) = row.get("id").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        let output_is_text = row
            .pointer("/architecture/modality")
            .and_then(Value::as_str)
            .and_then(|modality| modality.split_once("->"))
            .is_some_and(|(_, output)| output.trim().eq_ignore_ascii_case("text"));
        let promotional_free = row.get("isFree").and_then(Value::as_bool) == Some(true)
            || row.get("free").and_then(Value::as_bool) == Some(true)
            || is_promotional_free_model(model);
        if output_is_text
            && !promotional_free
            && is_cline_model(model)
            && seen.insert(model.to_owned())
        {
            models.push(model.to_owned());
        }
    }
    if models.is_empty() {
        return Err("Cline model catalog had no supported text models".to_owned());
    }
    Ok(models)
}

fn is_cline_model(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 512
        && !model.starts_with("cline-pass/")
        && !model.bytes().any(|byte| byte.is_ascii_control())
}

fn is_promotional_free_model(model: &str) -> bool {
    model.ends_with(":free") || model == "openrouter/free"
}
