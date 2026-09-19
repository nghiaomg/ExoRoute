use super::*;
use outcome::CompletedUpstream;
use provider::ProviderUpstreamResult;
use target_preparation::RequestScope;

/// Builds the streaming client response from a completed upstream result.
///
/// The response owns the provider permit and circuit lease through
/// `StreamLog`, so cancellation or client disconnect releases both resources
/// when the translated stream is dropped.
#[allow(clippy::too_many_arguments)]
pub(super) async fn stream_response(
    scope: &RequestScope,
    completed: CompletedUpstream,
    canonical: &protocol::CanonicalRequest,
    route_alias: &str,
    live: &RequestLiveGuard,
    background_continuity: bool,
    started: Instant,
    analytics: Option<RequestAnalytics>,
    failures: &mut TargetLoopFailures,
    log: &RequestLogPayload<'_>,
) -> Option<Response> {
    let CompletedUpstream {
        result,
        mut circuit_probe,
        provider_permit,
        adapter,
        adapter_id: _,
        provider_id,
        upstream_protocol,
    } = completed;
    let upstream_chunks = match result {
        ProviderUpstreamResult::Response(upstream) => {
            if !provider_adapters::accepts_event_stream_response(
                &upstream,
                adapter.allows_missing_event_stream_content_type(),
            ) {
                let error = format!(
                    "provider '{}' returned a non-SSE response to a streaming request",
                    provider_id
                );
                circuit_probe.failed();
                failures.last_status = StatusCode::BAD_GATEWAY;
                failures.last_error = error.clone();
                log_request(
                    &scope.state,
                    Some(live),
                    log.provider_failure(
                        &provider_id,
                        upstream_protocol,
                        failures.selected_credential_id.as_deref(),
                        failures.last_status,
                        &error,
                        &canonical.model,
                    ),
                )
                .await;
                drop(provider_permit);
                return None;
            }
            Box::pin(upstream.bytes_stream())
        }
        ProviderUpstreamResult::Stream { chunks, .. } => chunks,
        ProviderUpstreamResult::AdapterResponse { .. }
        | ProviderUpstreamResult::AdapterSseFailure { .. } => {
            let error = format!(
                "provider '{}' returned an invalid adapter stream result",
                provider_id
            );
            circuit_probe.failed();
            failures.last_status = StatusCode::BAD_GATEWAY;
            failures.last_error = error.clone();
            log_request(
                &scope.state,
                Some(live),
                log.provider_failure(
                    &provider_id,
                    upstream_protocol,
                    failures.selected_credential_id.as_deref(),
                    failures.last_status,
                    &error,
                    &canonical.model,
                ),
            )
            .await;
            drop(provider_permit);
            return None;
        }
    };

    let outcome = StreamOutcome::default();
    let mut producer = stream_translation_from_chunks(
        upstream_chunks,
        StreamTranslationConfig {
            upstream_protocol,
            client_protocol: scope.client_protocol,
            request_id: scope.request_id.clone(),
            model: route_alias.to_owned(),
            idle_timeout: scope.operational_settings.stream_idle_timeout,
            continuity_enabled: background_continuity,
            overall_timeout: background_continuity.then_some(
                scope
                    .operational_settings
                    .upstream
                    .continuity_request_timeout,
            ),
            outcome: outcome.clone(),
            analytics,
            resource_limits: scope.resource_limits,
            shutdown: scope.state.shutdown_receiver(),
            log: Some(StreamLog {
                state: scope.state.clone(),
                request_id: scope.request_id.clone(),
                route_alias: route_alias.to_owned(),
                provider_id,
                model: canonical.model.clone(),
                api_key_id: log.api_key_id.map(str::to_owned),
                provider_credential_id: failures.selected_credential_id.clone(),
                client_protocol: scope.client_protocol,
                upstream_protocol,
                started,
                run_id: None,
                live: live.clone(),
                _provider_permit: provider_permit,
                circuit_probe,
                sse_processing_slots: scope.state.gates.gateway_sse_processing.clone(),
            }),
        },
    );
    producer.extensions_mut().insert(outcome);
    Some(producer)
}

/// Decodes a completed non-streaming provider response and returns `None`
/// when the target can safely be failed over.
#[allow(clippy::too_many_arguments)]
pub(super) async fn decode_response(
    scope: &RequestScope,
    completed: CompletedUpstream,
    canonical: &protocol::CanonicalRequest,
    route_alias: &str,
    live: &RequestLiveGuard,
    log: &RequestLogPayload<'_>,
    _started: Instant,
    analytics: Option<&RequestAnalytics>,
    failures: &mut TargetLoopFailures,
) -> Option<Response> {
    let CompletedUpstream {
        result,
        mut circuit_probe,
        provider_permit,
        adapter,
        adapter_id,
        provider_id,
        upstream_protocol,
    } = completed;
    let upstream_value = match read_upstream_value(
        result,
        adapter,
        &adapter_id,
        scope.operational_settings.upstream_response_limit_bytes,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            circuit_probe.failed();
            failures.last_status = StatusCode::BAD_GATEWAY;
            failures.last_error = format!("provider '{}' response failed: {error}", provider_id);
            log_request(
                &scope.state,
                Some(live),
                log.provider_failure(
                    &provider_id,
                    upstream_protocol,
                    failures.selected_credential_id.as_deref(),
                    failures.last_status,
                    &failures.last_error,
                    &canonical.model,
                ),
            )
            .await;
            drop(provider_permit);
            return None;
        }
    };
    let decoded = match protocol::decode_upstream_response(
        upstream_protocol,
        &upstream_value,
        &canonical.model,
    ) {
        Ok(response) => response,
        Err(error) => {
            circuit_probe.failed();
            failures.last_status = StatusCode::BAD_GATEWAY;
            failures.last_error = format!("could not decode provider response: {error}");
            log_request(
                &scope.state,
                Some(live),
                log.provider_failure(
                    &provider_id,
                    upstream_protocol,
                    failures.selected_credential_id.as_deref(),
                    failures.last_status,
                    &failures.last_error,
                    &canonical.model,
                ),
            )
            .await;
            drop(provider_permit);
            return None;
        }
    };
    if let Some(analytics) = analytics {
        analytics.set_token_usage(
            decoded.usage.as_ref().map(|usage| usage.input_tokens),
            decoded.usage.as_ref().map(|usage| usage.output_tokens),
        );
        analytics.set_cached_token_usage(
            decoded.usage.as_ref().map(|usage| usage.cached_tokens),
            decoded.usage.as_ref().map(|usage| usage.cache_input_tokens),
        );
    }
    circuit_probe.succeeded();
    log_request(
        &scope.state,
        Some(live),
        log.provider_success(
            &provider_id,
            upstream_protocol,
            failures.selected_credential_id.as_deref(),
            &decoded.model,
            decoded
                .usage
                .as_ref()
                .map(|usage| usage.input_tokens as i64),
            decoded
                .usage
                .as_ref()
                .map(|usage| usage.output_tokens as i64),
            decoded
                .usage
                .as_ref()
                .map(|usage| usage.cached_tokens as i64),
            decoded
                .usage
                .as_ref()
                .map(|usage| usage.cache_input_tokens as i64),
            decoded
                .usage
                .as_ref()
                .and_then(|usage| usage.cost_micro_usd),
        ),
    )
    .await;
    drop(provider_permit);

    let mut result = protocol::encode_response(scope.client_protocol, &decoded);
    result["model"] = json!(route_alias);
    Some(attach_request_live_guard(
        ([("x-request-id", scope.request_id.clone())], Json(result)).into_response(),
        live.clone(),
    ))
}

async fn read_upstream_value(
    result: ProviderUpstreamResult,
    adapter: &'static dyn provider_adapters::ProviderAdapter,
    adapter_id: &str,
    response_limit: usize,
) -> Result<Value, String> {
    match result {
        ProviderUpstreamResult::AdapterResponse { value, .. } => Ok(value),
        ProviderUpstreamResult::Response(upstream) if adapter.wants_event_stream() => {
            if !provider_adapters::accepts_event_stream_response(
                &upstream,
                adapter.allows_missing_event_stream_content_type(),
            ) {
                return Err("provider adapter returned a non-SSE response".to_owned());
            }
            provider_adapters::read_adapter_event_stream(adapter_id, upstream, response_limit)
                .await
                .map_err(|error| error.message)
        }
        ProviderUpstreamResult::Response(upstream) => {
            let bytes = provider_adapters::read_limited_response(upstream, response_limit)
                .await
                .map_err(|error| format!("provider response failed: {error}"))?;
            serde_json::from_slice::<Value>(&bytes)
                .map_err(|_| "provider returned invalid JSON".to_owned())
                .and_then(|value| provider_adapters::normalize_response(adapter_id, value))
        }
        ProviderUpstreamResult::Stream { .. } => {
            Err("preflighted stream result cannot reach non-stream decoding".to_owned())
        }
        ProviderUpstreamResult::AdapterSseFailure { error } => {
            Err(format!("provider adapter stream failed: {}", error.message))
        }
    }
}

/// Builds the final gateway error after all configured targets are exhausted.
#[allow(clippy::too_many_arguments)]
pub(super) async fn final_failure_response(
    scope: &RequestScope,
    canonical: &protocol::CanonicalRequest,
    route_alias: &str,
    api_key_id: Option<&str>,
    live: RequestLiveGuard,
    failures: TargetLoopFailures,
    started: Instant,
) -> Response {
    let final_status = if failures.translation_failed && !failures.request_encodable {
        StatusCode::BAD_REQUEST
    } else {
        failures.last_status
    };
    log_request(
        &scope.state,
        Some(&live),
        GatewayRequestLog {
            api_key_id,
            request_id: &scope.request_id,
            route: route_alias,
            provider: None,
            provider_credential_id: None,
            cost_micro_usd: None,
            model: &canonical.model,
            client: scope.client_protocol,
            upstream: None,
            status: final_status,
            duration_ms: started.elapsed().as_millis() as i64,
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            cache_input_tokens: None,
            error: Some(&failures.last_error),
        },
    )
    .await;
    let mut response = gateway_error(final_status, &failures.last_error);
    if let Some(retry_after) = failures.upstream_retry_after {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, retry_after);
    }
    attach_request_live_guard(response, live)
}
