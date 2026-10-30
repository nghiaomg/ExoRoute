use super::*;

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
fn stream_completion_events_preserve_provider_cost_for_each_client_protocol() {
    let cost = Some(125_000);
    let responses = encode_stream_finish(
        Protocol::Responses,
        "response-1",
        "cl/model",
        "stop",
        Some((4, 6, 0, 0)),
        cost,
        None,
    );
    assert!(responses.contains("\"cost\":0.125"));

    let messages = messages_stream_finish("stop", 4, 6, 0, 0, cost, &[]);
    assert!(messages.contains("\"cost\":0.125"));

    let chat = encode_stream_usage(4, 6, 0, 0, cost);
    assert!(chat.contains("\"cost\":0.125"));
}

#[test]
fn stream_usage_events_report_cache_fields_for_each_client_protocol() {
    // Chat Completions keeps the cached subset inside the prompt total: the
    // recorded cache denominator (6) covers the reported input count (4).
    let chat = encode_stream_usage(4, 6, 2, 6, None);
    assert!(chat.contains("\"prompt_tokens\":6"), "{chat}");
    assert!(
        chat.contains("\"prompt_tokens_details\":{\"cached_tokens\":2}"),
        "{chat}"
    );
    assert!(chat.contains("\"total_tokens\":12"), "{chat}");

    // Responses mirrors the Chat shape through input_tokens_details.
    let responses = encode_stream_finish(
        Protocol::Responses,
        "response-1",
        "cl/model",
        "stop",
        Some((4, 6, 2, 6)),
        None,
        None,
    );
    assert!(responses.contains("\"input_tokens\":6"), "{responses}");
    assert!(
        responses.contains("\"input_tokens_details\":{\"cached_tokens\":2}"),
        "{responses}"
    );

    // Messages reports the uncached remainder beside the cache reads, so
    // input_tokens + cache_read_input_tokens rebuilds the 9-token total.
    let messages = messages_stream_finish("stop", 2, 3, 3, 9, None, &[]);
    assert!(messages.contains("\"input_tokens\":6"), "{messages}");
    assert!(
        messages.contains("\"cache_read_input_tokens\":3"),
        "{messages}"
    );

    let start = messages_stream_start("msg-1", "m", 2, 3, 9);
    assert!(start.contains("\"cache_read_input_tokens\":3"), "{start}");
    assert!(start.contains("\"input_tokens\":6"), "{start}");

    let google = google_messages_stream_finish("stop", 2, 3, 3, 11, None, false);
    assert!(google.contains("\"input_tokens\":8"), "{google}");
    assert!(google.contains("\"cache_read_input_tokens\":3"), "{google}");
}
