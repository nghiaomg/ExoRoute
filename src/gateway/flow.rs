//! Request orchestration: JSON intake, continuity branching, and server
//! retries. Error formatting lives in `errors.rs`; adapter specifics live in
//! `request_preparation.rs` and the provider adapters.

use super::*;

pub(in crate::gateway) async fn gateway_json_request(
    request: Request<Body>,
) -> Result<(HeaderMap, Value, GatewayExecutionSettings), Response> {
    let (parts, body) = request.into_parts();
    let Some(context) = parts.extensions.get::<GatewayRequestContext>() else {
        return Err(gateway_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "gateway request context was not initialized",
        ));
    };
    let execution_settings = GatewayExecutionSettings {
        resource_limits: context.resource_limits,
        operational_settings: context.operational_settings,
        api_key_id: Some(context.api_key_id.clone()),
        analytics: context.analytics.clone(),
        stream_continuity_retry: false,
    };

    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .unwrap_or_default();
    let is_json = content_type
        .split_once('/')
        .is_some_and(|(top_level, subtype)| {
            top_level.eq_ignore_ascii_case("application")
                && (subtype.eq_ignore_ascii_case("json")
                    || subtype.to_ascii_lowercase().ends_with("+json"))
        });
    if !is_json {
        return Err(gateway_error(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "request Content-Type must be application/json",
        ));
    }

    if let Some(content_length) = parts.headers.get(header::CONTENT_LENGTH) {
        let content_length = content_length
            .to_str()
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| {
                gateway_error(StatusCode::BAD_REQUEST, "request Content-Length is invalid")
            })?;
        if u128::from(content_length)
            > execution_settings.resource_limits.gateway_body_limit_bytes as u128
        {
            return Err(gateway_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request body exceeds the configured gateway body limit",
            ));
        }
    }

    let mut body_bytes = Vec::new();
    let mut chunks = body.into_data_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| {
            gateway_error(StatusCode::BAD_REQUEST, "request body could not be read")
        })?;
        if chunk.len()
            > execution_settings
                .resource_limits
                .gateway_body_limit_bytes
                .saturating_sub(body_bytes.len())
        {
            return Err(gateway_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request body exceeds the configured gateway body limit",
            ));
        }
        body_bytes.extend_from_slice(&chunk);
    }
    let value = serde_json::from_slice(&body_bytes)
        .map_err(|_| gateway_error(StatusCode::BAD_REQUEST, "request body is not valid JSON"))?;
    Ok((parts.headers, value, execution_settings))
}

pub(in crate::gateway) async fn handle_request_with_limits_and_owner(
    state: AppState,
    headers: HeaderMap,
    body: Value,
    client_protocol: Protocol,
    execution_settings: GatewayExecutionSettings,
) -> Response {
    let body = Arc::new(body);
    if let Some(analytics) = &execution_settings.analytics {
        analytics.set_requested_model(body.get("model").and_then(Value::as_str));
    }
    let continuity_requested = execution_settings.resource_limits.stream_continuity_enabled
        && execution_settings.api_key_id.is_some()
        && body.get("stream").and_then(Value::as_bool) == Some(true);
    if continuity_requested {
        if let Some(analytics) = &execution_settings.analytics {
            analytics.defer_completion();
        }
        return start_continuous_request(state, headers, body, client_protocol, execution_settings)
            .await;
    }
    match tokio::time::timeout(
        server_retry_deadline(
            execution_settings.operational_settings.request_timeout,
            execution_settings.operational_settings.upstream,
        ),
        execute_with_server_retries(
            state,
            headers,
            body,
            client_protocol,
            None,
            execution_settings,
        ),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => gateway_error(
            StatusCode::GATEWAY_TIMEOUT,
            "route exceeded the total request deadline",
        ),
    }
}

pub(in crate::gateway) async fn start_continuous_request(
    state: AppState,
    mut headers: HeaderMap,
    body: Arc<Value>,
    client_protocol: Protocol,
    mut execution_settings: GatewayExecutionSettings,
) -> Response {
    let Some(api_key_id) = execution_settings.api_key_id.clone() else {
        return gateway_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "stream continuity request context is incomplete",
        );
    };
    let analytics = execution_settings.analytics.clone();
    let permit = match state
        .gates
        .stream_continuity_in_flight
        .clone()
        .try_acquire_owned()
    {
        Some(permit) => permit,
        None => {
            if let Some(analytics) = &analytics {
                analytics.finish(false);
            }
            return continuity::continuity_capacity_error(
                state.gates.stream_continuity_in_flight.limit(),
            );
        }
    };
    let request_id = headers
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty() && value.len() <= 128)
        .map(str::to_owned)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        headers.insert("x-request-id", value);
    }
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    if let Err(error) = continuity::create_run_with_settings(
        &state.db,
        &run_id,
        &api_key_id,
        execution_settings.operational_settings.upstream,
    )
    .await
    {
        tracing::error!(%error, %run_id, "could not reserve stream continuity before upstream dispatch");
        if let Some(analytics) = &analytics {
            analytics.finish(false);
        }
        return gateway_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "stream continuity could not be reserved; the request was not sent upstream",
        );
    }

    // Continuity work is independent of the downstream request future. Keep
    // the configured request timeout for each upstream attempt so a timed-out
    // provider can be retried; the background continuity worker supplies the
    // separate finite overall deadline.
    execution_settings.operational_settings.stream_idle_timeout = execution_settings
        .operational_settings
        .upstream
        .continuity_request_timeout;
    // The background stream owns client identity through its analytics tracker;
    // the inner handler marks itself as continuity work by receiving no key.
    execution_settings.api_key_id = None;
    execution_settings.stream_continuity_retry = true;
    let cancellation = state.register_stream_cancellation(&run_id).await;
    let (sender, receiver) = tokio::sync::mpsc::channel(8);
    let live = continuity::live_response_with_settings(
        receiver,
        &run_id,
        &request_id,
        state.shutdown_receiver(),
        execution_settings.operational_settings.upstream,
    );
    let request = execute_with_server_retries(
        state.clone(),
        headers,
        body,
        client_protocol,
        None,
        execution_settings,
    );
    continuity::start_background_request(
        state,
        run_id,
        request,
        sender,
        cancellation,
        permit,
        continuity::BackgroundRequestSettings {
            request_timeout: None,
            analytics,
        },
    )
    .await;
    live
}

#[cfg(test)]
pub(in crate::gateway) async fn handle_request_inner_with_adapter_base_url_override(
    state: AppState,
    headers: HeaderMap,
    body: Value,
    client_protocol: Protocol,
    adapter_base_url_override: Option<&str>,
) -> Response {
    let resource_limits = state.gateway_resource_limits();
    let operational_settings = state.operational_settings().settings;
    handle_request_inner_with_adapter_base_url_override_and_limits(
        state,
        headers,
        Arc::new(body),
        client_protocol,
        adapter_base_url_override,
        GatewayExecutionSettings {
            resource_limits,
            operational_settings,
            api_key_id: None,
            analytics: None,
            stream_continuity_retry: false,
        },
        0,
    )
    .await
}

pub(in crate::gateway) async fn execute_with_server_retries(
    state: AppState,
    headers: HeaderMap,
    body: Arc<Value>,
    client_protocol: Protocol,
    adapter_base_url_override: Option<&str>,
    execution_settings: GatewayExecutionSettings,
) -> Response {
    let mut attempt = 0;
    loop {
        let response = handle_request_inner_with_adapter_base_url_override_and_limits(
            state.clone(),
            headers.clone(),
            Arc::clone(&body),
            client_protocol,
            adapter_base_url_override,
            execution_settings.clone(),
            attempt,
        )
        .await;
        if execution_settings.stream_continuity_retry {
            if !is_retryable(response.status()) {
                return response;
            }
            let retry_after = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .map(Duration::from_secs);
            let delay = retry_policy::continuity_retry_delay(
                attempt,
                retry_after,
                execution_settings.operational_settings.upstream,
            );
            tracing::warn!(
                status = response.status().as_u16(),
                retry_number = attempt.saturating_add(1),
                target_rotation_offset = attempt,
                delay_ms = delay.as_millis() as u64,
                "stream continuity upstream failure; retrying indefinitely"
            );
            drop(response);
            tokio::time::sleep(delay).await;
            attempt = attempt.saturating_add(1);
            continue;
        }
        let upstream = execution_settings.operational_settings.upstream;
        if !response.status().is_server_error() || attempt + 1 >= upstream.server_retry_max_attempts
        {
            return response;
        }
        let retry_number = attempt + 1;
        let delay = retry_policy::server_retry_delay(attempt, upstream);
        tracing::warn!(
            status = response.status().as_u16(),
            retry_number,
            max_attempts = upstream.server_retry_max_attempts,
            target_rotation_offset = attempt,
            delay_ms = delay.as_millis() as u64,
            "gateway server failure; retrying request before responding to client"
        );
        drop(response);
        tokio::time::sleep(delay).await;
        attempt += 1;
    }
}

#[cfg(test)]
pub(in crate::gateway) fn endpoint_url(
    base_url: &str,
    protocol: Protocol,
) -> Result<reqwest::Url, String> {
    provider_adapters::adapter(provider_adapters::GENERIC_ADAPTER_ID)
        .expect("generic adapter is registered")
        .endpoint(base_url, protocol, None)
}

#[cfg(test)]
pub(in crate::gateway) fn prepare_codex_request(body: &mut Value, request_id: &str) {
    provider_adapters::prepare_request_body(provider_adapters::CODEX_ADAPTER_ID, body, request_id)
        .expect("Codex adapter is registered");
}
