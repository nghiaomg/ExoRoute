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
