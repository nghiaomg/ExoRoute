//! The Devin CLI turn pipeline: one agent turn rendered as Chat
//! Completions output, with the retry that covers Devin's transient
//! model-capacity refusal. Session and transport live in [`super::acp`].

use super::acp::{AcpSession, MAX_OUTPUT_TEXT_CHARS, TurnEvent, has_assistant_output};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

fn estimated_tokens(chars: usize) -> u64 {
    // Rough character/4 estimate, mirroring the CLI bridges in 9Router and
    // OmniRoute; the gateway marks these as estimates by contract.
    (chars as u64).div_ceil(4)
}

pub(super) fn unix_timestamp() -> u64 {
    // unix_seconds() already maps a pre-epoch clock to 0.
    crate::support::time::unix_seconds() as u64
}

fn frame(value: &Value) -> Vec<u8> {
    // json!-built values are acyclic with string keys, so serialization
    // cannot fail; the guard keeps that invariant explicit.
    let text = serde_json::to_string(value).expect("static JSON values always serialize");
    let mut bytes = b"data: ".to_vec();
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\n\n");
    bytes
}

pub(super) fn done_frame() -> Vec<u8> {
    b"data: [DONE]\n\n".to_vec()
}

/// Re-arms a refused turn against a fresh session on the same CLI process:
/// the child is still alive after a refused prompt, and re-initializing on
/// the existing transport is accepted. A short pause first gives a
/// capacity-bound backend room to recover. Returns the new prompt id, or
/// `None` when the re-handshake or the bounded wait failed — the caller then
/// reports the original refusal.
async fn retry_turn<W, R>(
    session: &mut AcpSession<W, R>,
    session_id: &str,
    model: &str,
    prompt_text: &str,
    deadline: tokio::time::Instant,
) -> Option<(u64, String)>
where
    W: AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
{
    // The refused session is dropped from the transport's store before the
    // fresh one is created; its reply is never read by design.
    let _ = session.forget_session(session_id).await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let rearm = async {
        let session_id = session.start_session(Some(model)).await.ok()?;
        let prompt_id = session.send_prompt(&session_id, prompt_text).await.ok()?;
        Some((prompt_id, session_id))
    };
    tokio::time::timeout_at(deadline, rearm)
        .await
        .ok()
        .flatten()
}

pub(super) fn chat_chunk_frame(
    completion_id: &str,
    model: &str,
    created: u64,
    delta: Value,
    finish_reason: Option<&str>,
    usage: Option<Value>,
) -> Vec<u8> {
    let mut body = json!({
        "id": completion_id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": model,
        "choices": [{"index": 0, "delta": delta, "finish_reason": finish_reason}],
    });
    if let Some(usage) = usage {
        body["usage"] = usage;
    }
    frame(&body)
}

/// Mid-stream failure: an OpenAI-style error event the gateway's stream
/// error extraction understands, then a clean `[DONE]` terminator.
fn error_turn_frame(message: &str) -> Vec<u8> {
    let mut bytes = frame(&json!({
        "error": {"message": message, "type": "upstream_error", "code": "devin_cli_failure"},
    }));
    bytes.extend_from_slice(b"data: [DONE]\n\n");
    bytes
}

/// Render parameters for a CLI turn, shared by the streaming and buffered
/// turn loops.
pub(super) struct TurnRender {
    pub completion_id: String,
    /// Caller-side tool names; empty when the client declared no tools.
    pub caller_tools: Vec<String>,
    /// OpenAI-style tool_call id for a converted envelope.
    pub tool_call_id: String,
    pub model: String,
    pub created: u64,
    /// The CLI session this turn runs in, so it can be deleted from the
    /// transport's persistent store once the turn ends.
    pub session_id: String,
    /// The flattened prompt text, kept so a transient server-side refusal can
    /// be retried once against a fresh session.
    pub prompt_text: String,
    pub prompt_chars: usize,
    pub request_timeout: Duration,
}

/// Whether the CLI turn was refused by Devin's backend for a transient
/// model-availability reason ("Selected model is at capacity. Please try a
/// different model."). Such a refusal arrives as the turn's error result, so
/// the upstream provably never accepted or billed the inference, and one
/// retry against a fresh session is safe.
pub(super) fn is_transient_model_refusal(error: &str) -> bool {
    let lowered = error.to_ascii_lowercase();
    lowered.contains("try a different model") || lowered.contains("at capacity")
}

/// Streams one CLI turn as Chat Completions SSE frames. `guard` is opaque
/// state that must outlive the stream — the spawned `Child` and the session
/// permit — so cancelling the response future (client disconnect, upstream
/// failure) drops and kills the agent process.
pub(super) fn streaming_turn<W, R, G>(
    mut session: AcpSession<W, R>,
    guard: G,
    prompt_id: u64,
    render: TurnRender,
) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send + 'static
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
    G: Send + 'static,
{
    async_stream::stream! {
        let _keep_alive = guard;
        let TurnRender { completion_id, caller_tools: _, tool_call_id: _, model, created, session_id, prompt_text, prompt_chars, request_timeout } = render;
        let mut session_id = session_id;
        let mut output_chars: usize = 0;
        let mut has_non_whitespace_output = false;
        let mut prompt_id = prompt_id;
        // Devin's backend occasionally refuses a turn because the selected
        // model is at capacity; the refusal is transient, so the turn is
        // retried once against a fresh session — but only before any text has
        // streamed, because a retry cannot unsend what the client saw.
        let mut retried_refusal = false;
        // The summarizer wraps its report in <summary>…</summary> markup that
        // must never reach the client; the stripper tolerates tags split
        // across chunk boundaries. See [`super::summary`] for the contract.
        let mut stripper = super::summary::SummaryStripper::new();
        yield Ok(chat_chunk_frame(
            &completion_id,
            &model,
            created,
            json!({"role": "assistant", "content": ""}),
            None,
            None,
        ));
        let deadline = tokio::time::Instant::now() + request_timeout;
        loop {
            match tokio::time::timeout_at(deadline, session.next_turn_event(prompt_id)).await {
                Err(_) => {
                    yield Ok(error_turn_frame("the Devin CLI session exceeded the request timeout"));
                    break;
                }
                Ok(Err(error)) => {
                    if !retried_refusal
                        && output_chars == 0
                        && is_transient_model_refusal(&error)
                        && let Some((new_prompt_id, new_session_id)) =
                            retry_turn(&mut session, &session_id, &model, &prompt_text, deadline).await
                    {
                        retried_refusal = true;
                        prompt_id = new_prompt_id;
                        session_id = new_session_id;
                        continue;
                    }
                    yield Ok(error_turn_frame(&error));
                    break;
                }
                Ok(Ok(None)) => {
                    yield Ok(error_turn_frame("the Devin CLI exited before finishing the turn"));
                    break;
                }
                Ok(Ok(Some(TurnEvent::Text(delta)))) => {
                    let delta = stripper.push(&delta);
                    if !delta.is_empty() {
                        output_chars += delta.chars().count();
                        has_non_whitespace_output |= !delta.trim().is_empty();
                        if output_chars > MAX_OUTPUT_TEXT_CHARS {
                            yield Ok(error_turn_frame("the Devin CLI reply exceeded the output limit"));
                            break;
                        }
                        yield Ok(chat_chunk_frame(
                            &completion_id,
                            &model,
                            created,
                            json!({"content": delta}),
                            None,
                            None,
                        ));
                    }
                }
                Ok(Ok(Some(TurnEvent::Ended { stop_reason }))) => {
                    let tail = stripper.finish();
                    if !tail.is_empty() {
                        output_chars += tail.chars().count();
                        has_non_whitespace_output |= !tail.trim().is_empty();
                        yield Ok(chat_chunk_frame(
                            &completion_id,
                            &model,
                            created,
                            json!({"content": tail}),
                            None,
                            None,
                        ));
                    }
                    if !has_non_whitespace_output {
                        yield Ok(error_turn_frame("the Devin CLI completed the turn without assistant text"));
                        break;
                    }
                    let finish_reason = if stop_reason == "max_tokens" { "length" } else { "stop" };
                    let usage = json!({
                        "prompt_tokens": estimated_tokens(prompt_chars),
                        "completion_tokens": estimated_tokens(output_chars),
                        "total_tokens": estimated_tokens(prompt_chars + output_chars),
                    });
                    yield Ok(chat_chunk_frame(
                        &completion_id,
                        &model,
                        created,
                        json!({}),
                        Some(finish_reason),
                        Some(usage),
                    ));
                    yield Ok(done_frame());
                    break;
                }
            }
        }
        let _ = session.forget_session(&session_id).await;
    }
}

/// Buffers one CLI turn into a complete `chat.completion` response for
/// non-streaming clients. Fails instead of emitting a partial answer: the
/// client has received nothing yet, so a clean error beats a truncated body.
pub(super) async fn buffered_turn<W, R, G>(
    mut session: AcpSession<W, R>,
    guard: G,
    prompt_id: u64,
    render: TurnRender,
) -> Result<Value, String>
where
    W: AsyncWrite + Unpin + Send + 'static,
    R: AsyncRead + Unpin + Send + 'static,
    G: Send + 'static,
{
    let _keep_alive = guard;
    let TurnRender {
        completion_id,
        caller_tools,
        tool_call_id,
        model,
        created,
        session_id,
        prompt_text,
        prompt_chars,
        request_timeout,
    } = render;
    let deadline = tokio::time::Instant::now() + request_timeout;
    let mut session_id = session_id;
    let mut prompt_id = prompt_id;
    // Same transient-refusal retry as the streaming loop: one fresh-session
    // re-prompt while nothing has been buffered yet.
    let mut retried_refusal = false;
    let mut text = String::new();
    let result = loop {
        let event = tokio::time::timeout_at(deadline, session.next_turn_event(prompt_id))
            .await
            .map_err(|_| "the Devin CLI session exceeded the request timeout".to_owned())?;
        match event {
            Err(error) => {
                if !retried_refusal
                    && text.is_empty()
                    && is_transient_model_refusal(&error)
                    && let Some((new_prompt_id, new_session_id)) =
                        retry_turn(&mut session, &session_id, &model, &prompt_text, deadline).await
                {
                    retried_refusal = true;
                    prompt_id = new_prompt_id;
                    session_id = new_session_id;
                    continue;
                }
                break Err(error);
            }
            Ok(Some(TurnEvent::Text(delta))) => {
                text.push_str(&delta);
                if text.chars().count() > MAX_OUTPUT_TEXT_CHARS {
                    break Err("the Devin CLI reply exceeded the output limit".to_owned());
                }
            }
            Ok(Some(TurnEvent::Ended { stop_reason })) => {
                // The summarizer wraps its report in <summary>…</summary>
                // markup; strip the envelope before the client sees the text
                // and before the tool-envelope resolution reads it. See
                // [`super::summary`] for the contract.
                let text = super::summary::strip_summary_envelope(&text);
                if !has_assistant_output(&text) {
                    break Err("the Devin CLI completed the turn without assistant text".to_owned());
                }
                let completion_tokens = estimated_tokens(text.chars().count());
                // Router-level tool-envelope resolution (see docs/OmniRoute
                // DEVIN_CLAUDE_BRIDGE.md): a model turn that wrapped an action
                // in a tool envelope converts into a proper tool_calls
                // response; raw markup never leaks to the client.
                let (message, finish_reason) = if caller_tools.is_empty() {
                    (
                        json!({"role": "assistant", "content": text}),
                        if stop_reason == "max_tokens" {
                            "length"
                        } else {
                            "stop"
                        },
                    )
                } else {
                    match super::tool_envelope::resolve_tool_envelope(&text, &caller_tools) {
                        super::tool_envelope::ToolEnvelopeOutcome::ToolCall(call) => (
                            json!({
                                "role": "assistant",
                                "content": null,
                                "tool_calls": [{
                                    "id": tool_call_id,
                                    "type": "function",
                                    "function": {"name": call.name, "arguments": call.arguments},
                                }],
                            }),
                            "tool_calls",
                        ),
                        super::tool_envelope::ToolEnvelopeOutcome::Text(clean) => {
                            if !has_assistant_output(&clean) {
                                break Err(
                                    "the Devin CLI emitted an unparsable tool call envelope"
                                        .to_owned(),
                                );
                            }
                            (
                                json!({"role": "assistant", "content": clean}),
                                if stop_reason == "max_tokens" {
                                    "length"
                                } else {
                                    "stop"
                                },
                            )
                        }
                    }
                };
                return Ok(json!({
                    "id": completion_id,
                    "object": "chat.completion",
                    "created": created,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "message": message,
                        "finish_reason": finish_reason,
                    }],
                    "usage": {
                        "prompt_tokens": estimated_tokens(prompt_chars),
                        "completion_tokens": completion_tokens,
                        "total_tokens": estimated_tokens(prompt_chars) + completion_tokens,
                    },
                }));
            }
            Ok(None) => break Err("the Devin CLI exited before finishing the turn".to_owned()),
        }
    };
    let _ = session.forget_session(&session_id).await;
    result
}
