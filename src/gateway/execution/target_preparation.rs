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
    pub(super) circuit_probe: CircuitProbePermit,
    pub(super) provider_permit: DynamicSemaphorePermit,
    pub(super) upstream_body: Value,
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
    let provider_record = match state
        .db
        .read(move |transaction| transaction.get::<Record>(Table::Providers, &provider_id))
        .await
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            return Err(TargetPreparationError::Skip(format!(
                "provider '{}' not found",
                target.provider_id
            )));
        }
        Err(error) => return Err(TargetPreparationError::Database(error)),
    };
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
        return Err(TargetPreparationError::SkipUnencodable(format!(
            "provider model '{}' has no configured upstream protocol; set it in the provider model settings",
            target.model
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
    if let Err(error) = provider_adapters::prepare_request_body(
        &provider.adapter_id,
        &mut upstream_body,
        &scope.adapter_session_id,
    ) {
        return Err(TargetPreparationError::Skip(format!(
            "provider '{}' adapter rejected the request: {error}",
            provider.id
        )));
    }
    if let Err(error) =
        provider_adapters::prepare_provider_images(&provider.adapter_id, state, &mut upstream_body)
            .await
    {
        return Err(TargetPreparationError::SkipWithStatus(
            format!(
                "provider '{}' could not prepare adapter input: {error}",
                provider.id
            ),
            StatusCode::BAD_GATEWAY,
        ));
    }
    let Some(adapter) = provider_adapters::adapter(&provider.adapter_id) else {
        return Err(TargetPreparationError::Skip(format!(
            "provider '{}' uses an unregistered adapter",
            provider.id
        )));
    };
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
    let (endpoint, provider_http) = match egress::provider_client(
        endpoint.as_str(),
        state.config.allow_private_provider_urls,
        scope.operational_settings.connect_timeout,
        scope.operational_settings.request_timeout,
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
        circuit_probe,
        provider_permit,
        upstream_body,
        target_model: &target.model,
    })
}
