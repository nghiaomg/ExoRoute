use super::*;
use bytes::Bytes;
use serde_json::Value;

pub(in crate::gateway::streaming) fn translation_stream(
    upstream: UpstreamChunkStream,
    config: StreamTranslationConfig,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    let processing_slots = config
        .log
        .as_ref()
        .map(|log| log.sse_processing_slots.clone());
    let mut reader = SseFrameReader::new(
        upstream,
        config.idle_timeout,
        config.continuity_enabled,
        config.overall_timeout,
        config.resource_limits,
        config.shutdown.clone(),
        processing_slots,
    );
    async_stream::stream! {
        let mut config = config;
        let upstream_id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
        let mut state = TranslationState::new(upstream_id);
        let mut terminal = StreamTerminal::CleanEnd;
        'read_stream: loop {
            let (event_name, data, frame_permit) = match reader.next().await {
                SseReadEvent::KeepAlive => {
                    yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b": keep-alive\n\n"));
                    continue 'read_stream;
                }
                SseReadEvent::Shutdown => {
                    terminal = StreamTerminal::Shutdown;
                    break 'read_stream;
                }
                SseReadEvent::End => break 'read_stream,
                SseReadEvent::Error(error) => {
                    // A transport failure or idle timeout after the upstream
                    // already delivered its finish signal or terminal event
                    // means the content is fully sent and the provider merely
                    // dropped the connection before the final sentinel
                    // ([DONE] / message_stop). Treat it as end-of-stream so a
                    // completed response is not turned into an injected error
                    // that breaks the client's stream; genuine mid-stream
                    // failures keep failing below.
                    if state.completed || state.saw_finish_signal {
                        break 'read_stream;
                    }
                    yield Ok(Bytes::from(encode_stream_error(
                        config.client_protocol,
                        &error,
                    )));
                    terminal = StreamTerminal::Error(error);
                    break 'read_stream;
                }
                SseReadEvent::Frame(RawSseFrame {
                    event_name,
                    data,
                    processing_permit,
                }) => (event_name, data, processing_permit),
            };
            if data == "[DONE]" {
                drop(frame_permit);
                if config.upstream_protocol == UpstreamProtocol::ChatCompletions {
                    state.completed = true;
                    terminal = StreamTerminal::Completed;
                    break 'read_stream;
                }
                continue 'read_stream;
            }
            if data.is_empty() {
                drop(frame_permit);
                continue 'read_stream;
            }
            let Ok(value) = serde_json::from_str::<Value>(&data) else {
                drop(data);
                drop(frame_permit);
                let error = "upstream sent malformed SSE JSON".to_owned();
                yield Ok(Bytes::from(encode_stream_error(config.client_protocol, &error)));
                terminal = StreamTerminal::Error(error);
                break 'read_stream;
            };
            let frame = StreamFrame { event_name, value };
            let mut frame_output = Vec::<Bytes>::with_capacity(3);
            let result = match config.upstream_protocol {
                UpstreamProtocol::ChatCompletions => {
                    handle_chat_frame(&frame, &mut state, &config, &mut frame_output)
                }
                UpstreamProtocol::Responses => {
                    handle_responses_frame(&frame, &mut state, &config, &mut frame_output)
                }
                UpstreamProtocol::Messages => {
                    handle_messages_frame(&frame, &mut state, &config, &mut frame_output)
                }
                UpstreamProtocol::GoogleGenerateContent => {
                    handle_google_frame(&frame, &mut state, &config, &mut frame_output)
                }
            };
            drop(frame);
            drop(data);
            drop(frame_permit);
            if let Err(error) = result {
                yield Ok(Bytes::from(encode_stream_error(
                    config.client_protocol,
                    &error.message,
                )));
                terminal = StreamTerminal::Error(error.message);
                break 'read_stream;
            }
            for output in frame_output {
                yield Ok(output);
            }
            if state.completed {
                terminal = StreamTerminal::Completed;
                break 'read_stream;
            }
        }

        if terminal == StreamTerminal::Shutdown {
            return;
        }
        // Some providers close the connection right after the finish signal
        // (chat finish_reason or Anthropic stop_reason) and omit the terminal
        // sentinel event ([DONE] / message_stop). The output is already fully
        // delivered at that point, so treat the finish signal as a clean
        // completion instead of reporting a truncated stream.
        if !state.completed && state.saw_finish_signal {
            state.completed = true;
            if config.upstream_protocol == UpstreamProtocol::Messages
                && config.client_protocol == Protocol::Responses
            {
                state.response_output =
                    Some(messages_response_output(&state.messages_blocks));
            }
        }
        if !state.completed
            && config.upstream_protocol == UpstreamProtocol::ChatCompletions
            && !state.chat_tool_calls.is_empty()
        {
            state.completed = true;
            if state.finish_reason == "stop" {
                state.finish_reason = "tool_calls".to_owned();
            }
        }
        let final_error = match &terminal {
            StreamTerminal::Error(message) => Some(message.clone()),
            _ if !state.completed => Some(
                "upstream stream ended before a terminal success event".to_owned(),
            ),
            _ if !state.saw_output => {
                Some("upstream completed the response without content".to_owned())
            }
            _ => None,
        };
        if let Some(error) = final_error {
            config.outcome.fail();
            for event in fail_events(&config, &error) {
                yield Ok(event);
            }
            if let Some(log) = config.log.take() {
                log_failed(log, &state, &error).await;
            }
        } else {
            config.outcome.complete();
            for event in complete_events(&config, &mut state) {
                yield Ok(event);
            }
            if let Some(log) = config.log.take() {
                log_completed(log, &state).await;
            }
        }
    }
}
