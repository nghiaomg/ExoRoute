use super::{
    MAX_PROVIDER_CUSTOM_HEADER_NAME_BYTES, MAX_PROVIDER_CUSTOM_HEADER_VALUE_BYTES,
    MAX_PROVIDER_CUSTOM_HEADERS_BYTES,
};
use crate::{infra::storage::Record, security::egress};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

#[derive(Clone, Debug, Deserialize)]
pub struct ProviderCustomHeaderInput {
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
}

pub(crate) fn validate_custom_header_entries(
    headers: &[ProviderCustomHeaderInput],
) -> Result<(), &'static str> {
    let mut names = HashSet::new();
    for header in headers {
        let name = header.name.trim();
        if name.is_empty() || name.len() > MAX_PROVIDER_CUSTOM_HEADER_NAME_BYTES {
            return Err("custom header names must contain between 1 and 128 bytes");
        }
        let canonical = egress::provider_custom_header_name(name)
            .map_err(|_| "custom header name is invalid or reserved")?;
        if !names.insert(canonical.as_str().to_owned()) {
            return Err("custom header names cannot be duplicated");
        }
        if let Some(value) = header.value.as_deref()
            && (value.len() > MAX_PROVIDER_CUSTOM_HEADER_VALUE_BYTES
                || http::HeaderValue::from_str(value).is_err())
        {
            return Err("custom header values must contain at most 4096 valid header bytes");
        }
    }
    Ok(())
}

pub(crate) fn normalize_custom_headers(
    headers: &[ProviderCustomHeaderInput],
    existing: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, &'static str> {
    validate_custom_header_entries(headers)?;
    let mut normalized = BTreeMap::new();
    for header in headers {
        let name = egress::provider_custom_header_name(header.name.trim())
            .map_err(|_| "custom header name is invalid or reserved")?;
        let value = match header.value.as_deref() {
            Some(value) => value.to_owned(),
            None => existing
                .get(name.as_str())
                .cloned()
                .ok_or("a new custom header must include a value")?,
        };
        normalized.insert(name.as_str().to_owned(), value);
    }
    let serialized =
        serde_json::to_vec(&normalized).map_err(|_| "custom headers could not be serialized")?;
    if serialized.len() > MAX_PROVIDER_CUSTOM_HEADERS_BYTES {
        return Err("custom headers exceed the 256 KiB total limit");
    }
    Ok(normalized)
}

#[allow(dead_code)]
pub(crate) fn normalize_custom_headers_for_tests(
    headers: &[ProviderCustomHeaderInput],
    existing: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, &'static str> {
    normalize_custom_headers(headers, existing)
}

pub(crate) fn encrypt_custom_headers(
    headers: &BTreeMap<String, String>,
    master_key: Option<&[u8; 32]>,
) -> Result<Option<Vec<u8>>, String> {
    if headers.is_empty() {
        return Ok(None);
    }
    let serialized = serde_json::to_string(headers)
        .map_err(|_| "could not serialize provider custom headers".to_owned())?;
    crate::security::encrypt_secret(master_key, &serialized)
}

pub(crate) fn stored_custom_headers(
    record: &Record,
    master_key: Option<&[u8; 32]>,
) -> Result<BTreeMap<String, String>, String> {
    let Some(stored) = record
        .optional_bytes("custom_headers")
        .map_err(|error| error.to_string())?
    else {
        return Ok(BTreeMap::new());
    };
    let serialized = crate::security::decrypt_secret(master_key, Some(stored))?
        .ok_or_else(|| "stored provider custom headers are empty".to_owned())?;
    let headers: BTreeMap<String, String> = serde_json::from_str(&serialized)
        .map_err(|_| "stored provider custom headers are invalid".to_owned())?;
    let entries = headers
        .iter()
        .map(|(name, value)| ProviderCustomHeaderInput {
            name: name.clone(),
            value: Some(value.clone()),
        })
        .collect::<Vec<_>>();
    normalize_custom_headers(&entries, &BTreeMap::new()).map_err(str::to_owned)
}

pub(crate) fn custom_headers_for_input(
    input: Option<&[ProviderCustomHeaderInput]>,
    existing: &BTreeMap<String, String>,
) -> Result<Option<BTreeMap<String, String>>, &'static str> {
    input
        .map(|headers| normalize_custom_headers(headers, existing).map(Some))
        .transpose()
        .map(|headers| headers.flatten())
}
