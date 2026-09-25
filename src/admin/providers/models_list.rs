//! Shared provider-model list/search handlers plus cursor codec.
//!
//! The paged and legacy unpaged list shapes stay here so search and the
//! combo editor share one bounded read path.

use super::models_store::manual_model_names;
use super::*;
use crate::infra::storage::{Record, StorageError, Table};

pub(crate) async fn list_provider_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ProviderModelPageQuery>,
) -> ApiResult {
    let always_bounded = false;
    list_provider_models_page(state, id, query, always_bounded).await
}

pub(crate) async fn search_provider_models(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ProviderModelPageQuery>,
) -> ApiResult {
    list_provider_models_page(state, id, query, true).await
}

async fn list_provider_models_page(
    state: AppState,
    id: String,
    query: ProviderModelPageQuery,
    always_bounded: bool,
) -> ApiResult {
    if query
        .q
        .as_deref()
        .is_some_and(|search| search.len() > MAX_PROVIDER_MODEL_SEARCH_BYTES)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider model search must not exceed 256 bytes",
        ));
    }

    // Keep the original unpaged response for existing admin screens and clients.
    // The combo editor opts into this bounded path so a large catalog cannot create
    // thousands of select options for every target row at once.
    if always_bounded || query.limit.is_some() || query.q.is_some() {
        let limit = query.limit.unwrap_or(DEFAULT_PROVIDER_MODEL_PAGE_SIZE);
        if !(1..=MAX_PROVIDER_MODEL_PAGE_SIZE).contains(&limit) {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "provider model page size must be between 1 and 50",
            ));
        }

        let search = query
            .q
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let provider_id = id.clone();
        let search = search.map(str::to_lowercase);
        let (provider_exists, models, manual_models, has_more) = state
            .db
            .read(move |transaction| {
                let provider_exists =
                    match transaction.get::<Record>(Table::Providers, &provider_id)? {
                        Some(provider) => !provider_is_deleting(&provider)?,
                        None => false,
                    };
                if !provider_exists {
                    return Ok((false, Vec::new(), Vec::new(), false));
                }
                let prefix = crate::infra::db::provider_model_index_prefix(&provider_id)?;
                let scan_limit = if search.is_some() {
                    MAX_PROVIDER_SCAN_ROWS.saturating_add(1)
                } else {
                    limit.saturating_add(1)
                };
                let rows = transaction.scan_prefix::<String>(
                    Table::ProviderModelIndex,
                    &prefix,
                    scan_limit,
                )?;
                if search.is_some() && rows.len() > MAX_PROVIDER_SCAN_ROWS {
                    return Err(StorageError::Busy);
                }
                let mut models = rows
                    .into_iter()
                    .map(|(_, model)| model)
                    .filter(|model| {
                        search
                            .as_ref()
                            .is_none_or(|term| model.to_lowercase().contains(term))
                    })
                    .take(limit.saturating_add(1))
                    .collect::<Vec<_>>();
                let has_more = models.len() > limit;
                models.truncate(limit);
                // Only the returned page is enriched, so the extra record
                // reads stay bounded by the page size.
                let manual_models = manual_model_names(transaction, &provider_id, &models)?;
                Ok((true, models, manual_models, has_more))
            })
            .await
            .map_err(internal)?;
        if !provider_exists {
            return Err(fail(StatusCode::NOT_FOUND, "provider not found"));
        }
        return Ok(Json(json!({
            "models": models,
            "manual_models": manual_models,
            "has_more": has_more,
        })));
    }

    let provider_id = id.clone();
    let (exists, models, manual_models) = state
        .db
        .read(move |transaction| {
            let exists = match transaction.get::<Record>(Table::Providers, &provider_id)? {
                Some(provider) => !provider_is_deleting(&provider)?,
                None => false,
            };
            if !exists {
                return Ok((false, Vec::new(), Vec::new()));
            }
            let prefix = crate::infra::db::provider_model_index_prefix(&provider_id)?;
            let rows = transaction.scan_prefix::<String>(
                Table::ProviderModelIndex,
                &prefix,
                MAX_PROVIDER_SCAN_ROWS.saturating_add(1),
            )?;
            if rows.len() > MAX_PROVIDER_SCAN_ROWS {
                return Err(StorageError::Busy);
            }
            let models = rows.into_iter().map(|(_, model)| model).collect::<Vec<_>>();
            let manual_models = manual_model_names(transaction, &provider_id, &models)?;
            Ok((true, models, manual_models))
        })
        .await
        .map_err(internal)?;
    if !exists {
        return Err(fail(StatusCode::NOT_FOUND, "provider not found"));
    }
    Ok(Json(json!({
        "models": models,
        "manual_models": manual_models,
    })))
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProviderModelPageQuery {
    pub(crate) q: Option<String>,
    pub(crate) limit: Option<usize>,
}

pub(crate) fn encode_provider_model_cursor(index_key: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(index_key.as_bytes())
}

pub(crate) fn decode_provider_model_cursor(
    cursor: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
    if cursor.len() > 1024 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider model cursor is invalid",
        ));
    }
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "provider model cursor is invalid"))?;
    String::from_utf8(decoded)
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "provider model cursor is invalid"))
}

#[cfg(test)]
pub(crate) fn provider_models_url(base_url: &str) -> Result<reqwest::Url, String> {
    provider_adapters::provider_models_url(base_url)
}

#[cfg(test)]
pub(crate) fn parse_provider_model_page(
    value: &Value,
) -> Result<provider_adapters::ProviderModelPage, String> {
    provider_adapters::parse_provider_model_page(value)
}
