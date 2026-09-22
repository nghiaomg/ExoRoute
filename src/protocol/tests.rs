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
fn responses_history_uses_role_compatible_content_types() {
    let input = json!({
        "model":"alias",
        "messages":[
            {"role":"user","content":"first question"},
            {"role":"assistant","content":"first answer"},
            {"role":"user","content":"follow-up"}
        ]
    });
    let canonical = decode_request(Protocol::ChatCompletions, &input).expect("valid chat history");
    let encoded = encode_request(Protocol::Responses, &canonical, "gpt-5.5")
        .expect("chat history translates to Responses");

    assert_eq!(encoded["input"][0]["role"], "user");
    assert_eq!(encoded["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(encoded["input"][1]["role"], "assistant");
    assert_eq!(encoded["input"][1]["content"][0]["type"], "output_text");
    assert_eq!(encoded["input"][2]["role"], "user");
    assert_eq!(encoded["input"][2]["content"][0]["type"], "input_text");
}

#[test]
fn responses_history_separates_reasoning_and_sanitizes_assistant_media() {
    let request = CanonicalRequest {
        source_protocol: Protocol::ChatCompletions,
        model: "alias".to_owned(),
        messages: vec![
            Message {
                role: Role::Assistant,
                content: vec![
                    ContentBlock::Reasoning {
                        text: "earlier reasoning".to_owned(),
                    },
                    ContentBlock::Image {
                        url: "https://images.test/earlier.png".to_owned(),
                        detail: Some("high".to_owned()),
                    },
                    ContentBlock::Document {
                        source: DocumentSource::Url {
                            url: "https://files.test/earlier.pdf".to_owned(),
                        },
                        filename: Some("earlier.pdf".to_owned()),
                    },
                ],
                name: None,
            },
            Message {
                role: Role::User,
                content: vec![ContentBlock::Text {
                    text: "continue".to_owned(),
                }],
                name: None,
            },
        ],
        tools: Vec::new(),
        tool_choice: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stop: Vec::new(),
        stream: false,
        metadata: BTreeMap::new(),
        output_styles_applied: false,
    };
    let encoded = encode_request(Protocol::Responses, &request, "gpt-5.5")
        .expect("assistant history translates to valid Responses items");

    // Responses input reasoning items cannot carry a content array (the upstream
    // rejects it with "expected maximum length 0"), so replayed assistant
    // reasoning text is dropped and the turn replays through text items only.
    let input = encoded["input"].as_array().expect("input array");
    assert!(
        input
            .iter()
            .all(|item| item.get("type").and_then(Value::as_str) != Some("reasoning")),
        "no reasoning items may be emitted into Responses input: {input:?}"
    );
    assert_eq!(encoded["input"][0]["role"], "assistant");
    assert_eq!(encoded["input"][0]["content"][0]["type"], "output_text");
    assert_eq!(encoded["input"][0]["content"][1]["type"], "output_text");
    assert_eq!(encoded["input"][1]["role"], "user");
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

#[test]
fn thinking_blocks_preserve_signatures_and_follow_provider_handling() {
    let input = json!({
        "model":"claude-test",
        "max_tokens":64,
        "messages":[
            {"role":"assistant","content":[
                {"type":"thinking","thinking":"original reasoning","signature":"signed-original","provider_extra":{"opaque":true}},
                {"type":"redacted_thinking","data":"opaque-signature"},
                {"type":"text","text":"visible answer"}
            ]},
            {"role":"user","content":"continue"}
        ]
    });
    let request = decode_request(Protocol::Messages, &input).expect("thinking input parses");

    let preserved = encode_upstream_request_with_thinking(
        UpstreamProtocol::Messages,
        &request,
        "claude-test",
        ThinkingHandling::Preserve,
    )
    .expect("preserved Anthropic request encodes");
    assert_eq!(
        preserved["messages"][0]["content"][0],
        json!({"type":"thinking","thinking":"original reasoning","signature":"signed-original","provider_extra":{"opaque":true}})
    );
    assert_eq!(
        preserved["messages"][0]["content"][1],
        json!({"type":"redacted_thinking","data":"opaque-signature"})
    );

    let overridden = encode_upstream_request_with_thinking(
        UpstreamProtocol::Messages,
        &request,
        "claude-test",
        ThinkingHandling::Override("provider replacement"),
    )
    .expect("thinking override encodes");
    assert_eq!(
        overridden["messages"][0]["content"][0],
        json!({"type":"text","text":"provider replacement"})
    );
    assert_eq!(
        overridden["messages"][0]["content"][1],
        json!({"type":"text","text":"provider replacement"})
    );
    assert_eq!(
        overridden["messages"][0]["content"][0].get("signature"),
        None,
        "a changed thinking body must not carry a stale signature"
    );

    let removed = encode_upstream_request_with_thinking(
        UpstreamProtocol::Messages,
        &request,
        "claude-test",
        ThinkingHandling::Remove,
    )
    .expect("thinking removal encodes");
    assert_eq!(removed["messages"].as_array().map(Vec::len), Some(2));
    assert_eq!(removed["messages"][0]["role"], "assistant");
    assert_eq!(
        removed["messages"][0]["content"],
        json!([{"type":"text","text":"visible answer"}])
    );
    assert_eq!(removed["messages"][1]["role"], "user");

    let redacted_translation = encode_upstream_request_with_thinking(
        UpstreamProtocol::ChatCompletions,
        &request,
        "chat-model",
        ThinkingHandling::Preserve,
    );
    assert!(redacted_translation.is_err());

    let mut request_without_redacted = request.clone();
    request_without_redacted.messages[0]
        .content
        .retain(|block| !matches!(block, ContentBlock::RedactedThinking { .. }));
    let translated = encode_upstream_request_with_thinking(
        UpstreamProtocol::ChatCompletions,
        &request_without_redacted,
        "chat-model",
        ThinkingHandling::Preserve,
    )
    .expect("visible thinking text converts for Chat Completions");
    assert_eq!(
        translated["messages"][0]["content"][0]["text"],
        "original reasoning"
    );
    assert_eq!(
        translated["messages"][0]["content"][1]["text"],
        "visible answer"
    );
}

#[test]
fn anthropic_response_thinking_blocks_decode_and_reencode_without_signature_loss() {
    let response = json!({
        "id":"msg-thinking",
        "model":"claude-test",
        "content":[
            {"type":"thinking","thinking":"reasoning","signature":"response-signature","provider_extra":{"opaque":true}},
            {"type":"redacted_thinking","data":"redacted-data","provider_extra":"opaque"},
            {"type":"text","text":"answer"}
        ],
        "stop_reason":"end_turn",
        "usage":{"input_tokens":3,"output_tokens":4}
    });
    let decoded = decode_response(Protocol::Messages, &response, "claude-test")
        .expect("thinking response decodes");
    let encoded = encode_response(Protocol::Messages, &decoded);
    assert_eq!(encoded["content"][0], response["content"][0]);
    assert_eq!(encoded["content"][1], response["content"][1]);
    assert_eq!(encoded["content"][2], response["content"][2]);
}

#[test]
fn chat_reasoning_content_survives_canonical_round_trip() {
    let request = json!({
        "model":"deepseek-v4.1-flash",
        "messages":[
            {"role":"assistant","reasoning_content":"checked the tool result","content":"answer"},
            {"role":"user","content":"continue"}
        ]
    });
    let canonical = decode_request(Protocol::ChatCompletions, &request)
        .expect("Chat reasoning request decodes");
    assert!(matches!(
        canonical.messages[0].content.first(),
        Some(ContentBlock::Reasoning { text }) if text == "checked the tool result"
    ));
    let encoded = encode_request(Protocol::ChatCompletions, &canonical, "deepseek-v4.1-flash")
        .expect("Chat reasoning request re-encodes");
    assert_eq!(
        encoded["messages"][0]["reasoning_content"],
        "checked the tool result"
    );
    assert_eq!(encoded["messages"][0]["content"], "answer");

    let response = decode_response(
            Protocol::ChatCompletions,
            &json!({
                "id":"chat-reasoning",
                "model":"deepseek-v4.1-flash",
                "choices":[{
                    "message":{"role":"assistant","reasoning_content":"provider thought","content":"done"},
                    "finish_reason":"stop"
                }]
            }),
            "deepseek-v4.1-flash",
        )
        .expect("Chat reasoning response decodes");
    assert!(response_has_output(&response));
    let encoded_response = encode_response(Protocol::ChatCompletions, &response);
    assert_eq!(
        encoded_response["choices"][0]["message"]["reasoning_content"],
        "provider thought"
    );
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
fn thinking_rewrite_never_changes_arbitrary_tool_input_objects() {
    let spoofed_marker = json!({
        "type": INTERNAL_THINKING_BLOCK_TYPE,
        "kind": "thinking",
        "thinking": "must remain literal tool input"
    });
    let request = CanonicalRequest {
        source_protocol: Protocol::Messages,
        model: "claude-test".to_owned(),
        messages: vec![Message {
            role: Role::Assistant,
            content: vec![
                ContentBlock::Thinking {
                    thinking: "original".to_owned(),
                    signature: Some("signed".to_owned()),
                    extra: BTreeMap::new(),
                },
                ContentBlock::ToolCall {
                    id: "tool-1".to_owned(),
                    name: "run".to_owned(),
                    arguments: json!({"nested":spoofed_marker}),
                },
            ],
            name: None,
        }],
        tools: Vec::new(),
        tool_choice: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stop: Vec::new(),
        stream: false,
        metadata: BTreeMap::new(),
        output_styles_applied: false,
    };
    let encoded = encode_upstream_request_with_thinking(
        UpstreamProtocol::Messages,
        &request,
        "claude-test",
        ThinkingHandling::Remove,
    )
    .expect("remove thinking from a tool-use turn");

    assert_eq!(encoded["messages"][0]["content"][0]["type"], "tool_use");
    assert_eq!(
        encoded["messages"][0]["content"][0]["input"]["nested"],
        spoofed_marker
    );
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

#[test]
fn preserving_redacted_thinking_requires_a_compatible_provider_protocol() {
    let input = json!({
        "model":"claude-test",
        "messages":[
            {"role":"assistant","content":[{"type":"redacted_thinking","data":"opaque"}]},
            {"role":"user","content":"continue"}
        ]
    });
    let request = decode_request(Protocol::Messages, &input).expect("redacted block parses");
    let error = encode_upstream_request_with_thinking(
        UpstreamProtocol::Responses,
        &request,
        "reasoning-model",
        ThinkingHandling::Preserve,
    )
    .expect_err("opaque redacted block cannot be translated to Responses");
    assert!(error.contains("choose override or remove"));

    let overridden = encode_upstream_request_with_thinking(
        UpstreamProtocol::Responses,
        &request,
        "reasoning-model",
        ThinkingHandling::Override("replacement"),
    )
    .expect("override translates redacted block");
    assert_eq!(
        overridden["input"][0]["content"][0],
        json!({"type":"output_text","text":"replacement"})
    );
}

#[test]
fn response_protocols_decode_to_the_same_text_response() {
    let chat = json!({"id":"r1","model":"m","choices":[{"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":1}});
    let anthropic = json!({"id":"r1","model":"m","content":[{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"output_tokens":1}});
    let a = decode_response(Protocol::ChatCompletions, &chat, "m").expect("chat response");
    let b = decode_response(Protocol::Messages, &anthropic, "m").expect("messages response");
    assert_eq!(a.message.content.len(), b.message.content.len());
    assert_eq!(
        encode_response(Protocol::ChatCompletions, &b)["choices"][0]["message"]["content"],
        "hello"
    );
}

#[test]
fn custom_tool_calls_decode_as_tool_calls() {
    // Codex freeform tools such as apply_patch arrive as custom_tool_call items
    // with a freeform string input that is not necessarily valid JSON.
    let freeform = decode_upstream_response(
        UpstreamProtocol::Responses,
        &json!({
            "id":"resp_1",
            "status":"completed",
            "output":[
                {"type":"reasoning","summary":[{"type":"summary_text","text":"editing"}]},
                {"type":"custom_tool_call","id":"ctc_1","call_id":"call_1","name":"apply_patch","input":"*** Begin Patch\n*** Update File: a.txt\n*** End Patch"}
            ]
        }),
        "codex",
    )
    .expect("custom tool call is valid assistant output");
    assert_eq!(freeform.finish_reason, "tool_calls");
    let calls: Vec<_> = freeform
        .message
        .content
        .iter()
        .filter(|block| matches!(block, ContentBlock::ToolCall { .. }))
        .collect();
    assert_eq!(calls.len(), 1);
    match calls[0] {
        ContentBlock::ToolCall {
            id,
            name,
            arguments,
        } => {
            assert_eq!(id, "call_1");
            assert_eq!(name, "apply_patch");
            assert_eq!(
                arguments,
                &json!("*** Begin Patch\n*** Update File: a.txt\n*** End Patch")
            );
        }
        _ => unreachable!(),
    }

    // Structured custom tool input that is valid JSON stays a JSON value.
    let structured = decode_upstream_response(
        UpstreamProtocol::Responses,
        &json!({
            "status":"completed",
            "output":[{"type":"custom_tool_call","call_id":"call_2","name":"run_query","input":"{\"sql\":\"select 1\"}"}]
        }),
        "codex",
    )
    .expect("structured custom tool call is valid output");
    match structured.message.content.first() {
        Some(ContentBlock::ToolCall { id, arguments, .. }) => {
            assert_eq!(id, "call_2");
            assert_eq!(arguments, &json!({"sql":"select 1"}));
        }
        other => panic!("expected tool call, got {other:?}"),
    }

    // An empty input is still a real tool call: like function_call, the call is
    // forwarded by name so the client can answer it.
    let empty_input = decode_upstream_response(
        UpstreamProtocol::Responses,
        &json!({"status":"completed","output":[{"type":"custom_tool_call","call_id":"call_3","name":"apply_patch","input":""}]}),
        "codex",
    )
    .expect("an empty-input call still forwards by name");
    assert_eq!(empty_input.finish_reason, "tool_calls");
    match empty_input.message.content.first() {
        Some(ContentBlock::ToolCall {
            id,
            name,
            arguments,
        }) => {
            assert_eq!(id, "call_3");
            assert_eq!(name, "apply_patch");
            assert_eq!(arguments, &json!(""));
        }
        other => panic!("expected tool call, got {other:?}"),
    }
}

#[test]
fn completed_responses_without_output_are_rejected() {
    let empty_chat = json!({
        "choices":[{
            "message":{"role":"assistant","content":null},
            "finish_reason":"stop"
        }]
    });
    let empty_responses = json!({
        "status":"completed",
        "output":[],
        "output_text":""
    });
    let empty_messages = json!({
        "content":[],
        "stop_reason":"end_turn"
    });

    for (protocol, value) in [
        (UpstreamProtocol::ChatCompletions, empty_chat),
        (UpstreamProtocol::Responses, empty_responses),
        (UpstreamProtocol::Messages, empty_messages),
    ] {
        let error = decode_upstream_response(protocol, &value, "m")
            .expect_err("empty completed response must fail validation");
        assert_eq!(error, "upstream completed the response without content");
    }
}

#[test]
fn tool_only_and_image_only_responses_remain_valid_output() {
    let tool_call = decode_upstream_response(
        UpstreamProtocol::ChatCompletions,
        &json!({
            "choices":[{
                "message":{
                    "role":"assistant",
                    "content":null,
                    "tool_calls":[{
                        "id":"call-1",
                        "type":"function",
                        "function":{"name":"lookup","arguments":"{}"}
                    }]
                },
                "finish_reason":"tool_calls"
            }]
        }),
        "m",
    )
    .expect("tool-only response is meaningful output");
    assert!(response_has_output(&tool_call));

    let image = decode_upstream_response(
        UpstreamProtocol::Responses,
        &json!({
            "status":"completed",
            "output":[{
                "type":"image_generation_call",
                "result":"aGVsbG8=",
                "media_type":"image/png"
            }]
        }),
        "m",
    )
    .expect("image-only response is meaningful output");
    assert!(response_has_output(&image));
}

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
fn non_stream_tool_calls_translate_through_canonical_form() {
    let messages = json!({
        "model":"alias",
        "max_tokens":100,
        "messages":[{"role":"assistant","content":[{"type":"tool_use","id":"call_1","name":"lookup","input":{"q":"weather"}}]}]
    });
    let canonical = decode_request(Protocol::Messages, &messages).expect("messages request");
    let chat = encode_request(Protocol::ChatCompletions, &canonical, "chat-model")
        .expect("chat request encodes");
    assert_eq!(
        chat["messages"][0]["tool_calls"][0]["function"]["name"],
        "lookup"
    );

    let anthropic_response = json!({"id":"r2","model":"m","content":[{"type":"tool_use","id":"call_1","name":"lookup","input":{"q":"weather"}}],"stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":2}});
    let response =
        decode_response(Protocol::Messages, &anthropic_response, "m").expect("messages response");
    let responses = encode_response(Protocol::Responses, &response);
    assert_eq!(responses["output"][0]["type"], "function_call");
    assert_eq!(responses["output"][0]["name"], "lookup");
}

#[test]
fn image_urls_and_data_uris_translate_between_all_request_protocols() {
    let chat = json!({
        "model":"m",
        "messages":[{"role":"user","content":[
            {"type":"image_url","image_url":{"url":"https://images.test/photo.png","detail":"high"}},
            {"type":"image_url","image_url":{"url":"data:image/png;base64,aGVsbG8="}}
        ]}]
    });
    let canonical = decode_request(Protocol::ChatCompletions, &chat).expect("valid images");

    let responses = encode_request(Protocol::Responses, &canonical, "m").expect("responses encode");
    assert_eq!(
        responses["input"][0]["content"][0]["image_url"],
        "https://images.test/photo.png"
    );
    assert_eq!(responses["input"][0]["content"][0]["detail"], "high");
    assert_eq!(
        responses["input"][0]["content"][1]["image_url"],
        "data:image/png;base64,aGVsbG8="
    );

    let messages = encode_request(Protocol::Messages, &canonical, "m").expect("messages encode");
    assert_eq!(
        messages["messages"][0]["content"][0]["source"]["type"],
        "url"
    );
    assert_eq!(
        messages["messages"][0]["content"][0]["source"]["url"],
        "https://images.test/photo.png"
    );
    assert_eq!(
        messages["messages"][0]["content"][1]["source"]["type"],
        "base64"
    );
    assert_eq!(
        messages["messages"][0]["content"][1]["source"]["media_type"],
        "image/png"
    );

    let no_detail = decode_request(
            Protocol::Responses,
            &json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","image_url":"https://images.test/no-detail.png"}]}]}),
        ).expect("responses image without detail");
    let chat_no_detail =
        encode_request(Protocol::ChatCompletions, &no_detail, "m").expect("chat encode");
    assert!(
        chat_no_detail["messages"][0]["content"][0]["image_url"]
            .get("detail")
            .is_none()
    );

    let response_detail = decode_request(
            Protocol::Responses,
            &json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","image_url":"https://images.test/detail.png","detail":"low"}]}]}),
        ).expect("responses image detail");
    assert_eq!(
        encode_request(Protocol::Responses, &response_detail, "m").unwrap()["input"][0]["content"]
            [0]["detail"],
        "low"
    );
    let anthropic = decode_request(
            Protocol::Messages,
            &json!({"model":"m","messages":[{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":"aGVsbG8="}}]}]}),
        ).expect("anthropic base64 image");
    assert_eq!(
        encode_request(Protocol::ChatCompletions, &anthropic, "m").unwrap()["messages"][0]["content"]
            [0]["image_url"]["url"],
        "data:image/jpeg;base64,aGVsbG8="
    );
}

#[test]
fn documents_translate_between_responses_and_anthropic_and_chat_rejects_them() {
    let responses_input = json!({
        "model":"m",
        "input":[{"role":"user","content":[
            {"type":"input_file","file_url":"https://files.test/report.pdf","filename":"report.pdf"},
            {"type":"input_file","file_data":"data:application/pdf;base64,aGVsbG8=","filename":"inline.pdf"}
        ]}]
    });
    let canonical =
        decode_request(Protocol::Responses, &responses_input).expect("responses documents");
    let anthropic = encode_request(Protocol::Messages, &canonical, "m").expect("anthropic docs");
    assert_eq!(
        anthropic["messages"][0]["content"][0]["source"]["type"],
        "url"
    );
    assert_eq!(
        anthropic["messages"][0]["content"][0]["source"]["url"],
        "https://files.test/report.pdf"
    );
    assert_eq!(
        anthropic["messages"][0]["content"][0]["title"],
        "report.pdf"
    );
    assert_eq!(
        anthropic["messages"][0]["content"][1]["source"]["type"],
        "base64"
    );
    assert_eq!(
        anthropic["messages"][0]["content"][1]["source"]["data"],
        "aGVsbG8="
    );
    assert!(
        encode_request(Protocol::ChatCompletions, &canonical, "m")
            .unwrap_err()
            .contains("cannot represent document")
    );

    let from_anthropic = decode_request(
            Protocol::Messages,
            &json!({"model":"m","messages":[{"role":"user","content":[
                {"type":"document","source":{"type":"url","url":"https://files.test/from-anthropic.pdf"}},
                {"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"aGVsbG8="}}
            ]}]}),
        ).expect("Anthropic URL and base64 documents");
    let back_to_responses = encode_request(Protocol::Responses, &from_anthropic, "m")
        .expect("Responses URL and base64 documents");
    assert_eq!(
        back_to_responses["input"][0]["content"][0]["file_url"],
        "https://files.test/from-anthropic.pdf"
    );
    assert_eq!(
        back_to_responses["input"][0]["content"][1]["file_data"],
        "data:application/pdf;base64,aGVsbG8="
    );

    let inline_text = decode_request(
            Protocol::Messages,
            &json!({"model":"m","messages":[{"role":"user","content":[{"type":"document","source":{"type":"text","media_type":"text/plain","data":"inline notes"},"title":"notes.txt"}]}]}),
        ).expect("inline text document");
    let openai = encode_request(Protocol::Responses, &inline_text, "m")
        .expect("responses inline text document");
    assert_eq!(openai["input"][0]["content"][0]["type"], "input_file");
    assert_eq!(openai["input"][0]["content"][0]["filename"], "notes.txt");
    assert_eq!(
        openai["input"][0]["content"][0]["file_data"],
        "data:text/plain;base64,aW5saW5lIG5vdGVz"
    );
    let decoded_again =
        decode_request(Protocol::Responses, &openai).expect("decoded inline text document");
    let anthropic_again = encode_request(Protocol::Messages, &decoded_again, "m")
        .expect("re-encoded inline text document");
    assert_eq!(
        anthropic_again["messages"][0]["content"][0]["source"]["type"],
        "text"
    );
    assert_eq!(
        anthropic_again["messages"][0]["content"][0]["source"]["data"],
        "inline notes"
    );
}

#[test]
fn provider_scoped_files_and_malformed_documents_are_rejected_explicitly() {
    for (protocol, body) in [
        (
            Protocol::Responses,
            json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","file_id":"file_img"}]}]}),
        ),
        (
            Protocol::Responses,
            json!({"model":"m","input":[{"role":"user","content":[{"type":"input_file","file_id":"file_doc"}]}]}),
        ),
        (
            Protocol::Messages,
            json!({"model":"m","messages":[{"role":"user","content":[{"type":"image","source":{"type":"file","file_id":"file_img"}}]}]}),
        ),
        (
            Protocol::Messages,
            json!({"model":"m","messages":[{"role":"user","content":[{"type":"document","source":{"type":"file","file_id":"file_doc"}}]}]}),
        ),
    ] {
        assert!(
            decode_request(protocol, &body)
                .unwrap_err()
                .contains("file")
        );
    }
    for body in [
        json!({"model":"m","input":[{"role":"user","content":[{"type":"input_file"}]}]}),
        json!({"model":"m","input":[{"role":"user","content":[{"type":"input_file","file_data":"data:application/pdf;base64,%%%"}]}]}),
        json!({"model":"m","input":[{"role":"user","content":[{"type":"input_file","file_url":"https://files.test/a.pdf","file_data":"aGVsbG8="}]}]}),
    ] {
        assert!(decode_request(Protocol::Responses, &body).is_err());
    }
}

#[test]
fn responses_generated_images_survive_canonical_conversion() {
    let response = decode_response(
            Protocol::Responses,
            &json!({"id":"r-image","model":"m","status":"completed","output":[{"id":"ig_1","type":"image_generation_call","status":"completed","result":"aGVsbG8="}]}),
            "m",
        ).expect("generated image response");
    assert!(matches!(
        response.message.content[0],
        ContentBlock::GeneratedImage { .. }
    ));

    let responses = encode_response(Protocol::Responses, &response);
    assert_eq!(responses["output"][0]["type"], "image_generation_call");
    assert_eq!(responses["output"][0]["result"], "aGVsbG8=");
    let chat = encode_response(Protocol::ChatCompletions, &response);
    assert_eq!(
        chat["choices"][0]["message"]["content"][0]["type"],
        "image_url"
    );
    assert_eq!(
        chat["choices"][0]["message"]["content"][0]["image_url"]["url"],
        "data:image/png;base64,aGVsbG8="
    );
    let messages = encode_response(Protocol::Messages, &response);
    assert_eq!(messages["content"][0]["type"], "image");
    assert_eq!(messages["content"][0]["source"]["type"], "base64");
    assert_eq!(messages["content"][0]["source"]["data"], "aGVsbG8=");
}
