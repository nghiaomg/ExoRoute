use super::*;

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
