use super::*;

#[test]
fn chat_reasoning_effort_translates_to_responses() {
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "reasoning_effort":"high",
        "verbosity":"low"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect("reasoning effort translates to Responses");

    assert_eq!(encoded["reasoning"], json!({"effort":"high"}));
    assert_eq!(encoded["text"], json!({"verbosity":"low"}));
}

#[test]
fn chat_responses_normalizes_omniroute_reasoning_options() {
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "effort":"low",
        "reasoning_effort":"medium",
        "reasoning":{"effort":"high","summary":"auto"},
        "thinking":{"type":"enabled","budget_tokens":1024},
        "enable_thinking":true,
        "stream_options":{"include_usage":true}
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect("reasoning options translate to Responses");

    // Explicit Responses-shaped reasoning has priority over every alias,
    // matching OmniRoute's request translator.
    assert_eq!(
        encoded["reasoning"],
        json!({"effort":"high","summary":"auto"})
    );
    assert!(encoded.get("stream_options").is_none());
    assert!(encoded.get("thinking").is_none());
    assert!(encoded.get("enable_thinking").is_none());
}

#[test]
fn chat_responses_maps_thinking_aliases_without_forwarding_them() {
    let disabled = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "thinking":false,
        "stream_options":{"include_usage":true}
    });
    let disabled = decode_request(Protocol::ChatCompletions, &disabled)
        .expect("disabled thinking request parses");
    let disabled = encode_request(Protocol::Responses, &disabled, "reasoning-model")
        .expect("disabled thinking translates");
    assert_eq!(disabled["reasoning"], json!({"effort":"none"}));
    assert!(disabled.get("stream_options").is_none());

    let budget = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "thinking":{"type":"enabled","budget_tokens":10240}
    });
    let budget =
        decode_request(Protocol::ChatCompletions, &budget).expect("budget thinking request parses");
    let budget = encode_request(Protocol::Responses, &budget, "reasoning-model")
        .expect("budget thinking translates");
    assert_eq!(budget["reasoning"], json!({"effort":"medium"}));
}

#[test]
fn chat_responses_accepts_codex_reasoning_effort_values() {
    // The Codex model tier rejects "minimal" upstream with an invalid_request
    // error, so the closest supported effort is sent instead.
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "reasoning":{"effort":"minimal","summary":"auto"}
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::Responses, &canonical, "gpt-5.6-luna")
        .expect("minimal effort degrades instead of failing upstream");
    assert_eq!(
        encoded["reasoning"],
        json!({"effort":"low","summary":"auto"})
    );

    // Models outside the Codex tier accept "minimal" natively, so it forwards.
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "reasoning_effort":"minimal"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::Responses, &canonical, "gpt-5.5")
        .expect("non-Codex models forward minimal unchanged");
    assert_eq!(encoded["reasoning"], json!({"effort":"minimal"}));

    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "reasoning_effort":"max"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::Responses, &canonical, "gpt-5.5")
        .expect("unsupported native max translates to xhigh");
    assert_eq!(encoded["reasoning"], json!({"effort":"xhigh"}));

    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "reasoning_effort":"ultra"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::Responses, &canonical, "gpt-5.6-luna")
        .expect("Codex ultra alias translates to native max");
    assert_eq!(encoded["reasoning"], json!({"effort":"max"}));

    // Variant suffixes still resolve to the base model for effort support.
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "reasoning_effort":"minimal"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::Responses, &canonical, "gpt-5.6-luna-high")
        .expect("variant suffix keeps the Codex-tier minimal rejection");
    assert_eq!(encoded["reasoning"], json!({"effort":"low"}));
}

#[test]
fn conflicting_thinking_aliases_are_rejected() {
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "thinking":true,
        "enable_thinking":false
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let error = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect_err("conflicting thinking aliases must be rejected");
    assert!(error.contains("thinking") && error.contains("enable_thinking"));
}

#[test]
fn thinking_settings_are_strict_and_bounded() {
    assert!(matches!(
        parse_thinking_handling("preserve", None),
        Ok(ThinkingHandling::Preserve)
    ));
    assert!(parse_thinking_handling("override", None).is_err());
    assert!(parse_thinking_handling("remove", Some("extra")).is_err());
    assert!(parse_thinking_handling("override", Some(" \n ")).is_err());
    assert!(
        parse_thinking_handling(
            "override",
            Some(&"x".repeat(MAX_THINKING_OVERRIDE_BYTES + 1))
        )
        .is_err()
    );
    assert!(parse_thinking_handling("preserve ", None).is_err());
}

#[test]
fn thinking_policy_translates_for_responses_and_google() {
    let input = json!({
        "model":"claude-test",
        "messages":[
            {"role":"assistant","content":[{"type":"thinking","thinking":"original","signature":"sig"}]},
            {"role":"user","content":"continue"}
        ]
    });
    let request = decode_request(Protocol::Messages, &input).expect("thinking request parses");

    let responses = encode_upstream_request_with_thinking(
        UpstreamProtocol::Responses,
        &request,
        "reasoning-model",
        ThinkingHandling::Override("replacement"),
    )
    .expect("Responses request translates thinking");
    assert_eq!(
        responses["input"][0]["content"][0],
        json!({"type":"output_text","text":"replacement"})
    );

    let google = encode_upstream_request_with_thinking(
        UpstreamProtocol::GoogleGenerateContent,
        &request,
        "gemini-test",
        ThinkingHandling::Preserve,
    )
    .expect("Google request translates thinking");
    assert_eq!(google["contents"][0]["parts"][0]["text"], "original");

    let google_removed = encode_upstream_request_with_thinking(
        UpstreamProtocol::GoogleGenerateContent,
        &request,
        "gemini-test",
        ThinkingHandling::Remove,
    )
    .expect("Google request removes thinking");
    assert_eq!(google_removed["contents"].as_array().map(Vec::len), Some(1));
    assert_eq!(google_removed["contents"][0]["role"], "user");
}
