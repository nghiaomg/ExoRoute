use super::*;
use bytes::Bytes;

/// Handles one Google GenerateContent upstream frame.
pub(in crate::gateway::streaming) fn handle_google_frame(
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
    let update = parse_google_stream_update(&frame.value).map_err(StreamError::new)?;
    if let Some(reason) = update.finish_reason.as_deref() {
        match normalize_google_stream_finish(
            reason,
            !state.google_tool_calls.is_empty() || !update.tool_calls.is_empty(),
        ) {
            Ok(reason) => {
                state.finish_reason = reason;
                state.completed = true;
            }
            Err(error) => return Err(StreamError::new(error)),
        }
    }
    merge_google_tool_calls(
        &mut state.google_tool_calls,
        &update.tool_calls,
        config.resource_limits.sse_buffer_limit_bytes,
    )
    .map_err(StreamError::new)?;
    shared::ensure_messages_client_start(state, config, output);
    shared::text_branch(frame, state, config, output)?;
    shared::completed_tail(state, config, output)?;
    Ok(())
}
