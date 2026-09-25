//! LMDB record codec for the gateway resource limits settings.

use super::fields::{field_usize, optional_field_usize, usize_field};
use crate::{
    config::GatewayResourceLimits,
    infra::storage::{Field, Record, StorageError},
};

pub(crate) fn gateway_limits_record(limits: GatewayResourceLimits) -> Result<Record, StorageError> {
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
