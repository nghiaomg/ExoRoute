//! Freebuff agent FINISH: session invalidation on credential-shaped
//! failures plus best-effort run completion.
//!
//! FINISH failures never fail the gateway response; they are debug-logged
//! so a dropped completion cannot break an otherwise successful request.

use super::catalog::FREEBUFF_SESSION_USER_AGENT;
use super::client::{freebuff_client, read_auxiliary_body, send_bounded};
use super::{AdapterRequestPreparation, UpstreamAuthContext, freebuff_session_cache_key};
use crate::state::AppState;
use http::StatusCode;
use serde_json::json;

pub(super) async fn finish_freebuff_agent_run(
    state: &AppState,
    preparation: AdapterRequestPreparation,
    auth: UpstreamAuthContext<'_>,
    status: Option<StatusCode>,
) {
    if let Some(secret) = auth.secret
        && status.is_some_and(|status| {
            matches!(
                status,
                StatusCode::UNAUTHORIZED
                    | StatusCode::FORBIDDEN
                    | StatusCode::NOT_FOUND
                    | StatusCode::GONE
                    | StatusCode::CONFLICT
            )
        })
    {
        state
            .invalidate_freebuff_session(freebuff_session_cache_key(secret))
            .await;
    }
    let (Some(endpoint), Some(run_id), Some(secret)) = (
        preparation.finish_endpoint,
        preparation.finish_run_id,
        auth.secret,
    ) else {
        return;
    };
    if auth.auth_type != "bearer" {
        return;
    }
    let upstream = state.operational_settings().settings.upstream;
    let Ok((endpoint, client)) = freebuff_client(state, &endpoint).await else {
        return;
    };
    let completion_status = if status.is_some_and(|status| status.is_success()) {
        "completed"
    } else {
        "failed"
    };
    let request = client
        .post(endpoint)
        .bearer_auth(secret)
        .header(http::header::CONTENT_TYPE, "application/json")
        .header(http::header::ACCEPT, "application/json")
        .header("User-Agent", FREEBUFF_SESSION_USER_AGENT)
        .json(&json!({
            "action":"FINISH",
            "runId":run_id,
            "status":completion_status,
            "totalSteps":1,
            "directCredits":0,
            "totalCredits":0
        }));
    let Ok(response) =
        send_bounded(request, "agent FINISH", upstream.freebuff_auxiliary_timeout).await
    else {
        return;
    };
    let finish_status = response.status();
    let _ = read_auxiliary_body(
        response,
        upstream.freebuff_auxiliary_timeout,
        upstream.freebuff_auxiliary_response_max_bytes,
    )
    .await;
    if !finish_status.is_success() {
        tracing::debug!(
            status = finish_status.as_u16(),
            "Freebuff agent FINISH was not accepted"
        );
    }
}
