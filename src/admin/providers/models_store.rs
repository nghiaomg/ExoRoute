//! Bulk catalog persistence and stale-row pruning.
//!
//! `store_provider_models` merges discovery results into the provider/model
//! tables; `prune_stale_models` removes Antigravity rows the hardened
//! discovery filter no longer accepts unless a combo still references them.

use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table};

pub(crate) async fn store_provider_models(
    db: &crate::infra::storage::Database,
    provider_id: &str,
    models: &[String],
    authoritative: bool,
) -> Result<(), (StatusCode, Json<Value>)> {
    let provider_id = provider_id.to_owned();
    let models = models.to_vec();
    db.write(move |transaction| {
        if models.len() > MAX_PROVIDER_MODELS {
            return Err(StorageError::Invalid(
                "provider model catalog exceeds 10000 models".to_owned(),
            ));
        }
        let mut provider = transaction
            .get::<Record>(Table::Providers, &provider_id)?
            .ok_or(StorageError::NotFound)?;
        ensure_provider_not_deleting(&provider)?;
        let primary_prefix = crate::infra::db::provider_model_prefix(&provider_id)?;
        let order_prefix = crate::infra::db::provider_model_index_prefix(&provider_id)?;
        let mut saved_protocols = std::collections::HashMap::new();
        if authoritative {
            for model in &models {
                if model.trim().is_empty() || model.len() > 256 {
                    return Err(StorageError::Invalid(
                        "provider model is empty or exceeds 256 bytes".to_owned(),
                    ));
                }
                let key = crate::infra::db::provider_model_key(&provider_id, model)?;
                if let Some(record) = transaction.get::<Record>(Table::ProviderModels, &key)? {
                    if record.text("provider_id")? != provider_id || record.text("model")? != model
                    {
                        return Err(StorageError::Invalid(
                            "provider model record does not match its key".to_owned(),
                        ));
                    }
                    if let Some(protocol) = record.optional_text("upstream_protocol")? {
                        saved_protocols.insert(model.clone(), protocol.to_owned());
                    }
                }
            }
            transaction.delete_prefix(Table::ProviderModels, &primary_prefix)?;
            transaction.delete_prefix(Table::ProviderModelIndex, &order_prefix)?;
        }
        let mut count = if authoritative {
            0
        } else {
            provider.integer("model_count")?
        };
        for model in &models {
            if model.trim().is_empty() || model.len() > 256 {
                return Err(StorageError::Invalid(
                    "provider model is empty or exceeds 256 bytes".to_owned(),
                ));
            }
            let order_key = crate::infra::db::provider_model_index_key(&provider_id, model)?;
            if transaction
                .get::<String>(Table::ProviderModelIndex, &order_key)?
                .is_some()
            {
                continue;
            }
            let primary_key = crate::infra::db::provider_model_key(&provider_id, model)?;
            let mut record = Record::new()
                .with("provider_id", Field::Text(provider_id.clone()))
                .with("model", Field::Text(model.clone()))
                .with("upstream_protocol", Field::Null);
            if let Some(protocol) = saved_protocols.get(model) {
                record.insert("upstream_protocol", Field::Text(protocol.clone()));
            }
            transaction.put_if_absent(Table::ProviderModels, &primary_key, &record)?;
            transaction.put_if_absent(Table::ProviderModelIndex, &order_key, &model)?;
            count = count.checked_add(1).ok_or_else(|| {
                StorageError::Invalid("provider model count overflowed".to_owned())
            })?;
        }
        provider.insert("model_count", Field::I64(count));
        transaction.put(Table::Providers, &provider_id, &provider)
    })
    .await
    .map_err(internal)
}

/// Removes stored Antigravity models that the hardened discovery filter no
/// longer accepts and that no combo target references. Runs before the fresh
/// discovery rows are stored so one import also cleans catalogs polluted with
/// `MODEL_PLACEHOLDER_*`, retired, or non-chat rows. Combo-referenced rows are
/// left in place and reported back so the dashboard can surface them instead
/// of silently breaking routing.
pub(super) async fn prune_stale_models(
    db: &crate::infra::storage::Database,
    provider_id: &str,
    fresh_models: &[String],
) -> Result<Vec<String>, (StatusCode, Json<Value>)> {
    let provider_id = provider_id.to_owned();
    let fresh = fresh_models.to_vec();
    db.write(move |transaction| {
        let mut provider = transaction
            .get::<Record>(Table::Providers, &provider_id)?
            .ok_or(StorageError::NotFound)?;
        ensure_provider_not_deleting(&provider)?;
        let adapter_id = provider.text("adapter_id")?.to_owned();
        if adapter_id != crate::provider_adapters::ANTIGRAVITY_ADAPTER_ID {
            return Err(StorageError::Invalid(
                "stale model pruning is only supported for Antigravity".to_owned(),
            ));
        }
        let fresh: std::collections::HashSet<&str> = fresh.iter().map(String::as_str).collect();
        let target_prefix = crate::admin::combos::route_target_provider_index_prefix(&provider_id)?;
        let target_rows = transaction.scan_prefix::<String>(
            Table::RouteTargetProviderIndex,
            &target_prefix,
            MAX_PROVIDER_SCAN_ROWS.saturating_add(1),
        )?;
        if target_rows.len() > MAX_PROVIDER_SCAN_ROWS {
            return Err(StorageError::Busy);
        }
        let mut referenced = std::collections::HashSet::new();
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
            referenced.insert(target.text("model")?.to_owned());
        }
        let order_prefix = crate::infra::db::provider_model_index_prefix(&provider_id)?;
        let stored = transaction.scan_prefix::<String>(
            Table::ProviderModelIndex,
            &order_prefix,
            MAX_PROVIDER_SCAN_ROWS.saturating_add(1),
        )?;
        if stored.len() > MAX_PROVIDER_SCAN_ROWS {
            return Err(StorageError::Busy);
        }
        let mut pruned = Vec::new();
        let mut removed = 0_i64;
        for (_, model) in stored {
            if fresh.contains(model.as_str())
                || referenced.contains(&model)
                || !crate::provider_adapters::auth::antigravity::is_stale_unsupported_model(&model)
            {
                continue;
            }
            let model_key = crate::infra::db::provider_model_key(&provider_id, &model)?;
            let model_index_key = crate::infra::db::provider_model_index_key(&provider_id, &model)?;
            transaction.delete(Table::ProviderModels, &model_key)?;
            transaction.delete(Table::ProviderModelIndex, &model_index_key)?;
            removed = removed.saturating_add(1);
            pruned.push(model);
        }
        pruned.sort();
        provider.insert(
            "model_count",
            Field::I64(
                provider
                    .integer("model_count")?
                    .saturating_sub(removed)
                    .max(0),
            ),
        );
        transaction.put(Table::Providers, &provider_id, &provider)?;
        Ok(pruned)
    })
    .await
    .map_err(internal)
}
