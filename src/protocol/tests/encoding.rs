use super::*;

#[test]
fn chat_request_round_trips_through_canonical_messages() {
    let input = json!({"model":"alias","stream":false,"messages":[{"role":"system","content":"be concise"},{"role":"user","content":[{"type":"text","text":"hello"}]}],"max_tokens":42});
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    assert_eq!(canonical.messages.len(), 2);
    let encoded = encode_request(Protocol::ChatCompletions, &canonical, "real-model")
        .expect("chat request encodes");
    assert_eq!(encoded["model"], "real-model");
    assert_eq!(encoded["messages"][1]["content"], "hello");
}

#[test]
fn system_prompt_survives_encoding_for_every_fallback_target_protocol() {
    // The bug report: a combo falls back across models on different upstream
    // protocols, and each target encodes the same canonical request. A system
    // prompt must reach every target, not just the first one.
    let input = json!({
        "model":"alias",
        "messages":[
            {"role":"system","content":"be concise"},
            {"role":"user","content":"hello"}
        ]
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");

    for protocol in UpstreamProtocol::all() {
        let encoded = encode_upstream_request(*protocol, &canonical, "target-model")
            .unwrap_or_else(|error| panic!("{protocol:?} fallback target encodes: {error}"));
        let wire = encoded.to_string();
        assert!(
            wire.contains("be concise"),
            "{protocol:?} fallback target lost the system prompt: {wire}"
        );
    }
}

#[test]
fn system_prompt_survives_repeated_encoding_between_fallbacks() {
    // The bug report: the same canonical request is encoded once per fallback
    // target. Encoding must never consume or mutate the request, or the
    // second (fallback) target silently loses the system prompt.
    let input = json!({
        "model":"alias",
        "messages":[
            {"role":"system","content":"be concise"},
            {"role":"user","content":"hello"}
        ]
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    for attempt in 0..2 {
        let encoded =
            encode_upstream_request(UpstreamProtocol::Messages, &canonical, "target-model")
                .unwrap_or_else(|error| panic!("fallback attempt {attempt} encodes: {error}"));
        let system = encoded["system"].as_str().expect("Anthropic system string");
        assert_eq!(system, "be concise");
        let messages = canonical.messages.len();
        assert_eq!(messages, 2, "encoding must not drop canonical messages");
    }
    assert_eq!(canonical.messages.len(), 2);
}
