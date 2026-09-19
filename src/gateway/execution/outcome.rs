use super::*;
use provider::{ProviderCredential, ProviderUpstreamResult, mark_provider_key_invalid};
use target_preparation::AttemptContext;

/// A completed upstream result together with the RAII permits and adapter
/// context needed to build the client response.
///
/// The circuit probe and provider bulkhead permit travel with the result so
/// the streaming path can move them into the stream log (releasing them
/// when the stream ends) and the decode path can record `succeeded()`.
pub(super) struct CompletedUpstream {
    pub(super) result: ProviderUpstreamResult,
    pub(super) circuit_probe: crate::state::CircuitProbePermit,
    pub(super) provider_permit: DynamicSemaphorePermit,
    pub(super) adapter: &'static dyn provider_adapters::ProviderAdapter,
    pub(super) adapter_id: String,
    pub(super) provider_id: String,
    pub(super) upstream_protocol: UpstreamProtocol,
}

/// What the dispatch stage decided about one target.
pub(super) enum DispatchOutcome {
    /// The upstream produced a usable response or stream.
    Completed(Box<CompletedUpstream>),
    /// This target failed; the loop should try the next one. The failure is
    /// already recorded and logged.
    TargetFailed,
    /// Stop the target loop entirely and respond with the recorded failure,
    /// matching the original pipeline's `break` branches.
    Terminal,
    /// Storage failed mid-rotation; the request must stop immediately.
    Storage(StorageError),
}

/// All credentials were rejected: summarize the credential-level rejection
/// the way the original pipeline did before trying the next target.
pub(super) async fn all_credentials_rejected(
    attempt: &mut AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
    live: &RequestLiveGuard,
) -> DispatchOutcome {
    let Some(status) = failures.last_key_rejection else {
        return adapter_sse_failure_outcome(attempt, failures, log, live).await;
    };
    failures.last_status = if is_quota_rejection(&attempt.provider.adapter_id, status) {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::BAD_GATEWAY
    };
    failures.last_error = format!(
        "provider '{}' rejected or rate-limited all available API keys (last response HTTP {})",
        attempt.provider.id,
        status.as_u16()
    );
    if failures.upstream_retry_after.is_some() {
        failures
            .last_error
            .push_str("; retry after the provider's cooldown");
    }
    log_request(
        &attempt.scope.state,
        Some(live),
        log.provider_failure(
            &attempt.provider.id,
            attempt.upstream_protocol,
            failures.selected_credential_id.as_deref(),
            status,
            &failures.last_error,
            attempt.target_model,
        ),
    )
    .await;
    // These explicit 4xx responses reject the request, so trying the next
    // configured provider cannot duplicate a completed inference.
    DispatchOutcome::TargetFailed
}

/// Applies the same key-failure bookkeeping to an adapter preflight rejection
/// that the HTTP-response classifier applies to an upstream rejection.
pub(super) async fn record_credential_rejection(
    attempt: &AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
    live: &RequestLiveGuard,
    credential: &ProviderCredential,
    error: provider_adapters::AdapterRequestError,
) -> Result<(), StorageError> {
    let status = error.status.unwrap_or(StatusCode::BAD_GATEWAY);
    failures.last_key_rejection = Some(status);
    if is_quota_rejection(&attempt.provider.adapter_id, status) {
        failures.upstream_retry_after = error.retry_after.clone();
        if attempt.provider.adapter_id == provider_adapters::ANTIGRAVITY_ADAPTER_ID
            && let Some(key_id) = credential.id.as_deref()
        {
            attempt
                .scope
                .state
                .record_antigravity_quota_rejection(
                    &attempt.provider.id,
                    key_id,
                    attempt.target_model,
                    error.retry_after.as_ref().and_then(retry_after_duration),
                )
                .await;
        }
    }
    let logged_error = provider_http_error_message(&attempt.provider.id, status, &error.message);
    if (status == StatusCode::UNAUTHORIZED
        || (attempt.provider.adapter_id == provider_adapters::ANTIGRAVITY_ADAPTER_ID
            && status == StatusCode::FORBIDDEN))
        && let Some(key_id) = credential.id.as_deref()
    {
        mark_provider_key_invalid(&attempt.scope.state, &attempt.provider.id, key_id).await?;
    }
    log_request(
        &attempt.scope.state,
        Some(live),
        log.provider_failure(
            &attempt.provider.id,
            attempt.upstream_protocol,
            credential.id.as_deref(),
            status,
            &logged_error,
            attempt.target_model,
        ),
    )
    .await;
    Ok(())
}

/// The adapter stream failed without being safe to fail over from.
async fn adapter_sse_failure_outcome(
    attempt: &mut AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
    live: &RequestLiveGuard,
) -> DispatchOutcome {
    if let Some(error) = failures.last_adapter_sse_failure.take() {
        attempt.circuit_probe.failed();
        failures.last_status = StatusCode::BAD_GATEWAY;
        failures.last_error = format!(
            "provider '{}' provider adapter stream failed: {}",
            attempt.provider.id, error.message
        );
        log_request(
            &attempt.scope.state,
            Some(live),
            log.provider_failure(
                &attempt.provider.id,
                attempt.upstream_protocol,
                failures.selected_credential_id.as_deref(),
                failures.last_status,
                &failures.last_error,
                attempt.target_model,
            ),
        )
        .await;
        // This was an explicit terminal Codex failure, and the non-streaming
        // caller has not received a response body.
    }
    DispatchOutcome::TargetFailed
}

/// The upstream request ended in a transport error (timeout, connect).
pub(super) async fn transport_failure(
    attempt: &mut AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
    live: &RequestLiveGuard,
    error: reqwest::Error,
) -> DispatchOutcome {
    attempt.circuit_probe.failed();
    failures.last_status = StatusCode::BAD_GATEWAY;
    let failure_kind = if error.is_timeout() {
        "timed out"
    } else if error.is_connect() {
        "could not connect"
    } else {
        "failed before receiving a response"
    };
    failures.last_error = format!("provider '{}' request {failure_kind}", attempt.provider.id);
    log_request(
        &attempt.scope.state,
        Some(live),
        log.provider_failure(
            &attempt.provider.id,
            attempt.upstream_protocol,
            failures.selected_credential_id.as_deref(),
            failures.last_status,
            &failures.last_error,
            attempt.target_model,
        ),
    )
    .await;
    // The server retry policy owns 5xx/transport failures. Try the next
    // configured target in this attempt before the bounded request-level
    // retry loop runs again.
    DispatchOutcome::TargetFailed
}

/// The upstream returned an unsuccessful HTTP status: classify it into the
/// recorded status/retry-after exactly as the original pipeline did.
pub(super) async fn unsuccessful_status(
    attempt: &mut AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
    live: &RequestLiveGuard,
    upstream: reqwest::Response,
    status: StatusCode,
) -> DispatchOutcome {
    let provider_id = &attempt.provider.id;
    if is_quota_rejection(&attempt.provider.adapter_id, status) {
        failures.upstream_retry_after = upstream
            .headers()
            .get(header::RETRY_AFTER)
            .filter(|value| value.as_bytes().len() <= 128)
            .cloned();
        attempt.circuit_probe.abort();
    } else if is_retryable(status) {
        attempt.circuit_probe.failed();
    } else {
        attempt.circuit_probe.abort();
    }
    failures.last_status = if is_quota_rejection(&attempt.provider.adapter_id, status) {
        StatusCode::TOO_MANY_REQUESTS
    } else if status == StatusCode::UNAUTHORIZED
        || status == StatusCode::FORBIDDEN
        || status.is_server_error()
        || status == StatusCode::REQUEST_TIMEOUT
    {
        StatusCode::BAD_GATEWAY
    } else if status.is_client_error()
        && status != StatusCode::REQUEST_TIMEOUT
        && status != StatusCode::TOO_MANY_REQUESTS
    {
        status
    } else {
        StatusCode::BAD_GATEWAY
    };
    failures.last_error = format!("provider '{provider_id}' returned HTTP {status}");
    let provider_error = read_provider_error_detail(
        upstream,
        attempt
            .scope
            .operational_settings
            .upstream_response_limit_bytes,
    )
    .await;
    let logged_error = provider_http_error_message(provider_id, status, &provider_error);
    log_request(
        &attempt.scope.state,
        Some(live),
        log.provider_failure(
            provider_id,
            attempt.upstream_protocol,
            failures.selected_credential_id.as_deref(),
            status,
            &logged_error,
            attempt.target_model,
        ),
    )
    .await;
    // Explicit 4xx rejections and server failures are handled inside the
    // gateway; the outer bounded retry loop prevents either outcome from
    // being returned immediately to the client.
    if can_fail_over_after_provider_rejection(status) {
        DispatchOutcome::TargetFailed
    } else {
        DispatchOutcome::Terminal
    }
}
