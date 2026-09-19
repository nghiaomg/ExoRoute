use super::*;
use provider::{ProviderCredential, ProviderUpstreamResult};
use send::SendStep;
use target_preparation::AttemptContext;

/// Classifies the upstream response into a completed result or a rotate
/// step, mirroring the original pipeline's response match arm by arm.
pub(super) async fn classify_response(
    attempt: &mut AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
    live: &RequestLiveGuard,
    credential: &ProviderCredential,
    response: Result<reqwest::Response, reqwest::Error>,
) -> SendStep {
    let provider_id = &attempt.provider.id;
    match response {
        Ok(response)
            if credential.id.is_some()
                && provider_adapters::retry_upstream_response_as_key_rejection(
                    &attempt.provider.adapter_id,
                )
                && provider_adapters::is_key_rejection_status(
                    &attempt.provider.adapter_id,
                    response.status(),
                ) =>
        {
            let status = response.status();
            failures.last_key_rejection = Some(status);
            if is_quota_rejection(&attempt.provider.adapter_id, status) {
                failures.upstream_retry_after = response
                    .headers()
                    .get(header::RETRY_AFTER)
                    .filter(|value| value.as_bytes().len() <= 128)
                    .cloned();
                record_quota_rejection(attempt, failures, credential).await;
            }
            let provider_error = read_provider_error_detail(
                response,
                attempt
                    .scope
                    .operational_settings
                    .upstream_response_limit_bytes,
            )
            .await;
            let logged_error = provider_http_error_message(provider_id, status, &provider_error);
            if (status == StatusCode::UNAUTHORIZED
                || (attempt.provider.adapter_id == provider_adapters::ANTIGRAVITY_ADAPTER_ID
                    && status == StatusCode::FORBIDDEN))
                && let Some(key_id) = credential.id.as_deref()
                && let Err(storage_error) =
                    mark_provider_key_invalid(&attempt.scope.state, provider_id, key_id).await
            {
                tracing::error!(
                    %storage_error,
                    provider_id = %provider_id,
                    "failed to mark a rejected provider API key invalid"
                );
                return SendStep::Failed(DispatchOutcome::Storage(storage_error));
            }
            log_request(
                &attempt.scope.state,
                Some(live),
                log.provider_failure(
                    provider_id,
                    attempt.upstream_protocol,
                    credential.id.as_deref(),
                    status,
                    &logged_error,
                    attempt.target_model,
                ),
            )
            .await;
            SendStep::Rotate
        }
        Ok(response)
            if attempt.adapter.wants_event_stream()
                && !attempt.scope.canonical_stream
                && response.status().is_success() =>
        {
            failures.selected_credential_id = credential.id.clone();
            mark_provider_key_used(&attempt.scope.state, credential).await;
            if provider_adapters::accepts_event_stream_response(
                &response,
                attempt.adapter.allows_missing_event_stream_content_type(),
            ) {
                match provider_adapters::read_adapter_event_stream(
                    &attempt.provider.adapter_id,
                    response,
                    attempt
                        .scope
                        .operational_settings
                        .upstream_response_limit_bytes,
                )
                .await
                {
                    Ok(value) => {
                        SendStep::Completed(ProviderUpstreamResult::AdapterResponse { value })
                    }
                    Err(error)
                        if provider_adapters::adapter_sse_error_can_fail_over(false, &error) =>
                    {
                        failures.last_adapter_sse_failure = Some(error);
                        SendStep::Rotate
                    }
                    Err(error) => {
                        SendStep::Completed(ProviderUpstreamResult::AdapterSseFailure { error })
                    }
                }
            } else {
                SendStep::Completed(ProviderUpstreamResult::Response(response))
            }
        }
        Ok(response) if attempt.scope.canonical_stream && response.status().is_success() => {
            failures.selected_credential_id = credential.id.clone();
            mark_provider_key_used(&attempt.scope.state, credential).await;
            if !provider_adapters::accepts_event_stream_response(
                &response,
                attempt.adapter.allows_missing_event_stream_content_type(),
            ) {
                SendStep::Completed(ProviderUpstreamResult::Response(response))
            } else {
                match preflight_stream(
                    response,
                    attempt.upstream_protocol,
                    attempt.scope.client_protocol,
                    attempt.target_model,
                    attempt.scope.operational_settings.stream_idle_timeout,
                    attempt.scope.resource_limits,
                )
                .await
                {
                    Ok(chunks) => SendStep::Completed(ProviderUpstreamResult::Stream { chunks }),
                    Err(error) => {
                        failures.last_status = StatusCode::BAD_GATEWAY;
                        failures.last_error = format!(
                            "provider '{}' stream failed before output: {error}",
                            provider_id
                        );
                        failures.last_adapter_sse_failure = Some(AdapterSseError::new(error, true));
                        SendStep::Rotate
                    }
                }
            }
        }
        Ok(response) => {
            mark_provider_key_used(&attempt.scope.state, credential).await;
            failures.selected_credential_id = credential.id.clone();
            if response.status().is_success()
                && attempt.provider.adapter_id == provider_adapters::ANTIGRAVITY_ADAPTER_ID
                && let Some(key_id) = credential.id.as_deref()
            {
                attempt
                    .scope
                    .state
                    .clear_antigravity_quota_breaker(
                        &attempt.provider.id,
                        key_id,
                        attempt.target_model,
                    )
                    .await;
            }
            if response.status().is_success() {
                SendStep::Completed(ProviderUpstreamResult::Response(response))
            } else {
                let status = response.status();
                SendStep::Failed(
                    outcome::unsuccessful_status(attempt, failures, log, live, response, status)
                        .await,
                )
            }
        }
        Err(error) => {
            failures.selected_credential_id = credential.id.clone();
            SendStep::Failed(outcome::transport_failure(attempt, failures, log, live, error).await)
        }
    }
}

/// Records an Antigravity quota cooldown when the rejection was quota-typed.
async fn record_quota_rejection(
    attempt: &AttemptContext<'_>,
    failures: &mut TargetLoopFailures,
    credential: &ProviderCredential,
) {
    if attempt.provider.adapter_id != provider_adapters::ANTIGRAVITY_ADAPTER_ID {
        return;
    }
    let Some(key_id) = credential.id.as_deref() else {
        return;
    };
    attempt
        .scope
        .state
        .record_antigravity_quota_rejection(
            &attempt.provider.id,
            key_id,
            attempt.target_model,
            failures
                .upstream_retry_after
                .as_ref()
                .and_then(retry_after_duration),
        )
        .await;
}
