//! Continuity replay endpoints: live SSE passthrough and resumable
//! `Last-Event-ID` replay responses with continuity stream headers.

use super::records::{load_run_with_settings, read_events_with_page_size};
use super::worker::wait_for_cancellation;
use crate::{config::UpstreamSettings, state::AppState};
use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use tokio::sync::{mpsc, watch};

#[cfg(test)]
pub(in crate::gateway) fn live_response(
    receiver: mpsc::Receiver<Bytes>,
    run_id: &str,
    request_id: &str,
    app_shutdown: watch::Receiver<bool>,
) -> Response {
    live_response_with_settings(
        receiver,
        run_id,
        request_id,
        app_shutdown,
        UpstreamSettings::default(),
    )
}

pub(in crate::gateway) fn live_response_with_settings(
    mut receiver: mpsc::Receiver<Bytes>,
    run_id: &str,
    request_id: &str,
    mut app_shutdown: watch::Receiver<bool>,
    upstream: UpstreamSettings,
) -> Response {
    let stream = async_stream::stream! {
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b": exoroute stream active\n\n"));
        loop {
            tokio::select! {
                biased;
                _ = wait_for_cancellation(&mut app_shutdown) => break,
                result = tokio::time::timeout(upstream.continuity_heartbeat_interval, receiver.recv()) => match result {
                    Ok(Some(chunk)) => yield Ok::<Bytes, std::io::Error>(chunk),
                    Ok(None) => break,
                    Err(_) => yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b": keep-alive\n\n")),
                }
            }
        }
    };
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    set_stream_headers(&mut response, run_id, request_id);
    response
}

pub(in crate::gateway) fn resume_response(
    state: AppState,
    run_id: String,
    api_key_id: String,
    after: i64,
) -> Response {
    let upstream = state.operational_settings().settings.upstream;
    let response_run_id = run_id.clone();
    let stream = async_stream::stream! {
        let mut last_sequence = after;
        let mut app_shutdown = state.shutdown_receiver();
        let mut keepalive = tokio::time::interval_at(
            tokio::time::Instant::now() + upstream.continuity_heartbeat_interval,
            upstream.continuity_heartbeat_interval,
        );
        keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b": exoroute stream resumed\n\n"));
        'resume: loop {
            let snapshot = tokio::select! {
                biased;
                _ = wait_for_cancellation(&mut app_shutdown) => break 'resume,
                result = load_run_with_settings(&state.db, &run_id, &api_key_id, upstream) => match result {
                Ok(Some(snapshot)) => snapshot,
                Ok(None) => {
                    yield Ok::<Bytes, std::io::Error>(Bytes::from("event: error\ndata: {\"error\":\"Stream run is no longer available\"}\n\n"));
                    break 'resume;
                }
                Err(error) => {
                    tracing::warn!(%error, %run_id, "could not load stream run while resuming");
                    yield Ok(Bytes::from("event: error\ndata: {\"error\":\"Stream replay could not be read\"}\n\n"));
                    break 'resume;
                }
                }
            };
            let events = tokio::select! {
                biased;
                _ = wait_for_cancellation(&mut app_shutdown) => break 'resume,
                result = read_events_with_page_size(&state.db, &run_id, last_sequence, upstream) => match result {
                Ok(events) => events,
                Err(error) => {
                    tracing::warn!(%error, %run_id, "could not read stream replay events");
                    yield Ok(Bytes::from("event: error\ndata: {\"error\":\"Stream replay could not be read\"}\n\n"));
                    break 'resume;
                }
                }
            };
            if !events.is_empty() {
                for (sequence, payload) in events {
                    last_sequence = sequence;
                    yield Ok(Bytes::from(payload));
                }
                continue;
            }
            if !snapshot.replay_complete && last_sequence >= snapshot.retained_through {
                yield Ok(Bytes::from("event: error\ndata: {\"error\":\"Stream replay reached its configured storage limit; the live task continued\"}\n\n"));
                break;
            }
            if snapshot.status != "running" {
                break;
            }
            tokio::select! {
                biased;
                _ = wait_for_cancellation(&mut app_shutdown) => break 'resume,
                _ = tokio::time::sleep(upstream.continuity_poll_interval) => {}
                _ = keepalive.tick() => {
                    yield Ok(Bytes::from_static(b": keep-alive\n\n"));
                }
            }
        }
    };
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    set_stream_headers(&mut response, &response_run_id, "");
    response
}

pub(in crate::gateway) fn set_stream_headers(
    response: &mut Response,
    run_id: &str,
    request_id: &str,
) {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    if let Ok(value) = HeaderValue::from_str(run_id) {
        headers.insert("x-exoroute-stream-id", value);
    }
    if !request_id.is_empty()
        && let Ok(value) = HeaderValue::from_str(request_id)
    {
        headers.insert("x-request-id", value);
    }
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    headers.insert(
        "access-control-expose-headers",
        HeaderValue::from_static("x-exoroute-stream-id,x-request-id"),
    );
}

pub(in crate::gateway) fn continuity_capacity_error(max_concurrency: usize) -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::Json(serde_json::json!({
            "error": {
                "message": format!("all {max_concurrency} stream continuity slots are in use"),
                "type": "server_error",
                "code": "stream_continuity_capacity"
            }
        })),
    )
        .into_response()
}
