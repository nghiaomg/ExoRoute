//! Provider-model alias resolution and OpenCode zero-config passthrough.
//!
//! Split from request preparation so the alias contract can evolve without
//! growing the request pipeline past the production file budget.

use super::request_preparation::Target;
use crate::{
    infra::storage::{ReadTxn, Record, StorageError, Table},
    protocol::{self, UpstreamProtocol},
    provider_adapters::{OPENCODE_GO_ADAPTER_ID, OPENCODE_ZEN_ADAPTER_ID},
};

pub(super) fn route_target_prefix(route_id: &str) -> Result<String, StorageError> {
    let mut encoded = String::with_capacity(route_id.len().saturating_mul(2));
    for byte in route_id.bytes() {
        use std::fmt::Write as _;
        write!(encoded, "{byte:02x}")
            .map_err(|_| StorageError::Invalid("could not encode route id".to_owned()))?;
    }
    let prefix = format!("r/{encoded}/");
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

/// OpenCode publishes models continuously; a newly released upstream id is not
/// in `ProviderModels` until discovery syncs it. Those ids are still plain
/// Chat-Completions/Responses/Messages names on the same endpoint, so route them
/// through the provider's stored adapter/base URL instead of rejecting them.
fn opencode_passthrough_target(
    provider: &Record,
    model: &str,
) -> Result<Option<Target>, StorageError> {
    let adapter_id = provider.text("adapter_id")?;
    if adapter_id != OPENCODE_GO_ADAPTER_ID && adapter_id != OPENCODE_ZEN_ADAPTER_ID {
        return Ok(None);
    }
    Ok(Some(Target {
        provider_id: provider.text("id")?.to_owned(),
        model: model.to_owned(),
        protocol: None,
        model_protocol: None,
        enabled: true,
        effort: None,
    }))
}

pub(super) enum ProviderModelAliasResolution {
    NotFound,
    Found(Target),
}

pub(super) async fn resolve_provider_model_alias(
    db: &crate::infra::storage::Database,
    public_model: &str,
) -> Result<ProviderModelAliasResolution, StorageError> {
    let Some((prefix, model)) = public_model.split_once('/') else {
        return Ok(ProviderModelAliasResolution::NotFound);
    };
    if prefix.is_empty() || model.is_empty() {
        return Ok(ProviderModelAliasResolution::NotFound);
    }
    let model_prefix = prefix.to_ascii_lowercase();
    let model_name = model.to_owned();
    db.read(move |transaction| {
        let prefix_key = format!("provider_model_prefix/{model_prefix}");
        let Some(provider_id) = transaction.get::<String>(Table::Indexes, &prefix_key)? else {
            return Ok(ProviderModelAliasResolution::NotFound);
        };
        let Some(provider) = transaction.get::<Record>(Table::Providers, &provider_id)? else {
            return Err(StorageError::Invalid(
                "provider prefix index points to a missing provider".to_owned(),
            ));
        };
        if !provider.boolean("enabled")? {
            return Ok(ProviderModelAliasResolution::NotFound);
        }
        // An exact saved-model match wins over the effort-suffix reading, so a
        // provider that genuinely names a model `...-max` keeps resolving to
        // it. Only when nothing matches does `cx/gpt-6-luna-max` fall back to
        // the base model `gpt-6-luna` plus a `max` effort hint.
        if let Some(target) = provider_model_alias_target(transaction, &provider_id, &model_name)? {
            return Ok(ProviderModelAliasResolution::Found(target));
        }
        if let Some(target) = opencode_passthrough_target(&provider, &model_name)? {
            return Ok(ProviderModelAliasResolution::Found(target));
        }
        if let Some((base_model, effort)) = protocol::split_model_effort_suffix(&model_name)
            && let Some(mut target) =
                provider_model_alias_target(transaction, &provider_id, base_model)?
        {
            target.effort = Some(effort.to_ascii_lowercase());
            return Ok(ProviderModelAliasResolution::Found(target));
        }
        Ok(ProviderModelAliasResolution::NotFound)
    })
    .await
}

/// Resolves one saved provider model name to its dispatch target, or `None`
/// when the provider has no such model.
fn provider_model_alias_target(
    transaction: &ReadTxn<'_, '_>,
    provider_id: &str,
    model_name: &str,
) -> Result<Option<Target>, StorageError> {
    let model_key = crate::infra::db::provider_model_index_key(provider_id, model_name)?;
    let Some(saved_model) = transaction.get::<String>(Table::ProviderModelIndex, &model_key)?
    else {
        return Ok(None);
    };
    if saved_model != model_name {
        return Ok(None);
    }
    let model_primary_key = crate::infra::db::provider_model_key(provider_id, model_name)?;
    let model_record = transaction
        .get::<Record>(Table::ProviderModels, &model_primary_key)?
        .ok_or_else(|| {
            StorageError::Invalid(
                "provider model index points to a missing model record".to_owned(),
            )
        })?;
    let model_protocol = model_record
        .optional_text("upstream_protocol")?
        .map(|value| {
            value.parse::<UpstreamProtocol>().map_err(|error| {
                StorageError::Invalid(format!("saved provider model protocol is invalid: {error}"))
            })
        })
        .transpose()?;
    Ok(Some(Target {
        provider_id: provider_id.to_owned(),
        model: saved_model,
        protocol: None,
        model_protocol,
        enabled: true,
        effort: None,
    }))
}
