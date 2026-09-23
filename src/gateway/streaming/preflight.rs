use super::*;
use crate::provider_adapters;

pub(crate) enum PreflightFrame {
    Continue,
    Ready,
    Retryable(String),
}

pub(crate) fn has_messages_stream_output(event: &str, value: &Value) -> bool {
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or(event);
    match event_type {
        "content_block_start" => {
            let block = value.get("content_block");
            match block
                .and_then(|block| block.get("type"))
                .and_then(Value::as_str)
            {
                Some("thinking")
                | Some("redacted_thinking")
                | Some("tool_use")
                | Some("server_tool_use") => true,
                Some("text") => block
                    .and_then(|block| block.get("text"))
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty()),
                _ => false,
            }
        }
        "content_block_delta" => {
            let delta = value.get("delta");
            match delta
                .and_then(|delta| delta.get("type"))
                .and_then(Value::as_str)
            {
                Some("text_delta") => delta
                    .and_then(|delta| delta.get("text"))
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty()),
                Some("thinking_delta") => delta
                    .and_then(|delta| delta.get("thinking"))
                    .and_then(Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty()),
                Some("input_json_delta") => delta
                    .and_then(|delta| delta.get("partial_json"))
                    .and_then(Value::as_str)
                    .is_some_and(|json| !json.is_empty()),
                Some("signature_delta") => delta
                    .and_then(|delta| delta.get("signature"))
                    .and_then(Value::as_str)
                    .is_some_and(|signature| !signature.is_empty()),
                _ => false,
            }
        }
        _ => false,
    }
}

pub(crate) fn preflight_frame(
    upstream_protocol: UpstreamProtocol,
    client_protocol: Protocol,
    model: &str,
    frame: &str,
    responses_accumulator: &mut ResponsesStreamAccumulator,
    max_response_bytes: usize,
) -> Result<PreflightFrame, String> {
    let mut event_name = String::new();
    let mut data = String::new();
    let mut saw_data = false;
    for line in frame.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_name = value.trim().to_owned();
        }
        if let Some(value) = line.strip_prefix("data:") {
            if saw_data {
                data.push('\n');
            }
            data.push_str(value.trim_start());
            saw_data = true;
        }
    }
    if data == "[DONE]" {
        return if upstream_protocol == UpstreamProtocol::ChatCompletions {
            Ok(PreflightFrame::Retryable(
                "upstream completed the response without content".to_owned(),
            ))
        } else {
            Ok(PreflightFrame::Continue)
        };
    }
    if data.is_empty() {
        return Ok(PreflightFrame::Continue);
    }
    let value = serde_json::from_str::<Value>(&data)
        .map_err(|_| "upstream sent malformed SSE JSON before output".to_owned())?;
    let event_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let response_event_type = if event_type.is_empty() {
        event_name.as_str()
    } else {
        event_type
    };
    if upstream_protocol == UpstreamProtocol::Responses {
        responses_accumulator.observe(response_event_type, &value, max_response_bytes)?;
    }
    let responses_terminal = (upstream_protocol == UpstreamProtocol::Responses)
        .then(|| responses_terminal_state(&event_name, &value))
        .flatten();
    let provider_failed = matches!(responses_terminal, Some(ResponsesTerminal::Failed))
        || (upstream_protocol != UpstreamProtocol::Responses
            && (event_name == "error" || event_type == "error"));
    if provider_failed {
        return Ok(PreflightFrame::Retryable(extract_stream_error(
            &event_name,
            &value,
        )));
    }

    let google_update = if upstream_protocol == UpstreamProtocol::GoogleGenerateContent {
        Some(parse_google_stream_update(&value).map_err(|error| error.to_owned())?)
    } else {
        None
    };
    let reconstructed_response = responses_terminal.and_then(|_| {
        responses_accumulator.reconstruct_response(value.get("response").unwrap_or(&value))
    });
    let terminal_response = reconstructed_response
        .as_ref()
        .or_else(|| responses_terminal.map(|_| value.get("response").unwrap_or(&value)));
    if let Some(response) = terminal_response
        && let Some(output) = response.get("output").and_then(Value::as_array)
        && let Err(error) = extract_responses_tool_calls(output)
    {
        return Ok(PreflightFrame::Retryable(error));
    }
    let has_output = extract_stream_text(upstream_protocol, &event_name, &value)
        .is_some_and(|text| !text.trim().is_empty())
        || (upstream_protocol == UpstreamProtocol::Messages
            && has_messages_stream_output(&event_name, &value))
        || encode_responses_image_event(upstream_protocol, client_protocol, &event_name, &value)
            .is_some()
        || (upstream_protocol == UpstreamProtocol::ChatCompletions
            && extract_chat_stream_tool_delta(&value).is_some())
        || (upstream_protocol == UpstreamProtocol::ChatCompletions
            && extract_chat_stream_reasoning_delta(&value)
                .is_some_and(|text| !text.trim().is_empty()))
        || (upstream_protocol == UpstreamProtocol::GoogleGenerateContent
            && google_update
                .as_ref()
                .is_some_and(|update| !update.tool_calls.is_empty()))
        || terminal_response.is_some_and(|response| {
            crate::protocol::decode_upstream_response(UpstreamProtocol::Responses, response, model)
                .is_ok()
        });
    if has_output {
        return Ok(PreflightFrame::Ready);
    }

    // A finish signal without a terminal sentinel event still tells us the
    // provider finished deliberately: retrying the same request would burn
    // tokens to produce the same empty output again.
    let completed = (upstream_protocol == UpstreamProtocol::Messages
        && (event_name == "message_stop"
            || event_type == "message_stop"
            || extract_stream_finish(upstream_protocol, &event_name, &value).is_some()))
        || (upstream_protocol == UpstreamProtocol::ChatCompletions
            && extract_stream_finish(upstream_protocol, &event_name, &value).is_some())
        || responses_terminal.is_some()
        || google_update
            .as_ref()
            .is_some_and(|update| update.finish_reason.is_some());
    if completed {
        return Ok(PreflightFrame::Retryable(
            "upstream completed the response without content".to_owned(),
        ));
    }
    Ok(PreflightFrame::Continue)
}

pub(crate) fn find_sse_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2))
        .or_else(|| {
            buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|index| (index, 4))
        })
}

/// Consume only enough of an upstream SSE response to establish that it has
/// meaningful output. This keeps protocol preambles out of the client until
/// the attempt is safe to expose; callers can retry a terminal empty/error
/// stream before returning an HTTP 200 response to the client.
pub(crate) async fn preflight_stream(
    upstream: reqwest::Response,
    upstream_protocol: UpstreamProtocol,
    client_protocol: Protocol,
    model: &str,
    idle_timeout: Duration,
    resource_limits: GatewayResourceLimits,
) -> Result<UpstreamChunkStream, String> {
    let mut chunks = upstream.bytes_stream();
    let mut prefix = Vec::<Bytes>::new();
    let mut prefix_bytes = 0usize;
    let mut buffer = Vec::<u8>::new();
    let mut stream_ended = false;
    let mut responses_accumulator = ResponsesStreamAccumulator::default();

    loop {
        if !stream_ended {
            match tokio::time::timeout(idle_timeout, chunks.next()).await {
                Ok(Some(Ok(chunk))) => {
                    // A reqwest body chunk is not an SSE frame. A provider can
                    // coalesce a small preamble and a meaningful event with a
                    // much larger tail in the same chunk. Consume that chunk
                    // incrementally so the limit applies only until the first
                    // semantic output, rather than rejecting the entire chunk
                    // before its first event can be inspected.
                    let mut offset = 0usize;
                    loop {
                        while let Some((boundary, delimiter_len)) = find_sse_boundary(&buffer) {
                            if resource_limits.sse_frame_limit_bytes != 0
                                && boundary.saturating_add(delimiter_len)
                                    > resource_limits.sse_frame_limit_bytes
                            {
                                return Err(
                                    "upstream SSE frame exceeded the configured limit".to_owned()
                                );
                            }
                            let frame_bytes =
                                buffer.drain(..boundary + delimiter_len).collect::<Vec<_>>();
                            let frame = String::from_utf8(frame_bytes).map_err(|_| {
                                "upstream sent invalid UTF-8 SSE data before output".to_owned()
                            })?;
                            match preflight_frame(
                                upstream_protocol,
                                client_protocol,
                                model,
                                &frame,
                                &mut responses_accumulator,
                                resource_limits.sse_buffer_limit_bytes,
                            )? {
                                PreflightFrame::Ready => {
                                    if offset < chunk.len() {
                                        prefix.push(chunk.slice(offset..));
                                    }
                                    let prefix_stream = futures_util::stream::iter(
                                        prefix.into_iter().map(Ok::<Bytes, reqwest::Error>),
                                    );
                                    return Ok(Box::pin(prefix_stream.chain(chunks)));
                                }
                                PreflightFrame::Continue => {}
                                PreflightFrame::Retryable(error) => return Err(error),
                            }
                        }

                        if offset == chunk.len() {
                            break;
                        }
                        let remaining = if resource_limits.sse_buffer_limit_bytes == 0 {
                            chunk.len().saturating_sub(offset)
                        } else {
                            resource_limits
                                .sse_buffer_limit_bytes
                                .saturating_sub(prefix_bytes)
                        };
                        if resource_limits.sse_buffer_limit_bytes != 0 && remaining == 0 {
                            return Err("upstream stream preflight exceeded the configured limit"
                                .to_owned());
                        }
                        let take = remaining.min(chunk.len().saturating_sub(offset));
                        let segment = chunk.slice(offset..offset + take);
                        buffer.extend_from_slice(&segment);
                        prefix_bytes = prefix_bytes.checked_add(take).ok_or_else(|| {
                            "upstream stream preflight exceeded the configured limit".to_owned()
                        })?;
                        prefix.push(segment);
                        offset += take;
                    }
                }
                Ok(Some(Err(error))) => {
                    return Err(format!(
                        "upstream stream read failed before output: {}",
                        provider_adapters::upstream_transport_error_message(&error)
                    ));
                }
                Ok(None) => {
                    stream_ended = true;
                    if buffer.is_empty() {
                        return Err(
                            "upstream stream ended before a terminal success event".to_owned()
                        );
                    }
                    if resource_limits.sse_buffer_limit_bytes != 0
                        && buffer.len().saturating_add(2) > resource_limits.sse_buffer_limit_bytes
                    {
                        return Err("upstream SSE buffer exceeded the configured limit".to_owned());
                    }
                    // Match stream_translation's support for a final frame
                    // that omits the blank-line delimiter.
                    buffer.extend_from_slice(b"\n\n");
                }
                Err(_) => return Err("upstream stream idle timeout before output".to_owned()),
            }
        }

        while let Some((boundary, delimiter_len)) = find_sse_boundary(&buffer) {
            if resource_limits.sse_frame_limit_bytes != 0
                && boundary.saturating_add(delimiter_len) > resource_limits.sse_frame_limit_bytes
            {
                return Err("upstream SSE frame exceeded the configured limit".to_owned());
            }
            let frame_bytes = buffer.drain(..boundary + delimiter_len).collect::<Vec<_>>();
            let frame = String::from_utf8(frame_bytes)
                .map_err(|_| "upstream sent invalid UTF-8 SSE data before output".to_owned())?;
            match preflight_frame(
                upstream_protocol,
                client_protocol,
                model,
                &frame,
                &mut responses_accumulator,
                resource_limits.sse_buffer_limit_bytes,
            )? {
                PreflightFrame::Ready => {
                    let prefix_stream = futures_util::stream::iter(
                        prefix.into_iter().map(Ok::<Bytes, reqwest::Error>),
                    );
                    return Ok(Box::pin(prefix_stream.chain(chunks)));
                }
                PreflightFrame::Continue => {}
                PreflightFrame::Retryable(error) => return Err(error),
            }
        }

        if stream_ended {
            return Err("upstream stream ended before a terminal success event".to_owned());
        }
    }
}
