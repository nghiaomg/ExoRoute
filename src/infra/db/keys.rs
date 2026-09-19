use super::StorageError;
use sha2::Digest;

pub const REQUEST_LOG_INDEX_PREFIX: &str = "request_log_by_time/";
pub const REQUEST_LOG_DESC_INDEX_PREFIX: &str = "request_log_by_desc/";

pub fn request_log_index_key(created_at: &str, id: &str) -> Result<String, StorageError> {
    let key = format!("{REQUEST_LOG_INDEX_PREFIX}{created_at}/{id}");
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub fn request_log_desc_index_key(created_at: &str, id: &str) -> Result<String, StorageError> {
    let key = format!(
        "{REQUEST_LOG_DESC_INDEX_PREFIX}{}{}/{}",
        reverse_sort_component(created_at),
        reverse_sort_component(id),
        id
    );
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub fn provider_model_prefix(provider_id: &str) -> Result<String, StorageError> {
    let prefix = format!("provider/{}/model/", hash_component(provider_id.as_bytes()));
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

pub fn provider_model_key(provider_id: &str, model: &str) -> Result<String, StorageError> {
    if model.len() > 256 {
        return Err(StorageError::Invalid(
            "provider model exceeds the supported key length".to_owned(),
        ));
    }
    let mut identity = Vec::with_capacity(
        provider_id
            .len()
            .saturating_add(model.len())
            .saturating_add(1),
    );
    identity.extend_from_slice(provider_id.as_bytes());
    identity.push(0);
    identity.extend_from_slice(model.as_bytes());
    let key = format!(
        "{}{model}/{}",
        provider_model_prefix(provider_id)?,
        hash_component(&identity)
    );
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub fn provider_model_index_prefix(provider_id: &str) -> Result<String, StorageError> {
    let prefix = format!(
        "provider/{}/model-order/",
        hash_component(provider_id.as_bytes())
    );
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

pub fn provider_model_index_key(provider_id: &str, model: &str) -> Result<String, StorageError> {
    if model.len() > 256 {
        return Err(StorageError::Invalid(
            "provider model exceeds the supported key length".to_owned(),
        ));
    }
    let normalized = model.to_lowercase();
    let key = format!(
        "{}{normalized}\u{1}{model}",
        provider_model_index_prefix(provider_id)?
    );
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

fn hash_component(value: &[u8]) -> String {
    let digest = sha2::Sha256::digest(value);
    let mut encoded = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

pub fn provider_api_key_index_prefix(
    provider_id: &str,
    available: bool,
) -> Result<String, StorageError> {
    let kind = if available { "available" } else { "all" };
    let prefix = format!(
        "provider/{}/{kind}/",
        hash_component(provider_id.as_bytes())
    );
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

pub fn provider_api_key_index_key(
    provider_id: &str,
    key_id: &str,
    available: bool,
) -> Result<String, StorageError> {
    let key = format!(
        "{}{key_id}",
        provider_api_key_index_prefix(provider_id, available)?
    );
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub fn provider_api_key_identity_index_prefix(
    provider_id: &str,
    identity: &str,
) -> Result<String, StorageError> {
    let prefix = format!(
        "provider/{}/oauth-identity/{}/",
        hash_component(provider_id.as_bytes()),
        hash_component(identity.as_bytes())
    );
    crate::infra::storage::validate_prefix(&prefix)?;
    Ok(prefix)
}

pub fn provider_api_key_identity_index_key(
    provider_id: &str,
    identity: &str,
    key_id: &str,
) -> Result<String, StorageError> {
    let key = format!(
        "{}{key_id}",
        provider_api_key_identity_index_prefix(provider_id, identity)?
    );
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub fn provider_api_key_created_prefix(provider_id: &str) -> Result<String, StorageError> {
    let prefix = format!(
        "provider/{}/created/",
        hash_component(provider_id.as_bytes())
    );
    crate::infra::storage::validate_key(&prefix)?;
    Ok(prefix)
}

/// Builds the ordering-index key for one provider key position. The position is
/// `<created_at>/<slot>` while the key keeps its creation order, and reordering
/// stores another key's position on the record, so this single suffix is both
/// the key's creation position and its effective fallback position.
pub fn provider_api_key_order_key(
    provider_id: &str,
    position: &str,
) -> Result<String, StorageError> {
    let key = format!(
        "{}{position}",
        provider_api_key_created_prefix(provider_id)?
    );
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(super) fn request_log_desc_index_key_from_time_key(
    index_key: &str,
    id: &str,
) -> Result<String, StorageError> {
    let created_at = index_key
        .strip_prefix(REQUEST_LOG_INDEX_PREFIX)
        .and_then(|suffix| suffix.split_once('/'))
        .map(|(created_at, _)| created_at)
        .ok_or_else(|| StorageError::Invalid("request log time index is malformed".to_owned()))?;
    request_log_desc_index_key(created_at, id)
}

fn reverse_sort_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len().saturating_mul(2).saturating_add(2));
    for byte in value.bytes() {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{:02x}", 255_u8.saturating_sub(byte));
    }
    encoded.push_str("ff");
    encoded
}
