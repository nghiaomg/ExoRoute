use super::*;

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
