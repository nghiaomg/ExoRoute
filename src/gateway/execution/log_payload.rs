use super::super::logging::GatewayRequestLog;
use super::target_preparation::RequestScope;
use super::*;

/// Builds the standard gateway request-log record.
///
/// Every failure path in the gateway pipeline logs the same identity fields
/// with only the status, credential, tokens, and message varying; one
/// builder keeps the copies from drifting apart.
pub(super) struct RequestLogPayload<'a> {
    pub(super) scope: &'a RequestScope,
    pub(super) route_alias: &'a str,
    pub(super) api_key_id: Option<&'a str>,
    pub(super) started: Instant,
    pub(super) reasoning_effort: Option<String>,
    /// The proxy bound to the provider of the current attempt, set per target
    /// so every failure or success log names the proxy the attempt used.
    pub(super) proxy_id: Option<String>,
}

impl<'a> RequestLogPayload<'a> {
    pub(super) fn set_reasoning_effort(&mut self, effort: Option<String>) {
        self.reasoning_effort = effort;
    }

    /// Records the proxy of the attempt being logged; reset to `None` between
    /// targets so a target without a proxy binding never inherits the
    /// previous target's proxy.
    pub(super) fn set_proxy_id(&mut self, proxy_id: Option<String>) {
        self.proxy_id = proxy_id;
    }

    /// Log for a failure of one specific provider attempt.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn provider_failure<'b>(
        &'b self,
        provider_id: &'b str,
        upstream_protocol: UpstreamProtocol,
        credential_id: Option<&'b str>,
        status: StatusCode,
        error: &'b str,
        model: &'b str,
    ) -> GatewayRequestLog<'b> {
        GatewayRequestLog {
            api_key_id: self.api_key_id,
            request_id: &self.scope.request_id,
            route: self.route_alias,
            provider: Some(provider_id),
            provider_credential_id: credential_id,
            cost_micro_usd: None,
            model,
            client: self.scope.client_protocol,
            upstream: Some(upstream_protocol),
            proxy_id: self.proxy_id.as_deref(),
            status,
            duration_ms: self.started.elapsed().as_millis() as i64,
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            cache_input_tokens: None,
            reasoning_effort: self.reasoning_effort.clone(),
            error: Some(error),
        }
    }

    /// Log for a successful decoded completion.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn provider_success<'b>(
        &'b self,
        provider_id: &'b str,
        upstream_protocol: UpstreamProtocol,
        credential_id: Option<&'b str>,
        model: &'b str,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        cached_tokens: Option<i64>,
        cache_input_tokens: Option<i64>,
        cost_micro_usd: Option<i64>,
    ) -> GatewayRequestLog<'b> {
        GatewayRequestLog {
            api_key_id: self.api_key_id,
            request_id: &self.scope.request_id,
            route: self.route_alias,
            provider: Some(provider_id),
            provider_credential_id: credential_id,
            cost_micro_usd,
            model,
            client: self.scope.client_protocol,
            upstream: Some(upstream_protocol),
            proxy_id: self.proxy_id.as_deref(),
            status: StatusCode::OK,
            duration_ms: self.started.elapsed().as_millis() as i64,
            input_tokens,
            output_tokens,
            cached_tokens,
            cache_input_tokens,
            reasoning_effort: self.reasoning_effort.clone(),
            error: None,
        }
    }
}
