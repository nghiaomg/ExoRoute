use super::*;

fn request(messages: Vec<Message>) -> CanonicalRequest {
    CanonicalRequest {
        source_protocol: Protocol::ChatCompletions,
        model: "opencode-model".to_owned(),
        messages,
        tools: Vec::new(),
        tool_choice: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stop: Vec::new(),
        stream: false,
        metadata: BTreeMap::new(),
        output_styles_applied: false,
    }
}

fn message(role: Role, content: Vec<ContentBlock>) -> Message {
    Message {
        role,
        content,
        name: None,
    }
}

#[test]
fn encodes_roles_generation_options_and_tool_declarations() {
    let mut request = request(vec![
        message(
            Role::System,
            vec![ContentBlock::Text {
                text: "Be concise".to_owned(),
            }],
        ),
        message(
            Role::User,
            vec![ContentBlock::Text {
                text: "Add these".to_owned(),
            }],
        ),
        message(
            Role::Assistant,
            vec![ContentBlock::ToolCall {
                id: "call-1".to_owned(),
                name: "sum".to_owned(),
                arguments: json!({"a":2,"b":3}),
            }],
        ),
        message(
            Role::Tool,
            vec![ContentBlock::ToolResult {
                tool_call_id: "call-1".to_owned(),
                content: vec![ContentBlock::Text {
                    text: "5".to_owned(),
                }],
            }],
        ),
    ]);
    request.tools.push(Tool {
        name: "sum".to_owned(),
        description: Some("Add two numbers".to_owned()),
        parameters: json!({"type":"object","properties":{"a":{"type":"number"}}}),
    });
    request.tool_choice = Some(json!({"type":"function","function":{"name":"sum"}}));
    request.temperature = Some(0.3);
    request.top_p = Some(0.8);
    request.max_tokens = Some(128);
    request.stop = vec!["END".to_owned()];
    request.stream = true;

    let encoded = encode_request(&request, "google-model").expect("request should encode");
    assert_eq!(
        encoded["systemInstruction"]["parts"][0]["text"],
        "Be concise"
    );
    assert_eq!(encoded["contents"][0]["role"], "user");
    assert_eq!(
        encoded["contents"][1]["parts"][0]["functionCall"]["id"],
        "call-1"
    );
    assert_eq!(encoded["contents"][2]["role"], "user");
    assert_eq!(
        encoded["contents"][2]["parts"][0]["functionResponse"]["name"],
        "sum"
    );
    assert_eq!(
        encoded["tools"][0]["functionDeclarations"][0]["name"],
        "sum"
    );
    assert_eq!(
        encoded["toolConfig"]["functionCallingConfig"]["mode"],
        "ANY"
    );
    assert_eq!(
        encoded["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
        "sum"
    );
    assert!(
        encoded["generationConfig"]["temperature"]
            .as_f64()
            .is_some_and(|value| (value - 0.3).abs() < 1e-6)
    );
    assert!(
        encoded["generationConfig"]["topP"]
            .as_f64()
            .is_some_and(|value| (value - 0.8).abs() < 1e-6)
    );
    assert_eq!(encoded["generationConfig"]["maxOutputTokens"], 128);
    assert_eq!(encoded["generationConfig"]["stopSequences"][0], "END");
    assert!(encoded.get("stream").is_none());
}

#[test]
fn encodes_inline_image_and_pdf_but_not_remote_media() {
    let media_request = request(vec![message(
        Role::User,
        vec![
            ContentBlock::Image {
                url: "data:image/png;base64,aGVsbG8=".to_owned(),
                detail: Some("high".to_owned()),
            },
            ContentBlock::Document {
                source: DocumentSource::Base64 {
                    media_type: "application/pdf".to_owned(),
                    data: "JVBERi0=".to_owned(),
                },
                filename: Some("document.pdf".to_owned()),
            },
        ],
    )]);
    let encoded = encode_request(&media_request, "google-model").expect("media should encode");
    assert_eq!(
        encoded["contents"][0]["parts"][0]["inlineData"]["mimeType"],
        "image/png"
    );
    assert_eq!(
        encoded["contents"][0]["parts"][1]["inlineData"]["mimeType"],
        "application/pdf"
    );

    let remote_image = request(vec![message(
        Role::User,
        vec![ContentBlock::Image {
            url: "https://example.com/image.png".to_owned(),
            detail: None,
        }],
    )]);
    assert!(encode_request(&remote_image, "m").is_err());

    let remote_document = request(vec![message(
        Role::User,
        vec![ContentBlock::Document {
            source: DocumentSource::Url {
                url: "https://example.com/file.pdf".to_owned(),
            },
            filename: None,
        }],
    )]);
    assert!(encode_request(&remote_document, "m").is_err());
}

#[test]
fn decodes_text_tools_finish_reason_and_usage() {
    let response = json!({
        "responseId":"resp-1",
        "modelVersion":"gemini-test",
        "candidates":[{
            "content":{"parts":[
                {"text":"hello"},
                {"functionCall":{"id":"call-9","name":"lookup","args":{"q":"x"}}}
            ]},
            "finishReason":"STOP"
        }],
        "usageMetadata":{"promptTokenCount":7,"cachedContentTokenCount":3,"candidatesTokenCount":4,"thoughtsTokenCount":2,"totalTokenCount":13}
    });
    let decoded = decode_response(&response, "requested").expect("response should decode");
    assert_eq!(decoded.id, "resp-1");
    assert_eq!(decoded.model, "gemini-test");
    assert_eq!(decoded.finish_reason, "tool_calls");
    assert_eq!(
        decoded.usage.as_ref().map(|usage| usage.input_tokens),
        Some(7)
    );
    assert_eq!(
        decoded.usage.as_ref().map(|usage| usage.output_tokens),
        Some(6)
    );
    assert_eq!(
        decoded.usage.as_ref().map(|usage| usage.cached_tokens),
        Some(3)
    );
    assert!(matches!(
        decoded.message.content.get(1),
        Some(ContentBlock::ToolCall { id, name, arguments })
            if id == "call-9" && name == "lookup" && arguments["q"] == "x"
    ));
}

#[test]
fn decodes_image_document_and_finish_reasons() {
    let response = json!({
        "candidates":[{
            "content":{"parts":[
                {"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}},
                {"inlineData":{"mimeType":"application/pdf","data":"JVBERi0="}}
            ]},
            "finishReason":"MAX_TOKENS"
        }],
        "usageMetadata":{"promptTokenCount":3,"totalTokenCount":9}
    });
    let decoded = decode_response(&response, "requested").expect("response should decode");
    assert_eq!(decoded.finish_reason, "length");
    assert_eq!(
        decoded.usage.as_ref().map(|usage| usage.output_tokens),
        Some(6)
    );
    assert!(matches!(
        &decoded.message.content[0],
        ContentBlock::GeneratedImage { .. }
    ));
    assert!(matches!(
        &decoded.message.content[1],
        ContentBlock::Document { .. }
    ));

    let blocked = json!({"candidates":[{"content":{"parts":[]},"finishReason":"SAFETY"}]});
    assert_eq!(
        decode_response(&blocked, "m")
            .expect("safety response")
            .finish_reason,
        "content_filter"
    );
}

#[test]
fn rejects_unrepresentable_metadata_media_and_tool_results() {
    let mut with_metadata = request(vec![message(
        Role::User,
        vec![ContentBlock::Text {
            text: "hi".to_owned(),
        }],
    )]);
    with_metadata
        .metadata
        .insert("response_format".to_owned(), json!({"type":"json_object"}));
    assert!(encode_request(&with_metadata, "m").is_err());

    let bad_pdf = request(vec![message(
        Role::User,
        vec![ContentBlock::Document {
            source: DocumentSource::Base64 {
                media_type: "application/msword".to_owned(),
                data: "YQ==".to_owned(),
            },
            filename: None,
        }],
    )]);
    assert!(encode_request(&bad_pdf, "m").is_err());

    let invalid_base64 = request(vec![message(
        Role::User,
        vec![ContentBlock::Image {
            url: "data:image/png;base64,aGVs!G8=".to_owned(),
            detail: None,
        }],
    )]);
    assert!(encode_request(&invalid_base64, "m").is_err());

    let unknown_tool_result = request(vec![message(
        Role::Tool,
        vec![ContentBlock::ToolResult {
            tool_call_id: "missing".to_owned(),
            content: vec![ContentBlock::Text {
                text: "done".to_owned(),
            }],
        }],
    )]);
    assert!(encode_request(&unknown_tool_result, "m").is_err());
}

#[test]
fn rejects_unsupported_response_parts_and_multiple_candidates() {
    let remote = json!({"candidates":[{"content":{"parts":[{"fileData":{"fileUri":"https://example.com/a.png","mimeType":"image/png"}}]},"finishReason":"STOP"}]});
    assert!(decode_response(&remote, "m").is_err());

    let multiple = json!({"candidates":[
        {"content":{"parts":[{"text":"one"}]},"finishReason":"STOP"},
        {"content":{"parts":[{"text":"two"}]},"finishReason":"STOP"}
    ]});
    assert!(decode_response(&multiple, "m").is_err());

    let malformed_call = json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"lookup","args":[]}}]},"finishReason":"STOP"}]});
    assert!(decode_response(&malformed_call, "m").is_err());
}
