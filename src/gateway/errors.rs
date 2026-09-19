//! Gateway error responses: JSON error bodies, storage mapping, and
//! rate-limit replies.
//!
//! Keeps response shaping separate from request orchestration so `flow.rs`
//! only decides control flow, never formats error payloads.

use super::*;

pub(in crate::gateway) fn gateway_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error":{"message":message,"type":if status==StatusCode::UNAUTHORIZED {"authentication_error"} else {"gateway_error"}}}))).into_response()
}

pub(in crate::gateway) fn gateway_database_error(error: StorageError) -> Response {
    let status = if error.is_busy() || matches!(&error, StorageError::MapFull) {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    tracing::error!(%error, "gateway storage operation failed");
    gateway_error(status, "Gateway storage is unavailable.")
}

pub(in crate::gateway) fn rate_limited_response(
    retry_after: Option<std::time::Duration>,
) -> Response {
    let mut response = gateway_error(StatusCode::TOO_MANY_REQUESTS, "gateway rate limit exceeded");
    if let Some(delay) = retry_after {
        let seconds = delay
            .as_secs()
            .saturating_add(u64::from(delay.subsec_nanos() > 0))
            .max(1);
        if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
    }
    response
}

pub(in crate::gateway) fn attach_request_live_guard(
    mut response: Response,
    live: RequestLiveGuard,
) -> Response {
    response.extensions_mut().insert(live);
    response
}

pub(in crate::gateway) fn track_analytics_response(
    response: Response,
    analytics: RequestAnalytics,
) -> Response {
    let status = response.status();
    let is_event_stream = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"));
    let stream_outcome = response.extensions().get::<StreamOutcome>().cloned();
    let (parts, body) = response.into_parts();
    let mut completion = analytics.completion_guard();
    let stream = async_stream::stream! {
        let mut chunks = body.into_data_stream();
        while let Some(chunk) = chunks.next().await {
            match chunk {
                Ok(chunk) => yield Ok::<Bytes, axum::Error>(chunk),
                Err(error) => {
                    completion.finish(false);
                    yield Err(error);
                    return;
                }
            }
        }
        let succeeded = if is_event_stream {
            stream_outcome.as_ref().is_some_and(StreamOutcome::completed)
        } else {
            status.is_success()
        };
        completion.finish(succeeded);
    };
    Response::from_parts(parts, Body::from_stream(stream))
}
