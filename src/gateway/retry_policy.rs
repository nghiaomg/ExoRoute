use super::{Duration, StatusCode};
use crate::config::UpstreamSettings;

pub(super) fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

pub(super) fn can_fail_over_after_provider_rejection(status: StatusCode) -> bool {
    status.is_server_error() || (status.is_client_error() && status != StatusCode::REQUEST_TIMEOUT)
}

pub(super) fn server_retry_deadline(
    per_attempt_timeout: Duration,
    upstream: UpstreamSettings,
) -> Duration {
    let attempt_budget = (0..upstream.server_retry_max_attempts)
        .fold(Duration::ZERO, |total, _| {
            total.saturating_add(per_attempt_timeout)
        });
    let backoff_budget = if upstream.server_retry_max_attempts > 1 {
        upstream
            .server_retry_delay_first
            .saturating_add(upstream.server_retry_delay_second)
    } else {
        Duration::ZERO
    };
    attempt_budget
        .saturating_add(backoff_budget)
        .min(upstream.continuity_request_timeout)
}

pub(super) fn server_retry_delay(attempt: usize, upstream: UpstreamSettings) -> Duration {
    if attempt == 0 {
        upstream.server_retry_delay_first
    } else {
        upstream.server_retry_delay_second
    }
}

pub(super) fn continuity_retry_delay(
    attempt: usize,
    retry_after: Option<Duration>,
    upstream: UpstreamSettings,
) -> Duration {
    let exponent = attempt.min(8) as u32;
    let multiplier = 1_u32 << exponent;
    let exponential = upstream
        .continuity_retry_base_delay
        .saturating_mul(multiplier)
        .min(upstream.continuity_retry_max_delay);
    retry_after
        .unwrap_or(exponential)
        .min(upstream.continuity_retry_max_delay)
}
