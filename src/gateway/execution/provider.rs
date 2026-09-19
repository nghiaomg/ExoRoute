use super::super::PROVIDER_KEY_LAST_USED_UPDATES;
use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    protocol::{self, UpstreamProtocol},
    provider_adapters::keys::ProviderCredentialType,
    provider_adapters::{self, AdapterSseError},
    security::decrypt_secret,
    state::AppState,
};
use serde_json::Value;
use std::collections::BTreeMap;

use super::super::streaming::UpstreamChunkStream;

pub(super) struct Provider {
    pub(super) id: String,
    pub(super) adapter_id: String,
    pub(super) base_url: String,
    pub(super) enabled: bool,
    pub(super) auth_type: String,
    pub(super) auth_header: Option<String>,
    pub(super) custom_headers: BTreeMap<String, String>,
    pub(super) secret: Option<String>,
    pub(super) preferred_protocol: UpstreamProtocol,
    pub(super) supported_protocols: Vec<String>,
    pub(super) thinking_mode: String,
    pub(super) thinking_override: Option<String>,
    pub(super) key_strategy: String,
}

#[derive(Clone)]
pub(super) struct ProviderCredential {
    pub(super) id: Option<String>,
    pub(super) secret: Option<String>,
    pub(super) encrypted_secret: Option<Vec<u8>>,
    pub(super) credential_type: Option<ProviderCredentialType>,
}

pub(super) enum ProviderUpstreamResult {
    Response(reqwest::Response),
    Stream { chunks: UpstreamChunkStream },
    AdapterResponse { value: Value },
    AdapterSseFailure { error: AdapterSseError },
}

pub(super) async fn mark_provider_key_used(state: &AppState, credential: &ProviderCredential) {
    if let Some(key_id) = credential
        .id
        .as_deref()
        .filter(|key_id| PROVIDER_KEY_LAST_USED_UPDATES.check(key_id).allowed)
    {
        let id = key_id.to_owned();
        let timestamp = match crate::infra::db::utc_timestamp_now() {
            Ok(timestamp) => timestamp,
            Err(error) => {
                tracing::debug!(%error, "could not timestamp provider API key usage");
                return;
            }
        };
        if let Err(error) = state
            .db
            .write(move |transaction| {
                let Some(mut record) = transaction.get::<Record>(Table::ProviderApiKeys, &id)?
                else {
                    return Ok(());
                };
                let provider_id = record.text("provider_id")?;
                let Some(provider) = transaction.get::<Record>(Table::Providers, provider_id)?
                else {
                    return Ok(());
                };
                if crate::admin::providers::provider_is_deleting(&provider)? {
                    return Ok(());
                }
                record.insert("last_used_at", Field::Text(timestamp.clone()));
                transaction.put(Table::ProviderApiKeys, &id, &record)
            })
            .await
        {
            tracing::debug!(%error, provider_key_id = %key_id, "could not persist provider API key usage timestamp");
        }
    }
}

pub(super) async fn mark_provider_key_invalid(
    state: &AppState,
    provider_id: &str,
    key_id: &str,
) -> Result<(), StorageError> {
    let provider_id = provider_id.to_owned();
    let key_id = key_id.to_owned();
    let timestamp = crate::infra::db::utc_timestamp_now()?;
    let last_error = "provider API key received HTTP 401".to_owned();
    let available_index =
        crate::infra::db::provider_api_key_index_key(&provider_id, &key_id, true)?;
    state
        .db
        .write(move |transaction| {
            let Some(mut key) = transaction.get::<Record>(Table::ProviderApiKeys, &key_id)? else {
                return Ok(());
            };
            if key.text("provider_id")? != provider_id {
                return Err(StorageError::Invalid(
                    "provider API key belongs to a different provider".to_owned(),
                ));
            }
            let Some(provider) = transaction.get::<Record>(Table::Providers, &provider_id)? else {
                return Ok(());
            };
            if crate::admin::providers::provider_is_deleting(&provider)? {
                return Ok(());
            }
            if key.boolean("enabled")? && !key.boolean("invalid")? {
                transaction.delete(Table::ProviderApiKeyAvailabilityIndex, &available_index)?;
                if let Some(mut provider) =
                    transaction.get::<Record>(Table::Providers, &provider_id)?
                {
                    let invalid_count = provider
                        .integer("invalid_api_key_count")?
                        .checked_add(1)
                        .ok_or_else(|| {
                        StorageError::Invalid(
                            "provider invalid API key count overflowed".to_owned(),
                        )
                    })?;
                    provider.insert("invalid_api_key_count", Field::I64(invalid_count));
                    transaction.put(Table::Providers, &provider_id, &provider)?;
                }
            }
            key.insert("invalid", Field::Bool(true));
            key.insert("last_error", Field::Text(last_error.clone()));
            key.insert("last_used_at", Field::Text(timestamp.clone()));
            transaction.put(Table::ProviderApiKeys, &key_id, &key)
        })
        .await
}

pub(super) fn decode_provider(
    record: &Record,
    master_key: Option<&[u8; 32]>,
) -> Result<Provider, String> {
    let adapter_id = record
        .text("adapter_id")
        .map_err(|error| error.to_string())?;
    if provider_adapters::adapter(adapter_id).is_none() {
        return Err("provider adapter is not registered".to_owned());
    }
    let preferred = record
        .text("preferred_protocol")
        .map_err(|error| error.to_string())?;
    let protocols = record
        .text("supported_protocols")
        .map_err(|error| error.to_string())?;
    let thinking_mode = record
        .optional_text("thinking_mode")
        .map_err(|error| error.to_string())?
        .unwrap_or(protocol::DEFAULT_THINKING_MODE)
        .to_owned();
    let thinking_override = record
        .optional_text("thinking_override")
        .map_err(|error| error.to_string())?
        .map(str::to_owned);
    protocol::parse_thinking_handling(&thinking_mode, thinking_override.as_deref())
        .map_err(str::to_owned)?;
    let secret_bytes = record
        .optional_bytes("secret")
        .map_err(|error| error.to_string())?;
    let custom_headers = crate::admin::providers::stored_custom_headers(record, master_key)?;
    Ok(Provider {
        id: record
            .text("id")
            .map_err(|error| error.to_string())?
            .to_owned(),
        adapter_id: adapter_id.to_owned(),
        base_url: record
            .text("base_url")
            .map_err(|error| error.to_string())?
            .to_owned(),
        enabled: record
            .boolean("enabled")
            .map_err(|error| error.to_string())?,
        auth_type: record
            .text("auth_type")
            .map_err(|error| error.to_string())?
            .to_owned(),
        auth_header: record
            .optional_text("auth_header")
            .map_err(|error| error.to_string())?
            .map(str::to_owned),
        custom_headers,
        secret: decrypt_secret(master_key, secret_bytes)?,
        preferred_protocol: preferred.parse()?,
        supported_protocols: serde_json::from_str(protocols).map_err(|e| e.to_string())?,
        thinking_mode,
        thinking_override,
        key_strategy: crate::admin::providers::stored_key_strategy(record)
            .map_err(|error| error.to_string())?,
    })
}
