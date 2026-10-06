use super::{
    AppState, MAX_API_MODEL_PAGE_BYTES, MAX_API_MODELS, ModelDiscoveryResult, ProviderAdapter,
    ProviderModelPage, adapter, capabilities,
};
use crate::security::egress;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) fn provider_models_url(base_url: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(base_url)
        .map_err(|_| "provider URL is invalid; update its settings and try again".to_owned())?;
    let current = url.path().trim_end_matches('/');
    let path = if current.ends_with("/models") {
        current.to_owned()
    } else if current.ends_with("/v1") {
        format!("{current}/models")
    } else {
        format!("{current}/v1/models")
    };
    url.set_path(&path);
    Ok(url)
}

pub(crate) fn parse_provider_model_page(value: &Value) -> Result<ProviderModelPage, String> {
    let rows = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
        .ok_or_else(|| "provider response does not contain a supported models list".to_owned())?;
    let mut models = Vec::new();
    for row in rows {
        let model = row
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| row.get("name").and_then(Value::as_str))
            .map(str::trim)
            .filter(|name| !name.is_empty());
        if let Some(model) = model {
            models.push(model.to_owned());
        }
    }
    let has_more = value
        .get("has_more")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let next_cursor = value
        .get("last_id")
        .or_else(|| value.get("lastId"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            has_more
                .then(|| {
                    rows.last().and_then(|row| {
                        row.get("id")
                            .and_then(Value::as_str)
                            .or_else(|| row.get("name").and_then(Value::as_str))
                            .map(str::to_owned)
                    })
                })
                .flatten()
        });
    Ok(ProviderModelPage {
        models,
        has_more,
        next_cursor,
    })
}

pub(crate) async fn discover_api_key_models(
    adapter_id: &str,
    request: super::AdapterApiKeyRequest<'_>,
) -> Result<ModelDiscoveryResult, String> {
    let Some(adapter) = adapter(adapter_id) else {
        return Err("provider adapter is not registered".to_owned());
    };
    let capabilities =
        capabilities(adapter_id).ok_or_else(|| "provider adapter is not registered".to_owned())?;
    if !capabilities.model_discovery || !capabilities.api_keys {
        return Err("provider adapter does not support API-key model discovery".to_owned());
    }
    adapter.discover_api_key_models(request).await
}

pub(crate) async fn discover_generic_api_key_models(
    adapter: &dyn ProviderAdapter,
    request: super::AdapterApiKeyRequest<'_>,
) -> Result<ModelDiscoveryResult, String> {
    let state = request.state;
    let base_url = request.base_url;
    let options = GenericModelDiscoveryRequest {
        auth_type: request.auth_type,
        auth_header: request.auth_header,
        custom_headers: request.custom_headers,
        preferred_protocol: request.preferred_protocol,
        credential: request.credential,
    };
    discover_generic_api_key_models_with_timeout(
        adapter,
        state,
        base_url,
        options,
        request
            .state
            .operational_settings()
            .settings
            .upstream
            .discovery_request_timeout,
    )
    .await
}

pub(crate) struct GenericModelDiscoveryRequest<'a> {
    pub(super) auth_type: &'a str,
    pub(super) auth_header: Option<&'a str>,
    pub(super) custom_headers: &'a BTreeMap<String, String>,
    pub(super) preferred_protocol: &'a str,
    pub(super) credential: &'a str,
}

pub(crate) async fn discover_generic_api_key_models_with_timeout(
    adapter: &dyn ProviderAdapter,
    state: &AppState,
    base_url: &str,
    options: GenericModelDiscoveryRequest<'_>,
    request_timeout_cap: std::time::Duration,
) -> Result<ModelDiscoveryResult, String> {
    let base = adapter.model_list_endpoint(base_url)?;
    let upstream = state.operational_settings().settings.upstream;
    let (base, provider_http) = egress::provider_client(
        base.as_str(),
        state.config.allow_private_provider_urls,
        state
            .config
            .connect_timeout
            .min(std::time::Duration::from_secs(3)),
        state.config.request_timeout.min(request_timeout_cap),
        false,
        concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
        state.operational_settings().settings.upstream,
    )
    .await?;
    let mut cursor: Option<String> = None;
    let mut seen = std::collections::HashSet::new();
    let mut models = Vec::new();

    for page_index in 0..upstream.model_discovery_max_pages {
        let mut url = base.clone();
        if let Some(cursor) = cursor.as_deref() {
            let parameter = if options.preferred_protocol == "messages" {
                "after_id"
            } else {
                "after"
            };
            url.query_pairs_mut().append_pair(parameter, cursor);
        }
        let mut provider_request = provider_http.get(url);
        provider_request = crate::provider_adapters::apply_custom_headers(
            provider_request,
            options.custom_headers,
        )?;
        if !options.credential.is_empty() {
            provider_request = adapter.apply_request_auth(
                provider_request,
                options.auth_type,
                options.auth_header,
                Some(options.credential),
                None,
                "model-discovery",
            )?;
        }
        if options.preferred_protocol == "messages" {
            provider_request = provider_request.header("anthropic-version", "2023-06-01");
        }
        let mut response = provider_request.send().await.map_err(|error| {
            format!(
                "could not reach provider while importing models: {}",
                error.without_url()
            )
        })?;
        let status = response.status();
        if status == http::StatusCode::NOT_FOUND || status == http::StatusCode::METHOD_NOT_ALLOWED {
            return Ok(ModelDiscoveryResult::Unsupported);
        }
        if !status.is_success() {
            return Err(format!(
                "provider returned HTTP {} while importing models",
                status.as_u16()
            ));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            format!(
                "could not read provider model list: {}",
                error.without_url()
            )
        })? {
            if body.len().saturating_add(chunk.len()) > MAX_API_MODEL_PAGE_BYTES {
                return Err("provider model list response exceeded the 4 MiB page limit".to_owned());
            }
            body.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| "provider returned an invalid JSON model list".to_owned())?;
        let page = parse_provider_model_page(&value)?;
        for model in page.models {
            if seen.insert(model.clone()) {
                models.push(model);
                if models.len() >= MAX_API_MODELS {
                    return Ok(ModelDiscoveryResult::Available {
                        models,
                        truncated: page.has_more,
                    });
                }
            }
        }
        if !page.has_more {
            return Ok(ModelDiscoveryResult::Available {
                models,
                truncated: false,
            });
        }
        let Some(next_cursor) = page.next_cursor else {
            return Ok(ModelDiscoveryResult::Available {
                models,
                truncated: true,
            });
        };
        if cursor.as_deref() == Some(next_cursor.as_str()) {
            return Ok(ModelDiscoveryResult::Available {
                models,
                truncated: true,
            });
        }
        cursor = Some(next_cursor);
        if page_index + 1 == upstream.model_discovery_max_pages {
            return Ok(ModelDiscoveryResult::Available {
                models,
                truncated: true,
            });
        }
    }
    Ok(ModelDiscoveryResult::Available {
        models,
        truncated: true,
    })
}
