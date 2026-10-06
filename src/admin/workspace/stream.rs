//! Streaming workspace chat relay: forwards one prepared inference as a
//! canonical SSE event stream to the dashboard.
//!
//! The no-persistence contract is identical to the one-shot relay: no request
//! log, no telemetry, no usage meter, and no credential state change. The
//! stream adds a transport-only responsibility — read the upstream event
//! stream incrementally, extract canonical text/reasoning deltas for the
//! provider's upstream protocol, and emit bounded dashboard SSE frames. One
//! upstream attempt is ever made: once the provider may have accepted or
//! billed an inference, the request is not retried or failed over, and a
//! mid-stream failure surfaces to the dashboard as a terminal `error` event.
//!
//! Every upstream frame is size-bounded before its delta is appended
//! (`MAX_UPSTREAM_FRAME_BYTES`), the accumulated reply is capped
//! (`MAX_STREAMED_REPLY_BYTES`), and the stream carries hard idle and overall
//! deadlines so a silent upstream cannot hold the dashboard connection. When
//! the dashboard disconnects, the bridge is dropped: the upstream response
//! body is closed with it, releasing the provider socket.

use super::*;
use crate::gateway::{provider_failure_message, sanitize_provider_error_body};
use crate::protocol::UpstreamProtocol;
use crate::provider_adapters::{
    MAX_WORKSPACE_CHAT_RESPONSE_BYTES, is_event_stream_content_type, read_limited_response,
    upstream_transport_error_message,
};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::StreamExt;
use std::time::Duration;

use super::delta::{UpstreamDelta, extract_upstream_delta};

/// Largest upstream SSE data payload accepted, in bytes. A frame carrying
/// more than this is malformed for a chat stream and ends the turn.
pub(super) const MAX_UPSTREAM_FRAME_BYTES: usize = 256 * 1024;

/// Cap on the accumulated assistant reply for one streamed turn.
pub(super) const MAX_STREAMED_REPLY_BYTES: usize = 1024 * 1024;

/// Liveness bound for one streamed turn: a stream that sends nothing for this
/// long ends with a timeout error instead of holding the connection.
pub(super) const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Hard ceiling for one streamed turn regardless of frame progress. Generous
/// for long generations; the bound protects sockets, not throughput.
pub(super) const STREAM_TOTAL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Bounded channel depth between the upstream reader task and the SSE
/// response. A dashboard that stops reading applies backpressure to the
/// upstream read loop instead of buffering the provider's whole reply.
const STREAM_CHANNEL_CAPACITY: usize = 32;

/// Broadcast interval for dashboard keep-alive comments while the turn runs.
const STREAM_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// Streams one dashboard chat turn as canonical SSE events:
/// `text-delta`, `reasoning-delta`, `done`, and `error`. Validation, target
/// preparation, and credential resolution are identical to the one-shot relay,
/// so failures before the upstream send return normal JSON API errors and
/// only in-stream failures travel as terminal `error` events.
pub(crate) async fn stream_workspace_chat(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Result<
    Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>,
    (StatusCode, Json<Value>),
> {
    let (provider_id, model) = parse_chat_target_ids(&input)?;
    let text = parse_chat_text_input(&input)?;
    let attachments = parse_chat_attachments(&input)?;
    let turn = prepare_chat_turn(&state, &provider_id, &model, &text, &attachments, true).await?;
    let credential = resolve_turn_credential(&state, &turn).await?;
    let auth = credential.as_turn_auth();

    let response = provider_adapters::relay_workspace_inference_stream(
        &turn.adapter_id,
        provider_adapters::AdapterWorkspaceChatRequest {
            state: &state,
            base_url: &turn.base_url,
            provider_id: &turn.provider_id,
            model: &turn.model,
            auth_type: &turn.auth_type,
            auth_header: turn.auth_header.as_deref(),
            custom_headers: &turn.custom_headers,
            protocol: turn.client_protocol,
            secret: auth.secret,
            oauth_auth: auth.oauth_auth,
            body: turn.upstream_body,
        },
    )
    .await
    .map_err(|error| {
        fail(
            error.status.unwrap_or(StatusCode::BAD_GATEWAY),
            error.message,
        )
    })?;

    let status = response.status();
    if !status.is_success() {
        // The status arrived before any streaming commitment, so the normal
        // sanitized JSON error shape applies and dashboard error handling
        // stays uniform. The bounded read never surfaces request secrets.
        let provider_detail = read_limited_response(response, MAX_WORKSPACE_CHAT_RESPONSE_BYTES)
            .await
            .ok()
            .map(|body| sanitize_provider_error_body(&body))
            .filter(|detail| !detail.trim().is_empty());
        let message = match provider_detail.as_deref() {
            Some(detail) if !detail.is_empty() => format!(
                "provider '{}' returned HTTP {}: {detail}",
                turn.provider_id,
                status.as_u16()
            ),
            _ => format!(
                "provider '{}' returned HTTP {}",
                turn.provider_id,
                status.as_u16()
            ),
        };
        return Err(fail(StatusCode::BAD_GATEWAY, message));
    }
    if !is_event_stream_content_type(&response) {
        // A provider that answers a streaming request with a plain JSON body
        // reports either a configuration problem or its own failure. Keep the
        // bounded, sanitized diagnostic so the dashboard shows why instead of
        // only the framing mismatch.
        let message = format!(
            "provider '{}' returned a non-SSE response to a streaming request",
            turn.provider_id
        );
        let message = match read_limited_response(response, MAX_WORKSPACE_CHAT_RESPONSE_BYTES)
            .await
            .ok()
            .and_then(|body| serde_json::from_slice::<Value>(&body).ok())
        {
            Some(value) => provider_failure_message(&message, &value),
            None => message,
        };
        return Err(fail(StatusCode::BAD_GATEWAY, message));
    }

    let (outgoing, receiver) = tokio::sync::mpsc::channel::<StreamFrame>(STREAM_CHANNEL_CAPACITY);
    let upstream_protocol = turn.upstream_protocol;
    // The reader task owns the upstream body. When the dashboard disconnects
    // the bridge below is dropped, the guard aborts this task, and the
    // dropped response closes the provider connection.
    let reader = tokio::task::spawn(async move {
        pump_upstream_stream(response, upstream_protocol, outgoing).await;
    });

    let keep_alive = KeepAlive::new()
        .interval(STREAM_KEEP_ALIVE_INTERVAL)
        .text("keep-alive");
    Ok(Sse::new(bridge_stream(receiver, reader)).keep_alive(keep_alive))
}

/// One dashboard-bound SSE event before encoding.
enum StreamFrame {
    TextDelta(String),
    ReasoningDelta(String),
    Done {
        model: String,
        finish_reason: String,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        duration_ms: i64,
    },
    Error(String),
}

/// State carried through the upstream read loop.
struct PumpState {
    protocol: UpstreamProtocol,
    /// Accumulated canonical assistant text, bounded.
    text: String,
    /// Accumulated canonical reasoning, bounded separately.
    reasoning: String,
    /// Latest upstream-reported usage, forwarded with the terminal event.
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    /// Upstream-reported model, filled by the terminal event.
    model: String,
    /// Normalized terminal reason (`stop`/`length`/…), filled by the terminal
    /// event; falls back to the upstream's own string.
    finish_reason: String,
    saw_terminal: bool,
}

impl PumpState {
    fn append_delta(target: &mut String, delta: &str, label: &str) -> Result<(), String> {
        let next_len = target
            .len()
            .checked_add(delta.len())
            .ok_or_else(|| format!("{label} exceeded the size limit"))?;
        if next_len > MAX_STREAMED_REPLY_BYTES {
            return Err(format!("{label} exceeded the size limit"));
        }
        target.push_str(delta);
        Ok(())
    }
}

/// Reads the upstream body incrementally and forwards canonical deltas on
/// `outgoing`. Returns when the upstream ends, a bound fires, or the channel
/// closes (dashboard disconnected); dropping `response` with the task releases
/// the provider connection either way.
async fn pump_upstream_stream(
    response: reqwest::Response,
    protocol: UpstreamProtocol,
    outgoing: tokio::sync::mpsc::Sender<StreamFrame>,
) {
    let started = tokio::time::Instant::now();
    let mut state = PumpState {
        protocol,
        text: String::new(),
        reasoning: String::new(),
        input_tokens: None,
        output_tokens: None,
        model: String::new(),
        finish_reason: String::new(),
        saw_terminal: false,
    };
    let mut chunks = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let idle = tokio::time::sleep(STREAM_IDLE_TIMEOUT);
    tokio::pin!(idle);
    let mut failure: Option<String> = None;

    loop {
        if buffer.len() > MAX_UPSTREAM_FRAME_BYTES {
            failure = Some("upstream stream frame exceeded the size limit".to_owned());
            break;
        }
        tokio::select! {
            _ = &mut idle => {
                failure = Some("upstream stream timed out waiting for the next event".to_owned());
                break;
            }
            _ = tokio::time::sleep_until(started + STREAM_TOTAL_TIMEOUT) => {
                failure = Some("upstream stream exceeded the total time limit".to_owned());
                break;
            }
            chunk = chunks.next() => {
                idle.as_mut().reset(tokio::time::Instant::now() + STREAM_IDLE_TIMEOUT);
                let Some(chunk) = chunk else { break };
                let chunk = match chunk {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        failure = Some(format!(
                            "upstream stream could not be read: {}",
                            upstream_transport_error_message(&error)
                        ));
                        break;
                    }
                };
                buffer.extend_from_slice(&chunk);
                match drain_frames(&mut buffer, &mut state, &outgoing).await {
                    Ok(()) => {}
                    Err(message) => {
                        failure = Some(message);
                        break;
                    }
                }
            }
        }
    }
    drop(chunks);
    drop(buffer);

    if state.saw_terminal {
        // The reply is complete; a transport failure after the terminal event
        // must not turn delivered content into an error turn.
        let _ = outgoing
            .send(StreamFrame::Done {
                model: state.model,
                finish_reason: state.finish_reason,
                input_tokens: state.input_tokens,
                output_tokens: state.output_tokens,
                duration_ms: started.elapsed().as_millis() as i64,
            })
            .await;
        return;
    }
    let failure =
        failure.unwrap_or_else(|| "upstream stream ended without a completed response".to_owned());
    let _ = outgoing.send(StreamFrame::Error(failure)).await;
}

/// Consumes every complete SSE frame in `buffer`, forwarding one canonical
/// delta per frame. Returns `Ok(true)` when a terminal event completed the
/// turn, `Ok(false)` while the turn continues.
async fn drain_frames(
    buffer: &mut Vec<u8>,
    state: &mut PumpState,
    outgoing: &tokio::sync::mpsc::Sender<StreamFrame>,
) -> Result<(), String> {
    while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
        let line: Vec<u8> = buffer.drain(..=position).collect();
        let line = &line[..line.len() - 1];
        let line = if line.last() == Some(&b'\r') {
            &line[..line.len() - 1]
        } else {
            line
        };
        let line = std::str::from_utf8(line)
            .map_err(|_| "upstream stream contained a non-UTF-8 frame".to_owned())?;
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let Some(data) = line.strip_prefix("data:") else {
            // Event names and other fields are ignored: delta extraction
            // keys off the payload's own `type`, matching every protocol.
            continue;
        };
        let data = data.strip_prefix(' ').unwrap_or(data);
        if data == "[DONE]" {
            state.saw_terminal = true;
            return Ok(());
        }
        if data.len() > MAX_UPSTREAM_FRAME_BYTES {
            return Err("upstream stream frame exceeded the size limit".to_owned());
        }
        let value: Value = serde_json::from_str(data)
            .map_err(|_| "upstream stream sent a malformed event payload".to_owned())?;
        match extract_upstream_delta(state.protocol, &value) {
            Some(UpstreamDelta::Text(text)) => {
                PumpState::append_delta(&mut state.text, &text, "streamed text")?;
                let _ = outgoing.send(StreamFrame::TextDelta(text)).await;
            }
            Some(UpstreamDelta::Reasoning(text)) => {
                PumpState::append_delta(&mut state.reasoning, &text, "streamed reasoning")?;
                let _ = outgoing.send(StreamFrame::ReasoningDelta(text)).await;
            }
            Some(UpstreamDelta::Usage {
                model,
                input_tokens,
                output_tokens,
            }) => {
                if !model.is_empty() {
                    state.model = model;
                }
                if input_tokens.is_some() {
                    state.input_tokens = input_tokens;
                }
                if output_tokens.is_some() {
                    state.output_tokens = output_tokens;
                }
            }
            Some(UpstreamDelta::Terminal {
                model,
                finish_reason,
                input_tokens,
                output_tokens,
            }) => {
                state.saw_terminal = true;
                if !model.is_empty() {
                    state.model = model;
                }
                if !finish_reason.is_empty() {
                    state.finish_reason = finish_reason;
                }
                if input_tokens.is_some() {
                    state.input_tokens = input_tokens;
                }
                if output_tokens.is_some() {
                    state.output_tokens = output_tokens;
                }
            }
            None => {}
        }
    }
    Ok(())
}

/// Aborts the reader task when the bridge is dropped before a terminal frame
/// (dashboard disconnect): aborting cancels the pump, dropping its owned
/// upstream response body and releasing the provider connection. On a natural
/// end the guard detaches the already-finishing task instead.
struct ReaderGuard {
    reader: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for ReaderGuard {
    fn drop(&mut self) {
        if let Some(reader) = self.reader.take() {
            reader.abort();
        }
    }
}

/// Bridges the bounded reader channel into the SSE response stream. While the
/// dashboard consumes frames this yields each encoded event; a terminal frame
/// ends the stream. When the dashboard disconnects the stream is dropped and
/// the guard aborts the reader task.
fn bridge_stream(
    mut receiver: tokio::sync::mpsc::Receiver<StreamFrame>,
    reader: tokio::task::JoinHandle<()>,
) -> impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>> {
    async_stream::stream! {
        let mut guard = ReaderGuard { reader: Some(reader) };
        while let Some(frame) = receiver.recv().await {
            let terminal = matches!(
                frame,
                StreamFrame::Done { .. } | StreamFrame::Error(_)
            );
            yield Ok::<Event, std::convert::Infallible>(encode_frame(frame));
            if terminal {
                break;
            }
        }
        // Natural end: detach so the already-finishing task is not aborted
        // mid-drop; a dropped task simply releases the response body.
        guard.reader = None;
    }
}

/// Encodes one dashboard SSE event with a compact JSON payload.
fn encode_frame(frame: StreamFrame) -> Event {
    match frame {
        StreamFrame::TextDelta(delta) => sse_event("text-delta", &json!({ "delta": delta })),
        StreamFrame::ReasoningDelta(delta) => {
            sse_event("reasoning-delta", &json!({ "delta": delta }))
        }
        StreamFrame::Done {
            model,
            finish_reason,
            input_tokens,
            output_tokens,
            duration_ms,
        } => sse_event(
            "done",
            &json!({
                "model": model,
                "finish_reason": finish_reason,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "duration_ms": duration_ms,
            }),
        ),
        StreamFrame::Error(message) => sse_event("error", &json!({ "message": message })),
    }
}

fn sse_event(name: &str, payload: &Value) -> Event {
    Event::default().event(name).data(payload.to_string())
}
