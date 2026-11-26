use super::validation::normalized_provider_model_prefix_for_adapter;
use crate::{
    infra::storage::{Record, StorageError},
    provider_adapters,
};
use sha2::Digest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderDeletionPhase {
    ApiKeys,
    ApiKeyAvailability,
    ApiKeyCreated,
    Models,
    ModelIndexes,
    RouteTargets,
    Finish,
}

impl ProviderDeletionPhase {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ApiKeys => "api_keys",
            Self::ApiKeyAvailability => "api_key_availability",
            Self::ApiKeyCreated => "api_key_created",
            Self::Models => "models",
            Self::ModelIndexes => "model_indexes",
            Self::RouteTargets => "route_targets",
            Self::Finish => "finish",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, StorageError> {
        match value {
            "api_keys" => Ok(Self::ApiKeys),
            "api_key_availability" => Ok(Self::ApiKeyAvailability),
            "api_key_created" => Ok(Self::ApiKeyCreated),
            "models" => Ok(Self::Models),
            "model_indexes" => Ok(Self::ModelIndexes),
            "route_targets" => Ok(Self::RouteTargets),
            "finish" => Ok(Self::Finish),
            _ => Err(StorageError::Invalid(
                "provider deletion phase is malformed".to_owned(),
            )),
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::RouteTargets => Self::ApiKeys,
            Self::ApiKeys => Self::ApiKeyAvailability,
            Self::ApiKeyAvailability => Self::ApiKeyCreated,
            Self::ApiKeyCreated => Self::Models,
            Self::Models => Self::ModelIndexes,
            Self::ModelIndexes => Self::Finish,
            Self::Finish => Self::Finish,
        }
    }
}

pub(crate) fn provider_is_deleting(provider: &Record) -> Result<bool, StorageError> {
    match provider.field("deleting") {
        None | Some(crate::infra::storage::Field::Null) => Ok(false),
        Some(crate::infra::storage::Field::Bool(value)) => Ok(*value),
        _ => Err(StorageError::Invalid(
            "provider deletion state is malformed".to_owned(),
        )),
    }
}

pub(crate) fn ensure_provider_not_deleting(provider: &Record) -> Result<(), StorageError> {
    if provider_is_deleting(provider)? {
        return Err(StorageError::Conflict);
    }
    Ok(())
}

pub(crate) fn validate_provider_deletion_state(provider: &Record) -> Result<bool, StorageError> {
    let deleting = provider_is_deleting(provider)?;
    if deleting {
        ProviderDeletionPhase::parse(provider.text("deletion_phase")?)?;
        if provider.boolean("enabled")? {
            return Err(StorageError::Invalid(
                "provider marked for deletion is still enabled".to_owned(),
            ));
        }
    } else if !matches!(
        provider.field("deletion_phase"),
        None | Some(crate::infra::storage::Field::Null)
    ) {
        return Err(StorageError::Invalid(
            "provider deletion phase exists without a deletion tombstone".to_owned(),
        ));
    }
    Ok(deleting)
}

pub(crate) fn provider_name_index_key(name: &str, id: &str) -> Result<String, StorageError> {
    let digest = sha2::Sha256::digest(id.as_bytes());
    let mut id_hash = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(id_hash, "{byte:02x}");
    }
    let key = format!("{}/{id_hash}", name.to_lowercase());
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn provider_prefix_index_key(model_prefix: &str) -> Result<String, StorageError> {
    let key = format!("provider_model_prefix/{model_prefix}");
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn provider_deletion_index_key(provider_id: &str) -> Result<String, StorageError> {
    let digest = sha2::Sha256::digest(provider_id.as_bytes());
    let mut encoded = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    let key = format!("provider_deleting/{encoded}");
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(crate) fn provider_model_prefix_value(
    record: &Record,
    id: &str,
) -> Result<String, StorageError> {
    match record.optional_text("model_prefix")? {
        Some(value) if !value.trim().is_empty() => Ok(value.trim().to_ascii_lowercase()),
        _ => {
            let adapter_id = record
                .optional_text("adapter_id")?
                .unwrap_or(provider_adapters::GENERIC_ADAPTER_ID);
            normalized_provider_model_prefix_for_adapter(None, id, adapter_id)
                .map_err(|message| StorageError::Invalid(message.to_owned()))
        }
    }
}
