use super::*;

#[test]
fn response_usage_extracts_cached_tokens_for_supported_protocol_shapes() {
    let chat = decode_response(
            Protocol::ChatCompletions,
            &json!({
                "choices":[{"message":{"content":"ok"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":10,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":6}}
            }),
            "m",
        )
        .expect("chat response");
    assert_eq!(
        chat.usage.as_ref().map(|usage| usage.cached_tokens),
        Some(6)
    );
    assert_eq!(
        chat.usage.as_ref().map(|usage| usage.cache_input_tokens),
        Some(10)
    );

    let responses = decode_response(
        Protocol::Responses,
        &json!({
            "status":"completed",
            "output":[{"type":"message","content":[{"type":"output_text","text":"ok"}]}],
            "usage":{"input_tokens":10,"output_tokens":2,"input_tokens_details":{"cached_tokens":6}}
        }),
        "m",
    )
    .expect("Responses response");
    assert_eq!(
        responses.usage.as_ref().map(|usage| usage.cached_tokens),
        Some(6)
    );

    let messages = decode_response(
            Protocol::Messages,
            &json!({
                "content":[{"type":"text","text":"ok"}],
                "stop_reason":"end_turn",
                "usage":{"input_tokens":2,"cache_read_input_tokens":3,"cache_creation_input_tokens":4,"output_tokens":2}
            }),
            "m",
        )
        .expect("Messages response");
    assert_eq!(
        messages.usage.as_ref().map(|usage| usage.cached_tokens),
        Some(3)
    );
    assert_eq!(
        messages
            .usage
            .as_ref()
            .map(|usage| usage.cache_input_tokens),
        Some(9)
    );
}

#[test]
fn usage_cost_is_rounded_to_micro_usd_and_encoded_for_each_protocol() {
    let response = decode_response(
            Protocol::ChatCompletions,
            &json!({
                "id": "cost-1",
                "model": "m",
                "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 2, "completion_tokens": 1, "cost": 0.1234567}
            }),
            "m",
        )
        .expect("cost response");
    assert_eq!(
        response
            .usage
            .as_ref()
            .and_then(|usage| usage.cost_micro_usd),
        Some(123_457)
    );

    for protocol in [
        Protocol::ChatCompletions,
        Protocol::Responses,
        Protocol::Messages,
    ] {
        let encoded = encode_response(protocol, &response);
        let usage = encoded.get("usage").expect("encoded usage");
        assert_eq!(
            usage.get("cost").and_then(parse_cost_micro_usd),
            Some(123_457)
        );
    }
    assert_eq!(parse_cost_micro_usd(&json!(-1.0)), None);
    assert_eq!(parse_cost_micro_usd(&json!("0.1")), None);
    assert_eq!(parse_cost_micro_usd(&Value::Null), None);
}

#[test]
fn encoded_usage_reports_cache_fields_for_each_client_protocol() {
    // OpenAI Chat upstream: the cached subset is already inside prompt_tokens,
    // so the encoded totals match the upstream counts unchanged.
    let chat = decode_response(
            Protocol::ChatCompletions,
            &json!({
                "id": "cache-1",
                "model": "m",
                "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 2, "prompt_tokens_details": {"cached_tokens": 6}}
            }),
            "m",
        )
        .expect("chat response");
    let encoded = encode_response(Protocol::ChatCompletions, &chat);
    let usage = encoded.get("usage").expect("encoded usage");
    assert_eq!(usage.get("prompt_tokens"), Some(&json!(10)));
    assert_eq!(usage.get("total_tokens"), Some(&json!(12)));
    assert_eq!(
        usage.pointer("/prompt_tokens_details/cached_tokens"),
        Some(&json!(6))
    );

    // The same totals split into an uncached remainder plus cache reads for
    // Messages clients: 4 uncached + 6 cached rebuild the 10-token prompt.
    let as_messages = encode_response(Protocol::Messages, &chat);
    let usage = as_messages.get("usage").expect("encoded usage");
    assert_eq!(usage.get("input_tokens"), Some(&json!(4)));
    assert_eq!(usage.get("cache_read_input_tokens"), Some(&json!(6)));

    // Anthropic upstream: input_tokens excludes cache reads and creation, so
    // the encoded Chat prompt total becomes input + read + creation while the
    // Messages encoding folds creation into the uncached remainder and both
    // shapes sum back to the same 9-token total.
    let messages = decode_response(
            Protocol::Messages,
            &json!({
                "content": [{"type": "text", "text": "ok"}],
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 2, "cache_read_input_tokens": 3, "cache_creation_input_tokens": 4, "output_tokens": 2}
            }),
            "m",
        )
        .expect("Messages response");
    let as_chat = encode_response(Protocol::ChatCompletions, &messages);
    let usage = as_chat.get("usage").expect("encoded usage");
    assert_eq!(usage.get("prompt_tokens"), Some(&json!(9)));
    assert_eq!(usage.get("total_tokens"), Some(&json!(11)));
    assert_eq!(
        usage.pointer("/prompt_tokens_details/cached_tokens"),
        Some(&json!(3))
    );

    let as_messages = encode_response(Protocol::Messages, &messages);
    let usage = as_messages.get("usage").expect("encoded usage");
    assert_eq!(usage.get("input_tokens"), Some(&json!(6)));
    assert_eq!(usage.get("cache_read_input_tokens"), Some(&json!(3)));
}
