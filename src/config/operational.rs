use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpstreamSettings {
    pub server_retry_max_attempts: usize,
    pub server_retry_delay_first: Duration,
    pub server_retry_delay_second: Duration,
    pub continuity_retry_base_delay: Duration,
    pub continuity_retry_max_delay: Duration,
    pub gateway_body_processing_queue_timeout: Duration,
    pub continuity_replay_bytes_per_run: usize,
    pub continuity_replay_bytes_total: usize,
    pub continuity_replay_events_per_run: i64,
    pub continuity_retained_runs: usize,
    pub continuity_resume_page_size: usize,
    pub continuity_poll_interval: Duration,
    pub continuity_heartbeat_interval: Duration,
    pub continuity_setup_error_bytes: usize,
    pub continuity_retention: Duration,
    pub continuity_request_timeout: Duration,
    pub provider_live_events_max_duration: Duration,
    pub provider_live_events_keepalive: Duration,
    pub model_discovery_max_pages: usize,
    pub discovery_request_timeout: Duration,
    pub usage_response_max_bytes: usize,
    pub opencode_usage_timeout: Duration,
    pub command_code_usage_timeout: Duration,
    pub command_code_optional_usage_timeout: Duration,
    pub freebuff_auxiliary_timeout: Duration,
    pub freebuff_auxiliary_response_max_bytes: usize,
    pub remote_image_max_count: usize,
    pub remote_image_max_bytes: usize,
    pub remote_image_total_max_bytes: usize,
    pub remote_image_connect_timeout: Duration,
    pub remote_image_request_timeout: Duration,
    pub codex_image_preparation_timeout: Duration,
    pub provider_client_cache_ttl: Duration,
    pub provider_client_cache_max_entries: usize,
    pub provider_client_max_resolved_addresses: usize,
}

impl UpstreamSettings {
    pub fn validate(self) -> Result<(), &'static str> {
        if !(1..=8).contains(&self.server_retry_max_attempts) {
            return Err("server retry attempts must be between 1 and 8");
        }
        if !valid_duration_ms(self.server_retry_delay_first, 1, 60_000)
            || !valid_duration_ms(self.server_retry_delay_second, 1, 60_000)
        {
            return Err("server retry delays must be between 1 and 60000 milliseconds");
        }
        if !valid_duration_ms(self.continuity_retry_base_delay, 1, 60_000)
            || !valid_duration_ms(self.continuity_retry_max_delay, 1, 3_600_000)
            || self.continuity_retry_max_delay < self.continuity_retry_base_delay
        {
            return Err("continuity retry delays are invalid");
        }
        if !valid_duration_ms(self.gateway_body_processing_queue_timeout, 100, 60_000) {
            return Err("gateway body queue timeout must be between 100 and 60000 milliseconds");
        }
        if !(MIB..=64 * MIB).contains(&self.continuity_replay_bytes_per_run)
            || !(self.continuity_replay_bytes_per_run..=1024 * MIB)
                .contains(&self.continuity_replay_bytes_total)
            || !(1..=1_000_000).contains(&self.continuity_replay_events_per_run)
            || !(1..=1_024).contains(&self.continuity_retained_runs)
            || !(1..=64).contains(&self.continuity_resume_page_size)
        {
            return Err("stream continuity storage limits are invalid");
        }
        if !valid_duration_ms(self.continuity_poll_interval, 10, 5_000)
            || !valid_duration_ms(self.continuity_heartbeat_interval, 1_000, 300_000)
            || !valid_duration_ms(
                self.continuity_retention,
                3_600_000,
                7 * 24 * 60 * 60 * 1000,
            )
            || !valid_duration_ms(
                self.continuity_request_timeout,
                60_000,
                7 * 24 * 60 * 60 * 1000,
            )
            || !(1024..=1024 * 1024).contains(&self.continuity_setup_error_bytes)
        {
            return Err("stream continuity timing and error limits are invalid");
        }
        if !valid_duration_ms(self.provider_live_events_max_duration, 60_000, 3_600_000)
            || !valid_duration_ms(self.provider_live_events_keepalive, 1_000, 300_000)
            || self.provider_live_events_keepalive >= self.provider_live_events_max_duration
        {
            return Err("provider live event stream settings are invalid");
        }
        if !(1..=100).contains(&self.model_discovery_max_pages)
            || !valid_duration_ms(self.discovery_request_timeout, 100, 300_000)
            || !(64 * KIB..=16 * MIB).contains(&self.usage_response_max_bytes)
            || !valid_duration_ms(self.opencode_usage_timeout, 100, 300_000)
            || !valid_duration_ms(self.command_code_usage_timeout, 100, 300_000)
            || !valid_duration_ms(self.command_code_optional_usage_timeout, 100, 300_000)
            || self.command_code_optional_usage_timeout > self.command_code_usage_timeout
            || !valid_duration_ms(self.freebuff_auxiliary_timeout, 100, 300_000)
            || !(4 * KIB..=16 * MIB).contains(&self.freebuff_auxiliary_response_max_bytes)
        {
            return Err("provider discovery and usage settings are invalid");
        }
        if !(1..=64).contains(&self.remote_image_max_count)
            || !(256 * KIB..=64 * MIB).contains(&self.remote_image_max_bytes)
            || !(self.remote_image_max_bytes..=256 * MIB)
                .contains(&self.remote_image_total_max_bytes)
            || !valid_duration_ms(self.remote_image_connect_timeout, 100, 60_000)
            || !valid_duration_ms(self.remote_image_request_timeout, 100, 300_000)
            || !valid_duration_ms(self.codex_image_preparation_timeout, 100, 600_000)
        {
            return Err("remote image settings are invalid");
        }
        if !valid_duration_ms(self.provider_client_cache_ttl, 1_000, 3_600_000)
            || !(1..=4_096).contains(&self.provider_client_cache_max_entries)
            || !(1..=256).contains(&self.provider_client_max_resolved_addresses)
        {
            return Err("provider client cache settings are invalid");
        }
        Ok(())
    }
}

fn valid_duration_ms(value: Duration, minimum: u64, maximum: u64) -> bool {
    let millis = value.as_millis();
    value.subsec_nanos().is_multiple_of(1_000_000)
        && (minimum as u128..=maximum as u128).contains(&millis)
}

impl Default for UpstreamSettings {
    fn default() -> Self {
        Self {
            server_retry_max_attempts: 3,
            server_retry_delay_first: Duration::from_millis(250),
            server_retry_delay_second: Duration::from_millis(500),
            continuity_retry_base_delay: Duration::from_millis(250),
            continuity_retry_max_delay: Duration::from_secs(60),
            gateway_body_processing_queue_timeout: Duration::from_secs(5),
            continuity_replay_bytes_per_run: 8 * MIB,
            continuity_replay_bytes_total: 128 * MIB,
            continuity_replay_events_per_run: 50_000,
            continuity_retained_runs: 128,
            continuity_resume_page_size: 8,
            continuity_poll_interval: Duration::from_millis(200),
            continuity_heartbeat_interval: Duration::from_secs(15),
            continuity_setup_error_bytes: 64 * KIB,
            continuity_retention: Duration::from_secs(24 * 60 * 60),
            continuity_request_timeout: Duration::from_secs(24 * 60 * 60),
            provider_live_events_max_duration: Duration::from_secs(5 * 60),
            provider_live_events_keepalive: Duration::from_secs(30),
            model_discovery_max_pages: 20,
            discovery_request_timeout: Duration::from_secs(10),
            usage_response_max_bytes: 256 * KIB,
            opencode_usage_timeout: Duration::from_secs(8),
            command_code_usage_timeout: Duration::from_secs(10),
            command_code_optional_usage_timeout: Duration::from_secs(3),
            freebuff_auxiliary_timeout: Duration::from_secs(10),
            freebuff_auxiliary_response_max_bytes: 64 * KIB,
            remote_image_max_count: 8,
            remote_image_max_bytes: 6 * MIB,
            remote_image_total_max_bytes: 8 * MIB,
            remote_image_connect_timeout: Duration::from_secs(3),
            remote_image_request_timeout: Duration::from_secs(15),
            codex_image_preparation_timeout: Duration::from_secs(30),
            provider_client_cache_ttl: Duration::from_secs(30),
            provider_client_cache_max_entries: 256,
            provider_client_max_resolved_addresses: 64,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationalSettings {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub stream_idle_timeout: Duration,
    pub circuit_breaker_enabled: bool,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_cooldown: Duration,
    /// Zero disables the total gateway admission gate.
    pub gateway_max_in_flight: usize,
    pub upstream_response_limit_bytes: usize,
    pub admin_api_max_requests: u32,
    pub admin_api_window: Duration,
    pub gateway_key_capacity: u32,
    pub gateway_key_refill_tokens: u32,
    pub gateway_key_refill_interval: Duration,
    pub request_log_retention_days: u32,
    pub request_log_max_rows: u32,
    pub upstream: UpstreamSettings,
}

impl OperationalSettings {
    pub const DEFAULT_GATEWAY_MAX_IN_FLIGHT: usize = 64;
    pub const MAX_GATEWAY_MAX_IN_FLIGHT: usize = 64;
    pub const DEFAULT_UPSTREAM_RESPONSE_LIMIT_BYTES: usize = 16 * 1024 * 1024;
    pub const MIN_UPSTREAM_RESPONSE_LIMIT_BYTES: usize = 1024 * 1024;
    pub const MAX_UPSTREAM_RESPONSE_LIMIT_BYTES: usize = 16 * 1024 * 1024;
    pub const MAX_REQUEST_LOG_ROWS: u32 = 100_000;

    pub fn validate(self) -> Result<(), &'static str> {
        if !(100..=120_000).contains(&self.connect_timeout.as_millis()) {
            return Err("connect timeout must be between 100 and 120000 milliseconds");
        }
        if !(100..=86_400_000).contains(&self.request_timeout.as_millis()) {
            return Err("request timeout must be between 100 and 86400000 milliseconds");
        }
        if !(100..=3_600_000).contains(&self.stream_idle_timeout.as_millis()) {
            return Err("stream idle timeout must be between 100 and 3600000 milliseconds");
        }
        if !(1..=100).contains(&self.circuit_breaker_threshold) {
            return Err("circuit breaker threshold must be between 1 and 100");
        }
        if !(1..=86_400).contains(&self.circuit_breaker_cooldown.as_secs())
            || !self.circuit_breaker_cooldown.subsec_nanos().eq(&0)
        {
            return Err("circuit breaker cooldown must be between 1 and 86400 seconds");
        }
        if self.gateway_max_in_flight != 0
            && !(1..=Self::MAX_GATEWAY_MAX_IN_FLIGHT).contains(&self.gateway_max_in_flight)
        {
            return Err("gateway in-flight limit must be unlimited or between 1 and 64");
        }
        if !(Self::MIN_UPSTREAM_RESPONSE_LIMIT_BYTES..=Self::MAX_UPSTREAM_RESPONSE_LIMIT_BYTES)
            .contains(&self.upstream_response_limit_bytes)
            || !self.upstream_response_limit_bytes.is_multiple_of(MIB)
        {
            return Err("upstream response limit must be between 1 and 16 MiB");
        }
        if self.admin_api_max_requests == 0
            || self.admin_api_window.is_zero()
            || self.admin_api_window > Duration::from_secs(86_400)
        {
            return Err("admin API rate limit must be positive with a window up to one day");
        }
        if self.gateway_key_capacity == 0
            || self.gateway_key_refill_tokens == 0
            || self.gateway_key_refill_interval.is_zero()
            || self.gateway_key_refill_interval > Duration::from_secs(86_400)
        {
            return Err("gateway API key rate limit values are invalid");
        }
        if !(1..=365).contains(&self.request_log_retention_days) {
            return Err("request log retention must be between 1 and 365 days");
        }
        if !(1..=Self::MAX_REQUEST_LOG_ROWS).contains(&self.request_log_max_rows) {
            return Err("request log row limit must be between 1 and 100000");
        }
        self.upstream.validate()?;
        Ok(())
    }

    pub fn from_env() -> Result<Self, String> {
        let connect_timeout = duration_ms_env("CONNECT_TIMEOUT_MS", 10_000, 100, 120_000)?;
        let request_timeout = duration_ms_env("REQUEST_TIMEOUT_MS", 300_000, 100, 86_400_000)?;
        let stream_idle_timeout =
            duration_ms_env("STREAM_IDLE_TIMEOUT_MS", 120_000, 100, 3_600_000)?;
        let circuit_breaker_threshold = parse_env("CIRCUIT_BREAKER_FAILURE_THRESHOLD", 5u32)?;
        let circuit_breaker_cooldown_seconds =
            parse_env("CIRCUIT_BREAKER_COOLDOWN_SECONDS", 30u64)?;
        let response_limit_mib = parse_env("UPSTREAM_RESPONSE_LIMIT_MIB", 16u64)?;
        let response_limit_bytes = response_limit_mib
            .checked_mul(1024 * 1024)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| "UPSTREAM_RESPONSE_LIMIT_MIB is invalid".to_owned())?;
        let gateway_max_in_flight = parse_env("GATEWAY_MAX_IN_FLIGHT", 64u64)?;
        let gateway_max_in_flight = usize::try_from(gateway_max_in_flight)
            .map_err(|_| "GATEWAY_MAX_IN_FLIGHT is invalid".to_owned())?;
        if gateway_max_in_flight == 0 {
            return Err(
                "GATEWAY_MAX_IN_FLIGHT cannot be unlimited; set unlimited from the dashboard and confirm its resource warning".to_owned(),
            );
        }
        let admin_api_max_requests = parse_env("ADMIN_API_RATE_LIMIT_REQUESTS", 300u32)?;
        let admin_api_window =
            Duration::from_secs(parse_env("ADMIN_API_RATE_LIMIT_WINDOW_SECONDS", 60u64)?);
        let gateway_key_capacity = parse_env("GATEWAY_KEY_RATE_LIMIT_CAPACITY", 5_000u32)?;
        let gateway_key_refill_tokens = parse_env("GATEWAY_KEY_RATE_LIMIT_REFILL_TOKENS", 1u32)?;
        let gateway_key_refill_interval = Duration::from_millis(parse_env(
            "GATEWAY_KEY_RATE_LIMIT_REFILL_INTERVAL_MS",
            1u64,
        )?);
        let request_log_retention_days = parse_env("REQUEST_LOG_RETENTION_DAYS", 30u32)?;
        let request_log_max_rows = parse_env("REQUEST_LOG_MAX_ROWS", 100_000u32)?;
        let settings = Self {
            connect_timeout,
            request_timeout,
            stream_idle_timeout,
            circuit_breaker_enabled: env::var("CIRCUIT_BREAKER_ENABLED")
                .map(|value| value != "false")
                .unwrap_or(true),
            circuit_breaker_threshold,
            circuit_breaker_cooldown: Duration::from_secs(circuit_breaker_cooldown_seconds),
            gateway_max_in_flight,
            upstream_response_limit_bytes: response_limit_bytes,
            admin_api_max_requests,
            admin_api_window,
            gateway_key_capacity,
            gateway_key_refill_tokens,
            gateway_key_refill_interval,
            request_log_retention_days,
            request_log_max_rows,
            upstream: UpstreamSettings::default(),
        };
        settings.validate().map_err(str::to_owned)?;
        Ok(settings)
    }
}

impl Default for OperationalSettings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(300),
            stream_idle_timeout: Duration::from_secs(120),
            circuit_breaker_enabled: true,
            circuit_breaker_threshold: 5,
            circuit_breaker_cooldown: Duration::from_secs(30),
            gateway_max_in_flight: Self::DEFAULT_GATEWAY_MAX_IN_FLIGHT,
            upstream_response_limit_bytes: Self::DEFAULT_UPSTREAM_RESPONSE_LIMIT_BYTES,
            admin_api_max_requests: 300,
            admin_api_window: Duration::from_secs(60),
            gateway_key_capacity: 5_000,
            gateway_key_refill_tokens: 1,
            gateway_key_refill_interval: Duration::from_millis(1),
            request_log_retention_days: 30,
            request_log_max_rows: Self::MAX_REQUEST_LOG_ROWS,
            upstream: UpstreamSettings::default(),
        }
    }
}

pub(crate) fn parse_env<T: std::str::FromStr>(key: &str, default: T) -> Result<T, String> {
    match env::var(key) {
        Ok(value) => value
            .parse()
            .map_err(|_| format!("invalid value for {key}")),
        Err(_) => Ok(default),
    }
}

pub(crate) fn duration_ms_env(
    key: &str,
    default: u64,
    minimum: u64,
    maximum: u64,
) -> Result<Duration, String> {
    let milliseconds = parse_env(key, default)?;
    checked_duration_ms(key, milliseconds, minimum, maximum)
}

pub(crate) fn checked_duration_ms(
    key: &str,
    milliseconds: u64,
    minimum: u64,
    maximum: u64,
) -> Result<Duration, String> {
    if !(minimum..=maximum).contains(&milliseconds) {
        return Err(format!(
            "{key} must be between {minimum} and {maximum} milliseconds"
        ));
    }
    Ok(Duration::from_millis(milliseconds))
}
