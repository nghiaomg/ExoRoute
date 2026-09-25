use super::*;

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
