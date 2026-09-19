//! Per-model upstream-protocol routing reads and writes.
//!
//! The routing table maps a saved model to its upstream protocol override;
//! effective protocol resolution falls back to adapter defaults.

use super::models_list::{decode_provider_model_cursor, encode_provider_model_cursor};
use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};

#[derive(Debug, Serialize)]
struct ProviderModelRoutingView {
    model: String,
    upstream_protocol: Option<String>,
    override_protocol: Option<String>,
    effective_upstream_protocol: Option<String>,
    routing_configured: bool,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProviderModelRoutingPageQuery {
    pub(crate) cursor: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderModelRoutingInput {
    pub(crate) model: String,
    pub(crate) upstream_protocol: Option<String>,
}

pub(crate) async fn list_provider_model_routing(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ProviderModelRoutingPageQuery>,
) -> ApiResult {
    let limit = query.limit.unwrap_or(DEFAULT_PROVIDER_MODEL_PAGE_SIZE);
    if !(1..=MAX_PROVIDER_MODEL_PAGE_SIZE).contains(&limit) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider model page size must be between 1 and 50",
        ));
    }
    let cursor = query
        .cursor
        .as_deref()
        .map(decode_provider_model_cursor)
        .transpose()?;
    let index_prefix = crate::infra::db::provider_model_index_prefix(&id).map_err(internal)?;
    if cursor
        .as_deref()
        .is_some_and(|key| !key.starts_with(&index_prefix))
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider model cursor does not belong to this provider",
        ));
    }

    let provider_id = id.clone();
    let prefix = index_prefix;
    let (adapter_id, rows) = state
        .db
        .read(move |transaction| {
            let Some(provider) = transaction.get::<Record>(Table::Providers, &provider_id)? else {
                return Ok((None, Vec::new()));
            };
            if provider_is_deleting(&provider)? {
                return Ok((None, Vec::new()));
            }
            let adapter_id = provider.text("adapter_id")?.to_owned();
            let after = cursor.as_deref();
            let index_rows = transaction.scan_prefix_after::<String>(
                Table::ProviderModelIndex,
                &prefix,
                after,
                limit.saturating_add(1),
            )?;
            let mut models = Vec::with_capacity(index_rows.len());
            for (index_key, model) in index_rows {
                let model_key = crate::infra::db::provider_model_key(&provider_id, &model)?;
                let record = transaction
                    .get::<Record>(Table::ProviderModels, &model_key)?
                    .ok_or_else(|| {
                        StorageError::Invalid(
                            "provider model index points to a missing model".to_owned(),
                        )
                    })?;
                if record.text("provider_id")? != provider_id || record.text("model")? != model {
                    return Err(StorageError::Invalid(
                        "provider model record does not match its index".to_owned(),
                    ));
                }
                models.push((
                    index_key,
                    model,
                    record
                        .optional_text("upstream_protocol")?
                        .map(str::to_owned),
                ));
            }
            Ok((Some(adapter_id), models))
        })
        .await
        .map_err(internal)?;
    let Some(adapter_id) = adapter_id else {
        return Err(fail(StatusCode::NOT_FOUND, "provider not found"));
    };
    let has_more = rows.len() > limit;
    let page_rows = rows.into_iter().take(limit).collect::<Vec<_>>();
    let next_cursor = if has_more {
        page_rows
            .last()
            .map(|(index_key, _, _)| encode_provider_model_cursor(index_key))
    } else {
        None
    };
    let models = page_rows
        .into_iter()
        .map(|(_, model, saved_override)| {
            let saved_protocol = saved_override
                .as_deref()
                .map(parse_upstream_protocol)
                .transpose()?;
            if saved_protocol.is_some_and(|protocol| {
                !provider_adapters::supports_upstream_protocol(&adapter_id, protocol)
            }) {
                return Err(internal(StorageError::Invalid(
                    "stored provider model protocol is not supported by its adapter".to_owned(),
                )));
            }
            let effective = saved_protocol.or_else(|| {
                provider_adapters::model_upstream_protocol(&adapter_id, &model).filter(|protocol| {
                    provider_adapters::supports_upstream_protocol(&adapter_id, *protocol)
                })
            });
            let saved_protocol = saved_protocol.map(|protocol| protocol.as_str().to_owned());
            Ok(ProviderModelRoutingView {
                model,
                upstream_protocol: saved_protocol.clone(),
                override_protocol: saved_protocol,
                effective_upstream_protocol: effective.map(|protocol| protocol.as_str().to_owned()),
                routing_configured: effective.is_some(),
            })
        })
        .collect::<Result<Vec<_>, (StatusCode, Json<Value>)>>()?;
    let supported_protocols = [
        crate::protocol::UpstreamProtocol::ChatCompletions,
        crate::protocol::UpstreamProtocol::Responses,
        crate::protocol::UpstreamProtocol::Messages,
        crate::protocol::UpstreamProtocol::GoogleGenerateContent,
    ]
    .into_iter()
    .filter(|protocol| provider_adapters::supports_upstream_protocol(&adapter_id, *protocol))
    .map(|protocol| protocol.as_str())
    .collect::<Vec<_>>();
    Ok(Json(json!({
        "models": models,
        "supported_protocols": supported_protocols,
        "next_cursor": next_cursor,
    })))
}

pub(crate) async fn update_provider_model_routing(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderModelRoutingInput>,
) -> ApiResult {
    if input.model.trim().is_empty() || input.model.len() > 256 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider model must be non-empty and no longer than 256 bytes",
        ));
    }
    let protocol = input
        .upstream_protocol
        .as_deref()
        .map(parse_upstream_protocol)
        .transpose()?;
    let provider_id = id.clone();
    let model = input.model;
    let response_model = model.clone();
    let result = state
        .db
        .write(move |transaction| {
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            let adapter_id = provider.text("adapter_id")?;
            if let Some(protocol) = protocol
                && !provider_adapters::supports_upstream_protocol(adapter_id, protocol)
            {
                return Err(StorageError::Invalid(
                    "upstream protocol is not supported by this provider adapter".to_owned(),
                ));
            }
            let model_index_key = crate::infra::db::provider_model_index_key(&provider_id, &model)?;
            match transaction.get::<String>(Table::ProviderModelIndex, &model_index_key)? {
                Some(indexed_model) if indexed_model == model => {}
                Some(_) => {
                    return Err(StorageError::Invalid(
                        "provider model index does not match its model record".to_owned(),
                    ));
                }
                None => return Err(StorageError::NotFound),
            }
            let model_key = crate::infra::db::provider_model_key(&provider_id, &model)?;
            let mut record = transaction
                .get::<Record>(Table::ProviderModels, &model_key)?
                .ok_or(StorageError::NotFound)?;
            if record.text("provider_id")? != provider_id || record.text("model")? != model {
                return Err(StorageError::Invalid(
                    "provider model record does not match its key".to_owned(),
                ));
            }
            record.insert(
                "upstream_protocol",
                protocol
                    .map(|value| Field::Text(value.as_str().to_owned()))
                    .unwrap_or(Field::Null),
            );
            transaction.put(Table::ProviderModels, &model_key, &record)?;
            Ok(())
        })
        .await;
    match result {
        Ok(()) => Ok(Json(json!({
            "model": response_model,
            "upstream_protocol": protocol.map(|value| value.as_str()),
        }))),
        Err(StorageError::NotFound) => Err(fail(
            StatusCode::NOT_FOUND,
            "provider or saved model not found",
        )),
        Err(StorageError::Invalid(message))
            if message == "upstream protocol is not supported by this provider adapter" =>
        {
            Err(fail(StatusCode::BAD_REQUEST, message))
        }
        Err(error) => Err(internal(error)),
    }
}

fn parse_upstream_protocol(
    value: &str,
) -> Result<crate::protocol::UpstreamProtocol, (StatusCode, Json<Value>)> {
    value
        .parse()
        .map_err(|_| fail(StatusCode::BAD_REQUEST, "unsupported upstream protocol"))
}
