use super::support::*;
use super::*;

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
