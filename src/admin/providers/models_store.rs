//! Bulk catalog persistence, per-model source tracking, and stale-row pruning.
//!
//! Every saved provider model records how it entered the catalog. A row
//! discovered by an import is replaced by the next authoritative import, while
//! a row the operator added from the dashboard survives every import. A row
//! written before the field existed is read as imported, so an existing
//! catalog keeps the behavior it had before the field landed.

use super::*;
use crate::infra::storage::{Field, ReadTxn, Record, StorageError, Table};

/// How a provider model row entered the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderModelSource {
    /// Discovered from the provider catalog; an authoritative import drops it
    /// when the provider stops reporting it.
    Import,
    /// Added from the dashboard; an import never drops it.
    Manual,
}

impl ProviderModelSource {
    const FIELD: &'static str = "source";

    fn as_str(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Manual => "manual",
        }
    }

    /// Reads the stored source. A row written before the field existed is
    /// imported, which keeps the previous import behavior for old catalogs.
    fn from_record(record: &Record) -> Result<Self, StorageError> {
        match record.optional_text(Self::FIELD)? {
            None | Some("import") => Ok(Self::Import),
            Some("manual") => Ok(Self::Manual),
            Some(_) => Err(StorageError::Invalid(
                "provider model has an unknown source".to_owned(),
            )),
        }
    }

    fn is_manual(self) -> bool {
        matches!(self, Self::Manual)
    }
}

/// What one store call changed, so an import can report what it kept.
pub(crate) struct ProviderModelStoreOutcome {
    /// Models kept only because the operator added them by hand.
    pub(crate) manual_kept: usize,
}

/// Merges `models` into the provider catalog with the given source.
///
/// `authoritative` marks a discovery result that owns the imported part of the
/// catalog: imported rows the provider no longer reports are deleted, while
/// manual rows are always kept. Without it the call only adds rows.
pub(crate) async fn store_provider_models(
    db: &crate::infra::storage::Database,
    provider_id: &str,
    models: &[String],
    source: ProviderModelSource,
    authoritative: bool,
) -> Result<ProviderModelStoreOutcome, (StatusCode, Json<Value>)> {
    if models.len() > MAX_PROVIDER_MODELS {
        return Err(invalid_catalog(
            "provider model catalog exceeds 10000 models",
        ));
    }
    for model in models {
        validate_stored_model_id(model)?;
    }
    let provider_id = provider_id.to_owned();
    let models = models.to_vec();
    db.write(move |transaction| {
        let mut provider = transaction
            .get::<Record>(Table::Providers, &provider_id)?
            .ok_or(StorageError::NotFound)?;
        ensure_provider_not_deleting(&provider)?;
        let index_prefix = crate::infra::db::provider_model_index_prefix(&provider_id)?;
        let rows = transaction.scan_prefix::<String>(
            Table::ProviderModelIndex,
            &index_prefix,
            MAX_PROVIDER_SCAN_ROWS.saturating_add(1),
        )?;
        if rows.len() > MAX_PROVIDER_SCAN_ROWS {
            return Err(StorageError::Busy);
        }
        let incoming = models
            .iter()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let mut stored_models = std::collections::HashSet::with_capacity(rows.len());
        let mut manual_kept = 0_usize;
        let mut count = 0_i64;
        for (index_key, model) in rows {
            let primary_key = crate::infra::db::provider_model_key(&provider_id, &model)?;
            let record = transaction
                .get::<Record>(Table::ProviderModels, &primary_key)?
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
            let stored_source = ProviderModelSource::from_record(&record)?;
            let incoming_model = incoming.contains(model.as_str());
            stored_models.insert(model.clone());
            if source.is_manual() && incoming_model && !stored_source.is_manual() {
                // Saving a discovered model by hand pins it against imports.
                let mut record = record;
                record.insert(
                    ProviderModelSource::FIELD,
                    Field::Text(source.as_str().to_owned()),
                );
                transaction.put(Table::ProviderModels, &primary_key, &record)?;
            }
            if stored_source.is_manual() {
                // A manual row only counts as kept when the import would
                // otherwise have dropped it for being absent upstream.
                if authoritative && !incoming_model {
                    manual_kept = manual_kept.saturating_add(1);
                }
                count = count.saturating_add(1);
                continue;
            }
            if !authoritative || incoming_model {
                count = count.saturating_add(1);
                continue;
            }
            transaction.delete(Table::ProviderModels, &primary_key)?;
            transaction.delete(Table::ProviderModelIndex, &index_key)?;
        }
        for model in &models {
            if stored_models.contains(model) {
                continue;
            }
            let primary_key = crate::infra::db::provider_model_key(&provider_id, model)?;
            let order_key = crate::infra::db::provider_model_index_key(&provider_id, model)?;
            let record = Record::new()
                .with("provider_id", Field::Text(provider_id.clone()))
                .with("model", Field::Text(model.clone()))
                .with("upstream_protocol", Field::Null)
                .with(
                    ProviderModelSource::FIELD,
                    Field::Text(source.as_str().to_owned()),
                );
            transaction.put(Table::ProviderModels, &primary_key, &record)?;
            transaction.put(Table::ProviderModelIndex, &order_key, model)?;
            count = count.checked_add(1).ok_or_else(|| {
                StorageError::Invalid("provider model count overflowed".to_owned())
            })?;
        }
        provider.insert("model_count", Field::I64(count));
        transaction.put(Table::Providers, &provider_id, &provider)?;
        Ok(ProviderModelStoreOutcome { manual_kept })
    })
    .await
    .map_err(internal)
}

/// Validates one model identifier exactly as the writer stores it.
fn validate_stored_model_id(model: &str) -> Result<(), (StatusCode, Json<Value>)> {
    if model.trim().is_empty() || model.len() > 256 {
        return Err(invalid_catalog(
            "provider model is empty or exceeds 256 bytes",
        ));
    }
    Ok(())
}

fn invalid_catalog(message: &'static str) -> (StatusCode, Json<Value>) {
    fail(StatusCode::BAD_REQUEST, message)
}

/// Manual models among `models`, read from their records. Callers pass at most
/// one page of models so the extra reads stay bounded by the response size.
pub(super) fn manual_model_names(
    transaction: &ReadTxn<'_, '_>,
    provider_id: &str,
    models: &[String],
) -> Result<Vec<String>, StorageError> {
    let mut manual = Vec::new();
    for model in models {
        let primary_key = crate::infra::db::provider_model_key(provider_id, model)?;
        let record = transaction
            .get::<Record>(Table::ProviderModels, &primary_key)?
            .ok_or_else(|| {
                StorageError::Invalid("provider model index points to a missing model".to_owned())
            })?;
        if ProviderModelSource::from_record(&record)?.is_manual() {
            manual.push(model.clone());
        }
    }
    Ok(manual)
}

/// Removes stored Antigravity models that the hardened discovery filter no
/// longer accepts and that no combo target references. Runs before the fresh
/// discovery rows are stored so one import also cleans catalogs polluted with
/// `MODEL_PLACEHOLDER_*`, retired, or non-chat rows. Combo-referenced rows and
/// manually added rows are left in place and reported back so the dashboard can
/// surface them instead of silently breaking routing.
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
            let record = transaction
                .get::<Record>(Table::ProviderModels, &model_key)?
                .ok_or_else(|| {
                    StorageError::Invalid(
                        "provider model index points to a missing model".to_owned(),
                    )
                })?;
            if ProviderModelSource::from_record(&record)?.is_manual() {
                continue;
            }
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
