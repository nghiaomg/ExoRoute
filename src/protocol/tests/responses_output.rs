use super::*;

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
