use super::support::*;
use super::*;

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
    let (address, server) = spawn_mock_http_server(vec![MockResponse::sse(body)]).await;

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
