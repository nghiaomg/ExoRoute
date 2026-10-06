use super::*;

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
