use super::*;

pub struct GatewayRequestLog<'a> {
    pub(super) request_id: &'a str,
    pub(super) route: &'a str,
    pub(super) provider: Option<&'a str>,
    pub(super) provider_credential_id: Option<&'a str>,
    pub(super) api_key_id: Option<&'a str>,
    pub(super) model: &'a str,
    pub(super) client: Protocol,
    pub(super) upstream: Option<UpstreamProtocol>,
    pub(super) status: StatusCode,
    pub(super) duration_ms: i64,
    pub(super) input_tokens: Option<i64>,
    pub(super) output_tokens: Option<i64>,
    pub(super) cached_tokens: Option<i64>,
    pub(super) cache_input_tokens: Option<i64>,
    pub(super) cost_micro_usd: Option<i64>,
    pub(super) error: Option<&'a str>,
}

pub async fn log_request(
    state: &AppState,
    live: Option<&RequestLiveGuard>,
    record: GatewayRequestLog<'_>,
) {
    let request_log = RequestLogRecord {
        id: uuid::Uuid::new_v4().to_string(),
        request_id: record.request_id.chars().take(128).collect(),
        route_alias: record.route.chars().take(256).collect(),
        provider_id: record
            .provider
            .map(|value| value.chars().take(256).collect()),
        provider_credential_id: record
            .provider_credential_id
            .map(|value| value.chars().take(256).collect()),
        api_key_id: record
            .api_key_id
            .map(|value| value.chars().take(128).collect()),
        model: record.model.chars().take(256).collect(),
        client_protocol: record.client.as_str().to_owned(),
        upstream_protocol: record.upstream.map(|protocol| protocol.as_str().to_owned()),
        status: record.status.as_u16() as i64,
        duration_ms: record.duration_ms.max(0),
        input_tokens: record.input_tokens,
        output_tokens: record.output_tokens,
        cached_tokens: record.cached_tokens,
        cache_input_tokens: record.cache_input_tokens,
        cost_micro_usd: record.cost_micro_usd,
        error: record
            .error
            .map(|value| value.chars().take(MAX_REQUEST_LOG_ERROR_CHARS).collect()),
    };
    if let Some(live) = live {
        live.update_log(&request_log);
    }
    state.enqueue_request_log(request_log);
    if let Some(credential_id) = record.provider_credential_id {
        state.telemetry.record_provider_usage(
            credential_id.to_owned(),
            record.cost_micro_usd,
            record.input_tokens,
            record.output_tokens,
        );
    }
}
