use crate::config::{GatewayResourceLimits, OperationalSettings, UpstreamSettings};
use crate::support::output_styles::{self, OutputStyleLevel, OutputStyleSelection};
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::admin) struct OperationalSettingsInput {
    connect_timeout_ms: u64,
    request_timeout_ms: u64,
    stream_idle_timeout_ms: u64,
    circuit_breaker_enabled: bool,
    circuit_breaker_threshold: u64,
    circuit_breaker_cooldown_seconds: u64,
    gateway_max_in_flight: u64,
    #[serde(default)]
    confirm_unlimited: bool,
    upstream_response_limit_mib: u64,
    admin_api_max_requests: u64,
    admin_api_window_seconds: u64,
    gateway_key_capacity: u64,
    gateway_key_refill_tokens: u64,
    gateway_key_refill_interval_ms: u64,
    request_log_retention_days: u64,
    request_log_max_rows: u64,
    #[serde(default)]
    upstream: Option<UpstreamSettingsInput>,
    expected_revision: i64,
}

impl OperationalSettingsInput {
    pub(super) fn into_settings(
        self,
        current_upstream: UpstreamSettings,
    ) -> Result<(OperationalSettings, i64, bool), &'static str> {
        if self.expected_revision < 0 {
            return Err("operational settings revision is invalid");
        }
        let upstream_response_limit_bytes = self
            .upstream_response_limit_mib
            .checked_mul(1024 * 1024)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or("upstream response limit is invalid")?;
        let upstream = self
            .upstream
            .map(UpstreamSettingsInput::into_settings)
            .transpose()?
            .unwrap_or(current_upstream);
        let settings = OperationalSettings {
            connect_timeout: Duration::from_millis(self.connect_timeout_ms),
            request_timeout: Duration::from_millis(self.request_timeout_ms),
            stream_idle_timeout: Duration::from_millis(self.stream_idle_timeout_ms),
            circuit_breaker_enabled: self.circuit_breaker_enabled,
            circuit_breaker_threshold: u32::try_from(self.circuit_breaker_threshold)
                .map_err(|_| "circuit breaker threshold is invalid")?,
            circuit_breaker_cooldown: Duration::from_secs(self.circuit_breaker_cooldown_seconds),
            gateway_max_in_flight: usize::try_from(self.gateway_max_in_flight)
                .map_err(|_| "gateway concurrency is invalid")?,
            upstream_response_limit_bytes,
            admin_api_max_requests: u32::try_from(self.admin_api_max_requests)
                .map_err(|_| "admin API rate limit is invalid")?,
            admin_api_window: Duration::from_secs(self.admin_api_window_seconds),
            gateway_key_capacity: u32::try_from(self.gateway_key_capacity)
                .map_err(|_| "gateway key capacity is invalid")?,
            gateway_key_refill_tokens: u32::try_from(self.gateway_key_refill_tokens)
                .map_err(|_| "gateway key refill amount is invalid")?,
            gateway_key_refill_interval: Duration::from_millis(self.gateway_key_refill_interval_ms),
            request_log_retention_days: u32::try_from(self.request_log_retention_days)
                .map_err(|_| "request log retention is invalid")?,
            request_log_max_rows: u32::try_from(self.request_log_max_rows)
                .map_err(|_| "request log row limit is invalid")?,
            upstream,
        };
        settings.validate()?;
        if settings.gateway_max_in_flight == 0 && !self.confirm_unlimited {
            return Err("confirm unlimited gateway concurrency before saving");
        }
        Ok((settings, self.expected_revision, self.confirm_unlimited))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpstreamSettingsInput {
    server_retry_max_attempts: u64,
    server_retry_delay_first_ms: u64,
    server_retry_delay_second_ms: u64,
    continuity_retry_base_delay_ms: u64,
    continuity_retry_max_delay_ms: u64,
    gateway_body_processing_queue_timeout_ms: u64,
    continuity_replay_bytes_per_run_mib: u64,
    continuity_replay_bytes_total_mib: u64,
    continuity_replay_events_per_run: u64,
    continuity_retained_runs: u64,
    continuity_resume_page_size: u64,
    continuity_poll_interval_ms: u64,
    continuity_heartbeat_interval_seconds: u64,
    continuity_setup_error_kib: u64,
    continuity_retention_seconds: u64,
    continuity_request_timeout_seconds: u64,
    provider_live_events_max_duration_seconds: u64,
    provider_live_events_keepalive_seconds: u64,
    model_discovery_max_pages: u64,
    discovery_request_timeout_seconds: u64,
    usage_response_limit_kib: u64,
    opencode_usage_timeout_seconds: u64,
    command_code_usage_timeout_seconds: u64,
    command_code_optional_usage_timeout_seconds: u64,
    freebuff_auxiliary_timeout_seconds: u64,
    freebuff_auxiliary_response_limit_kib: u64,
    remote_image_max_count: u64,
    remote_image_max_mib: u64,
    remote_image_total_max_mib: u64,
    remote_image_connect_timeout_seconds: u64,
    remote_image_request_timeout_seconds: u64,
    codex_image_preparation_timeout_seconds: u64,
    provider_client_cache_ttl_seconds: u64,
    provider_client_cache_max_entries: u64,
    provider_client_max_resolved_addresses: u64,
}

impl UpstreamSettingsInput {
    fn into_settings(self) -> Result<UpstreamSettings, &'static str> {
        let mib = |value: u64| {
            value
                .checked_mul(1024 * 1024)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or("upstream MiB value is invalid")
        };
        let kib = |value: u64| {
            value
                .checked_mul(1024)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or("upstream KiB value is invalid")
        };
        let settings = UpstreamSettings {
            server_retry_max_attempts: usize::try_from(self.server_retry_max_attempts)
                .map_err(|_| "server retry attempts are invalid")?,
            server_retry_delay_first: Duration::from_millis(self.server_retry_delay_first_ms),
            server_retry_delay_second: Duration::from_millis(self.server_retry_delay_second_ms),
            continuity_retry_base_delay: Duration::from_millis(self.continuity_retry_base_delay_ms),
            continuity_retry_max_delay: Duration::from_millis(self.continuity_retry_max_delay_ms),
            gateway_body_processing_queue_timeout: Duration::from_millis(
                self.gateway_body_processing_queue_timeout_ms,
            ),
            continuity_replay_bytes_per_run: mib(self.continuity_replay_bytes_per_run_mib)?,
            continuity_replay_bytes_total: mib(self.continuity_replay_bytes_total_mib)?,
            continuity_replay_events_per_run: i64::try_from(self.continuity_replay_events_per_run)
                .map_err(|_| "continuity replay event limit is invalid")?,
            continuity_retained_runs: usize::try_from(self.continuity_retained_runs)
                .map_err(|_| "continuity retained run limit is invalid")?,
            continuity_resume_page_size: usize::try_from(self.continuity_resume_page_size)
                .map_err(|_| "continuity resume page size is invalid")?,
            continuity_poll_interval: Duration::from_millis(self.continuity_poll_interval_ms),
            continuity_heartbeat_interval: Duration::from_secs(
                self.continuity_heartbeat_interval_seconds,
            ),
            continuity_setup_error_bytes: kib(self.continuity_setup_error_kib)?,
            continuity_retention: Duration::from_secs(self.continuity_retention_seconds),
            continuity_request_timeout: Duration::from_secs(
                self.continuity_request_timeout_seconds,
            ),
            provider_live_events_max_duration: Duration::from_secs(
                self.provider_live_events_max_duration_seconds,
            ),
            provider_live_events_keepalive: Duration::from_secs(
                self.provider_live_events_keepalive_seconds,
            ),
            model_discovery_max_pages: usize::try_from(self.model_discovery_max_pages)
                .map_err(|_| "model discovery page limit is invalid")?,
            discovery_request_timeout: Duration::from_secs(self.discovery_request_timeout_seconds),
            usage_response_max_bytes: kib(self.usage_response_limit_kib)?,
            opencode_usage_timeout: Duration::from_secs(self.opencode_usage_timeout_seconds),
            command_code_usage_timeout: Duration::from_secs(
                self.command_code_usage_timeout_seconds,
            ),
            command_code_optional_usage_timeout: Duration::from_secs(
                self.command_code_optional_usage_timeout_seconds,
            ),
            freebuff_auxiliary_timeout: Duration::from_secs(
                self.freebuff_auxiliary_timeout_seconds,
            ),
            freebuff_auxiliary_response_max_bytes: kib(self.freebuff_auxiliary_response_limit_kib)?,
            remote_image_max_count: usize::try_from(self.remote_image_max_count)
                .map_err(|_| "remote image count is invalid")?,
            remote_image_max_bytes: mib(self.remote_image_max_mib)?,
            remote_image_total_max_bytes: mib(self.remote_image_total_max_mib)?,
            remote_image_connect_timeout: Duration::from_secs(
                self.remote_image_connect_timeout_seconds,
            ),
            remote_image_request_timeout: Duration::from_secs(
                self.remote_image_request_timeout_seconds,
            ),
            codex_image_preparation_timeout: Duration::from_secs(
                self.codex_image_preparation_timeout_seconds,
            ),
            provider_client_cache_ttl: Duration::from_secs(self.provider_client_cache_ttl_seconds),
            provider_client_cache_max_entries: usize::try_from(
                self.provider_client_cache_max_entries,
            )
            .map_err(|_| "provider client cache size is invalid")?,
            provider_client_max_resolved_addresses: usize::try_from(
                self.provider_client_max_resolved_addresses,
            )
            .map_err(|_| "provider resolved address limit is invalid")?,
        };
        settings
            .validate()
            .map_err(|_| "upstream settings contain an invalid value")?;
        Ok(settings)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::admin) struct ResetOperationalSettingsInput {
    pub(super) expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::admin) struct OutputStylesInput {
    pub(super) styles: Vec<OutputStyleInputEntry>,
    pub(super) expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::admin) struct OutputStyleInputEntry {
    pub(super) id: String,
    pub(super) level: String,
}

impl OutputStylesInput {
    pub(super) fn into_styles(self) -> Result<(Vec<OutputStyleSelection>, i64), &'static str> {
        if self.expected_revision < 0 || self.expected_revision == i64::MAX {
            return Err("output styles revision is invalid");
        }
        if self.styles.len() > output_styles::MAX_STYLE_SELECTIONS {
            return Err("at most three output styles may be enabled");
        }
        let mut styles = Vec::with_capacity(self.styles.len());
        for entry in self.styles {
            if entry.id.trim() != entry.id || entry.level.trim() != entry.level {
                return Err("output style ID or level is not canonical");
            }
            let id = output_styles::OutputStyleId::parse(&entry.id)
                .map_err(|_| "output style ID is invalid")?;
            let level = OutputStyleLevel::parse(&entry.level)
                .map_err(|_| "output style level is invalid")?;
            styles.push(OutputStyleSelection { id, level });
        }
        let styles = output_styles::validate_styles(&styles)
            .map_err(|_| "output styles selection is invalid")?;
        Ok((styles, self.expected_revision))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::admin) struct ResetOutputStylesInput {
    pub(super) expected_revision: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::admin) struct GatewayResourceLimitsInput {
    pub(super) gateway_body_limit_mib: u64,
    pub(super) gateway_body_processing_concurrency: u64,
    pub(super) sse_frame_limit_kib: u64,
    pub(super) sse_buffer_limit_kib: u64,
    pub(super) provider_max_concurrency: u64,
    #[serde(default)]
    pub(super) confirm_unlimited_provider_concurrency: bool,
    #[serde(default)]
    pub(super) stream_continuity_enabled: Option<bool>,
    #[serde(default)]
    pub(super) stream_continuity_max_concurrency: Option<u64>,
    #[serde(default)]
    pub(super) expected_saved: Option<GatewayResourceLimitValuesInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::admin) struct GatewayResourceLimitValuesInput {
    gateway_body_limit_mib: u64,
    gateway_body_processing_concurrency: u64,
    sse_frame_limit_kib: u64,
    sse_buffer_limit_kib: u64,
    provider_max_concurrency: u64,
    #[serde(default)]
    stream_continuity_enabled: Option<bool>,
    #[serde(default)]
    stream_continuity_max_concurrency: Option<u64>,
}

impl GatewayResourceLimitValuesInput {
    pub(super) fn into_limits(
        self,
        preserve_stream_continuity: bool,
        preserve_stream_continuity_max_concurrency: usize,
    ) -> Result<GatewayResourceLimits, &'static str> {
        let body_limit = self
            .gateway_body_limit_mib
            .checked_mul(1024 * 1024)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or("gateway body limit is invalid")?;
        let frame_limit = self
            .sse_frame_limit_kib
            .checked_mul(1024)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or("SSE frame limit is invalid")?;
        let buffer_limit = self
            .sse_buffer_limit_kib
            .checked_mul(1024)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or("SSE buffer limit is invalid")?;
        let provider_max_concurrency = usize::try_from(self.provider_max_concurrency)
            .map_err(|_| "provider concurrency is invalid")?;
        let gateway_body_processing_concurrency =
            usize::try_from(self.gateway_body_processing_concurrency)
                .map_err(|_| "gateway body processing concurrency is invalid")?;
        let stream_continuity_max_concurrency = self
            .stream_continuity_max_concurrency
            .map(usize::try_from)
            .transpose()
            .map_err(|_| "stream continuity concurrency is invalid")?
            .unwrap_or(preserve_stream_continuity_max_concurrency);
        let limits = GatewayResourceLimits {
            gateway_body_limit_bytes: body_limit,
            gateway_body_processing_concurrency,
            sse_frame_limit_bytes: frame_limit,
            sse_buffer_limit_bytes: buffer_limit,
            provider_max_concurrency,
            stream_continuity_enabled: self
                .stream_continuity_enabled
                .unwrap_or(preserve_stream_continuity),
            stream_continuity_max_concurrency,
        };
        limits.validate()?;
        Ok(limits)
    }
}

impl GatewayResourceLimitsInput {
    pub(super) fn into_limits(
        self,
        preserve_stream_continuity: bool,
        preserve_stream_continuity_max_concurrency: usize,
    ) -> Result<(GatewayResourceLimits, Option<GatewayResourceLimits>, bool), &'static str> {
        let confirm_unlimited_provider_concurrency = self.confirm_unlimited_provider_concurrency;
        let limits = GatewayResourceLimitValuesInput {
            gateway_body_limit_mib: self.gateway_body_limit_mib,
            gateway_body_processing_concurrency: self.gateway_body_processing_concurrency,
            sse_frame_limit_kib: self.sse_frame_limit_kib,
            sse_buffer_limit_kib: self.sse_buffer_limit_kib,
            provider_max_concurrency: self.provider_max_concurrency,
            stream_continuity_enabled: self.stream_continuity_enabled,
            stream_continuity_max_concurrency: self.stream_continuity_max_concurrency,
        }
        .into_limits(
            preserve_stream_continuity,
            preserve_stream_continuity_max_concurrency,
        )?;
        let expected_saved = self
            .expected_saved
            .map(|expected| {
                expected.into_limits(
                    preserve_stream_continuity,
                    preserve_stream_continuity_max_concurrency,
                )
            })
            .transpose()?;
        Ok((
            limits,
            expected_saved,
            confirm_unlimited_provider_concurrency,
        ))
    }
}
