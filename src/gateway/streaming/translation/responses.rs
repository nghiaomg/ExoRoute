use super::*;
use bytes::Bytes;

/// Handles one OpenAI Responses upstream frame.
pub(in crate::gateway::streaming) fn handle_responses_frame(
    frame: &StreamFrame,
    state: &mut TranslationState,
    config: &StreamTranslationConfig,
    output: &mut Vec<Bytes>,
) -> Result<(), StreamError> {
    shared::pre_pass(frame, state, config);
    if let Err(error) = state.responses_accumulator.observe(
        frame.response_event_type(),
        &frame.value,
        config.resource_limits.sse_buffer_limit_bytes,
    ) {
        return Err(StreamError::new(error));
    }
    let responses_terminal = responses_terminal_state(&frame.event_name, &frame.value);
    if responses_terminal == Some(ResponsesTerminal::Failed) {
        return Err(StreamError::new(extract_stream_error(
            &frame.event_name,
            &frame.value,
        )));
    }
    if let Some(terminal) = responses_terminal {
        state.completed = true;
        if terminal == ResponsesTerminal::Incomplete {
            state.finish_reason = "length".to_owned();
        }
    }
    shared::ensure_messages_client_start(state, config, output);
    if let Some(encoded) = encode_responses_image_event(
        config.upstream_protocol,
        config.client_protocol,
        &frame.event_name,
        &frame.value,
    ) {
        state.saw_output = true;
        shared::ensure_plain_start(state, config, output);
        output.push(Bytes::from(encoded));
    }
    if let Some(encoded) = encode_responses_tool_event(
        config.upstream_protocol,
        config.client_protocol,
        &frame.event_name,
        &frame.value,
    ) {
        state.saw_output = true;
        state.forwarded_response_tool_events = true;
        shared::ensure_plain_start(state, config, output);
        output.push(Bytes::from(encoded));
    }
    shared::text_branch(frame, state, config, output)?;
    if responses_terminal.is_some() {
        emit_terminal_response_events(frame, state, config, output)?;
    }
    shared::completed_tail(state, config, output)?;
    Ok(())
}

/// Reconstructs the terminal response's output, records it on the state,
/// and forwards merged tool call events to non-Responses clients.
fn emit_terminal_response_events(
    frame: &StreamFrame,
    state: &mut TranslationState,
    config: &StreamTranslationConfig,
    output: &mut Vec<Bytes>,
) -> Result<(), StreamError> {
    let terminal_response = frame.value.get("response").unwrap_or(&frame.value);
    let reconstructed = state
        .responses_accumulator
        .reconstruct_response(terminal_response);
    let response = reconstructed.as_ref().unwrap_or(terminal_response);
    if crate::protocol::decode_upstream_response(
        UpstreamProtocol::Responses,
        response,
        &config.model,
    )
    .is_ok()
    {
        state.saw_output = true;
    }
    let Some(output_items) = response.get("output").and_then(Value::as_array).cloned() else {
        return Ok(());
    };
    state.response_output = Some(output_items.clone());
    let response_tool_calls =
        extract_responses_tool_calls(&output_items).map_err(StreamError::new)?;
    if response_tool_calls.is_empty() {
        return Ok(());
    }
    state.finish_reason = "tool_calls".to_owned();
    if config.client_protocol == Protocol::Responses && state.forwarded_response_tool_events {
        return Ok(());
    }
    let has_response_text = output_items.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("message")
            && item
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part.get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|text| !text.trim().is_empty())
                    })
                })
    });
    let (tool_events, _tool_output) = encode_google_tool_call_events(
        config.client_protocol,
        &response_tool_calls,
        &state.upstream_id,
        &config.model,
        has_response_text,
        config.client_protocol == Protocol::Messages && state.messages_text_started,
    )
    .map_err(StreamError::new)?;
    if config.client_protocol != Protocol::Messages && !tool_events.is_empty() {
        shared::ensure_plain_start(state, config, output);
    }
    if !tool_events.is_empty() {
        state.saw_output = true;
    }
    if config.client_protocol == Protocol::Messages {
        let first_tool_index = usize::from(state.messages_text_started);
        for index in 0..response_tool_calls.len() {
            let output_index = first_tool_index.saturating_add(index);
            if !state.messages_open_blocks.contains(&output_index) {
                state.messages_open_blocks.push(output_index);
            }
        }
    }
    output.extend(tool_events);
    Ok(())
}
