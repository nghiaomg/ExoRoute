use super::*;

#[test]
fn unsupported_chat_options_are_named_when_translating_to_responses() {
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "service_tier":" flex"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let error = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect_err("unsupported options must not be silently discarded");

    assert!(error.contains("service_tier"));
}

#[test]
fn chat_only_options_are_dropped_when_translating_to_responses() {
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "n":3,
        "seed":7,
        "user":"u-1",
        "metadata":{"trace":"abc"},
        "logprobs":true,
        "top_logprobs":5,
        "logit_bias":{ "50256": -100 },
        "frequency_penalty":0.5,
        "presence_penalty":-0.5,
        "safety_identifier":"sid-1",
        "stream_options":{"include_usage":true}
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect("chat-only options are dropped instead of failing translation");
    for field in [
        "n",
        "seed",
        "user",
        "metadata",
        "logprobs",
        "top_logprobs",
        "logit_bias",
        "frequency_penalty",
        "presence_penalty",
        "safety_identifier",
        "stream_options",
    ] {
        assert!(
            encoded.get(field).is_none(),
            "{field} must not reach Responses"
        );
    }
}

#[test]
fn chat_store_and_parallel_tool_calls_translate_to_responses() {
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "store":false,
        "parallel_tool_calls":true
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect("Responses-native options translate to Responses");
    assert_eq!(encoded["store"], json!(false));
    assert_eq!(encoded["parallel_tool_calls"], json!(true));

    let nulls = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "store":null,
        "parallel_tool_calls":null
    });
    let canonical = decode_request(Protocol::ChatCompletions, &nulls).expect("null options parse");
    let encoded = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect("null-valued options are omitted, not forwarded");
    assert!(encoded.get("store").is_none());
    assert!(encoded.get("parallel_tool_calls").is_none());

    let invalid = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "store":"false"
    });
    let canonical =
        decode_request(Protocol::ChatCompletions, &invalid).expect("string store parses");
    let error = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect_err("a non-boolean store must be reported");
    assert!(error.contains("store"));
}

#[test]
fn prompt_cache_key_forwards_between_openai_family_protocols() {
    // The bug report: prompt_cache_key is a legitimate Chat Completions
    // option (a cache-routing hint, not a credential) but the sensitive
    // substring guard rejected it before it could reach a command-code
    // provider. Same-protocol encoding must forward it unchanged.
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "prompt_cache_key":"conv-123"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let encoded = encode_request(Protocol::ChatCompletions, &canonical, "model-a")
        .expect("prompt_cache_key forwards between Chat Completions requests");
    assert_eq!(encoded["prompt_cache_key"], json!("conv-123"));

    // Responses uses the same field name and semantics, so translation
    // forwards it. This also lets the codex adapter keep the client's cache
    // key instead of replacing it with the gateway request id.
    let encoded = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect("prompt_cache_key translates to Responses");
    assert_eq!(encoded["prompt_cache_key"], json!("conv-123"));

    // Explicit null means unset on the Responses path.
    let null_key = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "prompt_cache_key":null
    });
    let canonical = decode_request(Protocol::ChatCompletions, &null_key).expect("null key parses");
    let encoded = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect("null-valued option is omitted, not forwarded");
    assert!(encoded.get("prompt_cache_key").is_none());

    // Validation still applies: empty, non-ASCII, overlong, and non-string
    // values are reported instead of silently forwarded.
    for (label, key_json, expected) in [
        ("empty", json!("   "), "non-empty"),
        ("non-ascii", json!("khóa-123"), "ASCII"),
        ("overlong", json!("x".repeat(257)), "256"),
        ("non-string", json!(12345), "string"),
    ] {
        let input = json!({
            "model":"alias",
            "messages":[{"role":"user","content":"hello"}],
            "prompt_cache_key":key_json
        });
        let canonical =
            decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
        let error = encode_request(Protocol::Responses, &canonical, "reasoning-model")
            .expect_err(&format!("{label} prompt_cache_key must be reported"));
        assert!(
            error.contains(expected) && error.contains("prompt_cache_key"),
            "{label} error must name the option and the problem: {error}"
        );
    }
}

#[test]
fn sensitive_option_names_are_still_rejected_between_chat_requests() {
    // The safety guard covers everything outside the exact-name allowlist:
    // a look-alike name must not slip through just because its prefix looks
    // like a known option.
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "prompt_cache_keys":"conv-123"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let error = encode_request(Protocol::ChatCompletions, &canonical, "model-a")
        .expect_err("look-alike sensitive names stay guarded");
    assert!(error.contains("cannot be forwarded safely"));

    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "api_key":"sk-test"
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let error = encode_request(Protocol::ChatCompletions, &canonical, "model-a")
        .expect_err("credential-like options stay guarded");
    assert!(error.contains("cannot be forwarded safely"));

    // Unrelated unknown options keep their existing behavior: unknown on the
    // Responses translation path, forwarded on the same-protocol path.
    let input = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "logit_biasx":{"50256":-100}
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat request");
    let error = encode_request(Protocol::Responses, &canonical, "reasoning-model")
        .expect_err("unknown options stay unsupported on Responses");
    assert!(error.contains("logit_biasx"));
    let encoded = encode_request(Protocol::ChatCompletions, &canonical, "model-a")
        .expect("unknown non-sensitive options still forward between chat requests");
    assert_eq!(encoded["logit_biasx"], json!({"50256":-100}));
}

#[test]
fn chat_response_format_translates_to_responses_text_format() {
    let json_object = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "response_format":{"type":"json_object"},
        "verbosity":"low"
    });
    let json_object = decode_request(Protocol::ChatCompletions, &json_object)
        .expect("JSON object request parses");
    let json_object = encode_request(Protocol::Responses, &json_object, "gpt-5.6-luna")
        .expect("JSON object format translates");
    assert_eq!(
        json_object["text"],
        json!({
            "verbosity":"low",
            "format":{"type":"json_object"}
        })
    );

    let json_schema = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "response_format":{
            "type":"json_schema",
            "json_schema":{
                "name":"answer",
                "description":"A structured answer",
                "schema":{"type":"object","properties":{"answer":{"type":"string"}}},
                "strict":true
            }
        }
    });
    let json_schema = decode_request(Protocol::ChatCompletions, &json_schema)
        .expect("JSON schema request parses");
    let json_schema = encode_request(Protocol::Responses, &json_schema, "gpt-5.6-luna")
        .expect("JSON schema format translates");
    assert_eq!(
        json_schema["text"]["format"],
        json!({
            "type":"json_schema",
            "name":"answer",
            "description":"A structured answer",
            "schema":{"type":"object","properties":{"answer":{"type":"string"}}},
            "strict":true
        })
    );
}

#[test]
fn invalid_chat_response_format_is_rejected_during_responses_translation() {
    let missing_schema = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "response_format":{
            "type":"json_schema",
            "json_schema":{"name":"answer"}
        }
    });
    let missing_schema = decode_request(Protocol::ChatCompletions, &missing_schema)
        .expect("invalid schema request still decodes before translation");
    let error = encode_request(Protocol::Responses, &missing_schema, "gpt-5.6-luna")
        .expect_err("missing schema must be rejected");
    assert!(error.contains("response_format.json_schema.schema"));

    let unsupported_type = json!({
        "model":"alias",
        "messages":[{"role":"user","content":"hello"}],
        "response_format":{"type":"xml"}
    });
    let unsupported_type = decode_request(Protocol::ChatCompletions, &unsupported_type)
        .expect("unsupported format request decodes before translation");
    let error = encode_request(Protocol::Responses, &unsupported_type, "gpt-5.6-luna")
        .expect_err("unsupported format must be rejected");
    assert!(error.contains("response_format.type"));
}
