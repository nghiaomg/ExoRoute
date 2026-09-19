use super::*;
use crate::infra::storage::{Record, StorageError};

pub(crate) fn adapter_has_local_quota_tracking(adapter_id: &str) -> bool {
    provider_adapters::capabilities(adapter_id)
        .is_some_and(|capabilities| capabilities.local_quota_tracking)
}

pub(crate) fn stored_local_rpm_target(
    record: &Record,
    adapter_id: &str,
) -> Result<Option<u32>, StorageError> {
    if !adapter_has_local_quota_tracking(adapter_id) {
        return Ok(None);
    }
    let target = record
        .optional_integer("local_rpm_target")?
        .map(u32::try_from)
        .transpose()
        .map_err(|_| StorageError::Invalid("stored local RPM target is invalid".to_owned()))?
        .unwrap_or(DEFAULT_LOCAL_RPM_TARGET);
    if target == 0 {
        return Err(StorageError::Invalid(
            "stored local RPM target must be positive".to_owned(),
        ));
    }
    Ok(Some(target))
}

pub(crate) fn stored_thinking_settings(
    record: &Record,
) -> Result<(String, Option<String>), StorageError> {
    let mode = record
        .optional_text("thinking_mode")?
        .unwrap_or(crate::protocol::DEFAULT_THINKING_MODE)
        .to_owned();
    let override_text = record
        .optional_text("thinking_override")?
        .map(str::to_owned);
    crate::protocol::parse_thinking_handling(&mode, override_text.as_deref())
        .map_err(|error| StorageError::Invalid(error.to_owned()))?;
    Ok((mode, override_text))
}

/// The key strategy a provider record without the field uses: the documented
/// fallback order, where the preferred enabled key serves every request.
pub(crate) const DEFAULT_KEY_STRATEGY: &str = "priority";
pub(crate) const KEY_STRATEGY_ROUND_ROBIN: &str = "round_robin";

/// The stored key strategy of a provider, defaulted for records written before
/// the setting existed and rejected when a record holds an unknown value.
pub(crate) fn stored_key_strategy(record: &Record) -> Result<String, StorageError> {
    let strategy = record
        .optional_text("key_strategy")?
        .unwrap_or(DEFAULT_KEY_STRATEGY)
        .to_owned();
    if strategy != DEFAULT_KEY_STRATEGY && strategy != KEY_STRATEGY_ROUND_ROBIN {
        return Err(StorageError::Invalid(
            "stored provider key strategy is invalid".to_owned(),
        ));
    }
    Ok(strategy)
}

pub(crate) fn create_local_rpm_target(input: &ProviderInput, adapter_id: &str) -> Option<u32> {
    adapter_has_local_quota_tracking(adapter_id)
        .then_some(input.local_rpm_target.unwrap_or(DEFAULT_LOCAL_RPM_TARGET))
}

pub(crate) fn updated_local_rpm_target(
    input: &ProviderInput,
    current: &Record,
    adapter_id: &str,
) -> Result<Option<u32>, StorageError> {
    if !adapter_has_local_quota_tracking(adapter_id) {
        return Ok(None);
    }
    match input.local_rpm_target {
        Some(target) => Ok(Some(target)),
        None => stored_local_rpm_target(current, adapter_id),
    }
}

pub(crate) fn yes() -> bool {
    true
}

pub(crate) fn no_auth() -> String {
    "none".to_owned()
}

pub(crate) fn default_protocol() -> String {
    "chat_completions".to_owned()
}

pub(crate) fn default_protocols() -> Vec<String> {
    vec!["chat_completions".to_owned()]
}

pub(crate) fn effective_adapter_id(input: &ProviderInput) -> String {
    input
        .adapter_id
        .as_deref()
        .map(str::trim)
        .filter(|adapter_id| !adapter_id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| provider_adapters::legacy_adapter_id(&input.auth_type).to_owned())
}

pub(crate) fn provider_keys(input: &ProviderInput) -> Vec<String> {
    let mut keys = input
        .api_keys
        .iter()
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .collect::<Vec<_>>();
    if keys.is_empty()
        && let Some(key) = input
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    {
        keys.push(key.to_owned());
    }
    keys
}

pub(crate) fn normalized_provider_logo_url(input: &ProviderInput) -> Option<String> {
    input
        .logo_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
}

pub(crate) fn validate_provider(
    input: &ProviderInput,
    adapter_id: &str,
    allow_private_provider_urls: bool,
) -> Result<(), (StatusCode, Json<Value>)> {
    if input.local_rpm_target == Some(0) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "local_rpm_target must be a positive integer",
        ));
    }
    if input.local_rpm_target.is_some() && !adapter_has_local_quota_tracking(adapter_id) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "local RPM targets are supported only by providers with local quota tracking",
        ));
    }
    if input.name.trim().is_empty() || input.name.len() > MAX_PROVIDER_NAME_BYTES {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider name is required and may contain at most 256 bytes",
        ));
    }
    if input.id.len() > MAX_PROVIDER_ID_BYTES
        || input.base_url.len() > MAX_PROVIDER_URL_BYTES
        || input
            .auth_header
            .as_ref()
            .is_some_and(|name| name.len() > 128)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider id, URL, or authentication header name exceeds its size limit",
        ));
    }
    if input
        .logo_url
        .as_ref()
        .is_some_and(|url| url.trim().len() > MAX_PROVIDER_LOGO_URL_BYTES)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider logo URL exceeds the 2048 byte limit",
        ));
    }
    if input.model_prefix.is_some() {
        normalized_provider_model_prefix(input.model_prefix.as_deref(), &input.id)
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    }
    if let Some(logo_url) = input
        .logo_url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        egress::validate_provider_url(logo_url, false).map_err(|_| {
            fail(
                StatusCode::BAD_REQUEST,
                "provider logo must be a public HTTPS URL",
            )
        })?;
    }
    if let Some(headers) = input.custom_headers.as_deref() {
        validate_custom_header_entries(headers)
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    }
    let keys = provider_keys(input);
    if keys.len() > MAX_PROVIDER_KEYS_PER_REQUEST {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "add at most 8 provider API keys per request",
        ));
    }
    if keys.iter().any(|key| key.len() > MAX_PROVIDER_KEY_BYTES) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider API keys may contain at most 4096 bytes",
        ));
    }
    if input.supported_protocols.is_empty() || input.supported_protocols.len() > 4 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "supported_protocols must contain between 1 and 4 protocols",
        ));
    }
    let mut unique_protocols = std::collections::HashSet::new();
    if input
        .supported_protocols
        .iter()
        .any(|protocol| !unique_protocols.insert(protocol))
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "supported_protocols cannot contain duplicates",
        ));
    }
    egress::validate_provider_url(&input.base_url, allow_private_provider_urls)
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    provider_adapters::validate_adapter_config(
        adapter_id,
        &input.auth_type,
        &input.base_url,
        &input.preferred_protocol,
        &input.supported_protocols,
        !keys.is_empty(),
    )
    .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    if input.auth_type == "header" && input.auth_header.as_deref().unwrap_or("").trim().is_empty() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "auth_header is required for header authentication",
        ));
    }
    if input.auth_type == "header" {
        egress::provider_auth_header_name(input.auth_header.as_deref().unwrap_or(""))
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    }
    let preferred = input
        .preferred_protocol
        .parse::<UpstreamProtocol>()
        .map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
    if !provider_adapters::supports_upstream_protocol(adapter_id, preferred) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "preferred_protocol is not supported by this provider adapter",
        ));
    }
    for protocol in &input.supported_protocols {
        let parsed = protocol
            .parse::<UpstreamProtocol>()
            .map_err(|e| fail(StatusCode::BAD_REQUEST, e))?;
        if !provider_adapters::supports_upstream_protocol(adapter_id, parsed) {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "supported_protocols contains a protocol not supported by this provider adapter",
            ));
        }
    }
    Ok(())
}

pub(crate) fn normalized_provider_model_prefix(
    value: Option<&str>,
    provider_id: &str,
) -> Result<String, &'static str> {
    let fallback;
    let value = match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => value,
        None => {
            fallback = resource_id(provider_id);
            fallback.as_str()
        }
    };
    let normalized = value.to_ascii_lowercase();
    let valid = normalized.len() <= MAX_PROVIDER_MODEL_PREFIX_BYTES
        && normalized.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index > 0 && b"._-".contains(&byte))
        });
    if !valid {
        return Err(
            "model prefix must start with a letter or number and contain only letters, numbers, dots, underscores, or hyphens (64 characters maximum)",
        );
    }
    Ok(normalized)
}

pub(crate) fn normalized_provider_model_prefix_for_adapter(
    value: Option<&str>,
    provider_id: &str,
    adapter_id: &str,
) -> Result<String, &'static str> {
    let supplied = value.map(str::trim).filter(|value| !value.is_empty());
    let default = provider_adapters::preset_for_adapter(adapter_id)
        .and_then(|preset| preset.default_model_prefix);
    normalized_provider_model_prefix(supplied.or(default), provider_id)
}
