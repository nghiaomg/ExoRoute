//! Proxy record construction, key layout, and transactional writes.
//!
//! A proxy owns a bounded list of endpoints. An endpoint either names an
//! upstream proxy server (`url` plus optional encrypted credentials) or embeds
//! another proxy by reference, which is how one proxy groups several others the
//! same way a combo nests a combo. Both kinds live in `Table::ProxyEndpoints`
//! and are told apart by which field is set; a reference row writes explicit
//! nulls for every endpoint-only field so a reader that looks for `url` alone
//! cannot mistake it for a server endpoint.

use super::*;
use crate::infra::storage::{Field, Record, StorageError, Table, WriteTxn};

/// The stored shape of one proxy row.
pub(super) fn proxy_record(input: &ProxyInput) -> Result<Record, StorageError> {
    let now = crate::infra::db::utc_timestamp_now()?;
    Ok(Record::new()
        .with("id", Field::Text(input.id.clone()))
        .with("name", Field::Text(input.name.clone()))
        .with("strategy", Field::Text(input.strategy.clone()))
        .with("enabled", Field::Bool(input.enabled))
        .with("created_at", Field::Text(now.clone()))
        .with("updated_at", Field::Text(now)))
}

/// One endpoint row plus the secondary index entry it maintains.
///
/// A server endpoint indexes nothing here (providers index it instead); a
/// reference row indexes the proxy it embeds. Exactly one of `reference_index`
/// and the provider-index entries maintained elsewhere is ever set.
pub(super) struct EndpointRecordWrite {
    pub(super) endpoint_key: String,
    pub(super) reference_index: Option<String>,
    pub(super) record: Record,
    pub(super) preserve_secret: bool,
}

/// Builds every endpoint row for one proxy.
pub(super) fn proxy_endpoint_records(
    input: &ProxyInput,
) -> Result<Vec<EndpointRecordWrite>, StorageError> {
    let mut records = Vec::with_capacity(input.endpoints.len());
    for (ordinal, endpoint) in input.endpoints.iter().enumerate() {
        let endpoint_key = proxy_endpoint_key(&input.id, endpoint.priority, ordinal)?;
        if let Some(reference) = endpoint.normalized_reference() {
            let record = Record::new()
                .with("proxy_id", Field::Text(input.id.clone()))
                .with("endpoint_id", Field::Text(endpoint.endpoint_id.clone()))
                .with("ref_proxy_id", Field::Text(reference.to_owned()))
                .with("url", Field::Null)
                .with("username", Field::Null)
                .with("secret", Field::Null)
                .with("label", Field::Null)
                .with("priority", Field::I64(i64::from(endpoint.priority)))
                .with("enabled", Field::Bool(endpoint.enabled));
            records.push(EndpointRecordWrite {
                reference_index: Some(proxy_endpoint_ref_index_key(reference, &endpoint_key)?),
                endpoint_key,
                record,
                preserve_secret: false,
            });
            continue;
        }
        // Credentials are encrypted by the caller and handed in already
        // serialized: this layer never sees plaintext.
        let record = Record::new()
            .with("proxy_id", Field::Text(input.id.clone()))
            .with("endpoint_id", Field::Text(endpoint.endpoint_id.clone()))
            .with("ref_proxy_id", Field::Null)
            .with("url", Field::Text(endpoint.normalized_url()?))
            .with(
                "username",
                endpoint
                    .username
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| Field::Text(value.to_owned()))
                    .unwrap_or(Field::Null),
            )
            .with(
                "secret",
                endpoint
                    .secret_ciphertext
                    .as_ref()
                    .map(|value| Field::Bytes(value.clone()))
                    .unwrap_or(Field::Null),
            )
            .with(
                "label",
                endpoint
                    .label
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| Field::Text(value.to_owned()))
                    .unwrap_or(Field::Null),
            )
            .with("priority", Field::I64(i64::from(endpoint.priority)))
            .with("enabled", Field::Bool(endpoint.enabled));
        records.push(EndpointRecordWrite {
            reference_index: None,
            endpoint_key,
            record,
            preserve_secret: endpoint.secret.is_none() && endpoint.has_secret,
        });
    }
    Ok(records)
}

/// `px/{hex(proxy id)}/` — every endpoint row of one proxy.
pub(crate) fn proxy_endpoint_prefix(proxy_id: &str) -> Result<String, StorageError> {
    let prefix = format!("px/{}/", hex_component(proxy_id));
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

/// `px/{hex}/{priority:010}/{ordinal:010}` — fixed width keeps lexicographic
/// order equal to priority order for prefix scans.
pub(super) fn proxy_endpoint_key(
    proxy_id: &str,
    priority: u32,
    ordinal: usize,
) -> Result<String, StorageError> {
    let ordinal = u32::try_from(ordinal)
        .map_err(|_| StorageError::Invalid("proxy endpoint ordinal is invalid".to_owned()))?;
    let key = format!("{}{priority:010}/{ordinal:010}", proxy_endpoint_prefix(proxy_id)?);
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn proxy_endpoint_key_components(
    key: &str,
    proxy_id: &str,
) -> Result<(u32, u32), &'static str> {
    if proxy_id.is_empty() || proxy_id.len() > super::validation::MAX_PROXY_ID_BYTES
        || proxy_id.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("The backup contains a proxy endpoint with an invalid proxy ID.");
    }
    let prefix = proxy_endpoint_prefix(proxy_id)
        .map_err(|_| "The backup contains an invalid proxy endpoint key.")?;
    let suffix = key
        .strip_prefix(&prefix)
        .ok_or("The backup proxy endpoint key does not match its proxy ID.")?;
    let (priority, ordinal) = suffix
        .split_once('/')
        .ok_or("The backup contains an invalid proxy endpoint key.")?;
    if ordinal.contains('/') {
        return Err("The backup contains an invalid proxy endpoint key.");
    }
    let priority_value = priority
        .parse::<u32>()
        .map_err(|_| "The backup contains an invalid proxy endpoint key.")?;
    let ordinal_value = ordinal
        .parse::<u32>()
        .map_err(|_| "The backup contains an invalid proxy endpoint key.")?;
    if priority != format!("{priority_value:010}")
        || ordinal != format!("{ordinal_value:010}")
        || ordinal_value as usize >= crate::config::MAX_PROXY_ENDPOINTS
    {
        return Err("The backup contains a non-canonical proxy endpoint key.");
    }
    Ok((priority_value, ordinal_value))
}

/// `name/{name}\u{1}{id}` — the list scan orders proxies by name.
pub(crate) fn proxy_name_index_key(name: &str, id: &str) -> Result<String, StorageError> {
    let key = format!("name/{name}\u{1}{id}");
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

/// `p/{hex}/` — the endpoints of every proxy that embeds one proxy.
pub(crate) fn proxy_endpoint_ref_index_prefix(proxy_id: &str) -> Result<String, StorageError> {
    let prefix = format!("p/{}/", hex_component(proxy_id));
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

/// `p/{hex}/{endpoint_key}` — the reverse reference used for delete blocking.
pub(crate) fn proxy_endpoint_ref_index_key(
    reference: &str,
    endpoint_key: &str,
) -> Result<String, StorageError> {
    let key = format!(
        "{}{endpoint_key}",
        proxy_endpoint_ref_index_prefix(reference)?
    );
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

/// `pr/{hex}/` — every provider bound to one proxy.
pub(crate) fn proxy_provider_index_prefix(proxy_id: &str) -> Result<String, StorageError> {
    let prefix = format!("pr/{}/", hex_component(proxy_id));
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

/// `pr/{hex(proxy id)}/{hex(provider id)}` — the reverse binding written when a
/// provider is created or updated with a proxy and removed when it is deleted.
pub(crate) fn proxy_provider_index_key(
    proxy_id: &str,
    provider_id: &str,
) -> Result<String, StorageError> {
    let key = format!(
        "{}{}",
        proxy_provider_index_prefix(proxy_id)?,
        hex_component(provider_id)
    );
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

/// Writes one proxy and replaces its endpoint rows.
///
/// `update` selects delete-then-rewrite, which is what makes priority reordering
/// and endpoint removal work without tracking individual row diffs.
pub(super) fn save_proxy(
    transaction: &mut WriteTxn<'_, '_>,
    id: &str,
    proxy: &Record,
    name_index: &str,
    endpoints: &[EndpointRecordWrite],
    update: bool,
) -> Result<(), StorageError> {
    // A concurrent proxy deletion must not be resurrected by this write.
    if let Some(current) = transaction.get::<Record>(Table::Proxies, id)? {
        super::ensure_proxy_not_deleting(&current)?;
    }
    if update {
        let old_endpoints = transaction.scan_prefix::<Record>(
            Table::ProxyEndpoints,
            &proxy_endpoint_prefix(id)?,
            MAX_PROXY_ENDPOINTS_PAGE,
        )?;
        let mut old_secrets = std::collections::HashMap::with_capacity(old_endpoints.len());
        for (old_key, old_endpoint) in old_endpoints {
            let stable_id = old_endpoint.optional_text("endpoint_id")?.map(str::to_owned);
            if let Some(secret) = old_endpoint.field("secret") {
                if let Some(stable_id) = stable_id {
                    old_secrets.insert(stable_id, secret.clone());
                }
            }
            transaction.delete(Table::ProxyEndpoints, &old_key)?;
            if let Some(reference) = old_endpoint.optional_text("ref_proxy_id")? {
                transaction.delete(
                    Table::ProxyEndpointRefIndex,
                    &proxy_endpoint_ref_index_key(reference, &old_key)?,
                )?;
            }
        }
        transaction.put(Table::Proxies, id, proxy)?;
        let proxy_id = id.to_owned();
        transaction.put(Table::ProxyNameIndex, name_index, &proxy_id)?;
        for endpoint in endpoints {
            let mut record = endpoint.record.clone();
            if endpoint.preserve_secret {
                if let Some(secret) = old_secrets.get(&endpoint.record.text("endpoint_id")?.to_owned()) {
                    record.insert("secret", secret.clone());
                }
            }
            transaction.put_if_absent(Table::ProxyEndpoints, &endpoint.endpoint_key, &record)?;
            if let Some(reference_index) = endpoint.reference_index.as_deref() {
                transaction.put(Table::ProxyEndpointRefIndex, reference_index, &proxy_id)?;
            }
        }
        return Ok(());
    }
    transaction.put(Table::Proxies, id, proxy)?;
    let proxy_id = id.to_owned();
    transaction.put(Table::ProxyNameIndex, name_index, &proxy_id)?;
    for endpoint in endpoints {
        transaction.put_if_absent(Table::ProxyEndpoints, &endpoint.endpoint_key, &endpoint.record)?;
        if let Some(reference_index) = endpoint.reference_index.as_deref() {
            transaction.put(Table::ProxyEndpointRefIndex, reference_index, &proxy_id)?;
        }
    }
    Ok(())
}

fn hex_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        encoded.push_str(&format!("{byte:02x}"));
    }
    encoded
}

#[cfg(test)]
pub(super) use tests_support::endpoint_key_for_test;

#[cfg(test)]
mod tests_support {
    use super::*;

    pub(in crate::admin::proxies) fn endpoint_key_for_test(
        proxy_id: &str,
        priority: u32,
        ordinal: usize,
    ) -> String {
        proxy_endpoint_key(proxy_id, priority, ordinal).expect("test endpoint key is valid")
    }
}
