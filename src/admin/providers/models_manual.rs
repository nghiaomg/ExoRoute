//! Manually curated provider models: validation, insert, and guarded delete.
//!
//! A model referenced by a combo target cannot be deleted here; the caller
//! must remove the combo target first so routing never dangles.

use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManualProviderModelInput {
    #[serde(default)]
    pub(crate) model: String,
}

pub(crate) fn validate_manual_provider_model_id(model: &str) -> Result<String, &'static str> {
    let model = model.trim();
    if model.is_empty() {
        return Err("provider model ID is required");
    }
    if model.len() > MAX_PROVIDER_MODEL_SEARCH_BYTES {
        return Err("provider model IDs may contain at most 256 bytes");
    }
    if model.chars().any(char::is_control) {
        return Err("provider model IDs cannot contain control characters");
    }
    Ok(model.to_owned())
}

pub(crate) async fn add_manual_provider_model(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ManualProviderModelInput>,
) -> ApiResult {
    let model = validate_manual_provider_model_id(&input.model)
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    let provider_id = id.clone();
    state
        .db
        .read(move |transaction| {
            let provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;
            Ok(())
        })
        .await
        .map_err(|error| match error {
            StorageError::NotFound => fail(StatusCode::NOT_FOUND, "provider not found"),
            StorageError::Conflict => fail(StatusCode::CONFLICT, "provider is being deleted"),
            other => internal(other),
        })?;
    super::models_store::store_provider_models(&state.db, &id, std::slice::from_ref(&model), false)
        .await?;
    Ok(Json(json!({"model": model, "saved": true})))
}

pub(crate) async fn delete_provider_model(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ManualProviderModelInput>,
) -> ApiResult {
    let model = validate_manual_provider_model_id(&input.model)
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    let provider_id = id.clone();
    let response_model = model.clone();
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            ensure_provider_not_deleting(&provider)?;

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
            let model_record = transaction
                .get::<Record>(Table::ProviderModels, &model_key)?
                .ok_or(StorageError::NotFound)?;
            if model_record.text("provider_id")? != provider_id
                || model_record.text("model")? != model
            {
                return Err(StorageError::Invalid(
                    "provider model record does not match its key".to_owned(),
                ));
            }

            let target_prefix = crate::admin::combos::route_target_provider_index_prefix(
                &provider_id,
            )?;
            let target_rows = transaction.scan_prefix::<String>(
                Table::RouteTargetProviderIndex,
                &target_prefix,
                MAX_PROVIDER_SCAN_ROWS.saturating_add(1),
            )?;
            if target_rows.len() > MAX_PROVIDER_SCAN_ROWS {
                return Err(StorageError::Busy);
            }
            for (_, target_key) in target_rows {
                let target = transaction
                    .get::<Record>(Table::RouteTargets, &target_key)?
                    .ok_or_else(|| {
                        StorageError::Invalid(
                            "provider route-target index points to a missing target".to_owned(),
                        )
                    })?;
                if target.text("provider_id")? != provider_id {
                    return Err(StorageError::Invalid(
                        "provider route-target index points to a different provider".to_owned(),
                    ));
                }
                if target.text("model")? == model {
                    return Err(StorageError::Invalid(
                        "provider model is used by a combo; remove its combo target before deleting it".to_owned(),
                    ));
                }
            }

            transaction.delete(Table::ProviderModels, &model_key)?;
            transaction.delete(Table::ProviderModelIndex, &model_index_key)?;
            provider.insert(
                "model_count",
                Field::I64(provider.integer("model_count")?.saturating_sub(1).max(0)),
            );
            transaction.put(Table::Providers, &provider_id, &provider)?;
            Ok(())
        })
        .await
        .map_err(|error| match error {
            StorageError::NotFound => fail(
                StatusCode::NOT_FOUND,
                "provider or saved model not found",
            ),
            StorageError::Conflict => fail(StatusCode::CONFLICT, "provider is being deleted"),
            StorageError::Invalid(message)
                if message
                    == "provider model is used by a combo; remove its combo target before deleting it" =>
            {
                fail(StatusCode::CONFLICT, message)
            }
            other => internal(other),
        })?;
    Ok(Json(json!({"ok": true, "model": response_model})))
}
