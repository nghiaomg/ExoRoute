use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table, WriteTxn};
pub(super) fn route_record(input: &ComboInput) -> Result<Record, StorageError> {
    let accepted = serde_json::to_string(&input.accepted_protocols)
        .map_err(|error| StorageError::Codec(error.to_string()))?;
    let now = crate::infra::db::utc_timestamp_now()?;
    Ok(Record::new()
        .with("id", Field::Text(input.id.clone()))
        .with("name", Field::Text(input.name.clone()))
        .with("strategy", Field::Text(input.strategy.clone()))
        .with("accepted_protocols", Field::Text(accepted))
        .with("enabled", Field::Bool(input.enabled))
        .with("created_at", Field::Text(now.clone()))
        .with("updated_at", Field::Text(now)))
}

/// One target row plus the derived index entry it maintains.
///
/// A provider target indexes by provider; a nested combo reference indexes by
/// the combo it points at. Exactly one of the two is set, and that is also how
/// the two kinds are told apart when a stored row is read back.
pub(super) struct TargetRecordWrite {
    pub(super) target_key: String,
    pub(super) provider_index: Option<String>,
    pub(super) combo_index: Option<String>,
    pub(super) record: Record,
}

pub(super) fn route_target_records(
    input: &ComboInput,
) -> Result<Vec<TargetRecordWrite>, StorageError> {
    let mut records = Vec::with_capacity(input.targets.len());
    for (ordinal, target) in input.targets.iter().enumerate() {
        let target_key = route_target_key(&input.id, target.priority, ordinal)?;
        if let Some(combo_id) = target.combo_id.as_deref() {
            // A reference target stores no provider. The provider and model
            // fields are written as explicit nulls rather than omitted so a
            // reader that only looks for `provider_id` cannot mistake the row
            // for a provider target with an empty id.
            let record = Record::new()
                .with("route_id", Field::Text(input.id.clone()))
                .with("combo_id", Field::Text(combo_id.to_owned()))
                .with("provider_id", Field::Null)
                .with("model", Field::Null)
                .with("protocol", Field::Null)
                .with("priority", Field::I64(i64::from(target.priority)))
                .with("weight", Field::Null)
                .with("enabled", Field::Bool(target.enabled));
            records.push(TargetRecordWrite {
                combo_index: Some(route_target_combo_index_key(combo_id, &target_key)?),
                provider_index: None,
                target_key,
                record,
            });
            continue;
        }
        let record = Record::new()
            .with("route_id", Field::Text(input.id.clone()))
            .with("provider_id", Field::Text(target.provider_id.clone()))
            .with("model", Field::Text(target.model.trim().to_owned()))
            .with(
                "protocol",
                target
                    .protocol
                    .clone()
                    .map(Field::Text)
                    .unwrap_or(Field::Null),
            )
            .with("priority", Field::I64(i64::from(target.priority)))
            .with(
                "weight",
                target
                    .weight
                    .map(|value| Field::I64(i64::from(value)))
                    .unwrap_or(Field::Null),
            )
            .with("enabled", Field::Bool(target.enabled));
        records.push(TargetRecordWrite {
            provider_index: Some(route_target_provider_index_key(
                &target.provider_id,
                &target_key,
            )?),
            combo_index: None,
            target_key,
            record,
        });
    }
    Ok(records)
}

pub(super) fn route_target_prefix(route_id: &str) -> Result<String, StorageError> {
    let prefix = format!("r/{}/", hex_component(route_id));
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

pub(crate) fn route_target_key(
    route_id: &str,
    priority: u32,
    ordinal: usize,
) -> Result<String, StorageError> {
    let ordinal = u32::try_from(ordinal)
        .map_err(|_| StorageError::Invalid("too many route targets".to_owned()))?;
    let key = format!("r/{}/{priority:010}/{ordinal:010}", hex_component(route_id));
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn route_name_index_key(name: &str, id: &str) -> Result<String, StorageError> {
    let key = format!("{ROUTE_NAME_INDEX_PREFIX}{name}\u{1}{id}");
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn route_target_provider_index_prefix(
    provider_id: &str,
) -> Result<String, StorageError> {
    let key = format!("p/{}/", hex_component(provider_id));
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn route_target_provider_index_key(
    provider_id: &str,
    target_key: &str,
) -> Result<String, StorageError> {
    let key = format!("p/{}/{target_key}", hex_component(provider_id));
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn route_target_combo_index_prefix(combo_id: &str) -> Result<String, StorageError> {
    let key = format!("c/{}/", hex_component(combo_id));
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn route_target_combo_index_key(
    combo_id: &str,
    target_key: &str,
) -> Result<String, StorageError> {
    let key = format!("c/{}/{target_key}", hex_component(combo_id));
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

fn hex_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len().saturating_mul(2));
    for byte in value.bytes() {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

pub(super) fn save_combo(
    transaction: &mut WriteTxn<'_, '_>,
    id: &str,
    route: &Record,
    name_index: &str,
    targets: &[TargetRecordWrite],
    update: bool,
) -> Result<(), StorageError> {
    // Check provider targets in the same write transaction that stores the
    // route. A provider deletion can begin after the async model validation
    // read, so this is the authoritative race barrier. Nested combo
    // references carry no provider; the caller validates those against the
    // reference graph in this same transaction.
    for target in targets {
        let Some(provider_id) = target.record.optional_text("provider_id")? else {
            continue;
        };
        let provider = transaction
            .get::<Record>(Table::Providers, provider_id)?
            .ok_or(StorageError::NotFound)?;
        super::providers::ensure_provider_not_deleting(&provider)?;
    }
    if update {
        let old_targets = transaction.scan_prefix::<Record>(
            Table::RouteTargets,
            &route_target_prefix(id)?,
            MAX_ROUTE_TARGETS_PAGE,
        )?;
        for (old_key, old_target) in old_targets {
            transaction.delete(Table::RouteTargets, &old_key)?;
            if let Some(provider_id) = old_target.optional_text("provider_id")? {
                transaction.delete(
                    Table::RouteTargetProviderIndex,
                    &route_target_provider_index_key(provider_id, &old_key)?,
                )?;
            }
            if let Some(combo_id) = old_target.optional_text("combo_id")? {
                transaction.delete(
                    Table::RouteTargetComboIndex,
                    &route_target_combo_index_key(combo_id, &old_key)?,
                )?;
            }
        }
    }
    transaction.put(Table::Routes, id, route)?;
    let route_id = id.to_owned();
    transaction.put(Table::RouteNameIndex, name_index, &route_id)?;
    for target in targets {
        transaction.put_if_absent(Table::RouteTargets, &target.target_key, &target.record)?;
        if let Some(provider_index) = target.provider_index.as_deref() {
            transaction.put(
                Table::RouteTargetProviderIndex,
                provider_index,
                &target.target_key,
            )?;
        }
        if let Some(combo_index) = target.combo_index.as_deref() {
            transaction.put(Table::RouteTargetComboIndex, combo_index, &route_id)?;
        }
    }
    Ok(())
}
