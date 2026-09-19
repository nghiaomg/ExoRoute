//! Post-stream finalization logging: circuit probe outcome, continuity run
//! status, and the bounded gateway request log entry for a finished stream.

use super::*;

/// Record a failed stream: marks the circuit probe as failed, closes the
/// continuity run as failed, and logs the bounded failure request record.
pub(crate) async fn log_failed_stream(
    log: StreamLog,
    error: &str,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    cache_input_tokens: Option<i64>,
) {
    let mut circuit_probe = log.circuit_probe;
    circuit_probe.failed();
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
    )
    .await;
}

/// Record a completed stream: marks the circuit probe as succeeded, closes
/// the continuity run as completed, and logs the success request record.
pub(crate) async fn log_completed_stream(
    log: StreamLog,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    cache_input_tokens: Option<i64>,
    cost_micro_usd: Option<i64>,
) {
    let mut circuit_probe = log.circuit_probe;
    circuit_probe.succeeded();
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
    )
    .await;
}
