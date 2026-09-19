use super::*;

pub(crate) fn validate_backup_payload(
    payload: &BackupPayload,
    allow_private_provider_urls: bool,
    confirm_unlimited: bool,
) -> Result<RestoredRuntimeSettings, &'static str> {
    crate::infra::storage::validate_snapshot_entries(&payload.entries)
        .map_err(|_| "The backup contains invalid or unsupported LMDB records.")?;
    let mut providers = HashMap::<String, Record>::new();
    let mut routes = HashSet::<String>::new();
    let mut api_keys = HashSet::<String>::new();
    let mut provider_key_ids = HashSet::<String>::new();
    let mut meter_credential_ids = HashSet::<String>::new();
    let mut provider_usable_key_counts = HashMap::<String, usize>::new();
    let mut models = HashSet::<String>::new();
    let mut model_prefixes = HashSet::<String>::new();
    let mut deleting_providers = HashSet::<String>::new();
    let mut route_target_ordinals = HashMap::<String, HashSet<u32>>::new();
    let mut admin_auth_seen = false;
    let mut output_styles: Option<OutputStylesSnapshot> = None;
    for entry in &payload.entries {
        match entry.table {
            Table::AdminAuth => {
                if entry.key != "singleton" || admin_auth_seen {
                    return Err("The backup contains an invalid admin authentication record.");
                }
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "The backup admin authentication record is invalid.")?;
                let password_hash = record
                    .text("password_hash")
                    .map_err(|_| "The backup admin authentication record is invalid.")?;
                if !super::auth::is_supported_admin_password_hash(password_hash) {
                    return Err("The backup admin password hash is unsupported or invalid.");
                }
                let _ = record
                    .boolean("must_change_password")
                    .map_err(|_| "The backup admin authentication record is invalid.")?;
                let _ = record
                    .text("updated_at")
                    .map_err(|_| "The backup admin authentication record is invalid.")?;
                admin_auth_seen = true;
            }
            Table::Providers => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A provider record in the backup is invalid.")?;
                let id = record
                    .text("id")
                    .map_err(|_| "A provider record in the backup is invalid.")?;
                if id != entry.key || id.is_empty() || id.len() > 256 {
                    return Err("A provider record in the backup has an invalid ID.");
                }
                let adapter_id = record
                    .text("adapter_id")
                    .map_err(|_| "A provider record in the backup is invalid.")?;
                let local_rpm_target = record
                    .optional_integer("local_rpm_target")
                    .map_err(|_| "A provider record in the backup is invalid.")?;
                super::providers::stored_thinking_settings(&record)
                    .map_err(|_| "A provider record has invalid thinking settings.")?;
                super::providers::stored_key_strategy(&record)
                    .map_err(|_| "A provider record has an invalid key strategy.")?;
                let name = record
                    .text("name")
                    .map_err(|_| "A provider record in the backup is invalid.")?;
                let base_url = record
                    .text("base_url")
                    .map_err(|_| "A provider record in the backup is invalid.")?;
                let _ = record
                    .text("auth_type")
                    .map_err(|_| "A provider record in the backup is invalid.")?;
                let _ = record
                    .text("preferred_protocol")
                    .map_err(|_| "A provider record in the backup is invalid.")?;
                if crate::admin::providers::validate_provider_deletion_state(&record)
                    .map_err(|_| "A provider record in the backup has invalid deletion state.")?
                {
                    deleting_providers.insert(id.to_owned());
                }
                let _: Vec<String> = serde_json::from_str(
                    record
                        .text("supported_protocols")
                        .map_err(|_| "A provider record in the backup is invalid.")?,
                )
                .map_err(|_| "A provider has an invalid supported-protocol list.")?;
                if name.trim().is_empty()
                    || name.len() > 256
                    || provider_adapters::capabilities(adapter_id).is_none()
                {
                    return Err("A provider record has an invalid name or unregistered adapter.");
                }
                if let Some(target) = local_rpm_target {
                    if target <= 0 || u32::try_from(target).is_err() {
                        return Err("A provider record has an invalid local RPM target.");
                    }
                    if !provider_adapters::capabilities(adapter_id)
                        .is_some_and(|capabilities| capabilities.local_quota_tracking)
                    {
                        return Err(
                            "A provider record contains a local RPM target for an unsupported adapter.",
                        );
                    }
                }
                egress::validate_provider_url(base_url, allow_private_provider_urls)
                    .map_err(|_| "A provider in the backup has a URL that is not allowed by this installation.")?;
                let prefix = super::providers::normalized_provider_model_prefix_for_adapter(
                    record.optional_text("model_prefix").ok().flatten(),
                    id,
                    adapter_id,
                )
                .map_err(|_| "A provider in the backup has an invalid model prefix.")?;
                if !model_prefixes.insert(prefix) {
                    return Err(
                        "The backup assigns the same model prefix to more than one provider.",
                    );
                }
                if providers.insert(id.to_owned(), record).is_some() {
                    return Err("The backup contains duplicate provider IDs.");
                }
            }
            Table::Routes => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A route record in the backup is invalid.")?;
                let id = record
                    .text("id")
                    .map_err(|_| "A route record in the backup is invalid.")?;
                if id != entry.key || id.is_empty() || !routes.insert(id.to_owned()) {
                    return Err("The backup contains an invalid or duplicate route ID.");
                }
            }
            Table::ApiKeys => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "An API key record in the backup is invalid.")?;
                let id = record
                    .text("id")
                    .map_err(|_| "An API key record in the backup is invalid.")?;
                if id != entry.key
                    || !api_keys.insert(id.to_owned())
                    || record
                        .bytes("token_hash")
                        .map_err(|_| "An API key record in the backup is invalid.")?
                        .len()
                        != 32
                {
                    return Err("The backup contains an invalid or duplicate gateway API key.");
                }
                let _ = record
                    .boolean("enabled")
                    .map_err(|_| "An API key record in the backup is invalid.")?;
                let _ = record
                    .text("name")
                    .map_err(|_| "An API key record in the backup is invalid.")?;
            }
            Table::ProviderApiKeys => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A provider API key record in the backup is invalid.")?;
                let id = record
                    .text("id")
                    .map_err(|_| "A provider API key record in the backup is invalid.")?;
                let provider_id = record
                    .text("provider_id")
                    .map_err(|_| "A provider API key record in the backup is invalid.")?;
                if id != entry.key
                    || id.is_empty()
                    || id.len() > 256
                    || record
                        .bytes("secret")
                        .map_err(|_| "A provider API key record in the backup is invalid.")?
                        .is_empty()
                    || record
                        .bytes("secret")
                        .map_err(|_| "A provider API key record in the backup is invalid.")?
                        .len()
                        > MAX_PROVIDER_KEYS_PER_BACKUP_RECORD
                    || !provider_key_ids.insert(id.to_owned())
                {
                    return Err("The backup contains an invalid or duplicate provider API key.");
                }
                let enabled = record
                    .boolean("enabled")
                    .map_err(|_| "A provider API key record in the backup is invalid.")?;
                let invalid = record
                    .boolean("invalid")
                    .map_err(|_| "A provider API key record in the backup is invalid.")?;
                let _ = record
                    .text("name")
                    .map_err(|_| "A provider API key record in the backup is invalid.")?;
                let _ = record
                    .text("created_at")
                    .map_err(|_| "A provider API key record in the backup is invalid.")?;
                if enabled && !invalid {
                    let count = provider_usable_key_counts
                        .entry(provider_id.to_owned())
                        .or_default();
                    *count = count
                        .checked_add(1)
                        .ok_or("The backup contains too many provider API keys.")?;
                }
            }
            Table::ProviderModels => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A provider model record in the backup is invalid.")?;
                let provider_id = record
                    .text("provider_id")
                    .map_err(|_| "A provider model record in the backup is invalid.")?;
                let model = record
                    .text("model")
                    .map_err(|_| "A provider model record in the backup is invalid.")?;
                if let Some(protocol) = record
                    .optional_text("upstream_protocol")
                    .map_err(|_| "A provider model record in the backup is invalid.")?
                {
                    protocol
                        .parse::<crate::protocol::UpstreamProtocol>()
                        .map_err(|_| {
                            "A provider model record in the backup has an invalid upstream protocol."
                        })?;
                }
                let expected_key = crate::infra::db::provider_model_key(provider_id, model)
                    .map_err(|_| "A provider model record in the backup has an invalid key.")?;
                if entry.key != expected_key
                    || model.trim().is_empty()
                    || model.len() > 256
                    || !models.insert(format!("{provider_id}\0{model}"))
                {
                    return Err(
                        "The backup contains an invalid, duplicate, or mis-keyed provider model.",
                    );
                }
            }
            Table::RouteTargets => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                let route_id = record
                    .text("route_id")
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                let _ = record
                    .text("provider_id")
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                let model = record
                    .text("model")
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                if model.trim().is_empty() || model.len() > 256 || model != model.trim() {
                    return Err("The backup contains a route target with an invalid model.");
                }
                if let Some(protocol) = record
                    .optional_text("protocol")
                    .map_err(|_| "A route target record in the backup is invalid.")?
                {
                    protocol
                        .parse::<crate::protocol::UpstreamProtocol>()
                        .map_err(
                            |_| "The backup contains a route target with an invalid protocol.",
                        )?;
                }
                let priority = record
                    .integer("priority")
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                let priority = u32::try_from(priority)
                    .map_err(|_| "The backup contains a route target with an invalid priority.")?;
                let weight = record
                    .optional_integer("weight")
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                if weight.is_some_and(|value| u32::try_from(value).is_err()) {
                    return Err("The backup contains a route target with an invalid weight.");
                }
                let _ = record
                    .boolean("enabled")
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                let (key_priority, ordinal) = route_target_key_components(&entry.key, route_id)?;
                if key_priority != priority {
                    return Err("The backup route target key does not match its priority.");
                }
                if !route_target_ordinals
                    .entry(route_id.to_owned())
                    .or_default()
                    .insert(ordinal)
                {
                    return Err("The backup contains duplicate route target ordinals.");
                }
            }
            Table::RequestLogs => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A request log record in the backup is invalid.")?;
                if record
                    .text("id")
                    .map_err(|_| "A request log record in the backup is invalid.")?
                    != entry.key
                {
                    return Err("A request log record has an invalid ID.");
                }
                let _ = record
                    .text("created_at")
                    .map_err(|_| "A request log record in the backup is invalid.")?;
            }
            Table::ProviderUsageMeters => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A provider usage meter record in the backup is invalid.")?;
                let (credential_id, minute_text) = entry
                    .key
                    .rsplit_once('/')
                    .ok_or("A provider usage meter key in the backup is invalid.")?;
                if credential_id.is_empty()
                    || credential_id.len() > 256
                    || credential_id.contains('/')
                    || credential_id.bytes().any(|byte| byte.is_ascii_control())
                    || minute_text.len() != 20
                    || !minute_text.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err("A provider usage meter key in the backup is invalid.");
                }
                let minute = minute_text
                    .parse::<i64>()
                    .map_err(|_| "A provider usage meter key in the backup is invalid.")?;
                if minute < 0
                    || record
                        .text("credential_id")
                        .map_err(|_| "A provider usage meter record in the backup is invalid.")?
                        != credential_id
                    || record
                        .integer("minute")
                        .map_err(|_| "A provider usage meter record in the backup is invalid.")?
                        != minute
                {
                    return Err("A provider usage meter record has an invalid key.");
                }
                for field in [
                    "request_count",
                    "cost_micro_usd",
                    "cost_events",
                    "missing_cost_events",
                    "input_tokens",
                    "output_tokens",
                ] {
                    if record
                        .integer(field)
                        .map_err(|_| "A provider usage meter record in the backup is invalid.")?
                        < 0
                    {
                        return Err("A provider usage meter record contains a negative count.");
                    }
                }
                meter_credential_ids.insert(credential_id.to_owned());
            }
            Table::OutputStyles => {
                if entry.key != "singleton" || output_styles.is_some() {
                    return Err("The backup contains an invalid output styles record.");
                }
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "The backup output styles record is invalid.")?;
                let snapshot = crate::infra::db::output_styles_from_record(&record)
                    .map_err(|_| "The backup output styles record is invalid.")?;
                output_styles = Some(snapshot);
            }
            _ => {}
        }
    }
    for credential_id in &meter_credential_ids {
        if !provider_key_ids.contains(credential_id) {
            return Err(
                "A provider usage meter refers to a provider credential that is missing from the backup.",
            );
        }
    }
    for ordinals in route_target_ordinals.values() {
        let Some(max_ordinal) = ordinals.iter().copied().max() else {
            continue;
        };
        let expected_count = usize::try_from(max_ordinal)
            .ok()
            .and_then(|value| value.checked_add(1))
            .ok_or("The backup contains invalid route target ordinals.")?;
        if ordinals.len() != expected_count
            || !(0..=max_ordinal).all(|ordinal| ordinals.contains(&ordinal))
        {
            return Err("The backup contains a non-contiguous route target order.");
        }
    }
    for entry in &payload.entries {
        match entry.table {
            Table::ProviderApiKeys => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A provider API key record in the backup is invalid.")?;
                if !providers.contains_key(
                    record
                        .text("provider_id")
                        .map_err(|_| "A provider API key record in the backup is invalid.")?,
                ) {
                    return Err(
                        "A provider API key refers to a provider that is missing from the backup.",
                    );
                }
            }
            Table::ProviderModels => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A provider model record in the backup is invalid.")?;
                let provider_id = record
                    .text("provider_id")
                    .map_err(|_| "A provider model record in the backup is invalid.")?;
                let Some(provider) = providers.get(provider_id) else {
                    return Err(
                        "A provider model refers to a provider that is missing from the backup.",
                    );
                };
                if let Some(value) = record
                    .optional_text("upstream_protocol")
                    .map_err(|_| "A provider model record in the backup is invalid.")?
                {
                    let protocol = value
                        .parse::<crate::protocol::UpstreamProtocol>()
                        .map_err(|_| {
                            "A provider model record in the backup has an invalid upstream protocol."
                        })?;
                    let adapter_id = provider
                        .text("adapter_id")
                        .map_err(|_| "A provider record in the backup is invalid.")?;
                    if !provider_adapters::supports_upstream_protocol(adapter_id, protocol) {
                        return Err(
                            "A provider model uses an upstream protocol unsupported by its adapter.",
                        );
                    }
                }
            }
            Table::RouteTargets => {
                let record: Record = bincode::deserialize(&entry.value)
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                let route_id = record
                    .text("route_id")
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                let provider_id = record
                    .text("provider_id")
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                let model = record
                    .text("model")
                    .map_err(|_| "A route target record in the backup is invalid.")?;
                if !routes.contains(route_id) || !providers.contains_key(provider_id) {
                    return Err(
                        "A route target refers to a provider or route that is missing from the backup.",
                    );
                }
                let provider = providers.get(provider_id).ok_or(
                    "A route target refers to a provider or route that is missing from the backup.",
                )?;
                if let Some(value) = record
                    .optional_text("protocol")
                    .map_err(|_| "A route target record in the backup is invalid.")?
                {
                    let protocol = value.parse::<crate::protocol::UpstreamProtocol>().map_err(
                        |_| "The backup contains a route target with an invalid protocol.",
                    )?;
                    let adapter_id = provider
                        .text("adapter_id")
                        .map_err(|_| "A provider record in the backup is invalid.")?;
                    if !provider_adapters::supports_upstream_protocol(adapter_id, protocol) {
                        return Err(
                            "A route target uses an upstream protocol unsupported by its provider adapter.",
                        );
                    }
                    let supported: Vec<String> = serde_json::from_str(
                        provider
                            .text("supported_protocols")
                            .map_err(|_| "A provider record in the backup is invalid.")?,
                    )
                    .map_err(|_| "A provider has an invalid supported-protocol list.")?;
                    if !supported.iter().any(|value| value == protocol.as_str()) {
                        return Err(
                            "A route target uses an upstream protocol not enabled for its provider.",
                        );
                    }
                }
                if !models.contains(&format!("{provider_id}\0{model}"))
                    && !deleting_providers.contains(provider_id)
                {
                    return Err(
                        "A route target refers to a model that is missing from its provider.",
                    );
                }
            }
            _ => {}
        }
    }
    for (provider_id, provider) in &providers {
        let adapter_id = provider
            .text("adapter_id")
            .map_err(|_| "A provider record in the backup is invalid.")?;
        let auth_type = provider
            .text("auth_type")
            .map_err(|_| "A provider record in the backup is invalid.")?;
        let base_url = provider
            .text("base_url")
            .map_err(|_| "A provider record in the backup is invalid.")?;
        let preferred_protocol = provider
            .text("preferred_protocol")
            .map_err(|_| "A provider record in the backup is invalid.")?;
        let supported_protocols: Vec<String> = serde_json::from_str(
            provider
                .text("supported_protocols")
                .map_err(|_| "A provider record in the backup is invalid.")?,
        )
        .map_err(|_| "A provider has an invalid supported-protocol list.")?;
        let preferred = preferred_protocol
            .parse::<crate::protocol::UpstreamProtocol>()
            .map_err(|_| "A provider has an invalid preferred upstream protocol.")?;
        if supported_protocols.is_empty()
            || supported_protocols.len() > 4
            || !supported_protocols
                .iter()
                .any(|value| value == preferred.as_str())
        {
            return Err("A provider has an invalid preferred or supported upstream protocol.");
        }
        let mut seen_protocols = std::collections::HashSet::new();
        for value in &supported_protocols {
            let protocol = value
                .parse::<crate::protocol::UpstreamProtocol>()
                .map_err(|_| "A provider has an invalid supported upstream protocol.")?;
            if !seen_protocols.insert(protocol.as_str())
                || !provider_adapters::supports_upstream_protocol(adapter_id, protocol)
            {
                return Err("A provider has a duplicate or adapter-unsupported upstream protocol.");
            }
        }
        provider_adapters::validate_adapter_config(
            adapter_id,
            auth_type,
            base_url,
            preferred_protocol,
            &supported_protocols,
            provider_usable_key_counts
                .get(provider_id)
                .copied()
                .unwrap_or(0)
                > 0,
        )
        .map_err(
            |_| "A provider in the backup has invalid adapter, credential, or protocol settings.",
        )?;
    }
    if !admin_auth_seen {
        return Err("The backup does not contain admin authentication state.");
    }
    let limits_record = payload
        .entries
        .iter()
        .find(|entry| entry.table == Table::GatewayResourceLimits && entry.key == "singleton")
        .map(|entry| {
            bincode::deserialize::<Record>(&entry.value)
                .map_err(|_| "The backup gateway limits are invalid.")
        })
        .transpose()?
        .ok_or("The backup does not contain gateway resource limits.")?;
    let gateway_resource_limits = crate::infra::db::gateway_limits_from_record(&limits_record)
        .map_err(|_| "The backup contains invalid gateway resource limits.")?;
    let settings_record = payload
        .entries
        .iter()
        .find(|entry| entry.table == Table::OperationalSettings && entry.key == "singleton")
        .map(|entry| {
            bincode::deserialize::<Record>(&entry.value)
                .map_err(|_| "The backup operational settings are invalid.")
        })
        .transpose()?
        .ok_or("The backup does not contain operational settings.")?;
    let operational_settings = crate::infra::db::operational_settings_from_record(&settings_record)
        .map_err(|_| "The backup contains invalid operational settings.")?;
    if (operational_settings.settings.gateway_max_in_flight == 0
        || gateway_resource_limits.provider_max_concurrency
            == crate::config::GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY)
        && !confirm_unlimited
    {
        return Err(
            "This backup enables unlimited gateway or per-provider concurrency. Confirm the import warning before applying it.",
        );
    }
    Ok(RestoredRuntimeSettings {
        gateway_resource_limits,
        operational_settings,
        output_styles,
    })
}

pub(crate) fn route_target_key_components(
    key: &str,
    route_id: &str,
) -> Result<(u32, u32), &'static str> {
    if route_id.is_empty()
        || route_id.len() > 128
        || route_id.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("The backup contains a route target with an invalid route ID.");
    }

    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded_route_id = String::with_capacity(route_id.len().saturating_mul(2));
    for byte in route_id.bytes() {
        encoded_route_id.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded_route_id.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    let prefix = format!("r/{encoded_route_id}/");
    let suffix = key
        .strip_prefix(&prefix)
        .ok_or("The backup route target key does not match its route ID.")?;
    let (priority, ordinal) = suffix
        .split_once('/')
        .ok_or("The backup contains an invalid route target key.")?;
    if ordinal.contains('/') {
        return Err("The backup contains an invalid route target key.");
    }

    let priority_value = priority
        .parse::<u32>()
        .map_err(|_| "The backup contains an invalid route target key.")?;
    let ordinal_value = ordinal
        .parse::<u32>()
        .map_err(|_| "The backup contains an invalid route target key.")?;
    if priority != format!("{priority_value:010}")
        || ordinal != format!("{ordinal_value:010}")
        || usize::try_from(ordinal_value)
            .map_or(true, |ordinal| ordinal >= super::combos::MAX_ROUTE_TARGETS)
    {
        return Err("The backup contains a non-canonical route target key.");
    }
    Ok((priority_value, ordinal_value))
}
