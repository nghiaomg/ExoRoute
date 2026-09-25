use super::*;

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
