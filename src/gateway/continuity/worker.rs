//! Continuity background workers: bounded tasks that keep consuming an
//! upstream stream after client disconnect, with cancellation handling.

use super::records::{
    BackgroundRequestSettings, append_event_with_settings, finish_run_with_settings,
    mark_replay_incomplete,
};
use crate::gateway::streaming::StreamOutcome;
use crate::{
    infra::telemetry::AnalyticsCompletionGuard,
    state::{AppState, DynamicSemaphorePermit, RequestLiveGuard},
};
use axum::{body::to_bytes, http::header, response::Response};
use bytes::Bytes;
use futures_util::StreamExt;
use std::future::Future;
use tokio::sync::{mpsc, watch};

/// Drop guard that unregisters a stream's cancellation entry no matter how the
/// background task exits (completion, cancellation, shutdown, timeout, or
/// panic while unwinding). Individual exit paths may still call
/// `remove_stream_cancellation` explicitly; the guard makes that a no-op.
struct StreamCancellationGuard {
    state: AppState,
    run_id: String,
}

impl Drop for StreamCancellationGuard {
    fn drop(&mut self) {
        self.state.remove_stream_cancellation(&self.run_id);
    }
}

struct StreamWorkerControl {
    live_sender: mpsc::Sender<Bytes>,
    cancellation: watch::Receiver<bool>,
    app_shutdown: watch::Receiver<bool>,
    permit: Option<DynamicSemaphorePermit>,
}

impl StreamWorkerControl {
    fn release_admission(&mut self) {
        self.permit.take();
    }
}

#[cfg(test)]
pub(in crate::gateway) async fn start_background_stream(
    state: AppState,
    run_id: String,
    producer: Response,
    live_sender: mpsc::Sender<Bytes>,
    cancellation: watch::Receiver<bool>,
    permit: DynamicSemaphorePermit,
) {
    tokio::spawn(async move {
        let mut control = StreamWorkerControl {
            live_sender,
            cancellation,
            app_shutdown: state.shutdown_receiver(),
            permit: Some(permit),
        };
        let _cancellation_guard = StreamCancellationGuard {
            state: state.clone(),
            run_id: run_id.clone(),
        };
        consume_producer(state, run_id, producer, &mut control, None, None).await;
    });
}

pub(in crate::gateway) async fn start_background_request<F>(
    state: AppState,
    run_id: String,
    request: F,
    live_sender: mpsc::Sender<Bytes>,
    cancellation: watch::Receiver<bool>,
    permit: DynamicSemaphorePermit,
    settings: BackgroundRequestSettings,
) where
    F: Future<Output = Response> + Send + 'static,
{
    let BackgroundRequestSettings {
        request_timeout,
        analytics,
    } = settings;
    let upstream = state.operational_settings().settings.upstream;
    let app_shutdown = state.shutdown_receiver();
    // Construct this before spawning so an unpolled task dropped at shutdown
    // still records the admitted request as unsuccessful.
    let mut analytics_completion = analytics.map(|analytics| analytics.completion_guard());
    tokio::spawn(async move {
        let mut control = StreamWorkerControl {
            live_sender,
            cancellation,
            app_shutdown,
            permit: Some(permit),
        };
        let _cancellation_guard = StreamCancellationGuard {
            state: state.clone(),
            run_id: run_id.clone(),
        };
        let request = async move {
            match request_timeout {
                Some(timeout) => tokio::time::timeout(timeout, request).await,
                None => Ok(request.await),
            }
        };
        tokio::pin!(request);
        let response = tokio::select! {
            biased;
            _ = wait_for_cancellation(&mut control.app_shutdown) => {
                if let Some(completion) = analytics_completion.as_mut() {
                    completion.finish(false);
                }
                return;
            }
            _ = wait_for_cancellation(&mut control.cancellation) => {
                // Release the continuity admission before publishing the
                // cancelled status. Consumers use that status as the
                // completion boundary and must not observe a slot that is
                // still held by this worker.
                control.release_admission();
                let mut journal_available = true;
                finish_cancelled_run(
                    &state,
                    &run_id,
                    0,
                    &control.live_sender,
                    &mut journal_available,
                )
                .await;
                if let Some(completion) = analytics_completion.as_mut() {
                    completion.finish(false);
                }
                return;
            }
            response = &mut request => match response {
                Ok(response) => response,
                Err(_) => {
                    fail_before_stream(
                        &state,
                        &run_id,
                        &mut control,
                        None,
                        &format!(
                            "stream request exceeded the {}-second deadline",
                            upstream.continuity_request_timeout.as_secs()
                        ),
                    ).await;
                    if let Some(completion) = analytics_completion.as_mut() {
                        completion.finish(false);
                    }
                    return;
                }
            }
        };
        if *control.app_shutdown.borrow() {
            if let Some(completion) = analytics_completion.as_mut() {
                completion.finish(false);
            }
            return;
        }
        if !is_successful_sse(&response) {
            let (mut response_parts, response_body) = response.into_parts();
            let live = response_parts.extensions.remove::<RequestLiveGuard>();
            let response = Response::from_parts(response_parts, response_body);
            fail_before_stream(
                &state,
                &run_id,
                &mut control,
                Some(response),
                "gateway could not start the event stream",
            )
            .await;
            drop(live);
            if let Some(completion) = analytics_completion.as_mut() {
                completion.finish(false);
            }
            return;
        }
        let outcome = response.extensions().get::<StreamOutcome>().cloned();
        consume_producer(
            state,
            run_id,
            response,
            &mut control,
            outcome,
            analytics_completion,
        )
        .await;
    });
}

pub(in crate::gateway) async fn wait_for_cancellation(cancellation: &mut watch::Receiver<bool>) {
    loop {
        if *cancellation.borrow() {
            return;
        }
        if cancellation.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

fn is_successful_sse(response: &Response) -> bool {
    response.status().is_success()
        && response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
}

async fn fail_before_stream(
    state: &AppState,
    run_id: &str,
    control: &mut StreamWorkerControl,
    response: Option<Response>,
    fallback_message: &str,
) {
    let upstream = state.operational_settings().settings.upstream;
    let (status, body) = match response {
        Some(response) => {
            let status = response.status();
            let body = tokio::select! {
                _ = wait_for_cancellation(&mut control.app_shutdown) => return,
                body = to_bytes(response.into_body(), upstream.continuity_setup_error_bytes) => body.ok(),
            };
            (Some(status), body)
        }
        None => (None, None),
    };
    let message = body
        .as_deref()
        .and_then(|body| serde_json::from_slice::<serde_json::Value>(body).ok())
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .filter(|message| !message.trim().is_empty() && message.len() <= 512)
        .unwrap_or_else(|| {
            status.map_or_else(
                || fallback_message.to_owned(),
                |status| format!("{fallback_message} (HTTP {})", status.as_u16()),
            )
        });
    let event = format!(
        "event: error\ndata: {}\n\n",
        serde_json::json!({"error":{"message":message}})
    );
    let payload = numbered_event(1, event.as_bytes());
    let mut journal_available = true;
    let append_result = tokio::select! {
        _ = wait_for_cancellation(&mut control.app_shutdown) => return,
        result = append_event_with_settings(&state.db, run_id, &payload, upstream) => result,
    };
    if let Err(error) = append_result {
        journal_available = false;
        tracing::warn!(%error, %run_id, "could not journal a stream setup error");
        if *control.app_shutdown.borrow() {
            return;
        }
        let _ = mark_replay_incomplete(&state.db, run_id, 1).await;
    }
    if *control.app_shutdown.borrow() {
        return;
    }
    let mut cancellation_watch_open = true;
    if !send_live_or_cancel(
        &control.live_sender,
        payload,
        &mut control.cancellation,
        &mut cancellation_watch_open,
        &mut control.app_shutdown,
    )
    .await
    {
        if !*control.app_shutdown.borrow() {
            control.release_admission();
            finish_cancelled_run(
                state,
                run_id,
                1,
                &control.live_sender,
                &mut journal_available,
            )
            .await;
        }
        return;
    }
    if *control.app_shutdown.borrow() {
        return;
    }
    let finish_result = tokio::select! {
        _ = wait_for_cancellation(&mut control.app_shutdown) => return,
        result = finish_run_with_settings(
            &state.db,
            run_id,
            "failed",
            Some("stream setup failed before the provider event stream started"),
            upstream,
        ) => result,
    };
    if let Err(error) = finish_result {
        tracing::warn!(%error, %run_id, "could not save failed stream setup status");
    }
}

async fn consume_producer(
    state: AppState,
    run_id: String,
    producer: Response,
    control: &mut StreamWorkerControl,
    outcome: Option<StreamOutcome>,
    mut analytics_completion: Option<AnalyticsCompletionGuard>,
) {
    let upstream = state.operational_settings().settings.upstream;
    let (_, body) = producer.into_parts();
    let mut chunks = body.into_data_stream();
    let mut sequence = 0i64;
    let mut journal_available = true;
    let mut cancellation_watch_open = true;
    let mut shutting_down = *control.app_shutdown.borrow();

    loop {
        if shutting_down || *control.app_shutdown.borrow() {
            shutting_down = true;
            break;
        }
        if cancellation_watch_open && *control.cancellation.borrow() {
            control.release_admission();
            finish_cancelled_run(
                &state,
                &run_id,
                sequence,
                &control.live_sender,
                &mut journal_available,
            )
            .await;
            if let Some(completion) = analytics_completion.as_mut() {
                completion.finish(false);
            }
            break;
        }
        tokio::select! {
            biased;
            _ = wait_for_cancellation(&mut control.app_shutdown) => {
                shutting_down = true;
                break;
            }
            changed = control.cancellation.changed(), if cancellation_watch_open => {
                match changed {
                    Ok(()) if *control.cancellation.borrow() => {
                        control.release_admission();
                        finish_cancelled_run(
                            &state,
                            &run_id,
                            sequence,
                            &control.live_sender,
                            &mut journal_available,
                        )
                        .await;
                        if let Some(completion) = analytics_completion.as_mut() {
                            completion.finish(false);
                        }
                        break;
                    }
                    Ok(()) => {}
                    Err(_) => cancellation_watch_open = false,
                }
            }
            next = chunks.next() => {
                match next {
                    Some(Ok(chunk)) => {
                        let next_sequence = if journal_available {
                            let event_payload = numbered_event(
                                sequence.saturating_add(1),
                                &chunk,
                            );
                            let append_result = tokio::select! {
                                _ = wait_for_cancellation(&mut control.app_shutdown) => {
                                    shutting_down = true;
                                    None
                                }
                                result = append_event_with_settings(&state.db, &run_id, &event_payload, upstream) => Some(result),
                            };
                            let Some(append_result) = append_result else {
                                break;
                            };
                            match append_result {
                                Ok(sequence) => sequence,
                                Err(error) => {
                                    journal_available = false;
                                    let fallback = sequence.saturating_add(1);
                                    tracing::warn!(%error, %run_id, "stream replay journal stopped accepting events; live stream will continue");
                                    if !*control.app_shutdown.borrow() {
                                        let _ = mark_replay_incomplete(&state.db, &run_id, fallback).await;
                                    }
                                    fallback
                                }
                            }
                        } else {
                            sequence.saturating_add(1)
                        };
                        sequence = next_sequence;
                        let payload = numbered_event(sequence, &chunk);
                        if !send_live_or_cancel(
                            &control.live_sender,
                            payload,
                            &mut control.cancellation,
                            &mut cancellation_watch_open,
                            &mut control.app_shutdown,
                        )
                        .await {
                            if *control.app_shutdown.borrow() {
                                shutting_down = true;
                            } else {
                                control.release_admission();
                                finish_cancelled_run(
                                    &state,
                                    &run_id,
                                    sequence,
                                    &control.live_sender,
                                    &mut journal_available,
                                )
                                .await;
                            }
                            if let Some(completion) = analytics_completion.as_mut() {
                                completion.finish(false);
                            }
                            break;
                        }
                    }
                    Some(Err(error)) => {
                        if *control.app_shutdown.borrow() {
                            shutting_down = true;
                            break;
                        }
                        tracing::warn!(%error, %run_id, "translated stream body failed");
                        let _ = tokio::select! {
                            _ = wait_for_cancellation(&mut control.app_shutdown) => {
                                shutting_down = true;
                                None
                            }
                            result = finish_run_with_settings(&state.db, &run_id, "failed", Some("translated stream body failed"), upstream) => Some(result),
                        };
                        if let Some(completion) = analytics_completion.as_mut() {
                            completion.finish(false);
                        }
                        break;
                    }
                    None => {
                        if *control.app_shutdown.borrow() {
                            shutting_down = true;
                            break;
                        }
                        let failed = outcome.as_ref().is_some_and(StreamOutcome::failed);
                        let completed = outcome.as_ref().is_none_or(StreamOutcome::completed);
                        let (status, error) = if failed || !completed {
                            ("failed", Some("upstream stream failed before successful completion"))
                        } else {
                            ("completed", None)
                        };
                        let finish_result = tokio::select! {
                            _ = wait_for_cancellation(&mut control.app_shutdown) => {
                                shutting_down = true;
                                None
                            }
                            result = finish_run_with_settings(&state.db, &run_id, status, error, upstream) => Some(result),
                        };
                        if let Some(Err(error)) = finish_result {
                            tracing::warn!(%error, %run_id, "could not save completed stream status");
                        }
                        if let Some(completion) = analytics_completion.as_mut() {
                            completion.finish(status == "completed");
                        }
                        break;
                    }
                }
            }
        }
    }

    if !shutting_down
        && !journal_available
        && let Err(error) = mark_replay_incomplete(&state.db, &run_id, sequence).await
    {
        tracing::warn!(%error, %run_id, "could not finalize the incomplete stream replay journal");
    }
}

async fn finish_cancelled_run(
    state: &AppState,
    run_id: &str,
    sequence: i64,
    live_sender: &mpsc::Sender<Bytes>,
    journal_available: &mut bool,
) {
    let upstream = state.operational_settings().settings.upstream;
    let cancellation_sequence = sequence.saturating_add(1);
    let payload = numbered_event(
        cancellation_sequence,
        b"event: error\ndata: {\"error\":\"Stream cancelled by user\"}\n\n",
    );
    if *journal_available
        && let Err(error) = append_event_with_settings(&state.db, run_id, &payload, upstream).await
    {
        *journal_available = false;
        tracing::warn!(%error, %run_id, "stream replay journal failed while cancelling a stream");
        let _ = mark_replay_incomplete(&state.db, run_id, cancellation_sequence).await;
    }
    // Cancellation must release the upstream body and its permit even if a
    // client has stopped reading and the bounded live channel is full. The
    // journal keeps this terminal event available for the next connection.
    let _ = live_sender.try_send(payload);
    if let Err(error) = finish_run_with_settings(
        &state.db,
        run_id,
        "cancelled",
        Some("cancelled by user"),
        upstream,
    )
    .await
    {
        tracing::warn!(%error, %run_id, "could not save cancelled stream status");
    }
}

async fn send_live_or_cancel(
    sender: &mpsc::Sender<Bytes>,
    payload: Bytes,
    cancellation: &mut watch::Receiver<bool>,
    watch_open: &mut bool,
    app_shutdown: &mut watch::Receiver<bool>,
) -> bool {
    loop {
        if *app_shutdown.borrow() {
            return false;
        }
        if *watch_open && *cancellation.borrow() {
            return false;
        }
        tokio::select! {
            result = sender.send(payload.clone()) => {
                if *app_shutdown.borrow()
                    || (*watch_open && *cancellation.borrow())
                {
                    return false;
                }
                return result.is_ok() || sender.is_closed();
            },
            changed = cancellation.changed(), if *watch_open => {
                match changed {
                    Ok(()) if *cancellation.borrow() => return false,
                    Ok(()) => {}
                    Err(_) => *watch_open = false,
                }
            },
            changed = app_shutdown.changed() => {
                if changed.is_ok() && *app_shutdown.borrow() {
                    return false;
                }
            }
        }
    }
}

pub(in crate::gateway) fn numbered_event(sequence: i64, payload: &[u8]) -> Bytes {
    let prefix = format!("id: {sequence}\n");
    let mut numbered = Vec::with_capacity(prefix.len().saturating_add(payload.len()));
    numbered.extend_from_slice(prefix.as_bytes());
    numbered.extend_from_slice(payload);
    Bytes::from(numbered)
}
