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
