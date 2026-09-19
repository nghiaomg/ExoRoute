use crate::infra::storage::{Field, Record};

pub(crate) struct ProviderRecordData<'a> {
    pub(super) id: &'a str,
    pub(super) name: &'a str,
    pub(super) base_url: &'a str,
    pub(super) logo_url: Option<&'a str>,
    pub(super) model_prefix: &'a str,
    pub(super) adapter_id: &'a str,
    pub(super) enabled: bool,
    pub(super) auth_type: &'a str,
    pub(super) auth_header: Option<&'a str>,
    pub(super) custom_headers: Option<&'a [u8]>,
    pub(super) preferred_protocol: &'a str,
    pub(super) supported_protocols: &'a str,
    pub(super) thinking_mode: &'a str,
    pub(super) thinking_override: Option<&'a str>,
    pub(super) key_strategy: &'a str,
    pub(super) created_at: &'a str,
    pub(super) updated_at: &'a str,
    pub(super) api_key_count: i64,
    pub(super) invalid_api_key_count: i64,
    pub(super) model_count: i64,
    pub(super) local_rpm_target: Option<u32>,
}

pub(crate) fn provider_record(data: ProviderRecordData<'_>) -> Record {
    let ProviderRecordData {
        id,
        name,
        base_url,
        logo_url,
        model_prefix,
        adapter_id,
        enabled,
        auth_type,
        auth_header,
        custom_headers,
        preferred_protocol,
        supported_protocols,
        thinking_mode,
        thinking_override,
        key_strategy,
        created_at,
        updated_at,
        api_key_count,
        invalid_api_key_count,
        model_count,
        local_rpm_target,
    } = data;
    Record::new()
        .with("id", Field::Text(id.to_owned()))
        .with("name", Field::Text(name.to_owned()))
        .with("base_url", Field::Text(base_url.to_owned()))
        .with(
            "logo_url",
            logo_url
                .map(|value| Field::Text(value.to_owned()))
                .unwrap_or(Field::Null),
        )
        .with("model_prefix", Field::Text(model_prefix.to_owned()))
        .with("adapter_id", Field::Text(adapter_id.to_owned()))
        .with("enabled", Field::Bool(enabled))
        .with("auth_type", Field::Text(auth_type.to_owned()))
        .with(
            "auth_header",
            auth_header
                .map(|value| Field::Text(value.to_owned()))
                .unwrap_or(Field::Null),
        )
        .with(
            "custom_headers",
            custom_headers
                .map(|value| Field::Bytes(value.to_vec()))
                .unwrap_or(Field::Null),
        )
        .with(
            "preferred_protocol",
            Field::Text(preferred_protocol.to_owned()),
        )
        .with(
            "supported_protocols",
            Field::Text(supported_protocols.to_owned()),
        )
        .with("thinking_mode", Field::Text(thinking_mode.to_owned()))
        .with(
            "thinking_override",
            thinking_override
                .map(|value| Field::Text(value.to_owned()))
                .unwrap_or(Field::Null),
        )
        .with("key_strategy", Field::Text(key_strategy.to_owned()))
        .with("created_at", Field::Text(created_at.to_owned()))
        .with("updated_at", Field::Text(updated_at.to_owned()))
        .with("api_key_count", Field::I64(api_key_count))
        .with("invalid_api_key_count", Field::I64(invalid_api_key_count))
        .with("model_count", Field::I64(model_count))
        .with(
            "local_rpm_target",
            local_rpm_target
                .map(|value| Field::I64(i64::from(value)))
                .unwrap_or(Field::Null),
        )
}
