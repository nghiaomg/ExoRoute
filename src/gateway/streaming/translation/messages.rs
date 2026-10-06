use super::*;
use bytes::Bytes;

/// Handles one Anthropic Messages upstream frame.
pub(in crate::gateway::streaming) fn handle_messages_frame(
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
    if frame.event_name == "message_stop" || frame.event_type() == "message_stop" {
        state.completed = true;
    }
    shared::ensure_messages_client_start(state, config, output);
    let message_event_type = frame
        .value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or(frame.event_name.as_str());
    let message_result = match message_event_type {
        "content_block_start" => encode_messages_stream_block_start(
            config.client_protocol,
            &frame.value,
            &mut state.messages_blocks,
            &mut state.messages_next_output_index,
            &mut state.messages_open_blocks,
            &state.upstream_id,
            &config.model,
        ),
        "content_block_delta" => encode_messages_stream_delta(
            &frame.value,
            &mut MessagesStreamDeltaContext {
                client_protocol: config.client_protocol,
                blocks: &mut state.messages_blocks,
                next_output_index: &mut state.messages_next_output_index,
                open_message_blocks: &mut state.messages_open_blocks,
                response_id: &state.upstream_id,
                model: &config.model,
                response_limit: config.resource_limits.sse_buffer_limit_bytes,
            },
        ),
        "content_block_stop" => Ok((
            encode_messages_stream_block_stop(
                config.client_protocol,
                &frame.value,
                &mut state.messages_blocks,
                &mut state.messages_open_blocks,
                &state.upstream_id,
                &config.model,
            ),
            false,
        )),
        _ => Ok((Vec::new(), false)),
    };
    let (message_events, message_output) = message_result.map_err(StreamError::new)?;
    if message_output {
        state.saw_output = true;
    }
    if (message_output || !message_events.is_empty())
        && config.client_protocol != Protocol::Messages
    {
        shared::ensure_plain_start(state, config, output);
    }
    output.extend(message_events);
    shared::text_branch(frame, state, config, output)?;
    shared::completed_tail(state, config, output)?;
    Ok(())
}
