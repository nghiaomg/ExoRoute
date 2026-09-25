use super::*;

/// Handles one Chat Completions upstream frame.
pub(in crate::gateway::streaming) fn handle_chat_frame(
    frame: &StreamFrame,
    state: &mut TranslationState,
    config: &StreamTranslationConfig,
    output: &mut Vec<Bytes>,
) -> Result<(), StreamError> {
    shared::pre_pass(frame, state, config);
    if frame.event_name == "error" || frame.event_type() == "error" {
        return Err(StreamError::new(extract_stream_error(
            &frame.event_name,
            &frame.value,
        )));
    }
    if let Some(delta) = extract_chat_stream_tool_delta(&frame.value)
        && let Err(error) = state
            .chat_tool_calls
            .merge_delta(delta, config.resource_limits.sse_buffer_limit_bytes)
    {
        return Err(StreamError::new(error));
    }
    shared::ensure_messages_client_start(state, config, output);
    if let Some(reasoning) = extract_chat_stream_reasoning_delta(&frame.value) {
        state.saw_output = true;
        shared::ensure_plain_start(state, config, output);
        output.push(Bytes::from(encode_chat_stream_reasoning_delta(
            &state.upstream_id,
            &config.model,
            reasoning,
        )));
    }
    shared::text_branch(frame, state, config, output)?;
    if let Some(delta) = extract_chat_stream_tool_delta(&frame.value) {
        state.saw_output = true;
        if config.client_protocol == Protocol::ChatCompletions {
            shared::ensure_plain_start(state, config, output);
            output.push(Bytes::from(encode_chat_stream_tool_delta(
                &state.upstream_id,
                &config.model,
                delta,
            )));
        }
    }
    shared::completed_tail(state, config, output)?;
    Ok(())
}
