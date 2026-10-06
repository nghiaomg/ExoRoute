//! Post-stream finalization logging: circuit probe outcome, continuity run
//! status, and the bounded gateway request log entry for a finished stream.
//!
//! Every statistics event the gateway records has a request-log row behind it:
//! the normal finalizers below cover streams that reach their terminal event,
//! and [`StreamLog`]'s `Drop` covers the streams that are abandoned first
//! (client disconnect, client cancellation, or gateway shutdown).

use super::*;

/// 499 "Client Closed Request" is the established non-standard status for a
/// client that went away before its response completed. `from_u16` is const
/// and only fails outside 100..=999, so the fallback is unreachable for this
/// constant; it keeps the value total instead of using an `expect`.
const CLIENT_CLOSED_REQUEST: StatusCode = match StatusCode::from_u16(499) {
    Ok(status) => status,
    Err(_) => StatusCode::BAD_GATEWAY,
};

impl Drop for StreamLog {
    /// Records the abandoned stream. Statistics already count this request
    /// (the analytics guard completes when the response body is dropped), so
    /// without this entry the request history would silently drop requests
    /// that the totals still include. `Drop` may not await: the record is
    /// bounded and handed to the telemetry queue with a non-blocking send.
    fn drop(&mut self) {
        if self.finalized() {
            return;
        }
        log_abandoned_stream(self);
    }
}

/// Logs a stream that ended without a terminal event: the client disconnected
/// or cancelled mid-stream, or a gateway shutdown interrupted it. Token counts
/// stay unknown unless the upstream already reported usage.
fn log_abandoned_stream(log: &StreamLog) {
    let (status, error) = if log.interrupted_by_shutdown() {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "gateway shutdown interrupted the stream before it completed",
        )
    } else {
        (
            CLIENT_CLOSED_REQUEST,
            "client disconnected before the stream completed",
        )
    };
    let (input_tokens, output_tokens, cached_tokens, cache_input_tokens) = log
        .analytics
        .as_ref()
        .map(RequestAnalytics::observed_tokens)
        .unwrap_or((0, 0, 0, 0));
    let has_usage = input_tokens > 0 || output_tokens > 0;
    let has_cache = cached_tokens > 0 || cache_input_tokens > 0;
    log_request(
        &log.state,
        Some(&log.live),
        GatewayRequestLog {
            api_key_id: log.api_key_id.as_deref(),
            provider_credential_id: log.provider_credential_id.as_deref(),
            request_id: &log.request_id,
            route: &log.route_alias,
            provider: Some(&log.provider_id),
            model: &log.model,
            client: log.client_protocol,
            upstream: Some(log.upstream_protocol),
            status,
            duration_ms: log.started.elapsed().as_millis() as i64,
            input_tokens: has_usage.then_some(input_tokens),
            output_tokens: has_usage.then_some(output_tokens),
            cached_tokens: has_cache.then_some(cached_tokens),
            cache_input_tokens: has_cache.then_some(cache_input_tokens),
            // An abandoned stream never delivered its authoritative total
            // usage event, so the cost stays unknown.
            cost_micro_usd: None,
            error: Some(error),
        },
    );
}

/// Record a failed stream: marks the circuit probe as failed, closes the
/// continuity run as failed, and logs the bounded failure request record.
pub(crate) async fn log_failed_stream(
    mut log: StreamLog,
    error: &str,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    cache_input_tokens: Option<i64>,
) {
    log.mark_finalized();
    log.circuit_probe.failed();
    if let Some(run_id) = log.run_id.as_deref()
        && let Err(journal_error) = crate::gateway::continuity::finish_run(
            &log.state.db,
            run_id,
            "failed",
            Some("upstream stream failed"),
        )
        .await
    {
        tracing::warn!(%journal_error, %run_id, "could not save failed stream status");
    }
    log_request(
        &log.state,
        Some(&log.live),
        GatewayRequestLog {
            api_key_id: log.api_key_id.as_deref(),
            provider_credential_id: log.provider_credential_id.as_deref(),
            request_id: &log.request_id,
            route: &log.route_alias,
            provider: Some(&log.provider_id),
            model: &log.model,
            client: log.client_protocol,
            upstream: Some(log.upstream_protocol),
            status: StatusCode::BAD_GATEWAY,
            duration_ms: log.started.elapsed().as_millis() as i64,
            // Preserve token counts observed before a framing or upstream
            // failure. The cost stays unknown because an incomplete stream
            // may not have delivered its authoritative total-usage event.
            input_tokens,
            output_tokens,
            cached_tokens,
            cache_input_tokens,
            cost_micro_usd: None,
            error: Some(error),
        },
    );
}

/// Record a completed stream: marks the circuit probe as succeeded, closes
/// the continuity run as completed, and logs the success request record.
pub(crate) async fn log_completed_stream(
    mut log: StreamLog,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    cache_input_tokens: Option<i64>,
    cost_micro_usd: Option<i64>,
) {
    log.mark_finalized();
    log.circuit_probe.succeeded();
    if let Some(run_id) = log.run_id.as_deref()
        && let Err(journal_error) =
            crate::gateway::continuity::finish_run(&log.state.db, run_id, "completed", None).await
    {
        tracing::warn!(%journal_error, %run_id, "could not save completed stream status");
    }
    log_request(
        &log.state,
        Some(&log.live),
        GatewayRequestLog {
            api_key_id: log.api_key_id.as_deref(),
            provider_credential_id: log.provider_credential_id.as_deref(),
            request_id: &log.request_id,
            route: &log.route_alias,
            provider: Some(&log.provider_id),
            model: &log.model,
            client: log.client_protocol,
            upstream: Some(log.upstream_protocol),
            status: StatusCode::OK,
            duration_ms: log.started.elapsed().as_millis() as i64,
            input_tokens,
            output_tokens,
            cached_tokens,
            cache_input_tokens,
            cost_micro_usd,
            error: None,
        },
    );
}
