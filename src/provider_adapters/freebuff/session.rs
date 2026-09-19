//! Freebuff session lifecycle: cached session reuse plus agent START.
//!
//! Sessions are keyed by credential hash and locked to one model; a cached
//! session for another model is a conflict, never a silent retry.

use super::catalog::{
    FREEBUFF_AGENT_RUNS_PATH, FREEBUFF_SESSION_PATH, FREEBUFF_SESSION_USER_AGENT,
};
use super::client::{
    freebuff_client, header_value, read_auxiliary_body, retry_after, send_bounded,
    valid_internal_id,
};
use super::errors::{cached_freebuff_session_conflict, freebuff_session_error};
use super::{
    AdapterRequestContext, AdapterRequestError, AdapterRequestPreparation, endpoint_from_root,
    freebuff_session_cache_key,
};
use crate::protocol::UpstreamProtocol;
use crate::state::FreebuffSessionCacheEntry;
use http::{HeaderMap, HeaderValue, StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct FreebuffSessionResponse {
    #[serde(rename = "instanceId")]
    instance_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FreebuffAgentRunResponse {
    #[serde(rename = "runId")]
    run_id: Option<String>,
}

pub(super) async fn prepare_freebuff_request(
    context: AdapterRequestContext<'_>,
    body: &mut Value,
) -> Result<AdapterRequestPreparation, AdapterRequestError> {
    if context.auth.protocol != UpstreamProtocol::ChatCompletions {
        return Err(AdapterRequestError::new(
            None,
            None,
            "Freebuff supports Chat Completions upstream only",
        ));
    }
    if context.auth.auth_type != "bearer" {
        return Err(AdapterRequestError::new(
            None,
            None,
            "Freebuff requires Bearer authentication",
        ));
    }
    let Some(secret) = context.auth.secret else {
        return Err(AdapterRequestError::new(
            None,
            None,
            "Freebuff Auth Token required",
        ));
    };
    let configured_base = super::validate_base_url(context.base_url)
        .map_err(|error| AdapterRequestError::new(None, None, error))?;
    let request_root = context
        .adapter_base_url_override
        .unwrap_or(configured_base.as_str());
    let session_endpoint = endpoint_from_root(request_root, FREEBUFF_SESSION_PATH)
        .map_err(|error| AdapterRequestError::new(None, None, error))?;
    let agent_endpoint = endpoint_from_root(request_root, FREEBUFF_AGENT_RUNS_PATH)
        .map_err(|error| AdapterRequestError::new(None, None, error))?;
    let completion_model = super::catalog::requested_model(context.model).trim();
    if completion_model.is_empty() {
        return Err(AdapterRequestError::new(
            None,
            None,
            "Freebuff model ID is required",
        ));
    }
    let agent_id = super::catalog::agent_for_model(completion_model);
    let upstream = context.state.operational_settings().settings.upstream;
    let (session_endpoint, client) = freebuff_client(context.state, &session_endpoint).await?;
    let session_cache_key = freebuff_session_cache_key(secret);
    let session_lock = context.state.freebuff_session_lock(session_cache_key).await;
    let instance_id = {
        let _session_guard = session_lock.lock().await;
        if let Some(session) = context
            .state
            .cached_freebuff_session(session_cache_key)
            .await
        {
            if session.model != completion_model {
                return Err(cached_freebuff_session_conflict(
                    &session.model,
                    completion_model,
                ));
            }
            session.instance_id
        } else {
            let session_request = client
                .post(session_endpoint.clone())
                .bearer_auth(secret)
                .header(http::header::CONTENT_TYPE, "application/json")
                .header(http::header::ACCEPT, "application/json")
                .header("User-Agent", FREEBUFF_SESSION_USER_AGENT)
                .header("x-freebuff-model", completion_model)
                .json(&json!({}));
            let response = send_bounded(
                session_request,
                "session",
                upstream.freebuff_auxiliary_timeout,
            )
            .await?;
            let status = response.status();
            if !status.is_success() {
                let retry_after = retry_after(&response);
                let body = read_auxiliary_body(
                    response,
                    upstream.freebuff_auxiliary_timeout,
                    upstream.freebuff_auxiliary_response_max_bytes,
                )
                .await
                .map_err(|error| {
                    AdapterRequestError::new(Some(status), retry_after.clone(), error.message)
                })?;
                let (message, provider_response_body) = freebuff_session_error(status, &body);
                return Err(AdapterRequestError::new(Some(status), retry_after, message)
                    .with_provider_response_body(provider_response_body));
            }
            let body_bytes = read_auxiliary_body(
                response,
                upstream.freebuff_auxiliary_timeout,
                upstream.freebuff_auxiliary_response_max_bytes,
            )
            .await
            .map_err(|error| {
                AdapterRequestError::new(Some(StatusCode::BAD_GATEWAY), None, error.message)
            })?;
            let session: FreebuffSessionResponse =
                serde_json::from_slice(&body_bytes).map_err(|_| {
                    AdapterRequestError::new(
                        Some(StatusCode::BAD_GATEWAY),
                        None,
                        "Freebuff session returned invalid JSON",
                    )
                })?;
            let instance_id = session.instance_id.as_deref().map(str::trim).unwrap_or("");
            if !valid_internal_id(instance_id) {
                return Err(AdapterRequestError::new(
                    Some(StatusCode::BAD_GATEWAY),
                    None,
                    "Freebuff session response did not contain a valid instanceId",
                ));
            }
            let instance_id = instance_id.to_owned();
            context
                .state
                .cache_freebuff_session(
                    session_cache_key,
                    FreebuffSessionCacheEntry {
                        model: completion_model.to_owned(),
                        instance_id: instance_id.clone(),
                        last_used_at: std::time::Instant::now(),
                    },
                )
                .await;
            instance_id
        }
    };

    let run_id = start_agent_run(
        &client,
        &agent_endpoint,
        secret,
        agent_id,
        upstream.freebuff_auxiliary_timeout,
        upstream.freebuff_auxiliary_response_max_bytes,
    )
    .await;
    let client_id = format!("exoroute-{}", context.request_id);
    super::body::prepare_freebuff_body(
        body,
        completion_model,
        context.streaming,
        &client_id,
        &instance_id,
        run_id.as_deref(),
    )?;

    let mut headers = HeaderMap::new();
    let (name, value) = header_value("x-freebuff-instance-id", &instance_id)
        .map_err(|error| AdapterRequestError::new(None, None, error))?;
    headers.insert(name, value);
    let (name, value) = header_value("x-codebuff-agent-id", agent_id)
        .map_err(|error| AdapterRequestError::new(None, None, error))?;
    headers.insert(name, value);
    if let Some(run_id) = run_id.as_deref() {
        let (name, value) = header_value("x-codebuff-run-id", run_id)
            .map_err(|error| AdapterRequestError::new(None, None, error))?;
        headers.insert(name, value);
    }
    headers.insert(
        http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        http::header::ACCEPT,
        HeaderValue::from_static(super::catalog::FREEBUFF_COMPLETION_ACCEPT),
    );
    headers.insert(
        http::header::USER_AGENT,
        HeaderValue::from_static(super::catalog::FREEBUFF_COMPLETION_USER_AGENT),
    );
    let finish_endpoint = run_id.as_ref().map(|_| agent_endpoint);
    Ok(AdapterRequestPreparation::new(
        headers,
        finish_endpoint,
        run_id,
    ))
}

pub(super) async fn start_agent_run(
    client: &reqwest::Client,
    endpoint: &reqwest::Url,
    secret: &str,
    agent_id: &str,
    timeout: std::time::Duration,
    max_bytes: usize,
) -> Option<String> {
    let request = client
        .post(endpoint.clone())
        .bearer_auth(secret)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .header("User-Agent", FREEBUFF_SESSION_USER_AGENT)
        .json(&json!({"action":"START","agentId":agent_id}));
    let response = match send_bounded(request, "agent START", timeout).await {
        Ok(response) => response,
        Err(_) => return None,
    };
    let status = response.status();
    if !status.is_success() {
        let _ = read_auxiliary_body(response, timeout, max_bytes).await;
        return None;
    }
    let body = match read_auxiliary_body(response, timeout, max_bytes).await {
        Ok(body) => body,
        Err(_) => return None,
    };
    let response: FreebuffAgentRunResponse = serde_json::from_slice(&body).ok()?;
    let run_id = response.run_id?.trim().to_owned();
    valid_internal_id(&run_id).then_some(run_id)
}
