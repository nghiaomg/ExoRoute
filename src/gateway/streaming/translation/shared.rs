use super::*;
use bytes::Bytes;

/// Protocol-agnostic pre-pass: usage/cost extraction and finish-signal
/// detection. Emits no bytes.
pub(in crate::gateway::streaming) fn pre_pass(
    frame: &StreamFrame,
    state: &mut TranslationState,
    config: &StreamTranslationConfig,
) {
    let (input, output) =
        extract_stream_usage(config.upstream_protocol, &frame.event_name, &frame.value);
    let (cached, cache_input) =
        extract_stream_cache_usage(config.upstream_protocol, &frame.event_name, &frame.value);
    if let Some(cost) =
        extract_stream_cost(config.upstream_protocol, &frame.event_name, &frame.value)
    {
        state.cost_micro_usd = Some(cost);
    }
    state.note_usage(&config.analytics, input, output, cached, cache_input);
    if let Some(reason) =
        extract_stream_finish(config.upstream_protocol, &frame.event_name, &frame.value)
    {
        state.finish_reason = reason;
        state.saw_finish_signal = true;
    }
}

/// Emits the Messages-client `message_start` for any upstream protocol.
pub(in crate::gateway::streaming) fn ensure_messages_client_start(
    state: &mut TranslationState,
    config: &StreamTranslationConfig,
    output: &mut Vec<Bytes>,
) {
    if state.sent_start || config.client_protocol != Protocol::Messages {
        return;
    }
    let start = if config.upstream_protocol == UpstreamProtocol::GoogleGenerateContent {
        google_messages_stream_start(&state.upstream_id, &config.model, state.input_tokens)
    } else {
        messages_stream_start(&state.upstream_id, &config.model, state.input_tokens)
    };
    output.push(Bytes::from(start));
    state.sent_start = true;
}

/// Emits the plain protocol start event on first output.
pub(in crate::gateway::streaming) fn ensure_plain_start(
    state: &mut TranslationState,
    config: &StreamTranslationConfig,
    output: &mut Vec<Bytes>,
) {
    if state.sent_start {
        return;
    }
    let start = encode_stream_start(config.client_protocol, &state.upstream_id, &config.model);
    if !start.is_empty() {
        output.push(Bytes::from(start));
        state.sent_start = true;
    }
}

/// Extracts and re-encodes streamed text for every non-Messages upstream,
/// including the Google-specific Responses/Messages buffering.
pub(in crate::gateway::streaming) fn text_branch(
    frame: &StreamFrame,
    state: &mut TranslationState,
    config: &StreamTranslationConfig,
    output: &mut Vec<Bytes>,
) -> Result<(), StreamError> {
    if config.upstream_protocol == UpstreamProtocol::Messages {
        return Ok(());
    }
    let Some(text) = extract_stream_text(config.upstream_protocol, &frame.event_name, &frame.value)
    else {
        return Ok(());
    };
    if !text.trim().is_empty() {
        state.saw_output = true;
        state.chat_stream_text_sent = true;
    }
    if config.upstream_protocol == UpstreamProtocol::GoogleGenerateContent
        && config.client_protocol == Protocol::Responses
    {
        if config.resource_limits.sse_buffer_limit_bytes != 0
            && state.google_responses_text.len().saturating_add(text.len())
                > config.resource_limits.sse_buffer_limit_bytes
        {
            return Err(StreamError::new(
                "Google streamed text exceeded the configured response limit",
            ));
        }
        state.google_responses_text.push_str(&text);
    }
    if config.client_protocol != Protocol::Messages {
        ensure_plain_start(state, config, output);
    }
    let encoded = if config.upstream_protocol == UpstreamProtocol::GoogleGenerateContent
        && config.client_protocol == Protocol::Messages
    {
        if !state.google_message_text_started {
            output.push(Bytes::from(google_messages_text_start(0)));
            state.google_message_text_started = true;
        }
        google_messages_stream_text(0, &text)
    } else {
        encode_stream_text(
            config.client_protocol,
            &state.upstream_id,
            &config.model,
            &text,
        )
    };
    if config.client_protocol == Protocol::Messages
        && config.upstream_protocol != UpstreamProtocol::GoogleGenerateContent
        && !state.messages_text_started
    {
        output.push(Bytes::from(messages_text_start(0)));
        state.messages_text_started = true;
        state.messages_open_blocks.push(0);
    }
    if !encoded.is_empty() {
        output.push(Bytes::from(encoded));
    }
    Ok(())
}

/// Per-frame work that runs once the frame completed the upstream stream:
/// Messages→Responses output reconstruction and Google tool-call events.
pub(in crate::gateway::streaming) fn completed_tail(
    state: &mut TranslationState,
    config: &StreamTranslationConfig,
    output: &mut Vec<Bytes>,
) -> Result<(), StreamError> {
    if !state.completed {
        return Ok(());
    }
    if config.upstream_protocol == UpstreamProtocol::Messages
        && config.client_protocol == Protocol::Responses
    {
        state.response_output = Some(messages_response_output(&state.messages_blocks));
    }
    if config.upstream_protocol == UpstreamProtocol::GoogleGenerateContent {
        let (tool_events, tool_output) = encode_google_tool_call_events(
            config.client_protocol,
            &state.google_tool_calls,
            &state.upstream_id,
            &config.model,
            !state.google_responses_text.is_empty(),
            state.google_message_text_started,
        )
        .map_err(StreamError::new)?;
        if config.client_protocol != Protocol::Messages && !tool_events.is_empty() {
            ensure_plain_start(state, config, output);
        }
        if !tool_events.is_empty() {
            state.saw_output = true;
        }
        output.extend(tool_events);
        if config.client_protocol == Protocol::Responses {
            let mut items = Vec::with_capacity(tool_output.len() + 1);
            if !state.google_responses_text.is_empty() {
                items.push(json!({
                    "id":"msg_0",
                    "type":"message",
                    "role":"assistant",
                    "content":[{"type":"output_text","text":state.google_responses_text,"annotations":[]}]
                }));
            }
            items.extend(tool_output);
            state.response_output = Some(items);
        }
    }
    Ok(())
}
