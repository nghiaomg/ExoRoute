use super::ensure_provider_not_deleting;
use crate::{
    infra::db,
    infra::storage::{Field, Record, StorageError, Table, WriteTxn},
};

pub(crate) struct ProviderApiKeyRecordData<'a> {
    pub id: &'a str,
    pub provider_id: &'a str,
    pub name: &'a str,
    pub secret: &'a [u8],
    pub last_error: Option<&'a str>,
    pub last_test_passed: bool,
    pub last_test_status: Option<i64>,
    pub tested_at: &'a str,
    pub created_at: &'a str,
}

pub(crate) fn provider_api_key_record(data: ProviderApiKeyRecordData<'_>) -> Record {
    let ProviderApiKeyRecordData {
        id,
        provider_id,
        name,
        secret,
        last_error,
        last_test_passed,
        last_test_status,
        tested_at,
        created_at,
    } = data;
    Record::new()
        .with("id", Field::Text(id.to_owned()))
        .with("provider_id", Field::Text(provider_id.to_owned()))
        .with("credential_type", Field::Text("api_key".to_owned()))
        .with("name", Field::Text(name.to_owned()))
        .with("secret", Field::Bytes(secret.to_vec()))
        .with("usage_budget_5h_micros", Field::Null)
        .with("usage_budget_7d_micros", Field::Null)
        .with("usage_budget_30d_micros", Field::Null)
        .with("enabled", Field::Bool(true))
        .with("invalid", Field::Bool(false))
        .with(
            "last_error",
            last_error
                .map(|value| Field::Text(value.to_owned()))
                .unwrap_or(Field::Null),
        )
        .with("last_test_passed", Field::Bool(last_test_passed))
        .with(
            "last_test_status",
            last_test_status.map(Field::I64).unwrap_or(Field::Null),
        )
        .with("last_tested_at", Field::Text(tested_at.to_owned()))
        .with("last_used_at", Field::Null)
        .with("created_at", Field::Text(created_at.to_owned()))
}

/// The key's effective position in its provider's fallback order. A key keeps
/// `<created_at>/<key_id>` until an operator reorders it; reordering stores the
/// swapped position in `order_position` so the ordering index, the dashboard
/// pages, and backup validation all agree.
pub(crate) fn provider_api_key_order_position(record: &Record) -> Result<String, StorageError> {
    if let Some(position) = record.optional_text("order_position")? {
        return Ok(position.to_owned());
    }
    Ok(format!(
        "{}/{}",
        record.text("created_at")?,
        record.text("id")?
    ))
}

pub(crate) fn provider_api_key_index_keys(
    record: &Record,
) -> Result<(String, String, String), StorageError> {
    let id = record.text("id")?;
    let provider_id = record.text("provider_id")?;
    Ok((
        db::provider_api_key_index_key(provider_id, id, false)?,
        db::provider_api_key_order_key(provider_id, &provider_api_key_order_position(record)?)?,
        db::provider_api_key_index_key(
            provider_id,
            id,
            record.boolean("enabled")? && !record.boolean("invalid")?,
        )?,
    ))
}

pub(crate) fn provider_api_key_identity_index_key(
    record: &Record,
) -> Result<Option<String>, StorageError> {
    if record.optional_text("credential_type")? != Some("oauth") {
        return Ok(None);
    }
    let Some(identity) = record.optional_text("oauth_account_identity")? else {
        return Ok(None);
    };
    Ok(Some(db::provider_api_key_identity_index_key(
        record.text("provider_id")?,
        identity,
        record.text("id")?,
    )?))
}

pub(crate) fn store_provider_api_key(
    transaction: &mut WriteTxn<'_, '_>,
    record: &Record,
) -> Result<(), StorageError> {
    let id = record.text("id")?;
    let provider_id = record.text("provider_id")?;
    let provider = transaction
        .get::<Record>(Table::Providers, provider_id)?
        .ok_or(StorageError::NotFound)?;
    ensure_provider_not_deleting(&provider)?;
    let (all_index, created_index, _) = provider_api_key_index_keys(record)?;
    transaction.put_if_absent(Table::ProviderApiKeys, id, record)?;
    transaction.put_if_absent(Table::ProviderApiKeyIndex, &all_index, &id.to_owned())?;
    transaction.put_if_absent(
        Table::ProviderApiKeyCreatedIndex,
        &created_index,
        &id.to_owned(),
    )?;
    if record.boolean("enabled")? && !record.boolean("invalid")? {
        let available_index = db::provider_api_key_index_key(provider_id, id, true)?;
        transaction.put_if_absent(
            Table::ProviderApiKeyAvailabilityIndex,
            &available_index,
            &id.to_owned(),
        )?;
    }
    if let Some(identity_index) = provider_api_key_identity_index_key(record)? {
        transaction.put_if_absent(Table::ProviderApiKeyIndex, &identity_index, &id.to_owned())?;
    }
    Ok(())
}
