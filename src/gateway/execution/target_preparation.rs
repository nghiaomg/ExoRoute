use super::*;
use crate::state::{CircuitProbePermit, DynamicSemaphorePermit};
use provider::{Provider, decode_provider};

/// Request-scoped values shared by every attempt in one gateway request.
///
/// They live in one place so the preparation and dispatch stages read them
/// without threading eight parameters through every helper, mirroring the
/// local variables the original pipeline kept in scope.
pub(super) struct RequestScope {
    pub(super) state: AppState,
    pub(super) request_id: String,
    pub(super) adapter_session_id: String,
    pub(super) adapter_base_url_override: Option<String>,
    pub(super) operational_settings: OperationalSettings,
    pub(super) resource_limits: GatewayResourceLimits,
    pub(super) canonical_stream: bool,
    pub(super) client_protocol: Protocol,
    pub(super) client_headers: HeaderMap,
}

/// The negotiated, encoded, permissioned context for one upstream attempt.
///
/// Holding the permits preserves the RAII invariants of the original
/// pipeline: the circuit probe and provider bulkhead permit are released
/// exactly when this context drops, and the probe outcome methods stay
/// available to the dispatch stage and orchestrator.
pub(super) struct AttemptContext<'a> {
    pub(super) scope: &'a RequestScope,
    pub(super) provider: Provider,
    pub(super) upstream_protocol: UpstreamProtocol,
    pub(super) adapter: &'static dyn ProviderAdapter,
    pub(super) endpoint: reqwest::Url,
    pub(super) provider_http: reqwest::Client,
    /// Remaining proxy endpoints, tried only when reqwest reports a connect error.
    pub(super) proxy_fallbacks: Vec<crate::security::egress::ProxyEgress>,
    pub(super) circuit_probe: CircuitProbePermit,
    pub(super) provider_permit: DynamicSemaphorePermit,
    pub(super) upstream_body: Value,
    pub(super) effective_effort: Option<String>,
    pub(super) target_model: &'a str,
}

/// The reason a target could not be prepared.
pub(super) enum TargetPreparationError {
    /// Storage failed while reading provider or credential state. The whole
    /// request must stop with the storage error response.
    Database(StorageError),
    /// This target was skipped; the loop should try the next one. The string
    /// is the same diagnostic the original pipeline recorded as `last_error`.
    Skip(String),
    /// A skip that also overrides the recorded failure status.
    SkipWithStatus(String, StatusCode),
    /// Translation failed for this target and the request shape is not
    /// encodable for any provider; the final status becomes BAD_REQUEST.
    SkipUnencodable(String),
}

/// Reads, validates, encodes, and permissions one route target.
///
/// This is the first half of the original monolithic gateway pipeline: it
/// ends when the bulkhead permit is acquired, before the credential loop
/// begins. The `canonical.model` mutation happens here at the same point the
/// original performed it, so failure logs record the same model values.
pub(super) async fn prepare_target<'a>(
    scope: &'a RequestScope,
    canonical: &mut protocol::CanonicalRequest,
    target: &'a Target,
    live: &RequestLiveGuard,
) -> Result<AttemptContext<'a>, TargetPreparationError> {
    let state = &scope.state;
    let provider_id = target.provider_id.clone();
    let provider_data = match state
        .db
        .read(move |transaction| {
            let provider = transaction.get::<Record>(Table::Providers, &provider_id)?;
            let proxy_id = if let Some(record) = provider.as_ref() {
                record.optional_text("proxy_id")?.map(str::to_owned)
            } else {
                None
            };
            let proxy_group = if let Some(proxy_id) = proxy_id {
                Some(crate::admin::load_proxy_endpoint_configs(transaction, &proxy_id)?)
            } else {
                None
            };
            Ok((provider, proxy_group))
        })
        .await
    {
        Ok(data) => data,
        Err(error) => return Err(TargetPreparationError::Database(error)),
    };
    let Some(provider_record) = provider_data.0 else {
        return Err(TargetPreparationError::Skip(format!(
            "provider '{}' not found",
            target.provider_id
        )));
    };
    let proxy_group = provider_data.1;
    let provider = match decode_provider(&provider_record, state.config.master_key.as_ref()) {
        Ok(provider) => provider,
        Err(error) => return Err(TargetPreparationError::Skip(error)),
    };
    if !provider.enabled {
        return Err(TargetPreparationError::Skip(format!(
            "provider '{}' is disabled",
            provider.id
        )));
    }
    live.update_provider(Some(&provider.id));
    let upstream_protocol = target
        .protocol
        .or(target.model_protocol)
        .or_else(|| provider_adapters::model_upstream_protocol(&provider.adapter_id, &target.model))
        .or_else(|| {
            (!provider_adapters::capabilities(&provider.adapter_id)
                .is_some_and(|capabilities| capabilities.model_protocol_routing))
            .then_some(provider.preferred_protocol)
        });
    let Some(upstream_protocol) = upstream_protocol else {
        // The provider id is part of the message because the same model id can
        // be saved on several providers, and the fix (a per-model protocol
        // override) is applied on one provider row at a time.
        return Err(TargetPreparationError::SkipUnencodable(format!(
            "provider '{}' model '{}' has no configured upstream protocol; set it in the provider model settings",
            provider.id, target.model
        )));
    };
    if !provider_adapters::supports_upstream_protocol(&provider.adapter_id, upstream_protocol) {
        return Err(TargetPreparationError::SkipUnencodable(format!(
            "provider '{}' adapter does not support {}",
            provider.id,
            upstream_protocol.as_str()
        )));
    }
    if !provider
        .supported_protocols
        .iter()
        .any(|p| p == upstream_protocol.as_str())
    {
        return Err(TargetPreparationError::Skip(format!(
            "provider '{}' does not declare support for {}",
            provider.id,
            upstream_protocol.as_str()
        )));
    }
    canonical.model = target.model.clone();
    let thinking_handling = match protocol::parse_thinking_handling(
        &provider.thinking_mode,
        provider.thinking_override.as_deref(),
    ) {
        Ok(handling) => handling,
        Err(_) => {
            return Err(TargetPreparationError::Skip(format!(
                "provider '{}' has invalid thinking settings",
                provider.id
            )));
        }
    };
    let mut upstream_body = match protocol::encode_upstream_request_with_thinking(
        upstream_protocol,
        canonical,
        &target.model,
        thinking_handling,
    ) {
        Ok(body) => body,
        Err(error) => {
            return Err(TargetPreparationError::SkipUnencodable(format!(
                "provider '{}' cannot accept this request through {}: {error}",
                provider.id,
                upstream_protocol.as_str()
            )));
        }
    };
    // A `-{effort}` model-name suffix resolved during alias lookup lands here,
    // after protocol encoding but before the adapter sees the body, so
    // adapter-level normalizers (for example Command Code's reasoning_effort
    // clamp or Codex's encrypted-reasoning include) observe the final value.
    if let Some(effort) = target.effort.as_deref()
        && let Err(error) = protocol::apply_model_effort(
            upstream_protocol,
            &mut upstream_body,
            effort,
            &target.model,
        )
    {
        return Err(TargetPreparationError::SkipUnencodable(format!(
            "provider '{}' cannot apply the '{effort}' model effort through {}: {error}",
            provider.id,
            upstream_protocol.as_str()
        )));
    }
    if let Err(error) =
        provider_adapters::prepare_request_body(&provider.adapter_id, &mut upstream_body, &scope.adapter_session_id)
    {
        return Err(TargetPreparationError::Skip(format!(
            "provider '{}' adapter rejected the request: {error}",
            provider.id
        )));
    }
    if let Err(error) =
        provider_adapters::prepare_provider_images(&provider.adapter_id, state, &mut upstream_body).await
    {
        return Err(TargetPreparationError::SkipWithStatus(
            format!("provider '{}' could not prepare adapter input: {error}", provider.id),
            StatusCode::BAD_GATEWAY,
        ));
    }
    let Some(adapter) = provider_adapters::adapter(&provider.adapter_id) else {
        return Err(TargetPreparationError::Skip(format!(
            "provider '{}' uses an unregistered adapter",
            provider.id
        )));
    };
    let effective_effort = protocol::effective_effort(upstream_protocol, &upstream_body)
        .map(|effort| effort.as_str().to_owned());
    let endpoint = match provider_adapters::upstream_endpoint(
        &provider.adapter_id,
        &provider.base_url,
        upstream_protocol,
        &target.model,
        canonical.stream,
        scope.adapter_base_url_override.as_deref(),
    ) {
        Ok(url) => url,
        Err(error) => return Err(TargetPreparationError::Skip(error)),
    };
    let streaming = scope.canonical_stream || provider_adapters::wants_event_stream(&provider.adapter_id);
    let mut proxy_fallbacks = Vec::new();
    let (endpoint, provider_http) = if provider_adapters::uses_local_transport(&provider.adapter_id)
    {
        // Local transports never reach egress: the endpoint is a process
        // locator (devin://acp/stdio) and the send step spawns the provider's
        // own CLI process. The placeholder client keeps the attempt context
        // shape so classification and streaming treat a local reply exactly
        // like an HTTP reply.
        (endpoint, provider_adapters::local_transport_client())
    } else if provider.proxy_id.is_some() {
        let proxy_group = proxy_group.ok_or_else(|| {
            TargetPreparationError::Skip("provider proxy binding has no group".to_owned())
        })?;
        if !proxy_group.enabled {
            return Err(TargetPreparationError::Skip(format!(
                "provider '{}' is bound to a disabled proxy",
                provider.id
            )));
        }
        if proxy_group.endpoints.is_empty() {
            return Err(TargetPreparationError::Skip(format!(
                "provider '{}' is bound to a proxy with no enabled endpoints",
                provider.id
            )));
        }
        let mut endpoint_configs = proxy_group.endpoints;
        if proxy_group.strategy == "round_robin" && endpoint_configs.len() > 1 {
            let start = state
                .advance_proxy_endpoint_cursor(&proxy_group.proxy_id, endpoint_configs.len())
                .await;
            endpoint_configs.rotate_left(start);
        }
        let mut endpoint_configs = endpoint_configs.into_iter();
        let first_config = endpoint_configs.next().ok_or_else(|| {
            TargetPreparationError::Skip("bound proxy has no usable endpoint".to_owned())
        })?;
        let first_proxy = crate::security::egress::proxy_egress_from_record(
            &first_config.endpoint_id,
            &first_config.record,
            state.config.master_key.as_ref(),
            state.config.allow_private_provider_urls,
        )
        .map_err(TargetPreparationError::Skip)?;
        for endpoint_config in endpoint_configs {
            proxy_fallbacks.push(
                crate::security::egress::proxy_egress_from_record(
                    &endpoint_config.endpoint_id,
                    &endpoint_config.record,
                    state.config.master_key.as_ref(),
                    state.config.allow_private_provider_urls,
                )
                .map_err(TargetPreparationError::Skip)?,
            );
        }
        let (url, client) = egress::provider_client_with_proxy(
            endpoint.as_str(),
            state.config.allow_private_provider_urls,
            scope.operational_settings.connect_timeout,
            scope.operational_settings.request_timeout,
            streaming,
            concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
            scope.operational_settings.upstream,
            Some(&first_proxy),
        )
        .await
        .map_err(|error| {
            TargetPreparationError::Skip(format!(
                "provider '{}' proxy egress rejected: {error}",
                provider.id
            ))
        })?;
        (url, client)
    } else {
        match egress::provider_client(
            endpoint.as_str(),
            state.config.allow_private_provider_urls,
            scope.operational_settings.connect_timeout,
            scope.operational_settings.request_timeout,
            // Streaming responses must not inherit the total request timeout:
            // reqwest's timeout spans the whole body and would abort a healthy
            // stream once `request_timeout` elapses. The SSE reader owns stream
            // liveness through its idle deadlines instead.
            scope.canonical_stream || provider_adapters::wants_event_stream(&provider.adapter_id),
            concat!("ExoRoute/", env!("CARGO_PKG_VERSION")),
            scope.operational_settings.upstream,
        )
        .await
        {
            Ok(client) => client,
            Err(error) => {
                return Err(TargetPreparationError::Skip(format!(
                    "provider '{}' egress rejected: {error}",
                    provider.id
                )));
            }
        }
    };
    let Some(circuit_probe) =
        state.permit_provider_with_settings(&provider.id, scope.operational_settings)
    else {
        return Err(TargetPreparationError::Skip(format!(
            "provider '{}' circuit is open",
            provider.id
        )));
    };
    let provider_permit = match state.try_provider_permit(&provider.id).await {
        Some(permit) => permit,
        None => {
            return Err(TargetPreparationError::SkipWithStatus(
                format!("provider '{}' is at its concurrency limit", provider.id),
                StatusCode::SERVICE_UNAVAILABLE,
            ));
        }
    };
    Ok(AttemptContext {
        scope,
        provider,
        upstream_protocol,
        adapter,
        endpoint,
        provider_http,
        proxy_fallbacks,
        circuit_probe,
        provider_permit,
        upstream_body,
        effective_effort,
        target_model: &target.model,
    })
}
