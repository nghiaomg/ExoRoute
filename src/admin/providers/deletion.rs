use super::super::{ApiResult, fail, internal};
use super::{
    PROVIDER_DELETE_BATCH_SIZE, PROVIDER_DELETE_RECOVERY_INTERVAL, PROVIDER_DELETE_RETRY_INTERVAL,
    ProviderDeletionPhase, notify_enabled_provider_count_changed, provider_api_key_index_keys,
    provider_deletion_index_key, provider_model_prefix_value, provider_name_index_key,
    provider_prefix_index_key, validate_provider_deletion_state,
};
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::json;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderDeletionStep {
    InProgress,
    Complete,
    NotPending,
}

/// Starts deletion atomically with disabling provider routing. The provider row
/// itself is the durable job record: every cleanup batch updates its phase in
/// the same LMDB transaction as the deletions, so startup can resume safely.
pub(crate) async fn mark_provider_deleting(
    db: &crate::infra::storage::Database,
    id: String,
) -> Result<bool, StorageError> {
    db.write(move |transaction| {
        let Some(mut provider) = transaction.get::<Record>(Table::Providers, &id)? else {
            return Ok(false);
        };
        if validate_provider_deletion_state(&provider)? {
            return Ok(true);
        }

        let name_index = provider_name_index_key(provider.text("name")?, &id)?;
        let model_prefix = provider_model_prefix_value(&provider, &id)?;
        let prefix_index = provider_prefix_index_key(&model_prefix)?;
        provider.insert("enabled", Field::Bool(false));
        provider.insert("deleting", Field::Bool(true));
        provider.insert(
            "deletion_phase",
            Field::Text(ProviderDeletionPhase::RouteTargets.as_str().to_owned()),
        );
        transaction.delete(Table::ProviderNameIndex, &name_index)?;
        transaction.delete(Table::Indexes, &prefix_index)?;
        transaction.put(Table::Indexes, &provider_deletion_index_key(&id)?, &id)?;
        let count = transaction
            .get::<i64>(Table::Meta, "provider_count")?
            .ok_or_else(|| StorageError::Invalid("provider counter is missing".to_owned()))?;
        transaction.put(
            Table::Meta,
            "provider_count",
            &count.saturating_sub(1).max(0),
        )?;
        transaction.put(Table::Providers, &id, &provider)?;
        Ok(true)
    })
    .await
}

/// Removes at most one fixed-size page from one table. Rows are removed from
/// the front of the provider-scoped prefix, so the durable phase is sufficient
/// to resume: the next transaction starts at the first remaining row. This
/// avoids persisting a cursor that could skip rows after backup/import restore.
pub(crate) async fn provider_deletion_step(
    db: &crate::infra::storage::Database,
    id: String,
) -> Result<ProviderDeletionStep, StorageError> {
    db.write(move |transaction| {
        let Some(mut provider) = transaction.get::<Record>(Table::Providers, &id)? else {
            transaction.delete(Table::Indexes, &provider_deletion_index_key(&id)?)?;
            return Ok(ProviderDeletionStep::Complete);
        };
        if !validate_provider_deletion_state(&provider)? {
            transaction.delete(Table::Indexes, &provider_deletion_index_key(&id)?)?;
            return Ok(ProviderDeletionStep::NotPending);
        }
        let phase = ProviderDeletionPhase::parse(provider.text("deletion_phase")?)?;
        let row_limit = PROVIDER_DELETE_BATCH_SIZE.saturating_add(1);
        let transition = match phase {
            ProviderDeletionPhase::ApiKeys => {
                let prefix = crate::infra::db::provider_api_key_index_prefix(&id, false)?;
                let rows = transaction.scan_prefix::<String>(
                    Table::ProviderApiKeyIndex,
                    &prefix,
                    row_limit,
                )?;
                let exhausted = rows.len() <= PROVIDER_DELETE_BATCH_SIZE;
                let mut deleted_count = 0_i64;
                let mut deleted_invalid_count = 0_i64;
                for (index, key_id) in rows.into_iter().take(PROVIDER_DELETE_BATCH_SIZE) {
                    if let Some(key) = transaction.get::<Record>(Table::ProviderApiKeys, &key_id)?
                        && key.text("id")? == key_id
                        && key.text("provider_id")? == id
                    {
                        let (_, created_index, _) = provider_api_key_index_keys(&key)?;
                        transaction
                            .delete_prefix(Table::ProviderUsageMeters, &format!("{key_id}/"))?;
                        transaction.delete(Table::ProviderApiKeys, &key_id)?;
                        transaction.delete(Table::ProviderApiKeyCreatedIndex, &created_index)?;
                        if let Some(identity_index) =
                            crate::admin::providers::provider_api_key_identity_index_key(&key)?
                        {
                            transaction.delete(Table::ProviderApiKeyIndex, &identity_index)?;
                        }
                        if key.boolean("invalid")? {
                            deleted_invalid_count = deleted_invalid_count.saturating_add(1);
                        }
                        deleted_count = deleted_count.saturating_add(1);
                    }
                    let available =
                        crate::infra::db::provider_api_key_index_key(&id, &key_id, true)?;
                    transaction.delete(Table::ProviderApiKeyAvailabilityIndex, &available)?;
                    transaction.delete(Table::ProviderApiKeyIndex, &index)?;
                }
                provider.insert(
                    "api_key_count",
                    Field::I64(
                        provider
                            .integer("api_key_count")?
                            .saturating_sub(deleted_count)
                            .max(0),
                    ),
                );
                provider.insert(
                    "invalid_api_key_count",
                    Field::I64(
                        provider
                            .integer("invalid_api_key_count")?
                            .saturating_sub(deleted_invalid_count)
                            .max(0),
                    ),
                );
                exhausted
            }
            ProviderDeletionPhase::ApiKeyAvailability => {
                let prefix = crate::infra::db::provider_api_key_index_prefix(&id, true)?;
                let rows = transaction.scan_prefix::<String>(
                    Table::ProviderApiKeyAvailabilityIndex,
                    &prefix,
                    row_limit,
                )?;
                let exhausted = rows.len() <= PROVIDER_DELETE_BATCH_SIZE;
                for (index, _) in rows.into_iter().take(PROVIDER_DELETE_BATCH_SIZE) {
                    transaction.delete(Table::ProviderApiKeyAvailabilityIndex, &index)?;
                }
                exhausted
            }
            ProviderDeletionPhase::ApiKeyCreated => {
                let prefix = crate::infra::db::provider_api_key_created_prefix(&id)?;
                let rows = transaction.scan_prefix::<String>(
                    Table::ProviderApiKeyCreatedIndex,
                    &prefix,
                    row_limit,
                )?;
                let exhausted = rows.len() <= PROVIDER_DELETE_BATCH_SIZE;
                for (index, _) in rows.into_iter().take(PROVIDER_DELETE_BATCH_SIZE) {
                    transaction.delete(Table::ProviderApiKeyCreatedIndex, &index)?;
                }
                exhausted
            }
            ProviderDeletionPhase::Models => {
                let prefix = crate::infra::db::provider_model_prefix(&id)?;
                let rows =
                    transaction.scan_prefix::<Record>(Table::ProviderModels, &prefix, row_limit)?;
                let exhausted = rows.len() <= PROVIDER_DELETE_BATCH_SIZE;
                for (key, model_record) in rows.into_iter().take(PROVIDER_DELETE_BATCH_SIZE) {
                    if model_record.text("provider_id")? != id {
                        return Err(StorageError::Invalid(
                            "provider model belongs to a different provider".to_owned(),
                        ));
                    }
                    let model = model_record.text("model")?;
                    let model_index = crate::infra::db::provider_model_index_key(&id, model)?;
                    transaction.delete(Table::ProviderModels, &key)?;
                    transaction.delete(Table::ProviderModelIndex, &model_index)?;
                }
                exhausted
            }
            ProviderDeletionPhase::ModelIndexes => {
                let prefix = crate::infra::db::provider_model_index_prefix(&id)?;
                let rows = transaction.scan_prefix::<String>(
                    Table::ProviderModelIndex,
                    &prefix,
                    row_limit,
                )?;
                let exhausted = rows.len() <= PROVIDER_DELETE_BATCH_SIZE;
                for (index, _) in rows.into_iter().take(PROVIDER_DELETE_BATCH_SIZE) {
                    transaction.delete(Table::ProviderModelIndex, &index)?;
                }
                exhausted
            }
            ProviderDeletionPhase::RouteTargets => {
                let prefix = super::super::combos::route_target_provider_index_prefix(&id)?;
                let rows = transaction.scan_prefix::<String>(
                    Table::RouteTargetProviderIndex,
                    &prefix,
                    row_limit,
                )?;
                let exhausted = rows.len() <= PROVIDER_DELETE_BATCH_SIZE;
                for (index, target_key) in rows.into_iter().take(PROVIDER_DELETE_BATCH_SIZE) {
                    if let Some(target) =
                        transaction.get::<Record>(Table::RouteTargets, &target_key)?
                        && target.text("provider_id")? == id
                    {
                        transaction.delete(Table::RouteTargets, &target_key)?;
                    }
                    transaction.delete(Table::RouteTargetProviderIndex, &index)?;
                }
                exhausted
            }
            ProviderDeletionPhase::Finish => {
                let name_index = provider_name_index_key(provider.text("name")?, &id)?;
                let model_prefix = provider_model_prefix_value(&provider, &id)?;
                let prefix_index = provider_prefix_index_key(&model_prefix)?;
                transaction.delete(Table::ProviderNameIndex, &name_index)?;
                transaction.delete(Table::Indexes, &prefix_index)?;
                transaction.delete(Table::Indexes, &provider_deletion_index_key(&id)?)?;
                transaction.delete(Table::Providers, &id)?;
                return Ok(ProviderDeletionStep::Complete);
            }
        };

        if transition {
            let next = phase.next();
            provider.insert("deletion_phase", Field::Text(next.as_str().to_owned()));
            if next == ProviderDeletionPhase::Finish {
                // The counters are advisory and may have been stale before a
                // deletion began. All authoritative child/index phases are now
                // drained, so the final record must report no children.
                provider.insert("api_key_count", Field::I64(0));
                provider.insert("invalid_api_key_count", Field::I64(0));
                provider.insert("model_count", Field::I64(0));
            }
        }
        transaction.put(Table::Providers, &id, &provider)?;
        Ok(ProviderDeletionStep::InProgress)
    })
    .await
}

pub(crate) async fn next_provider_deletion(
    db: &crate::infra::storage::Database,
) -> Result<Option<String>, StorageError> {
    const PENDING_DELETION_PREFIX: &str = "provider_deleting/";
    let rows = db
        .read(move |transaction| {
            transaction.scan_prefix::<String>(Table::Indexes, PENDING_DELETION_PREFIX, 1)
        })
        .await?;
    Ok(rows.into_iter().next().map(|(_, id)| id))
}

/// Recovers persisted provider deletions after startup and periodically checks
/// for tombstones restored from a backup. The single worker keeps background
/// work bounded; the synchronous admin handler may also advance its own job,
/// but every step re-reads phase and children under the serialized LMDB writer.
pub(crate) async fn provider_deletion_recovery_worker(state: AppState) {
    let mut shutdown = state.shutdown_receiver();
    loop {
        let mut retry = false;
        loop {
            if *shutdown.borrow() {
                return;
            }
            let pending = match next_provider_deletion(&state.db).await {
                Ok(pending) => pending,
                Err(error) => {
                    tracing::warn!(%error, "could not scan pending provider deletions; will retry");
                    retry = true;
                    break;
                }
            };
            let Some(id) = pending else {
                break;
            };
            match provider_deletion_step(&state.db, id.clone()).await {
                Ok(ProviderDeletionStep::Complete) => {
                    state.clear_provider_runtime_state(&id).await;
                    tokio::task::yield_now().await;
                }
                Ok(ProviderDeletionStep::InProgress) => tokio::task::yield_now().await,
                Ok(ProviderDeletionStep::NotPending) => tokio::task::yield_now().await,
                Err(error) => {
                    tracing::warn!(%error, provider_id = %id, "provider deletion batch failed; will retry");
                    retry = true;
                    break;
                }
            }
        }
        let delay = if retry {
            PROVIDER_DELETE_RETRY_INTERVAL
        } else {
            PROVIDER_DELETE_RECOVERY_INTERVAL
        };
        tokio::select! {
            _ = shutdown.changed() => return,
            _ = state.provider_deletion_notify.notified() => {},
            _ = tokio::time::sleep(delay) => {},
        }
    }
}

pub(crate) async fn delete_provider(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult {
    let exists = mark_provider_deleting(&state.db, id.clone())
        .await
        .map_err(internal)?;
    if !exists {
        return Err(fail(StatusCode::NOT_FOUND, "provider not found"));
    }
    state.invalidate_provider_usage_cache().await;
    state.provider_deletion_notify.notify_one();
    notify_enabled_provider_count_changed(&state).await;

    // Keep the existing synchronous endpoint contract. If this handler is
    // cancelled, its current LMDB transaction remains atomic and the detached
    // recovery worker resumes the persisted tombstone.
    while let ProviderDeletionStep::InProgress = provider_deletion_step(&state.db, id.clone())
        .await
        .map_err(internal)?
    {
        tokio::task::yield_now().await;
    }
    state.clear_provider_runtime_state(&id).await;
    Ok(Json(json!({"ok":true})))
}
