use super::*;
use bytes::Bytes;
use serde_json::{Value, json};

pub(super) fn translation_stream(
    upstream: UpstreamChunkStream,
    config: StreamTranslationConfig,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    let StreamTranslationConfig {
        upstream_protocol,
        client_protocol,
        request_id: _,
        model,
        idle_timeout,
        continuity_enabled,
        overall_timeout,
        outcome,
        analytics,
        resource_limits,
        shutdown,
        log,
    } = config;
    let upstream_id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
    let processing_slots = log.as_ref().map(|log| log.sse_processing_slots.clone());
    let mut reader = SseFrameReader::new(
        upstream,
        idle_timeout,
        continuity_enabled,
        overall_timeout,
        resource_limits,
        shutdown,
        processing_slots,
    );
    async_stream::stream! {
        let mut sent_start = false;
        let mut completed = false;
        let mut saw_finish_signal = false;
        let mut saw_output = false;
        let mut stream_error = None::<String>;
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        let mut cached_tokens = 0u64;
        let mut cache_input_tokens = 0u64;
        let mut cost_micro_usd = None::<i64>;
        let mut has_usage = false;
        let mut finish_reason = "stop".to_owned();
        let mut response_output: Option<Vec<Value>> = None;
        let mut responses_accumulator = ResponsesStreamAccumulator::default();
        let mut forwarded_response_tool_events = false;
        let mut google_tool_calls = Vec::<GoogleStreamToolCall>::new();
        let mut google_message_text_started = false;
        let mut messages_blocks = Vec::<MessagesStreamBlock>::new();
        let mut messages_next_output_index = 0usize;
        let mut messages_open_blocks = Vec::<usize>::new();
        let mut messages_text_started = false;
        let mut chat_stream_text_sent = false;
        let mut google_responses_text = String::new();
        let mut chat_tool_calls = ChatStreamToolCallAccumulator::default();
        let mut shutting_down = false;
        'read_stream: loop {
            let (event_name, data, frame_permit) = match reader.next().await {
                SseReadEvent::KeepAlive => {
                    yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b": keep-alive\n\n"));
                    continue 'read_stream;
                }
                SseReadEvent::Shutdown => {
                    shutting_down = true;
                    break 'read_stream;
                }
                SseReadEvent::End => break 'read_stream,
                SseReadEvent::Error(error) => {
                    stream_error = Some(error);
                    yield Ok(Bytes::from(encode_stream_error(
                        client_protocol,
                        stream_error.as_deref().unwrap_or("upstream stream failed"),
                    )));
                    break 'read_stream;
                }
                SseReadEvent::Frame(RawSseFrame {
                    event_name,
                    data,
                    processing_permit,
                }) => (event_name, data, processing_permit),
            };
                if data == "[DONE]" {
                    if upstream_protocol == UpstreamProtocol::ChatCompletions {
                        completed = true;
                        drop(frame_permit);
                        break 'read_stream;
                    }
                    drop(frame_permit);
                    continue;
                }
                if data.is_empty() {
                    drop(frame_permit);
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(&data) else {
                    stream_error = Some("upstream sent malformed SSE JSON".to_owned());
                    drop(event_name);
                    drop(data);
                    drop(frame_permit);
                    yield Ok(Bytes::from(encode_stream_error(
                        client_protocol,
                        stream_error.as_deref().unwrap_or("upstream stream failed"),
                    )));
                    break 'read_stream;
                };
                let event_type = value.get("type").and_then(Value::as_str).unwrap_or_default();
                let response_event_type = if event_type.is_empty() {
                    event_name.as_str()
                } else {
                    event_type
                };
                if upstream_protocol == UpstreamProtocol::Responses
                    && let Err(error) = responses_accumulator.observe(
                        response_event_type,
                        &value,
                        resource_limits.sse_buffer_limit_bytes,
                    )
                {
                    stream_error = Some(error);
                    drop(value);
                    drop(event_name);
                    drop(data);
                    drop(frame_permit);
                    yield Ok(Bytes::from(encode_stream_error(
                        client_protocol,
                        stream_error.as_deref().unwrap_or("upstream stream failed"),
                    )));
                    break 'read_stream;
                }
                let responses_terminal = (upstream_protocol == UpstreamProtocol::Responses)
                    .then(|| responses_terminal_state(&event_name, &value))
                    .flatten();
                let provider_failed = matches!(responses_terminal, Some(ResponsesTerminal::Failed))
                    || (upstream_protocol != UpstreamProtocol::Responses
                        && (event_name == "error" || event_type == "error"));
                if provider_failed {
                    stream_error = Some(extract_stream_error(&event_name, &value));
                    drop(value);
                    drop(event_name);
                    drop(data);
                    drop(frame_permit);
                    yield Ok(Bytes::from(encode_stream_error(
                        client_protocol,
                        stream_error.as_deref().unwrap_or("upstream stream failed"),
                    )));
                    break 'read_stream;
                }
                let google_update = if upstream_protocol == UpstreamProtocol::GoogleGenerateContent {
                    match parse_google_stream_update(&value) {
                        Ok(update) => Some(update),
                        Err(error) => {
                            stream_error = Some(error);
                            drop(value);
                            drop(event_name);
                            drop(data);
                            drop(frame_permit);
                            yield Ok(Bytes::from(encode_stream_error(
                                client_protocol,
                                stream_error.as_deref().unwrap_or("upstream stream failed"),
                            )));
                            break 'read_stream;
                        }
                    }
                } else {
                    None
                };
                if upstream_protocol == UpstreamProtocol::Messages
                    && (event_name == "message_stop" || event_type == "message_stop")
                {
                    completed = true;
                }
                if let Some(terminal) = responses_terminal {
                    completed = true;
                    if terminal == ResponsesTerminal::Incomplete {
                        finish_reason = "length".to_owned();
                    }
                }
                if let Some(update) = google_update.as_ref() {
                    if let Some(reason) = update.finish_reason.as_deref() {
                        match normalize_google_stream_finish(
                            reason,
                            !google_tool_calls.is_empty() || !update.tool_calls.is_empty(),
                        ) {
                            Ok(reason) => {
                                finish_reason = reason;
                                completed = true;
                            }
                            Err(error) => {
                                stream_error = Some(error);
                                drop(value);
                                drop(event_name);
                                drop(data);
                                drop(frame_permit);
                                yield Ok(Bytes::from(encode_stream_error(
                                    client_protocol,
                                    stream_error.as_deref().unwrap_or("upstream stream failed"),
                                )));
                                break 'read_stream;
                            }
                        }
                    }
                    if let Err(error) = merge_google_tool_calls(
                        &mut google_tool_calls,
                        &update.tool_calls,
                        resource_limits.sse_buffer_limit_bytes,
                    ) {
                        stream_error = Some(error);
                        drop(value);
                        drop(event_name);
                        drop(data);
                        drop(frame_permit);
                        yield Ok(Bytes::from(encode_stream_error(
                            client_protocol,
                            stream_error.as_deref().unwrap_or("upstream stream failed"),
                        )));
                        break 'read_stream;
                    }
                }
                let mut frame_output = Vec::<Bytes>::with_capacity(3);
                let (input, output) = extract_stream_usage(upstream_protocol, &event_name, &value);
                let (cached, cache_input) =
                    extract_stream_cache_usage(upstream_protocol, &event_name, &value);
                if let Some(cost) = extract_stream_cost(upstream_protocol, event_name.as_str(), &value) {
                    cost_micro_usd = Some(cost);
                }
                if let Some(tokens) = input { input_tokens = tokens; has_usage = true; }
                if let Some(tokens) = output { output_tokens = tokens; has_usage = true; }
                if let Some(tokens) = cached { cached_tokens = tokens; }
                if let Some(tokens) = cache_input { cache_input_tokens = tokens; }
                if let Some(analytics) = &analytics {
                    analytics.set_token_usage(input, output);
                    analytics.set_cached_token_usage(cached, cache_input);
                }
                if let Some(reason) = extract_stream_finish(upstream_protocol, &event_name, &value) {
                    finish_reason = reason;
                    saw_finish_signal = true;
                }
                if upstream_protocol == UpstreamProtocol::ChatCompletions
                    && let Some(delta) = extract_chat_stream_tool_delta(&value)
                    && let Err(error) =
                        chat_tool_calls.merge_delta(delta, resource_limits.sse_buffer_limit_bytes)
                {
                    stream_error = Some(error);
                    drop(value);
                    drop(event_name);
                    drop(data);
                    drop(frame_permit);
                    yield Ok(Bytes::from(encode_stream_error(
                        client_protocol,
                        stream_error.as_deref().unwrap_or("upstream stream failed"),
                    )));
                    break 'read_stream;
                }
                if client_protocol == Protocol::Messages && !sent_start {
                    let start = if upstream_protocol == UpstreamProtocol::GoogleGenerateContent {
                        google_messages_stream_start(&upstream_id, &model, input_tokens)
                    } else {
                        messages_stream_start(&upstream_id, &model, input_tokens)
                    };
                    frame_output.push(Bytes::from(start));
                    sent_start = true;
                }
                if upstream_protocol == UpstreamProtocol::Messages {
                    let message_event_type = value
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or(event_name.as_str());
                    let message_result = match message_event_type {
                        "content_block_start" => encode_messages_stream_block_start(
                            client_protocol,
                            &value,
                            &mut messages_blocks,
                            &mut messages_next_output_index,
                            &mut messages_open_blocks,
                            &upstream_id,
                            &model,
                        ),
                        "content_block_delta" => encode_messages_stream_delta(
                            &value,
                            &mut MessagesStreamDeltaContext {
                                client_protocol,
                                blocks: &mut messages_blocks,
                                next_output_index: &mut messages_next_output_index,
                                open_message_blocks: &mut messages_open_blocks,
                                response_id: &upstream_id,
                                model: &model,
                                response_limit: resource_limits.sse_buffer_limit_bytes,
                            },
                        ),
                        "content_block_stop" => Ok((
                            encode_messages_stream_block_stop(
                                client_protocol,
                                &value,
                                &mut messages_blocks,
                                &mut messages_open_blocks,
                                &upstream_id,
                                &model,
                            ),
                            false,
                        )),
                        _ => Ok((Vec::new(), false)),
                    };
                    let (message_events, message_output) = match message_result {
                        Ok(result) => result,
                        Err(error) => {
                            stream_error = Some(error);
                            drop(value);
                            drop(event_name);
                            drop(data);
                            drop(frame_permit);
                            yield Ok(Bytes::from(encode_stream_error(
                                client_protocol,
                                stream_error.as_deref().unwrap_or("upstream stream failed"),
                            )));
                            break 'read_stream;
                        }
                    };
                    if message_output {
                        saw_output = true;
                    }
                    if (message_output || !message_events.is_empty())
                        && client_protocol != Protocol::Messages
                        && !sent_start
                    {
                        let start = encode_stream_start(client_protocol, &upstream_id, &model);
                        if !start.is_empty() {
                            frame_output.push(Bytes::from(start));
                        }
                        sent_start = true;
                    }
                    frame_output.extend(message_events);
                }
                if let Some(encoded) = encode_responses_image_event(
                    upstream_protocol,
                    client_protocol,
                    &event_name,
                    &value,
                ) {
                    saw_output = true;
                    if !sent_start {
                        let start = encode_stream_start(client_protocol, &upstream_id, &model);
                        if !start.is_empty() { frame_output.push(Bytes::from(start)); }
                        sent_start = true;
                    }
                    frame_output.push(Bytes::from(encoded));
                }
                if let Some(encoded) = encode_responses_tool_event(
                    upstream_protocol,
                    client_protocol,
                    &event_name,
                    &value,
                ) {
                    saw_output = true;
                    forwarded_response_tool_events = true;
                    if !sent_start {
                        let start = encode_stream_start(client_protocol, &upstream_id, &model);
                        if !start.is_empty() {
                            frame_output.push(Bytes::from(start));
                        }
                        sent_start = true;
                    }
                    frame_output.push(Bytes::from(encoded));
                }
                if upstream_protocol == UpstreamProtocol::ChatCompletions
                    && let Some(reasoning) = extract_chat_stream_reasoning_delta(&value)
                {
                    saw_output = true;
                    if !sent_start {
                        let start = encode_stream_start(client_protocol, &upstream_id, &model);
                        if !start.is_empty() {
                            frame_output.push(Bytes::from(start));
                        }
                        sent_start = true;
                    }
                    frame_output.push(Bytes::from(encode_chat_stream_reasoning_delta(
                        &upstream_id,
                        &model,
                        reasoning,
                    )));
                }
                let event = (upstream_protocol != UpstreamProtocol::Messages)
                    .then(|| extract_stream_text(upstream_protocol, &event_name, &value))
                    .flatten()
                    .map(|text| crate::protocol::StreamEvent::TextDelta { text });
                if let Some(crate::protocol::StreamEvent::TextDelta { text }) = event {
                    if !text.trim().is_empty() {
                        saw_output = true;
                        chat_stream_text_sent = true;
                    }
                    if upstream_protocol == UpstreamProtocol::GoogleGenerateContent
                        && client_protocol == Protocol::Responses
                    {
                        if resource_limits.sse_buffer_limit_bytes != 0
                            && google_responses_text.len().saturating_add(text.len())
                                > resource_limits.sse_buffer_limit_bytes
                        {
                            stream_error = Some(
                                "Google streamed text exceeded the configured response limit"
                                    .to_owned(),
                            );
                            drop(value);
                            drop(event_name);
                            drop(data);
                            drop(frame_permit);
                            yield Ok(Bytes::from(encode_stream_error(
                                client_protocol,
                                stream_error.as_deref().unwrap_or("upstream stream failed"),
                            )));
                            break 'read_stream;
                        }
                        google_responses_text.push_str(&text);
                    }
                    if client_protocol != Protocol::Messages && !sent_start {
                        let start = encode_stream_start(client_protocol, &upstream_id, &model);
                        if !start.is_empty() { frame_output.push(Bytes::from(start)); }
                        sent_start = true;
                    }
                    let encoded = if upstream_protocol == UpstreamProtocol::GoogleGenerateContent
                        && client_protocol == Protocol::Messages
                    {
                        if !google_message_text_started {
                            frame_output.push(Bytes::from(google_messages_text_start(0)));
                            google_message_text_started = true;
                        }
                        google_messages_stream_text(0, &text)
                    } else {
                        encode_stream_text(client_protocol, &upstream_id, &model, &text)
                    };
                    if client_protocol == Protocol::Messages
                        && upstream_protocol != UpstreamProtocol::GoogleGenerateContent
                        && !messages_text_started
                    {
                        frame_output.push(Bytes::from(messages_text_start(0)));
                        messages_text_started = true;
                        messages_open_blocks.push(0);
                    }
                    if !encoded.is_empty() { frame_output.push(Bytes::from(encoded)); }
                }
                if upstream_protocol == UpstreamProtocol::ChatCompletions
                    && let Some(delta) = extract_chat_stream_tool_delta(&value)
                {
                    saw_output = true;
                    if client_protocol == Protocol::ChatCompletions {
                        if !sent_start {
                            let start = encode_stream_start(client_protocol, &upstream_id, &model);
                            if !start.is_empty() {
                                frame_output.push(Bytes::from(start));
                            }
                            sent_start = true;
                        }
                        frame_output.push(Bytes::from(encode_chat_stream_tool_delta(
                            &upstream_id,
                            &model,
                            delta,
                        )));
                    }
                    // Non-Chat clients receive the merged calls when the
                    // stream completes; their protocol shapes have no
                    // incremental tool-delta form to mirror Chat chunks with.
                }
                if responses_terminal.is_some() {
                    let terminal_response = value.get("response").unwrap_or(&value);
                    let reconstructed_response =
                        responses_accumulator.reconstruct_response(terminal_response);
                    let response = reconstructed_response
                        .as_ref()
                        .unwrap_or(terminal_response);
                    if crate::protocol::decode_upstream_response(
                        UpstreamProtocol::Responses,
                        response,
                        &model,
                    )
                    .is_ok()
                    {
                        saw_output = true;
                    }
                    let output = response
                        .get("output")
                        .and_then(Value::as_array)
                        .cloned();
                    response_output = output.clone();
                    if let Some(output) = output {
                        let response_tool_calls = match extract_responses_tool_calls(&output) {
                            Ok(calls) => calls,
                            Err(error) => {
                                stream_error = Some(error);
                                drop(value);
                                drop(event_name);
                                drop(data);
                                drop(frame_permit);
                                yield Ok(Bytes::from(encode_stream_error(
                                    client_protocol,
                                    stream_error.as_deref().unwrap_or("upstream stream failed"),
                                )));
                                break 'read_stream;
                            }
                        };
                        if !response_tool_calls.is_empty() {
                            finish_reason = "tool_calls".to_owned();
                            if client_protocol != Protocol::Responses
                                || !forwarded_response_tool_events
                            {
                                let has_response_text = output.iter().any(|item| {
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
                                let (tool_events, _tool_output) =
                                    match encode_google_tool_call_events(
                                        client_protocol,
                                        &response_tool_calls,
                                        &upstream_id,
                                        &model,
                                        has_response_text,
                                        client_protocol == Protocol::Messages
                                            && messages_text_started,
                                    ) {
                                        Ok(events) => events,
                                        Err(error) => {
                                            stream_error = Some(error);
                                            drop(value);
                                            drop(event_name);
                                            drop(data);
                                            drop(frame_permit);
                                            yield Ok(Bytes::from(encode_stream_error(
                                                client_protocol,
                                                stream_error
                                                    .as_deref()
                                                    .unwrap_or("upstream stream failed"),
                                            )));
                                            break 'read_stream;
                                        }
                                    };
                                if client_protocol != Protocol::Messages
                                    && !tool_events.is_empty()
                                    && !sent_start
                                {
                                    let start =
                                        encode_stream_start(client_protocol, &upstream_id, &model);
                                    if !start.is_empty() {
                                        frame_output.push(Bytes::from(start));
                                    }
                                    sent_start = true;
                                }
                                if !tool_events.is_empty() {
                                    saw_output = true;
                                }
                                if client_protocol == Protocol::Messages {
                                    let first_tool_index = usize::from(messages_text_started);
                                    for index in 0..response_tool_calls.len() {
                                        let output_index = first_tool_index.saturating_add(index);
                                        if !messages_open_blocks.contains(&output_index) {
                                            messages_open_blocks.push(output_index);
                                        }
                                    }
                                }
                                frame_output.extend(tool_events);
                            }
                        }
                    }
                }
                if completed
                    && upstream_protocol == UpstreamProtocol::Messages
                    && client_protocol == Protocol::Responses
                {
                    response_output = Some(messages_response_output(&messages_blocks));
                }
                if completed && upstream_protocol == UpstreamProtocol::GoogleGenerateContent {
                    let (tool_events, tool_output) = match encode_google_tool_call_events(
                        client_protocol,
                        &google_tool_calls,
                        &upstream_id,
                        &model,
                        !google_responses_text.is_empty(),
                        google_message_text_started,
                    ) {
                        Ok(output) => output,
                        Err(error) => {
                            stream_error = Some(error);
                            drop(value);
                            drop(event_name);
                            drop(data);
                            drop(frame_permit);
                            yield Ok(Bytes::from(encode_stream_error(
                                client_protocol,
                                stream_error.as_deref().unwrap_or("upstream stream failed"),
                            )));
                            break 'read_stream;
                        }
                    };
                    if client_protocol != Protocol::Messages && !tool_events.is_empty() && !sent_start {
                        let start = encode_stream_start(client_protocol, &upstream_id, &model);
                        if !start.is_empty() {
                            frame_output.push(Bytes::from(start));
                        }
                        sent_start = true;
                    }
                    if !tool_events.is_empty() {
                        saw_output = true;
                    }
                    frame_output.extend(tool_events);
                    if client_protocol == Protocol::Responses {
                        let mut output = Vec::with_capacity(tool_output.len() + 1);
                        if !google_responses_text.is_empty() {
                            output.push(json!({
                                "id":"msg_0",
                                "type":"message",
                                "role":"assistant",
                                "content":[{"type":"output_text","text":google_responses_text,"annotations":[]}]
                            }));
                        }
                        output.extend(tool_output);
                        response_output = Some(output);
                    }
                }
                drop(value);
                drop(event_name);
                drop(data);
                drop(frame_permit);
                for output in frame_output {
                    yield Ok(output);
                }
                if completed {
                    break 'read_stream;
                }
        }

        if shutting_down {
            return;
        }
        // Some providers close the connection right after the finish signal
        // (chat finish_reason or Anthropic stop_reason) and omit the terminal
        // sentinel event ([DONE] / message_stop). The output is already fully
        // delivered at that point, so treat the finish signal as a clean
        // completion instead of reporting a truncated stream.
        if !completed && saw_finish_signal {
            completed = true;
            if upstream_protocol == UpstreamProtocol::Messages
                && client_protocol == Protocol::Responses
            {
                response_output = Some(messages_response_output(&messages_blocks));
            }
        }
        if !completed
            && upstream_protocol == UpstreamProtocol::ChatCompletions
            && !chat_tool_calls.is_empty()
        {
            completed = true;
            if finish_reason == "stop" {
                finish_reason = "tool_calls".to_owned();
            }
        }
        let had_stream_error = stream_error.is_some();
        let final_error = stream_error.or_else(|| {
            if !completed {
                Some("upstream stream ended before a terminal success event".to_owned())
            } else if !saw_output {
                Some("upstream completed the response without content".to_owned())
            } else {
                None
            }
        });
        if let Some(error) = final_error.as_deref() {
            outcome.fail();
            if !had_stream_error {
                yield Ok(Bytes::from(encode_stream_error(client_protocol, error)));
            }
            if let Some(log) = log {
                log_failed_stream(
                    log,
                    error,
                    has_usage.then_some(input_tokens.min(i64::MAX as u64) as i64),
                    has_usage.then_some(output_tokens.min(i64::MAX as u64) as i64),
                    (cache_input_tokens > 0 || cached_tokens > 0)
                        .then_some(cached_tokens.min(i64::MAX as u64) as i64),
                    (cache_input_tokens > 0 || cached_tokens > 0)
                        .then_some(cache_input_tokens.min(i64::MAX as u64) as i64),
                )
                .await;
            }
        } else {
            outcome.complete();
            if !sent_start {
                if client_protocol == Protocol::Messages {
                    let start = if upstream_protocol == UpstreamProtocol::GoogleGenerateContent {
                        google_messages_stream_start(&upstream_id, &model, input_tokens)
                    } else {
                        messages_stream_start(&upstream_id, &model, input_tokens)
                    };
                    yield Ok(Bytes::from(start));
                } else {
                    let start = encode_stream_start(client_protocol, &upstream_id, &model);
                    if !start.is_empty() { yield Ok(Bytes::from(start)); }
                }
            }
            let usage = has_usage.then_some((input_tokens, output_tokens));
            if upstream_protocol == UpstreamProtocol::ChatCompletions
                && client_protocol != Protocol::ChatCompletions
            {
                if finish_reason == "stop" {
                    finish_reason = "tool_calls".to_owned();
                }
                if client_protocol == Protocol::Messages {
                    let first_tool_index = usize::from(messages_text_started);
                    let tool_count = chat_tool_calls.call_count();
                    for offset in 0..tool_count {
                        let output_index = first_tool_index.saturating_add(offset);
                        if !messages_open_blocks.contains(&output_index) {
                            messages_open_blocks.push(output_index);
                        }
                    }
                }
                if client_protocol == Protocol::Responses
                    && response_output.is_none()
                    && !saw_output
                {
                    response_output = Some(Vec::new());
                }
                if !sent_start {
                    let start = encode_stream_start(client_protocol, &upstream_id, &model);
                    if !start.is_empty() {
                        yield Ok(Bytes::from(start));
                    }
                }
                for call in chat_tool_calls.drain() {
                    let (tool_events, tool_output) = match encode_google_tool_call_events(
                        client_protocol,
                        std::slice::from_ref(&call),
                        &upstream_id,
                        &model,
                        chat_stream_text_sent,
                        chat_stream_text_sent,
                    ) {
                        Ok(output) => output,
                        Err(error) => {
                            stream_error = Some(error);
                            yield Ok(Bytes::from(encode_stream_error(
                                client_protocol,
                                stream_error.as_deref().unwrap_or("upstream stream failed"),
                            )));
                            break;
                        }
                    };
                    if !tool_output.is_empty() {
                        match response_output.as_mut() {
                            Some(output) => output.extend(tool_output),
                            None => response_output = Some(tool_output),
                        }
                    }
                    for event in tool_events {
                        yield Ok(event);
                    }
                }
            }
            let finish = if upstream_protocol == UpstreamProtocol::GoogleGenerateContent
                && client_protocol == Protocol::Messages
            {
                google_messages_stream_finish(
                    &finish_reason,
                    input_tokens,
                    output_tokens,
                    cost_micro_usd,
                    google_message_text_started,
                )
            } else if client_protocol == Protocol::Messages {
                messages_stream_finish(
                    &finish_reason,
                    input_tokens,
                    output_tokens,
                    cost_micro_usd,
                    &messages_open_blocks,
                )
            } else {
                encode_stream_finish(
                    client_protocol,
                    &upstream_id,
                    &model,
                    &finish_reason,
                    usage,
                    cost_micro_usd,
                    response_output.as_deref(),
                )
            };
            yield Ok(Bytes::from(finish));
            if client_protocol == Protocol::ChatCompletions {
                if usage.is_some() { yield Ok(Bytes::from(encode_stream_usage(input_tokens, output_tokens, cost_micro_usd))); }
                yield Ok(Bytes::from("data: [DONE]\n\n"));
            }
            if let Some(log) = log {
                log_completed_stream(
                    log,
                    has_usage.then_some(input_tokens.min(i64::MAX as u64) as i64),
                    has_usage.then_some(output_tokens.min(i64::MAX as u64) as i64),
                    (cache_input_tokens > 0 || cached_tokens > 0)
                        .then_some(cached_tokens.min(i64::MAX as u64) as i64),
                    (cache_input_tokens > 0 || cached_tokens > 0)
                        .then_some(cache_input_tokens.min(i64::MAX as u64) as i64),
                    cost_micro_usd,
                )
                .await;
            }
        }
    }
}
