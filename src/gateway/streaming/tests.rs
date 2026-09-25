use super::*;

async fn run_preflight_test(
    body: &str,
    upstream_protocol: UpstreamProtocol,
    client_protocol: Protocol,
    resource_limits: GatewayResourceLimits,
) -> Result<String, String> {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("preflight test listener");
    let address = listener.local_addr().expect("preflight test address");
    let body = body.to_owned();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("preflight test request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket
            .write_all(headers.as_bytes())
            .await
            .expect("preflight test headers");
        socket
            .write_all(body.as_bytes())
            .await
            .expect("preflight test body");
    });

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("preflight test upstream response");
    let result = preflight_stream(
        upstream,
        upstream_protocol,
        client_protocol,
        "test-model",
        Duration::from_secs(1),
        resource_limits,
    )
    .await;
    let result = match result {
        Ok(mut chunks) => {
            let mut output = Vec::new();
            while let Some(chunk) = chunks.next().await {
                output.extend_from_slice(&chunk.expect("preflight replay chunk"));
            }
            Ok(String::from_utf8(output).expect("preflight replay body"))
        }
        Err(error) => Err(error),
    };
    server.await.expect("preflight test server");
    result
}

#[tokio::test]
async fn preflight_rejects_empty_anthropic_stream_before_client_output() {
    let body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"content\":[]}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"upstream_error\",\"message\":\"upstream completed the response without content\"}}\n\n",
    );
    let error = run_preflight_test(
        body,
        UpstreamProtocol::Messages,
        Protocol::Messages,
        GatewayResourceLimits::default(),
    )
    .await
    .expect_err("empty provider stream must stay retryable");
    assert!(
        error.contains("upstream completed the response without content"),
        "{error}"
    );
}

#[tokio::test]
async fn preflight_replays_buffered_prefix_once_meaningful_output_arrives() {
    let body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"content\":[]}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
    );
    let replayed = run_preflight_test(
        body,
        UpstreamProtocol::Messages,
        Protocol::Messages,
        GatewayResourceLimits::default(),
    )
    .await
    .expect("meaningful provider output should pass preflight");
    assert_eq!(replayed, body);
}

#[tokio::test]
async fn preflight_accepts_semantic_output_inside_an_oversized_transport_chunk() {
    let mut body = String::from(concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"content\":[]}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
    ));
    body.push_str(
        &": padding\n\n".repeat(GatewayResourceLimits::DEFAULT_SSE_BUFFER_LIMIT_BYTES / 8),
    );
    let replayed = run_preflight_test(
        &body,
        UpstreamProtocol::Messages,
        Protocol::Messages,
        GatewayResourceLimits::default(),
    )
    .await
    .expect("semantic output must be found before the transport chunk limit");
    assert_eq!(replayed, body);
}

#[tokio::test]
async fn preflight_allows_large_prefix_when_sse_limits_are_unlimited() {
    let mut body = String::from(": padding\n\n");
    body.push_str(
        &": padding\n\n".repeat(GatewayResourceLimits::DEFAULT_SSE_BUFFER_LIMIT_BYTES / 8),
    );
    body.push_str(concat!(
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}\n\n",
        ));
    let replayed = run_preflight_test(
        &body,
        UpstreamProtocol::Messages,
        Protocol::Messages,
        GatewayResourceLimits {
            sse_frame_limit_bytes: GatewayResourceLimits::UNLIMITED_SSE_FRAME_LIMIT_BYTES,
            sse_buffer_limit_bytes: GatewayResourceLimits::UNLIMITED_SSE_BUFFER_LIMIT_BYTES,
            ..GatewayResourceLimits::default()
        },
    )
    .await
    .expect("unlimited SSE limits should allow a large preflight prefix");
    assert_eq!(replayed, body);
}

#[test]
fn anthropic_thinking_and_tool_events_are_semantic_stream_output() {
    let thinking_start = json!({
        "type":"content_block_start",
        "index":0,
        "content_block":{"type":"thinking","thinking":""}
    });
    let tool_start = json!({
        "type":"content_block_start",
        "index":1,
        "content_block":{"type":"tool_use","id":"call-1","name":"exec","input":{}}
    });
    let tool_delta = json!({
        "type":"content_block_delta",
        "index":1,
        "delta":{"type":"input_json_delta","partial_json":r#"{"cmd":"pwd"}"#}
    });

    for value in [thinking_start, tool_start, tool_delta] {
        assert!(has_messages_stream_output("", &value));
        let mut responses_accumulator = ResponsesStreamAccumulator::default();
        assert!(matches!(
            preflight_frame(
                UpstreamProtocol::Messages,
                Protocol::Messages,
                "command-code/model",
                &format!("event: {}\ndata: {}\n\n", value["type"], value),
                &mut responses_accumulator,
                GatewayResourceLimits::default().sse_buffer_limit_bytes,
            ),
            Ok(PreflightFrame::Ready)
        ));
    }
}

#[test]
fn anthropic_tool_and_thinking_deltas_translate_to_client_protocols() {
    let tool_start = json!({
        "type":"content_block_start",
        "index":0,
        "content_block":{"type":"tool_use","id":"call-1","name":"exec","input":{}}
    });
    let mut blocks = Vec::new();
    let mut next_index = 0;
    let mut open_blocks = Vec::new();
    let (events, output) = encode_messages_stream_block_start(
        Protocol::ChatCompletions,
        &tool_start,
        &mut blocks,
        &mut next_index,
        &mut open_blocks,
        "response-1",
        "command-code/model",
    )
    .expect("Anthropic tool start encodes");
    assert!(output);
    assert!(String::from_utf8_lossy(&events[0]).contains("tool_calls"));

    let tool_delta = json!({
        "type":"content_block_delta",
        "index":0,
        "delta":{"type":"input_json_delta","partial_json":r#"{"cmd":"pwd"}"#}
    });
    let (events, output) = encode_messages_stream_delta(
        &tool_delta,
        &mut MessagesStreamDeltaContext {
            client_protocol: Protocol::Responses,
            blocks: &mut blocks,
            next_output_index: &mut next_index,
            open_message_blocks: &mut open_blocks,
            response_id: "response-1",
            model: "command-code/model",
            response_limit: 4096,
        },
    )
    .expect("Anthropic tool delta encodes");
    assert!(output);
    assert!(String::from_utf8_lossy(&events[0]).contains("response.function_call_arguments.delta"));

    let thinking_start = json!({
        "type":"content_block_start",
        "index":1,
        "content_block":{"type":"thinking","thinking":""}
    });
    let _ = encode_messages_stream_block_start(
        Protocol::Messages,
        &thinking_start,
        &mut blocks,
        &mut next_index,
        &mut open_blocks,
        "response-1",
        "command-code/model",
    )
    .expect("Anthropic thinking start encodes");
    let thinking_delta = json!({
        "type":"content_block_delta",
        "index":1,
        "delta":{"type":"thinking_delta","thinking":"checking"}
    });
    let (events, output) = encode_messages_stream_delta(
        &thinking_delta,
        &mut MessagesStreamDeltaContext {
            client_protocol: Protocol::ChatCompletions,
            blocks: &mut blocks,
            next_output_index: &mut next_index,
            open_message_blocks: &mut open_blocks,
            response_id: "response-1",
            model: "command-code/model",
            response_limit: 4096,
        },
    )
    .expect("Anthropic thinking delta encodes");
    assert!(output);
    assert!(String::from_utf8_lossy(&events[0]).contains("reasoning_content"));
}

#[tokio::test]
async fn anthropic_tool_only_stream_is_completed_without_empty_content_error() {
    let body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"content\":[]}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call-1\",\"name\":\"exec\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"cmd\\\":\\\"pwd\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation_from_chunks(
        Box::pin(futures_util::stream::iter(vec![
            Ok::<Bytes, reqwest::Error>(Bytes::from_static(body.as_bytes())),
        ])),
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::Messages,
            client_protocol: Protocol::Messages,
            request_id: "messages-tool-stream".to_owned(),
            model: "command-code/model".to_owned(),
            idle_timeout: Duration::from_secs(1),
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated Anthropic tool stream body");
    let translated = String::from_utf8(translated.to_vec()).expect("translated SSE text");
    assert!(translated.contains("tool_use"), "{translated}");
    assert!(translated.contains("input_json_delta"), "{translated}");
    assert!(!translated.contains("upstream completed the response without content"));
    assert!(outcome.completed());
}

#[tokio::test]
async fn chat_stream_missing_done_marker_completes_from_finish_reason() {
    let body = concat!(
        "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    );
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation_from_chunks(
        Box::pin(futures_util::stream::iter(vec![
            Ok::<Bytes, reqwest::Error>(Bytes::from_static(body.as_bytes())),
        ])),
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::ChatCompletions,
            client_protocol: Protocol::ChatCompletions,
            request_id: "chat-no-done".to_owned(),
            model: "command-code/model".to_owned(),
            idle_timeout: Duration::from_secs(1),
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated chat stream body");
    let translated = String::from_utf8(translated.to_vec()).expect("translated SSE text");
    assert!(
        translated.contains("\"finish_reason\":\"stop\""),
        "{translated}"
    );
    assert!(!translated.contains("upstream_error"), "{translated}");
    assert!(translated.ends_with("data: [DONE]\n\n"), "{translated}");
    assert!(outcome.completed());
    assert!(!outcome.failed());
}

#[tokio::test]
async fn chat_stream_eof_without_finish_signal_stays_failed() {
    let body = "data: {\"id\":\"c2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n";
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation_from_chunks(
        Box::pin(futures_util::stream::iter(vec![
            Ok::<Bytes, reqwest::Error>(Bytes::from_static(body.as_bytes())),
        ])),
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::ChatCompletions,
            client_protocol: Protocol::ChatCompletions,
            request_id: "chat-truncated".to_owned(),
            model: "command-code/model".to_owned(),
            idle_timeout: Duration::from_secs(1),
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated truncated chat stream body");
    let translated = String::from_utf8(translated.to_vec()).expect("translated SSE text");
    assert!(
        translated.contains("upstream stream ended before a terminal success event"),
        "{translated}"
    );
    assert!(outcome.failed());
    assert!(!outcome.completed());
}

#[tokio::test]
async fn anthropic_stream_without_message_stop_completes_from_stop_reason() {
    let body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"content\":[]}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
    );
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation_from_chunks(
        Box::pin(futures_util::stream::iter(vec![
            Ok::<Bytes, reqwest::Error>(Bytes::from_static(body.as_bytes())),
        ])),
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::Messages,
            client_protocol: Protocol::Messages,
            request_id: "messages-no-stop".to_owned(),
            model: "command-code/model".to_owned(),
            idle_timeout: Duration::from_secs(1),
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated Anthropic stream body");
    let translated = String::from_utf8(translated.to_vec()).expect("translated SSE text");
    assert!(!translated.contains("upstream_error"), "{translated}");
    assert!(
        translated.contains("\"stop_reason\":\"end_turn\""),
        "{translated}"
    );
    assert!(outcome.completed());
    assert!(!outcome.failed());
}

#[tokio::test]
async fn preflight_reports_deliberate_empty_completion_for_finish_signal_only_stream() {
    let body = "data: {\"id\":\"c3\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
    let error = run_preflight_test(
        body,
        UpstreamProtocol::ChatCompletions,
        Protocol::ChatCompletions,
        GatewayResourceLimits::default(),
    )
    .await
    .expect_err("finish signal without output stays retryable");
    assert!(
        error.contains("upstream completed the response without content"),
        "{error}"
    );
}

#[test]
fn preflight_accepts_chat_tool_deltas_for_every_client_protocol() {
    let chunk = json!({
        "choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"exec","arguments":"{\"cmd\":\"pwd\"}"}}]},"finish_reason":null}]
    });
    let frame = format!("data: {}\n\n", chunk);
    for client_protocol in [
        Protocol::ChatCompletions,
        Protocol::Responses,
        Protocol::Messages,
    ] {
        let mut responses_accumulator = ResponsesStreamAccumulator::default();
        assert!(
            matches!(
                preflight_frame(
                    UpstreamProtocol::ChatCompletions,
                    client_protocol,
                    "test-model",
                    &frame,
                    &mut responses_accumulator,
                    GatewayResourceLimits::default().sse_buffer_limit_bytes,
                ),
                Ok(PreflightFrame::Ready)
            ),
            "tool delta must be meaningful output for client {client_protocol:?}"
        );
    }
}

#[test]
fn chat_tool_call_accumulator_merges_chunks_and_limits_arguments() {
    let first = json!({
        "tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"exec","arguments":"{\"cmd\":"}}]
    });
    let second = json!({
        "tool_calls":[{"index":0,"function":{"arguments":"\"pwd\"}"}}]
    });
    let mut accumulator = ChatStreamToolCallAccumulator::default();
    accumulator.merge_delta(&first, 1024).expect("first delta");
    accumulator
        .merge_delta(&second, 1024)
        .expect("second delta");
    assert_eq!(accumulator.call_count(), 1);
    let calls = accumulator.drain();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call-1");
    assert_eq!(calls[0].name, "exec");
    assert_eq!(calls[0].arguments, json!({"cmd":"pwd"}));
    assert!(accumulator.is_empty());

    // Arguments exceeding the limit fail the stream instead of truncating.
    let mut oversized = ChatStreamToolCallAccumulator::default();
    assert!(
        oversized
            .merge_delta(&first, 4)
            .unwrap_err()
            .contains("exceeded"),
        "oversized arguments must fail"
    );
    // Invalid JSON arguments are preserved as a raw string, never dropped.
    let raw = json!({
        "tool_calls":[{"index":0,"id":"call-2","type":"function","function":{"name":"patch","arguments":"not-json"}}]
    });
    let mut raw_accumulator = ChatStreamToolCallAccumulator::default();
    raw_accumulator.merge_delta(&raw, 1024).expect("raw delta");
    let raw_calls = raw_accumulator.drain();
    assert_eq!(raw_calls[0].arguments, Value::String("not-json".to_owned()));
}

#[tokio::test]
async fn preflight_reconstructs_codex_function_call_before_empty_terminal_output() {
    let body = concat!(
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"status\":\"in_progress\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"{\\\"query\\\":\\\"status\\\"}\"}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\",\"output_index\":0,\"arguments\":\"{\\\"query\\\":\\\"status\\\"}\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.6-luna\",\"status\":\"completed\",\"output\":[]}}\n\n",
    );
    let replayed = run_preflight_test(
        body,
        UpstreamProtocol::Responses,
        Protocol::Responses,
        GatewayResourceLimits::default(),
    )
    .await
    .expect("incremental function call should satisfy preflight");
    assert_eq!(replayed, body);
}

#[test]
fn parses_google_stream_text_usage_and_terminal_finish() {
    let chunk = json!({
        "candidates":[{
            "content":{"parts":[{"text":"hello "},{"text":"world"}]},
            "finishReason":"STOP"
        }],
        "usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":5,"thoughtsTokenCount":2,"totalTokenCount":18}
    });

    assert_eq!(
        extract_stream_text(UpstreamProtocol::GoogleGenerateContent, "", &chunk).as_deref(),
        Some("hello world")
    );
    assert_eq!(
        extract_stream_usage(UpstreamProtocol::GoogleGenerateContent, "", &chunk),
        (Some(11), Some(7))
    );
    let update = parse_google_stream_update(&chunk).expect("Google chunk parses");
    assert_eq!(update.finish_reason.as_deref(), Some("STOP"));
    assert_eq!(
        normalize_google_stream_finish("STOP", false).as_deref(),
        Ok("stop")
    );
    assert_eq!(
        normalize_google_stream_finish("MAX_TOKENS", false).as_deref(),
        Ok("length")
    );
    assert_eq!(
        normalize_google_stream_finish("SAFETY", false).as_deref(),
        Ok("content_filter")
    );
}

#[test]
fn chat_stream_reasoning_delta_is_output_and_keeps_chat_shape() {
    let chunk = json!({
        "choices":[{
            "delta":{"reasoning_content":"checking the tool result"},
            "finish_reason":null,
            "index":0
        }]
    });
    assert_eq!(
        extract_chat_stream_reasoning_delta(&chunk),
        Some("checking the tool result")
    );
    let frame = encode_chat_stream_reasoning_delta("chat-1", "deepseek-v4.1-flash", "thinking");
    assert!(frame.contains("\"reasoning_content\":\"thinking\""));
    let mut responses_accumulator = ResponsesStreamAccumulator::default();
    assert!(matches!(
        preflight_frame(
            UpstreamProtocol::ChatCompletions,
            Protocol::ChatCompletions,
            "deepseek-v4.1-flash",
            &format!("data: {chunk}\n\n"),
            &mut responses_accumulator,
            GatewayResourceLimits::default().sse_buffer_limit_bytes,
        ),
        Ok(PreflightFrame::Ready)
    ));
}

#[tokio::test]
async fn chat_stream_forwards_tool_call_deltas_before_terminal_done() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("stream test listener");
    let address = listener.local_addr().expect("stream test address");
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("stream test request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        let chunks = [
            json!({
                "choices":[{"delta":{"role":"assistant"},"finish_reason":null,"index":0}]
            }),
            json!({
                "choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"exec","arguments":""}}]},"finish_reason":null,"index":0}]
            }),
            json!({
                "choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"cmd\":\"pwd\"}"}}]},"finish_reason":null,"index":0}]
            }),
            json!({
                "choices":[{"delta":{},"finish_reason":"tool_calls","index":0}]
            }),
            json!({
                "choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}
            }),
        ];
        let mut body = String::new();
        for chunk in chunks {
            body.push_str("data: ");
            body.push_str(&chunk.to_string());
            body.push_str("\n\n");
        }
        body.push_str("data: [DONE]\n\n");
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("stream test response");
    });

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("upstream stream response");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation(
        upstream,
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::ChatCompletions,
            client_protocol: Protocol::ChatCompletions,
            request_id: "request-1".to_owned(),
            model: "cmc/model".to_owned(),
            idle_timeout: Duration::from_secs(1),
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated stream body");
    server.await.expect("stream test server");

    let body = String::from_utf8(body.to_vec()).expect("translated SSE text");
    assert!(body.contains("\"tool_calls\":[{"), "{body}");
    assert!(body.contains("\"name\":\"exec\""));
    assert!(body.contains("{\\\"cmd\\\":\\\"pwd\\\"}"));
    assert!(body.contains("\"finish_reason\":\"tool_calls\""));
    assert!(body.ends_with("data: [DONE]\n\n"));
    assert!(outcome.completed());
    assert_eq!(chat_stream_finish_reason("function_call"), "tool_calls");
}

#[tokio::test]
async fn terminal_stream_without_output_is_failed_instead_of_completed() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("empty stream test listener");
    let address = listener.local_addr().expect("empty stream test address");
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("empty stream test request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        let body = "data: [DONE]\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("empty stream test response");
    });

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("empty upstream stream response");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation(
        upstream,
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::ChatCompletions,
            client_protocol: Protocol::ChatCompletions,
            request_id: "empty-request".to_owned(),
            model: "empty-model".to_owned(),
            idle_timeout: Duration::from_secs(1),
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated empty stream body");
    server.await.expect("empty stream test server");

    let body = String::from_utf8(body.to_vec()).expect("translated SSE text");
    assert!(body.contains("\"type\":\"upstream_error\""), "{body}");
    // The failure event must still be followed by the Chat Completions
    // terminal sentinel so client SSE parsers end on a deliberate
    // terminator instead of treating the failure as a dropped connection.
    assert!(body.ends_with("data: [DONE]\n\n"), "{body}");
    assert!(outcome.failed());
    assert!(!outcome.completed());
}

#[test]
fn extracts_cached_stream_usage_and_anthropic_cache_denominator() {
    let openai = json!({
        "usage":{"prompt_tokens":11,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":5}}
    });
    assert_eq!(
        extract_stream_cache_usage(UpstreamProtocol::ChatCompletions, "", &openai),
        (Some(5), Some(11))
    );

    let anthropic = json!({
        "type":"message_start",
        "message":{"usage":{"input_tokens":2,"cache_read_input_tokens":3,"cache_creation_input_tokens":4,"output_tokens":0}}
    });
    assert_eq!(
        extract_stream_cache_usage(UpstreamProtocol::Messages, "message_start", &anthropic),
        (Some(3), Some(9))
    );

    let google = json!({"usageMetadata":{"promptTokenCount":11,"cachedContentTokenCount":5}});
    assert_eq!(
        extract_stream_cache_usage(UpstreamProtocol::GoogleGenerateContent, "", &google),
        (Some(5), Some(11))
    );
}

#[test]
fn translates_google_function_calls_to_all_client_stream_formats() {
    let chunk = json!({
        "candidates":[{
            "content":{"parts":[{"functionCall":{"id":"call-1","name":"lookup","args":{"query":"status"}}}]},
            "finishReason":"STOP"
        }]
    });
    let update = parse_google_stream_update(&chunk).expect("function call parses");
    let mut calls = Vec::new();
    merge_google_tool_calls(&mut calls, &update.tool_calls, 4096).expect("function call merges");
    assert_eq!(
        normalize_google_stream_finish("STOP", !calls.is_empty()).as_deref(),
        Ok("tool_calls")
    );

    let (chat_events, _) = encode_google_tool_call_events(
        Protocol::ChatCompletions,
        &calls,
        "response-1",
        "ocg/model",
        false,
        false,
    )
    .expect("chat call encodes");
    let chat = String::from_utf8_lossy(&chat_events[0]);
    assert!(chat.contains("\"id\":\"call-1\""));
    assert!(chat.contains("\"name\":\"lookup\""));
    assert!(chat.contains("\"arguments\":\"{\\\"query\\\":\\\"status\\\"}\""));
    assert_eq!(chat_stream_finish_reason("tool_calls"), "tool_calls");

    let (responses_events, responses_output) = encode_google_tool_call_events(
        Protocol::Responses,
        &calls,
        "response-1",
        "ocg/model",
        true,
        false,
    )
    .expect("Responses call encodes");
    let responses = responses_events
        .iter()
        .map(|event| String::from_utf8_lossy(event).into_owned())
        .collect::<String>();
    assert!(responses.contains("response.output_item.added"));
    assert!(responses.contains("response.function_call_arguments.delta"));
    assert!(responses.contains("response.output_item.done"));
    assert_eq!(responses_output[0]["type"], "function_call");
    assert_eq!(responses_output[0]["call_id"], "call-1");

    let (messages_events, _) = encode_google_tool_call_events(
        Protocol::Messages,
        &calls,
        "response-1",
        "ocg/model",
        false,
        false,
    )
    .expect("Messages call encodes");
    let messages = messages_events
        .iter()
        .map(|event| String::from_utf8_lossy(event).into_owned())
        .collect::<String>();
    assert!(messages.contains("\"type\":\"tool_use\""));
    assert!(messages.contains("input_json_delta"));
    assert!(
        google_messages_stream_finish("tool_calls", 11, 7, None, false)
            .contains("\"stop_reason\":\"tool_use\"")
    );
}

#[test]
fn stream_completion_events_preserve_provider_cost_for_each_client_protocol() {
    let cost = Some(125_000);
    let responses = encode_stream_finish(
        Protocol::Responses,
        "response-1",
        "cl/model",
        "stop",
        Some((4, 6)),
        cost,
        None,
    );
    assert!(responses.contains("\"cost\":0.125"));

    let messages = messages_stream_finish("stop", 4, 6, cost, &[]);
    assert!(messages.contains("\"cost\":0.125"));

    let chat = encode_stream_usage(4, 6, cost);
    assert!(chat.contains("\"cost\":0.125"));
}

#[test]
fn merges_google_function_call_snapshots_with_stable_ids() {
    let first = parse_google_stream_update(&json!({
            "candidates":[{"content":{"parts":[{"functionCall":{"id":"stable","name":"lookup","args":{"q":"a"}}}]}}]
        }))
        .expect("first snapshot");
    let second = parse_google_stream_update(&json!({
            "candidates":[{"content":{"parts":[{"functionCall":{"id":"stable","name":"lookup","args":{"q":"ab"}}}]}}]
        }))
        .expect("second snapshot");
    let mut calls = Vec::new();
    merge_google_tool_calls(&mut calls, &first.tool_calls, 4096).expect("first merge");
    merge_google_tool_calls(&mut calls, &second.tool_calls, 4096).expect("snapshot merge");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].arguments["q"], "ab");
}

#[test]
fn rejects_unsupported_malformed_and_unbounded_google_stream_chunks() {
    assert!(parse_google_stream_update(&json!({
            "candidates":[{"content":{"parts":[{"inlineData":{"mimeType":"image/png","data":"aA=="}}]}}]
        }))
        .is_err());
    assert!(
        parse_google_stream_update(&json!({
            "candidates":[{"content":{"parts":[{"functionCall":{"name":"lookup","args":[]}}]}}]
        }))
        .is_err()
    );
    assert!(
        parse_google_stream_update(&json!({
            "promptFeedback":{"blockReason":"SAFETY"}
        }))
        .is_err()
    );
    assert!(normalize_google_stream_finish("NOT_A_GOOGLE_FINISH_REASON", false).is_err());

    let update = parse_google_stream_update(&json!({
            "candidates":[{"content":{"parts":[{"functionCall":{"name":"large","args":{"payload":"long"}}}]}}]
        }))
        .expect("call parses before resource check");
    let mut calls = Vec::new();
    assert!(merge_google_tool_calls(&mut calls, &update.tool_calls, 2).is_err());
}

#[test]
fn recognizes_responses_terminal_aliases_and_statuses() {
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({
                "type":"response.completed",
                "response":{"status":"completed"}
            })
        ),
        Some(ResponsesTerminal::Completed)
    );
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({
                "type":"response.done",
                "response":{"status":"completed"}
            })
        ),
        Some(ResponsesTerminal::Completed)
    );
    assert_eq!(
        responses_terminal_state(
            "response.incomplete",
            &json!({
                "response":{"status":"incomplete"}
            })
        ),
        Some(ResponsesTerminal::Incomplete)
    );
    assert_eq!(
        extract_stream_finish(
            UpstreamProtocol::Responses,
            "response.incomplete",
            &json!({
                "type":"response.incomplete",
                "response":{
                    "status":"incomplete",
                    "output":[{"type":"function_call"}]
                }
            })
        ),
        Some("length".to_owned())
    );
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({
                "type":"response.done",
                "response":{"status":"incomplete"}
            })
        ),
        Some(ResponsesTerminal::Incomplete)
    );
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({
                "type":"response.done",
                "response":{"status":"failed"}
            })
        ),
        Some(ResponsesTerminal::Failed)
    );
    assert_eq!(
        responses_terminal_state(
            "",
            &json!({"type":"response.failed","error":{"code":"upstream_error"}})
        ),
        Some(ResponsesTerminal::Failed)
    );
    assert_eq!(
        responses_terminal_state("error", &json!({"error":{"message":"provider stopped"}})),
        Some(ResponsesTerminal::Failed)
    );
    assert_eq!(
        responses_terminal_state(
            "response.output_text.delta",
            &json!({"type":"response.output_text.delta"})
        ),
        None
    );
}

#[test]
fn provider_stream_error_keeps_bounded_redacted_diagnostics() {
    let error = extract_stream_error(
        "error",
        &json!({
            "type":"error",
            "error":{
                "message":"upstream rejected the request",
                "code":"provider_overloaded",
                "api_key":"must-not-leak"
            }
        }),
    );
    assert!(error.contains("upstream rejected the request"));
    assert!(error.contains("provider_overloaded"));
    assert!(!error.contains("must-not-leak"));
    assert!(
        sanitize_stream_error_detail(&"x".repeat(MAX_STREAM_ERROR_CHARS + 100)).len()
            <= MAX_STREAM_ERROR_CHARS
    );
}

async fn run_stream_translation_test_for(
    upstream_protocol: UpstreamProtocol,
    client_protocol: Protocol,
    body: &str,
) -> (String, StreamOutcome) {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("stream test listener");
    let address = listener.local_addr().expect("stream test address");
    let body = body.to_owned();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("stream test request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket
            .write_all(headers.as_bytes())
            .await
            .expect("stream test headers");
        socket
            .write_all(body.as_bytes())
            .await
            .expect("stream test body");
    });

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("stream test upstream response");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation(
        upstream,
        StreamTranslationConfig {
            upstream_protocol,
            client_protocol,
            request_id: "stream-test".to_owned(),
            model: "cmc/model".to_owned(),
            idle_timeout: Duration::from_secs(1),
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated stream body");
    server.await.expect("stream test server");
    (
        String::from_utf8(translated.to_vec()).expect("translated SSE text"),
        outcome,
    )
}

async fn run_stream_translation_test(
    body: &str,
    continuity_enabled: bool,
    idle_timeout: Duration,
    overall_timeout: Option<Duration>,
    delay_before_body: Option<Duration>,
) -> (String, StreamOutcome) {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("Responses stream test listener");
    let address = listener
        .local_addr()
        .expect("Responses stream test address");
    let body = body.to_owned();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener
            .accept()
            .await
            .expect("Responses stream test request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket
            .write_all(headers.as_bytes())
            .await
            .expect("Responses stream test headers");
        if let Some(delay) = delay_before_body {
            tokio::time::sleep(delay).await;
        }
        socket
            .write_all(body.as_bytes())
            .await
            .expect("Responses stream test body");
    });

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("Responses stream test upstream response");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation(
        upstream,
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::Responses,
            client_protocol: Protocol::Responses,
            request_id: "responses-stream-test".to_owned(),
            model: "cmc/model".to_owned(),
            idle_timeout,
            continuity_enabled,
            overall_timeout,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("translated Responses stream body");
    server.await.expect("Responses stream test server");
    (
        String::from_utf8(translated.to_vec()).expect("translated Responses SSE text"),
        outcome,
    )
}

#[tokio::test]
async fn responses_done_and_incomplete_without_final_delimiter_are_terminal() {
    let done = concat!(
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"done\"}\n\n",
        "event: response.done\n",
        "data: {\"type\":\"response.done\",\"response\":{\"id\":\"resp-1\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"cmc/model\",\"output\":[{\"id\":\"msg-1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"done\",\"annotations\":[]}]}]}}"
    );
    let (done_body, done_outcome) =
        run_stream_translation_test(done, false, Duration::from_secs(1), None, None).await;
    assert!(
        done_body.contains("\"status\":\"completed\""),
        "{done_body}"
    );
    assert!(!done_body.contains("upstream_error"), "{done_body}");
    assert!(done_outcome.completed());

    let incomplete = concat!(
        "event: response.incomplete\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp-2\",\"object\":\"response\",\"status\":\"incomplete\",\"model\":\"cmc/model\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"output\":[{\"id\":\"msg-2\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"partial\",\"annotations\":[]}]}]}}"
    );
    let (incomplete_body, incomplete_outcome) =
        run_stream_translation_test(incomplete, false, Duration::from_secs(1), None, None).await;
    assert!(
        incomplete_body.contains("\"status\":\"incomplete\""),
        "{incomplete_body}"
    );
    assert!(
        !incomplete_body.contains("upstream_error"),
        "{incomplete_body}"
    );
    assert!(incomplete_outcome.completed());
}

#[tokio::test]
async fn responses_function_call_stream_is_reconstructed_when_terminal_output_is_empty() {
    let body = concat!(
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"status\":\"in_progress\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"output_index\":0,\"delta\":\"{\\\"query\\\":\\\"status\\\"}\"}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\",\"output_index\":0,\"arguments\":\"{\\\"query\\\":\\\"status\\\"}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"status\":\"completed\",\"call_id\":\"call_1\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"cmc/model\",\"output\":[]}}\n\n",
    );
    let (translated, outcome) =
        run_stream_translation_test(body, false, Duration::from_secs(1), None, None).await;
    assert!(
        translated.contains("response.function_call_arguments.delta"),
        "{translated}"
    );
    assert!(
        translated.contains("\"arguments\":\"{\\\"query\\\":\\\"status\\\"}\""),
        "{translated}"
    );
    assert!(
        translated.contains("\"finish_reason\":\"tool_calls\""),
        "{translated}"
    );
    assert!(!translated.contains("upstream_error"), "{translated}");
    assert!(outcome.completed());
}

#[tokio::test]
async fn continuity_timeout_is_not_hidden_by_local_heartbeats() {
    let (body, outcome) = run_stream_translation_test(
        "event: response.created\ndata: {\"type\":\"response.created\"}\n\n",
        true,
        Duration::from_secs(1),
        Some(Duration::from_millis(50)),
        Some(Duration::from_millis(200)),
    )
    .await;
    assert!(
        body.contains("stream continuity exceeded its overall deadline"),
        "{body}"
    );
    assert!(body.contains("upstream_error"), "{body}");
    assert!(outcome.failed());
    assert!(!outcome.completed());
}

#[tokio::test]
async fn chat_tool_call_stream_translates_to_responses_client() {
    let body = concat!(
        "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"exec\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c3\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"cmd\\\":\\\"pwd\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c4\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let (translated, outcome) = run_stream_translation_test_for(
        UpstreamProtocol::ChatCompletions,
        Protocol::Responses,
        body,
    )
    .await;
    assert!(outcome.completed(), "{translated}");
    assert!(!translated.contains("upstream_error"), "{translated}");
    assert!(
        translated.contains("response.output_item.added"),
        "{translated}"
    );
    assert!(translated.contains("\"name\":\"exec\""), "{translated}");
    assert!(translated.contains("\"function_call\""), "{translated}");
    assert!(
        translated.contains("response.function_call_arguments.delta"),
        "{translated}"
    );
    assert!(
        translated.contains("\"finish_reason\":\"tool_calls\""),
        "{translated}"
    );
    let completed = translated
        .split("event: response.completed")
        .nth(1)
        .expect("completed event");
    assert!(
        completed.contains("\"type\":\"function_call\""),
        "{translated}"
    );
}

#[tokio::test]
async fn chat_tool_call_stream_translates_to_messages_client() {
    let body = concat!(
        "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"exec\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c2\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"cmd\\\":\\\"pwd\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c3\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let (translated, outcome) = run_stream_translation_test_for(
        UpstreamProtocol::ChatCompletions,
        Protocol::Messages,
        body,
    )
    .await;
    assert!(outcome.completed(), "{translated}");
    assert!(!translated.contains("upstream_error"), "{translated}");
    assert!(translated.contains("content_block_start"), "{translated}");
    assert!(translated.contains("\"name\":\"exec\""), "{translated}");
    assert!(translated.contains("input_json_delta"), "{translated}");
    assert!(
        translated.contains("\"stop_reason\":\"tool_use\""),
        "{translated}"
    );
    assert!(
        translated.ends_with("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"),
        "{translated}"
    );
}

/// Streams `body` over a real TCP upstream and then holds the connection
/// open (without closing) for `hold_after_body` so the reader observes a
/// transport idle timeout after the sent frames, or closes cleanly when
/// `None`. Returns the translated client SSE text and the stream outcome.
async fn run_truncated_chat_stream_test(
    body: &str,
    hold_after_body: Option<Duration>,
    idle_timeout: Duration,
) -> (String, StreamOutcome) {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("truncated stream test listener");
    let address = listener
        .local_addr()
        .expect("truncated stream test address");
    let body = body.to_owned();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener
            .accept()
            .await
            .expect("truncated stream test request");
        let mut request = [0_u8; 1024];
        let _ = socket.read(&mut request).await;
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket
            .write_all(headers.as_bytes())
            .await
            .expect("truncated stream test headers");
        socket
            .write_all(body.as_bytes())
            .await
            .expect("truncated stream test body");
        if let Some(hold) = hold_after_body {
            // Keep the socket open and silent so the gateway reader hits its
            // idle timeout while the connection is still established.
            tokio::time::sleep(hold).await;
        }
    });

    let upstream = reqwest::Client::new()
        .get(format!("http://{address}/stream"))
        .send()
        .await
        .expect("truncated stream test upstream response");
    let (_shutdown_sender, shutdown) = watch::channel(false);
    let outcome = StreamOutcome::default();
    let response = stream_translation(
        upstream,
        StreamTranslationConfig {
            upstream_protocol: UpstreamProtocol::ChatCompletions,
            client_protocol: Protocol::ChatCompletions,
            request_id: "truncated-stream-test".to_owned(),
            model: "command-code/model".to_owned(),
            idle_timeout,
            continuity_enabled: false,
            overall_timeout: None,
            outcome: outcome.clone(),
            analytics: None,
            resource_limits: GatewayResourceLimits::default(),
            shutdown,
            log: None,
        },
    );
    let translated = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("truncated stream test body");
    server.await.expect("truncated stream test server");
    (
        String::from_utf8(translated.to_vec()).expect("truncated stream test SSE text"),
        outcome,
    )
}

/// A transport idle timeout after the upstream finish signal means the
/// response is already complete; the stalled connection must not inject an
/// error event into the client stream or mark the outcome failed.
#[tokio::test]
async fn chat_stream_error_after_finish_signal_completes_cleanly() {
    let body = concat!(
        "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    );
    let (translated, outcome) = run_truncated_chat_stream_test(
        body,
        Some(Duration::from_millis(600)),
        Duration::from_millis(150),
    )
    .await;
    assert!(
        translated.contains("\"finish_reason\":\"stop\""),
        "{translated}"
    );
    assert!(!translated.contains("upstream_error"), "{translated}");
    assert!(translated.ends_with("data: [DONE]\n\n"), "{translated}");
    assert!(outcome.completed());
    assert!(!outcome.failed());
}

/// A genuine mid-stream failure must still surface the error event, but the
/// Chat Completions terminal sentinel must follow so client SSE parsers end
/// on a deliberate terminator instead of a dropped connection mid-frame.
#[tokio::test]
async fn chat_stream_mid_stream_error_still_yields_done_sentinel() {
    let body = "data: {\"id\":\"c1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n";
    let (translated, outcome) = run_truncated_chat_stream_test(
        body,
        Some(Duration::from_millis(600)),
        Duration::from_millis(150),
    )
    .await;
    assert!(
        translated.contains("upstream stream ended before a terminal success event"),
        "{translated}"
    );
    assert!(
        translated.ends_with("data: [DONE]\n\n"),
        "chat sentinel must terminate the failed stream: {translated}"
    );
    assert!(outcome.failed());
    assert!(!outcome.completed());
}
