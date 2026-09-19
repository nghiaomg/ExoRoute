use super::OperationalSettingsRecord;
use crate::{
    config::{GatewayResourceLimits, OperationalSettings, UpstreamSettings},
    infra::storage::{Field, Record, StorageError},
    support::output_styles::{self, OutputStyleSelection, OutputStylesSnapshot},
};
use std::time::Duration;

pub(super) fn gateway_limits_record(limits: GatewayResourceLimits) -> Result<Record, StorageError> {
    Ok(Record::new()
        .with(
            "gateway_body_limit_bytes",
            usize_field(limits.gateway_body_limit_bytes, "gateway body limit")?,
        )
        .with(
            "gateway_body_processing_concurrency",
            usize_field(
                limits.gateway_body_processing_concurrency,
                "gateway body concurrency",
            )?,
        )
        .with(
            "sse_frame_limit_bytes",
            usize_field(limits.sse_frame_limit_bytes, "SSE frame limit")?,
        )
        .with(
            "sse_buffer_limit_bytes",
            usize_field(limits.sse_buffer_limit_bytes, "SSE buffer limit")?,
        )
        .with(
            "provider_max_concurrency",
            usize_field(limits.provider_max_concurrency, "provider concurrency")?,
        )
        .with(
            "stream_continuity_enabled",
            Field::Bool(limits.stream_continuity_enabled),
        )
        .with(
            "stream_continuity_max_concurrency",
            usize_field(
                limits.stream_continuity_max_concurrency,
                "stream continuity concurrency",
            )?,
        ))
}

pub(crate) fn gateway_limits_from_record(
    record: &Record,
) -> Result<GatewayResourceLimits, StorageError> {
    let limits = GatewayResourceLimits {
        gateway_body_limit_bytes: field_usize(record, "gateway_body_limit_bytes")?,
        gateway_body_processing_concurrency: field_usize(
            record,
            "gateway_body_processing_concurrency",
        )?,
        sse_frame_limit_bytes: field_usize(record, "sse_frame_limit_bytes")?,
        sse_buffer_limit_bytes: field_usize(record, "sse_buffer_limit_bytes")?,
        provider_max_concurrency: field_usize(record, "provider_max_concurrency")?,
        stream_continuity_enabled: record.boolean("stream_continuity_enabled")?,
        // Records written before the configurable continuity gate was added
        // remain valid and retain the former bounded default.
        stream_continuity_max_concurrency: optional_field_usize(
            record,
            "stream_continuity_max_concurrency",
        )?
        .unwrap_or(GatewayResourceLimits::DEFAULT_STREAM_CONTINUITY_MAX_CONCURRENCY),
    };
    limits
        .validate()
        .map_err(|error| StorageError::Invalid(error.to_owned()))?;
    Ok(limits)
}

pub(super) fn operational_settings_record(
    settings: OperationalSettings,
    revision: i64,
    overridden: bool,
) -> Result<Record, StorageError> {
    let duration_ms = |duration: Duration, name: &'static str| -> Result<Field, StorageError> {
        i64::try_from(duration.as_millis())
            .map(Field::I64)
            .map_err(|_| StorageError::Invalid(format!("{name} is outside the supported range")))
    };
    let duration_secs = |duration: Duration, name: &'static str| -> Result<Field, StorageError> {
        i64::try_from(duration.as_secs())
            .map(Field::I64)
            .map_err(|_| StorageError::Invalid(format!("{name} is outside the supported range")))
    };
    Ok(Record::new()
        .with(
            "connect_timeout_ms",
            duration_ms(settings.connect_timeout, "connect timeout")?,
        )
        .with(
            "request_timeout_ms",
            duration_ms(settings.request_timeout, "request timeout")?,
        )
        .with(
            "stream_idle_timeout_ms",
            duration_ms(settings.stream_idle_timeout, "stream idle timeout")?,
        )
        .with(
            "circuit_breaker_enabled",
            Field::Bool(settings.circuit_breaker_enabled),
        )
        .with(
            "circuit_breaker_threshold",
            Field::I64(i64::from(settings.circuit_breaker_threshold)),
        )
        .with(
            "circuit_breaker_cooldown_seconds",
            duration_secs(
                settings.circuit_breaker_cooldown,
                "circuit breaker cooldown",
            )?,
        )
        .with(
            "gateway_max_in_flight",
            usize_field(settings.gateway_max_in_flight, "gateway concurrency")?,
        )
        .with(
            "upstream_response_limit_bytes",
            usize_field(
                settings.upstream_response_limit_bytes,
                "upstream response limit",
            )?,
        )
        .with(
            "admin_api_max_requests",
            Field::I64(i64::from(settings.admin_api_max_requests)),
        )
        .with(
            "admin_api_window_seconds",
            duration_secs(settings.admin_api_window, "admin API window")?,
        )
        .with(
            "gateway_key_capacity",
            Field::I64(i64::from(settings.gateway_key_capacity)),
        )
        .with(
            "gateway_key_refill_tokens",
            Field::I64(i64::from(settings.gateway_key_refill_tokens)),
        )
        .with(
            "gateway_key_refill_interval_ms",
            duration_ms(
                settings.gateway_key_refill_interval,
                "gateway key refill interval",
            )?,
        )
        .with(
            "request_log_retention_days",
            Field::I64(i64::from(settings.request_log_retention_days)),
        )
        .with(
            "request_log_max_rows",
            Field::I64(i64::from(settings.request_log_max_rows)),
        )
        .with(
            "upstream_server_retry_max_attempts",
            usize_field(
                settings.upstream.server_retry_max_attempts,
                "server retry attempts",
            )?,
        )
        .with(
            "upstream_server_retry_delay_first_ms",
            duration_ms(
                settings.upstream.server_retry_delay_first,
                "server retry delay",
            )?,
        )
        .with(
            "upstream_server_retry_delay_second_ms",
            duration_ms(
                settings.upstream.server_retry_delay_second,
                "server retry delay",
            )?,
        )
        .with(
            "upstream_continuity_retry_base_delay_ms",
            duration_ms(
                settings.upstream.continuity_retry_base_delay,
                "continuity retry base delay",
            )?,
        )
        .with(
            "upstream_continuity_retry_max_delay_ms",
            duration_ms(
                settings.upstream.continuity_retry_max_delay,
                "continuity retry maximum delay",
            )?,
        )
        .with(
            "upstream_gateway_body_processing_queue_timeout_ms",
            duration_ms(
                settings.upstream.gateway_body_processing_queue_timeout,
                "gateway body queue timeout",
            )?,
        )
        .with(
            "upstream_continuity_replay_bytes_per_run",
            usize_field(
                settings.upstream.continuity_replay_bytes_per_run,
                "continuity replay bytes per run",
            )?,
        )
        .with(
            "upstream_continuity_replay_bytes_total",
            usize_field(
                settings.upstream.continuity_replay_bytes_total,
                "continuity replay bytes total",
            )?,
        )
        .with(
            "upstream_continuity_replay_events_per_run",
            Field::I64(settings.upstream.continuity_replay_events_per_run),
        )
        .with(
            "upstream_continuity_retained_runs",
            usize_field(
                settings.upstream.continuity_retained_runs,
                "continuity retained runs",
            )?,
        )
        .with(
            "upstream_continuity_resume_page_size",
            usize_field(
                settings.upstream.continuity_resume_page_size,
                "continuity resume page size",
            )?,
        )
        .with(
            "upstream_continuity_poll_interval_ms",
            duration_ms(
                settings.upstream.continuity_poll_interval,
                "continuity poll interval",
            )?,
        )
        .with(
            "upstream_continuity_heartbeat_interval_seconds",
            duration_secs(
                settings.upstream.continuity_heartbeat_interval,
                "continuity heartbeat interval",
            )?,
        )
        .with(
            "upstream_continuity_setup_error_bytes",
            usize_field(
                settings.upstream.continuity_setup_error_bytes,
                "continuity setup error limit",
            )?,
        )
        .with(
            "upstream_continuity_retention_seconds",
            duration_secs(
                settings.upstream.continuity_retention,
                "continuity retention",
            )?,
        )
        .with(
            "upstream_continuity_request_timeout_seconds",
            duration_secs(
                settings.upstream.continuity_request_timeout,
                "continuity request timeout",
            )?,
        )
        .with(
            "upstream_provider_live_events_max_duration_seconds",
            duration_secs(
                settings.upstream.provider_live_events_max_duration,
                "provider live event duration",
            )?,
        )
        .with(
            "upstream_provider_live_events_keepalive_seconds",
            duration_secs(
                settings.upstream.provider_live_events_keepalive,
                "provider live event keepalive",
            )?,
        )
        .with(
            "upstream_model_discovery_max_pages",
            usize_field(
                settings.upstream.model_discovery_max_pages,
                "model discovery page limit",
            )?,
        )
        .with(
            "upstream_discovery_request_timeout_seconds",
            duration_secs(
                settings.upstream.discovery_request_timeout,
                "discovery request timeout",
            )?,
        )
        .with(
            "upstream_usage_response_max_bytes",
            usize_field(
                settings.upstream.usage_response_max_bytes,
                "usage response limit",
            )?,
        )
        .with(
            "upstream_opencode_usage_timeout_seconds",
            duration_secs(
                settings.upstream.opencode_usage_timeout,
                "OpenCode usage timeout",
            )?,
        )
        .with(
            "upstream_command_code_usage_timeout_seconds",
            duration_secs(
                settings.upstream.command_code_usage_timeout,
                "Command Code usage timeout",
            )?,
        )
        .with(
            "upstream_command_code_optional_usage_timeout_seconds",
            duration_secs(
                settings.upstream.command_code_optional_usage_timeout,
                "Command Code optional usage timeout",
            )?,
        )
        .with(
            "upstream_freebuff_auxiliary_timeout_seconds",
            duration_secs(
                settings.upstream.freebuff_auxiliary_timeout,
                "Freebuff auxiliary timeout",
            )?,
        )
        .with(
            "upstream_freebuff_auxiliary_response_max_bytes",
            usize_field(
                settings.upstream.freebuff_auxiliary_response_max_bytes,
                "Freebuff auxiliary response limit",
            )?,
        )
        .with(
            "upstream_remote_image_max_count",
            usize_field(
                settings.upstream.remote_image_max_count,
                "remote image count",
            )?,
        )
        .with(
            "upstream_remote_image_max_bytes",
            usize_field(
                settings.upstream.remote_image_max_bytes,
                "remote image size",
            )?,
        )
        .with(
            "upstream_remote_image_total_max_bytes",
            usize_field(
                settings.upstream.remote_image_total_max_bytes,
                "remote image total size",
            )?,
        )
        .with(
            "upstream_remote_image_connect_timeout_seconds",
            duration_secs(
                settings.upstream.remote_image_connect_timeout,
                "remote image connect timeout",
            )?,
        )
        .with(
            "upstream_remote_image_request_timeout_seconds",
            duration_secs(
                settings.upstream.remote_image_request_timeout,
                "remote image request timeout",
            )?,
        )
        .with(
            "upstream_codex_image_preparation_timeout_seconds",
            duration_secs(
                settings.upstream.codex_image_preparation_timeout,
                "Codex image preparation timeout",
            )?,
        )
        .with(
            "upstream_provider_client_cache_ttl_seconds",
            duration_secs(
                settings.upstream.provider_client_cache_ttl,
                "provider client cache TTL",
            )?,
        )
        .with(
            "upstream_provider_client_cache_max_entries",
            usize_field(
                settings.upstream.provider_client_cache_max_entries,
                "provider client cache size",
            )?,
        )
        .with(
            "upstream_provider_client_max_resolved_addresses",
            usize_field(
                settings.upstream.provider_client_max_resolved_addresses,
                "provider resolved address limit",
            )?,
        )
        .with("revision", Field::I64(revision))
        .with("overridden", Field::Bool(overridden)))
}

pub(crate) fn operational_settings_from_record(
    record: &Record,
) -> Result<OperationalSettingsRecord, StorageError> {
    let u64_value = |name: &str| -> Result<u64, StorageError> {
        u64::try_from(record.integer(name)?)
            .map_err(|_| StorageError::Invalid(format!("operational setting '{name}' is invalid")))
    };
    let u32_value = |name: &str| -> Result<u32, StorageError> {
        u32::try_from(record.integer(name)?)
            .map_err(|_| StorageError::Invalid(format!("operational setting '{name}' is invalid")))
    };
    let defaults = UpstreamSettings::default();
    let optional_u64 = |name: &str, default: u64| -> Result<u64, StorageError> {
        match record.optional_integer(name)? {
            Some(value) => u64::try_from(value).map_err(|_| {
                StorageError::Invalid(format!("operational setting '{name}' is invalid"))
            }),
            None => Ok(default),
        }
    };
    let optional_usize = |name: &str, default: usize| -> Result<usize, StorageError> {
        usize::try_from(optional_u64(name, default as u64)?)
            .map_err(|_| StorageError::Invalid(format!("operational setting '{name}' is invalid")))
    };
    let optional_duration_ms = |name: &str, default: Duration| -> Result<Duration, StorageError> {
        Ok(Duration::from_millis(optional_u64(
            name,
            default.as_millis() as u64,
        )?))
    };
    let optional_duration_secs =
        |name: &str, default: Duration| -> Result<Duration, StorageError> {
            Ok(Duration::from_secs(optional_u64(name, default.as_secs())?))
        };
    let upstream = UpstreamSettings {
        server_retry_max_attempts: optional_usize(
            "upstream_server_retry_max_attempts",
            defaults.server_retry_max_attempts,
        )?,
        server_retry_delay_first: optional_duration_ms(
            "upstream_server_retry_delay_first_ms",
            defaults.server_retry_delay_first,
        )?,
        server_retry_delay_second: optional_duration_ms(
            "upstream_server_retry_delay_second_ms",
            defaults.server_retry_delay_second,
        )?,
        continuity_retry_base_delay: optional_duration_ms(
            "upstream_continuity_retry_base_delay_ms",
            defaults.continuity_retry_base_delay,
        )?,
        continuity_retry_max_delay: optional_duration_ms(
            "upstream_continuity_retry_max_delay_ms",
            defaults.continuity_retry_max_delay,
        )?,
        gateway_body_processing_queue_timeout: optional_duration_ms(
            "upstream_gateway_body_processing_queue_timeout_ms",
            defaults.gateway_body_processing_queue_timeout,
        )?,
        continuity_replay_bytes_per_run: optional_usize(
            "upstream_continuity_replay_bytes_per_run",
            defaults.continuity_replay_bytes_per_run,
        )?,
        continuity_replay_bytes_total: optional_usize(
            "upstream_continuity_replay_bytes_total",
            defaults.continuity_replay_bytes_total,
        )?,
        continuity_replay_events_per_run: i64::try_from(optional_u64(
            "upstream_continuity_replay_events_per_run",
            defaults.continuity_replay_events_per_run as u64,
        )?)
        .map_err(|_| {
            StorageError::Invalid("continuity replay event limit is invalid".to_owned())
        })?,
        continuity_retained_runs: optional_usize(
            "upstream_continuity_retained_runs",
            defaults.continuity_retained_runs,
        )?,
        continuity_resume_page_size: optional_usize(
            "upstream_continuity_resume_page_size",
            defaults.continuity_resume_page_size,
        )?,
        continuity_poll_interval: optional_duration_ms(
            "upstream_continuity_poll_interval_ms",
            defaults.continuity_poll_interval,
        )?,
        continuity_heartbeat_interval: optional_duration_secs(
            "upstream_continuity_heartbeat_interval_seconds",
            defaults.continuity_heartbeat_interval,
        )?,
        continuity_setup_error_bytes: optional_usize(
            "upstream_continuity_setup_error_bytes",
            defaults.continuity_setup_error_bytes,
        )?,
        continuity_retention: optional_duration_secs(
            "upstream_continuity_retention_seconds",
            defaults.continuity_retention,
        )?,
        continuity_request_timeout: optional_duration_secs(
            "upstream_continuity_request_timeout_seconds",
            defaults.continuity_request_timeout,
        )?,
        provider_live_events_max_duration: optional_duration_secs(
            "upstream_provider_live_events_max_duration_seconds",
            defaults.provider_live_events_max_duration,
        )?,
        provider_live_events_keepalive: optional_duration_secs(
            "upstream_provider_live_events_keepalive_seconds",
            defaults.provider_live_events_keepalive,
        )?,
        model_discovery_max_pages: optional_usize(
            "upstream_model_discovery_max_pages",
            defaults.model_discovery_max_pages,
        )?,
        discovery_request_timeout: optional_duration_secs(
            "upstream_discovery_request_timeout_seconds",
            defaults.discovery_request_timeout,
        )?,
        usage_response_max_bytes: optional_usize(
            "upstream_usage_response_max_bytes",
            defaults.usage_response_max_bytes,
        )?,
        opencode_usage_timeout: optional_duration_secs(
            "upstream_opencode_usage_timeout_seconds",
            defaults.opencode_usage_timeout,
        )?,
        command_code_usage_timeout: optional_duration_secs(
            "upstream_command_code_usage_timeout_seconds",
            defaults.command_code_usage_timeout,
        )?,
        command_code_optional_usage_timeout: optional_duration_secs(
            "upstream_command_code_optional_usage_timeout_seconds",
            defaults.command_code_optional_usage_timeout,
        )?,
        freebuff_auxiliary_timeout: optional_duration_secs(
            "upstream_freebuff_auxiliary_timeout_seconds",
            defaults.freebuff_auxiliary_timeout,
        )?,
        freebuff_auxiliary_response_max_bytes: optional_usize(
            "upstream_freebuff_auxiliary_response_max_bytes",
            defaults.freebuff_auxiliary_response_max_bytes,
        )?,
        remote_image_max_count: optional_usize(
            "upstream_remote_image_max_count",
            defaults.remote_image_max_count,
        )?,
        remote_image_max_bytes: optional_usize(
            "upstream_remote_image_max_bytes",
            defaults.remote_image_max_bytes,
        )?,
        remote_image_total_max_bytes: optional_usize(
            "upstream_remote_image_total_max_bytes",
            defaults.remote_image_total_max_bytes,
        )?,
        remote_image_connect_timeout: optional_duration_secs(
            "upstream_remote_image_connect_timeout_seconds",
            defaults.remote_image_connect_timeout,
        )?,
        remote_image_request_timeout: optional_duration_secs(
            "upstream_remote_image_request_timeout_seconds",
            defaults.remote_image_request_timeout,
        )?,
        codex_image_preparation_timeout: optional_duration_secs(
            "upstream_codex_image_preparation_timeout_seconds",
            defaults.codex_image_preparation_timeout,
        )?,
        provider_client_cache_ttl: optional_duration_secs(
            "upstream_provider_client_cache_ttl_seconds",
            defaults.provider_client_cache_ttl,
        )?,
        provider_client_cache_max_entries: optional_usize(
            "upstream_provider_client_cache_max_entries",
            defaults.provider_client_cache_max_entries,
        )?,
        provider_client_max_resolved_addresses: optional_usize(
            "upstream_provider_client_max_resolved_addresses",
            defaults.provider_client_max_resolved_addresses,
        )?,
    };
    let settings = OperationalSettings {
        connect_timeout: Duration::from_millis(u64_value("connect_timeout_ms")?),
        request_timeout: Duration::from_millis(u64_value("request_timeout_ms")?),
        stream_idle_timeout: Duration::from_millis(u64_value("stream_idle_timeout_ms")?),
        circuit_breaker_enabled: record.boolean("circuit_breaker_enabled")?,
        circuit_breaker_threshold: u32_value("circuit_breaker_threshold")?,
        circuit_breaker_cooldown: Duration::from_secs(u64_value(
            "circuit_breaker_cooldown_seconds",
        )?),
        gateway_max_in_flight: field_usize(record, "gateway_max_in_flight")?,
        upstream_response_limit_bytes: field_usize(record, "upstream_response_limit_bytes")?,
        admin_api_max_requests: u32_value("admin_api_max_requests")?,
        admin_api_window: Duration::from_secs(u64_value("admin_api_window_seconds")?),
        gateway_key_capacity: u32_value("gateway_key_capacity")?,
        gateway_key_refill_tokens: u32_value("gateway_key_refill_tokens")?,
        gateway_key_refill_interval: Duration::from_millis(u64_value(
            "gateway_key_refill_interval_ms",
        )?),
        request_log_retention_days: u32_value("request_log_retention_days")?,
        request_log_max_rows: u32_value("request_log_max_rows")?,
        upstream,
    };
    settings
        .validate()
        .map_err(|error| StorageError::Invalid(error.to_owned()))?;
    let revision = record.integer("revision")?;
    if revision < 0 {
        return Err(StorageError::Invalid(
            "operational settings revision is invalid".to_owned(),
        ));
    }
    Ok(OperationalSettingsRecord {
        settings,
        revision,
        overridden: record.boolean("overridden")?,
    })
}

pub(crate) fn output_styles_record(
    snapshot: &OutputStylesSnapshot,
) -> Result<Record, StorageError> {
    if snapshot.revision < 0 || snapshot.revision == i64::MAX {
        return Err(StorageError::Invalid(
            "output styles revision is invalid".to_owned(),
        ));
    }
    let styles = output_styles::validate_styles(&snapshot.styles).map_err(StorageError::Invalid)?;
    if !snapshot.overridden && !styles.is_empty() {
        return Err(StorageError::Invalid(
            "output styles defaults cannot contain enabled styles".to_owned(),
        ));
    }
    let encoded = bincode::serialize(&styles)
        .map_err(|error| StorageError::Codec(format!("output styles encoding failed: {error}")))?;
    if encoded.len() > output_styles::MAX_STYLES_PAYLOAD_BYTES {
        return Err(StorageError::Invalid(
            "output styles configuration is too large".to_owned(),
        ));
    }
    Ok(Record::new()
        .with(
            "format_version",
            Field::I64(output_styles::OUTPUT_STYLE_FORMAT_VERSION),
        )
        .with("styles", Field::Bytes(encoded))
        .with("revision", Field::I64(snapshot.revision))
        .with("overridden", Field::Bool(snapshot.overridden)))
}

pub(crate) fn output_styles_from_record(
    record: &Record,
) -> Result<OutputStylesSnapshot, StorageError> {
    if record.integer("format_version")? != output_styles::OUTPUT_STYLE_FORMAT_VERSION {
        return Err(StorageError::Invalid(
            "output styles record format is unsupported".to_owned(),
        ));
    }
    let encoded = record.bytes("styles")?;
    if encoded.len() > output_styles::MAX_STYLES_PAYLOAD_BYTES {
        return Err(StorageError::Invalid(
            "output styles configuration is too large".to_owned(),
        ));
    }
    let styles: Vec<OutputStyleSelection> = bincode::deserialize(encoded).map_err(|error| {
        StorageError::Codec(format!("output styles record is invalid: {error}"))
    })?;
    let normalized = output_styles::validate_styles(&styles).map_err(StorageError::Invalid)?;
    if normalized != styles {
        return Err(StorageError::Invalid(
            "output styles record is not in canonical order".to_owned(),
        ));
    }
    let revision = record.integer("revision")?;
    if revision < 0 || revision == i64::MAX {
        return Err(StorageError::Invalid(
            "output styles revision is invalid".to_owned(),
        ));
    }
    let overridden = record.boolean("overridden")?;
    if !overridden && !normalized.is_empty() {
        return Err(StorageError::Invalid(
            "output styles defaults cannot contain enabled styles".to_owned(),
        ));
    }
    Ok(OutputStylesSnapshot {
        styles: normalized,
        revision,
        overridden,
    })
}

fn usize_field(value: usize, name: &'static str) -> Result<Field, StorageError> {
    i64::try_from(value)
        .map(Field::I64)
        .map_err(|_| StorageError::Invalid(format!("{name} is outside the supported range")))
}

fn field_usize(record: &Record, name: &str) -> Result<usize, StorageError> {
    usize::try_from(record.integer(name)?)
        .map_err(|_| StorageError::Invalid(format!("LMDB record field '{name}' is invalid")))
}

fn optional_field_usize(record: &Record, name: &str) -> Result<Option<usize>, StorageError> {
    record
        .optional_integer(name)?
        .map(|value| {
            usize::try_from(value).map_err(|_| {
                StorageError::Invalid(format!("LMDB record field '{name}' is invalid"))
            })
        })
        .transpose()
}
