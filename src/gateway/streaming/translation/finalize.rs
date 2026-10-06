use super::*;
use bytes::Bytes;

/// Client-visible bytes for a failed stream: the translated error event
/// plus, for Chat Completions clients, the terminal sentinel so parsers
/// end on a deliberate terminator instead of a dropped connection.
pub(in crate::gateway::streaming) fn fail_events(
    config: &StreamTranslationConfig,
    error: &str,
) -> Vec<Bytes> {
    let mut events = vec![Bytes::from(encode_stream_error(
        config.client_protocol,
        error,
    ))];
    if config.client_protocol == Protocol::ChatCompletions {
        events.push(Bytes::from_static(b"data: [DONE]\n\n"));
    }
    events
}

/// Client-visible bytes for a completed stream, including cross-protocol
/// tool-call delivery and the protocol's terminal framing.
pub(in crate::gateway::streaming) fn complete_events(
    config: &StreamTranslationConfig,
    state: &mut TranslationState,
) -> Vec<Bytes> {
    let mut events = Vec::new();
    let client_protocol = config.client_protocol;
    let upstream_protocol = config.upstream_protocol;
    if !state.sent_start {
        let start = if client_protocol == Protocol::Messages {
            if upstream_protocol == UpstreamProtocol::GoogleGenerateContent {
                google_messages_stream_start(&state.upstream_id, &config.model, state.input_tokens)
            } else {
                messages_stream_start(&state.upstream_id, &config.model, state.input_tokens)
            }
        } else {
            encode_stream_start(client_protocol, &state.upstream_id, &config.model)
        };
        if !start.is_empty() {
            events.push(Bytes::from(start));
        }
        state.sent_start = true;
    }
    let usage = state
        .has_usage
        .then_some((state.input_tokens, state.output_tokens));
    if upstream_protocol == UpstreamProtocol::ChatCompletions
        && client_protocol != Protocol::ChatCompletions
    {
        if state.finish_reason == "stop" {
            state.finish_reason = "tool_calls".to_owned();
        }
        if client_protocol == Protocol::Messages {
            let first_tool_index = usize::from(state.messages_text_started);
            let tool_count = state.chat_tool_calls.call_count();
            for offset in 0..tool_count {
                let output_index = first_tool_index.saturating_add(offset);
                if !state.messages_open_blocks.contains(&output_index) {
                    state.messages_open_blocks.push(output_index);
                }
            }
        }
        if client_protocol == Protocol::Responses
            && state.response_output.is_none()
            && !state.saw_output
        {
            state.response_output = Some(Vec::new());
        }
        for call in state.chat_tool_calls.drain() {
            let (tool_events, tool_output) = match encode_google_tool_call_events(
                client_protocol,
                std::slice::from_ref(&call),
                &state.upstream_id,
                &config.model,
                state.chat_stream_text_sent,
                state.chat_stream_text_sent,
            ) {
                Ok(output) => output,
                Err(error) => {
                    events.push(Bytes::from(encode_stream_error(client_protocol, &error)));
                    break;
                }
            };
            if !tool_output.is_empty() {
                match state.response_output.as_mut() {
                    Some(output) => output.extend(tool_output),
                    None => state.response_output = Some(tool_output),
                }
            }
            events.extend(tool_events);
        }
    }
    let finish = if upstream_protocol == UpstreamProtocol::GoogleGenerateContent
        && client_protocol == Protocol::Messages
    {
        google_messages_stream_finish(
            &state.finish_reason,
            state.input_tokens,
            state.output_tokens,
            state.cost_micro_usd,
            state.google_message_text_started,
        )
    } else if client_protocol == Protocol::Messages {
        messages_stream_finish(
            &state.finish_reason,
            state.input_tokens,
            state.output_tokens,
            state.cost_micro_usd,
            &state.messages_open_blocks,
        )
    } else {
        encode_stream_finish(
            client_protocol,
            &state.upstream_id,
            &config.model,
            &state.finish_reason,
            usage,
            state.cost_micro_usd,
            state.response_output.as_deref(),
        )
    };
    events.push(Bytes::from(finish));
    if client_protocol == Protocol::ChatCompletions {
        if usage.is_some() {
            events.push(Bytes::from(encode_stream_usage(
                state.input_tokens,
                state.output_tokens,
                state.cost_micro_usd,
            )));
        }
        events.push(Bytes::from_static(b"data: [DONE]\n\n"));
    }
    events
}

/// Records the failed-stream log entry with usage captured mid-stream.
pub(in crate::gateway::streaming) async fn log_failed(
    log: StreamLog,
    state: &TranslationState,
    error: &str,
) {
    log_failed_stream(
        log,
        error,
        state
            .has_usage
            .then_some(state.input_tokens.min(i64::MAX as u64) as i64),
        state
            .has_usage
            .then_some(state.output_tokens.min(i64::MAX as u64) as i64),
        (state.cache_input_tokens > 0 || state.cached_tokens > 0)
            .then_some(state.cached_tokens.min(i64::MAX as u64) as i64),
        (state.cache_input_tokens > 0 || state.cached_tokens > 0)
            .then_some(state.cache_input_tokens.min(i64::MAX as u64) as i64),
    )
    .await;
}

/// Records the completed-stream log entry with usage captured mid-stream.
pub(in crate::gateway::streaming) async fn log_completed(log: StreamLog, state: &TranslationState) {
    log_completed_stream(
        log,
        state
            .has_usage
            .then_some(state.input_tokens.min(i64::MAX as u64) as i64),
        state
            .has_usage
            .then_some(state.output_tokens.min(i64::MAX as u64) as i64),
        (state.cache_input_tokens > 0 || state.cached_tokens > 0)
            .then_some(state.cached_tokens.min(i64::MAX as u64) as i64),
        (state.cache_input_tokens > 0 || state.cached_tokens > 0)
            .then_some(state.cache_input_tokens.min(i64::MAX as u64) as i64),
        state.cost_micro_usd,
    )
    .await;
}
