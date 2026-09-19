use super::*;
use target_preparation::{RequestScope, TargetPreparationError};

/// Request-scoped inputs for one gateway request execution.
pub(super) struct Orchestrator {
    pub(super) state: AppState,
    pub(super) headers: HeaderMap,
    pub(super) body: Arc<Value>,
    pub(super) client_protocol: Protocol,
    pub(super) adapter_base_url_override: Option<String>,
    pub(super) resource_limits: GatewayResourceLimits,
    pub(super) operational_settings: OperationalSettings,
    pub(super) api_key_id: Option<String>,
    pub(super) analytics: Option<RequestAnalytics>,
    pub(super) target_rotation_offset: usize,
}

impl Orchestrator {
    /// Runs the whole pipeline: request preparation, the route-target loop
    /// (prepare + dispatch per target), and response construction.
    pub(super) async fn run(self) -> Response {
        let started = Instant::now();
        let PreparedGatewayRequest {
            request_id,
            request_api_key_id,
            adapter_session_id,
            mut canonical,
            route_alias,
            targets,
            background_continuity,
        } = match prepare_gateway_request(GatewayRequestPreparation {
            state: self.state.clone(),
            headers: self.headers.clone(),
            body: self.body.clone(),
            client_protocol: self.client_protocol,
            resource_limits: self.resource_limits,
            api_key_id: self.api_key_id.clone(),
            analytics: self.analytics.clone(),
            target_rotation_offset: self.target_rotation_offset,
        })
        .await
        {
            Ok(prepared) => prepared,
            Err(error) => return error.into_response(),
        };
        let live = self.state.begin_request_live(
            &request_id,
            &route_alias,
            &canonical.model,
            self.client_protocol.as_str(),
            request_api_key_id.as_deref(),
        );
        let scope = RequestScope {
            state: self.state.clone(),
            request_id: request_id.clone(),
            adapter_session_id,
            adapter_base_url_override: self.adapter_base_url_override.clone(),
            operational_settings: self.operational_settings,
            resource_limits: self.resource_limits,
            canonical_stream: canonical.stream,
            client_protocol: self.client_protocol,
            client_headers: self.headers.clone(),
        };
        let log = RequestLogPayload {
            scope: &scope,
            route_alias: &route_alias,
            api_key_id: request_api_key_id.as_deref(),
            started,
        };
        let mut failures = TargetLoopFailures::new();
        for target in &targets {
            let attempt =
                match target_preparation::prepare_target(&scope, &mut canonical, target, &live)
                    .await
                {
                    Ok(attempt) => attempt,
                    Err(TargetPreparationError::Database(error)) => {
                        return attach_request_live_guard(gateway_database_error(error), live);
                    }
                    Err(TargetPreparationError::Skip(message)) => {
                        failures.last_error = message;
                        continue;
                    }
                    Err(TargetPreparationError::SkipWithStatus(message, status)) => {
                        failures.last_status = status;
                        failures.last_error = message;
                        continue;
                    }
                    Err(TargetPreparationError::SkipUnencodable(message)) => {
                        failures.last_status = StatusCode::BAD_REQUEST;
                        failures.last_error = message;
                        failures.translation_failed = true;
                        continue;
                    }
                };
            failures.request_encodable = true;
            let mut rotation =
                match CredentialRotation::start(&scope.state, &attempt.provider).await {
                    Ok(rotation) => rotation,
                    Err(error) => {
                        return attach_request_live_guard(gateway_database_error(error), live);
                    }
                };
            if rotation.current().is_none() {
                failures.last_error =
                    format!("provider '{}' has no usable API keys", attempt.provider.id);
                continue;
            }
            failures.last_key_rejection = None;
            failures.last_adapter_sse_failure = None;
            failures.selected_credential_id = None;
            match dispatch::dispatch_target(attempt, &mut failures, &log, &live, &mut rotation)
                .await
            {
                DispatchOutcome::Completed(upstream) => {
                    if let Some(response) = self
                        .respond(
                            &scope,
                            *upstream,
                            &route_alias,
                            &live,
                            &log,
                            background_continuity,
                            started,
                            &mut failures,
                        )
                        .await
                    {
                        return response;
                    }
                }
                DispatchOutcome::TargetFailed => continue,
                DispatchOutcome::Terminal => break,
                DispatchOutcome::Storage(error) => {
                    return attach_request_live_guard(gateway_database_error(error), live);
                }
            }
        }
        self.final_failure(
            &scope,
            &canonical,
            &route_alias,
            request_api_key_id.as_deref(),
            live.clone(),
            failures,
            started,
        )
        .await
    }

    /// Turns a completed upstream result into the client response.
    /// Returns `None` only when the result was consumed by a failure branch
    /// that should let the loop try the next target.
    #[allow(clippy::too_many_arguments)]
    async fn respond(
        &self,
        scope: &RequestScope,
        upstream: outcome::CompletedUpstream,
        route_alias: &str,
        live: &RequestLiveGuard,
        log: &RequestLogPayload<'_>,
        background_continuity: bool,
        started: Instant,
        failures: &mut TargetLoopFailures,
    ) -> Option<Response> {
        if scope.canonical_stream {
            return respond::stream_response(
                scope,
                upstream,
                route_alias,
                live,
                background_continuity,
                started,
                self.analytics.clone(),
                failures,
                log,
            )
            .await;
        }
        respond::decode_response(
            scope,
            upstream,
            route_alias,
            live,
            log,
            started,
            self.analytics.as_ref(),
            failures,
        )
        .await
    }

    /// The final failure response after every target was exhausted.
    #[allow(clippy::too_many_arguments)]
    async fn final_failure(
        &self,
        scope: &RequestScope,
        canonical: &protocol::CanonicalRequest,
        route_alias: &str,
        api_key_id: Option<&str>,
        live: RequestLiveGuard,
        failures: TargetLoopFailures,
        started: Instant,
    ) -> Response {
        respond::final_failure_response(
            scope,
            canonical,
            route_alias,
            api_key_id,
            live,
            failures,
            started,
        )
        .await
    }
}
