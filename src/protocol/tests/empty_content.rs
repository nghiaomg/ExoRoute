use super::*;

/// Asserts an encoded Chat Completions body contains only content shapes strict
/// OpenAI-family validators accept: a missing `content` field is legal only for
/// an assistant turn that carries tool calls, an array must not be empty, and no
/// part may encode an empty string.
fn assert_chat_content_is_schema_valid(body: &Value) {
    let messages = body["messages"].as_array().expect("messages array");
    assert!(!messages.is_empty(), "at least one message is encoded");
    for message in messages {
        match message.get("content") {
            None => assert!(
                message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .is_some_and(|calls| !calls.is_empty()),
                "only a tool-calling assistant turn may omit content: {message}"
            ),
            Some(Value::String(_)) => {}
            Some(Value::Array(parts)) => {
                assert!(
                    !parts.is_empty(),
                    "an empty content array is rejected upstream: {message}"
                );
                for part in parts {
                    assert!(
                        !chat_content_part_is_empty(part),
                        "empty content part encoded in {message}"
                    );
                    assert!(
                        part.is_object(),
                        "a content part must be an object, not a bare string: {message}"
                    );
                }
            }
            Some(other) => panic!("unexpected content shape {other} in {message}"),
        }
    }
}

/// Asserts an encoded Anthropic Messages body follows the same rule for content
/// blocks, including the blocks nested inside a `tool_result`.
fn assert_messages_content_is_schema_valid(body: &Value) {
    for message in body["messages"].as_array().expect("messages array") {
        let content = message
            .get("content")
            .expect("Anthropic content is required");
        match content {
            Value::String(_) => {}
            Value::Array(blocks) => {
                assert!(
                    !blocks.is_empty(),
                    "an empty content array is rejected upstream: {message}"
                );
                for block in blocks {
                    assert!(
                        !anthropic_content_block_is_empty(block),
                        "empty text block encoded in {message}"
                    );
                    if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                        match &block["content"] {
                            Value::String(_) => {}
                            Value::Array(inner) => {
                                assert!(
                                    !inner.is_empty(),
                                    "empty tool_result content is rejected upstream: {message}"
                                );
                                for part in inner {
                                    assert!(
                                        !anthropic_content_block_is_empty(part),
                                        "empty tool_result text block in {message}"
                                    );
                                }
                            }
                            other => panic!("unexpected tool_result content {other}"),
                        }
                    }
                }
            }
            other => panic!("unexpected content shape {other} in {message}"),
        }
    }
}

/// A tool that produced no output used to reach a Chat Completions provider as
/// `"content": [""]` — a bare empty string inside a content array. Strict
/// upstream validators answer HTTP 400 naming `messages.N.content`, which is how
/// `command-code` and `opencode-go` rejected the same conversation.
#[test]
fn empty_tool_result_output_never_encodes_an_empty_content_part() {
    let input = json!({
        "model":"mimo-v2.6-flash",
        "stream":true,
        "system":[{"type":"text","text":"You are a coding agent."}],
        "messages":[
            {"role":"user","content":[{"type":"text","text":"read a.txt"}]},
            {"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":""}]},
            {"role":"assistant","content":[{"type":"tool_use","id":"toolu_2","name":"Bash","input":{}}]}
        ]
    });
    let request = decode_request(Protocol::Messages, &input).expect("Anthropic input parses");

    let chat = encode_upstream_request_with_thinking(
        UpstreamProtocol::ChatCompletions,
        &request,
        "mimo-v2.6-flash",
        ThinkingHandling::Preserve,
    )
    .expect("Anthropic history converts to Chat Completions");
    assert_chat_content_is_schema_valid(&chat);
    // system, user, assistant(tool_calls), user(tool_result), assistant(tool_calls)
    assert_eq!(chat["messages"][3]["role"], "user");
    assert_eq!(
        chat["messages"][3]["content"],
        json!(""),
        "an empty tool result must not be encoded as an array holding an empty string"
    );
    assert!(
        chat["messages"][2].get("content").is_none(),
        "a tool-calling assistant turn carries no content when it has no text"
    );

    let messages = encode_upstream_request_with_thinking(
        UpstreamProtocol::Messages,
        &request,
        "mimo-v2.6-flash",
        ThinkingHandling::Preserve,
    )
    .expect("Anthropic history stays Anthropic");
    assert_messages_content_is_schema_valid(&messages);
    assert_eq!(
        messages["messages"][2]["content"][0]["content"],
        json!(""),
        "an empty tool result keeps its pairing through the string form"
    );
    assert_eq!(
        messages["messages"][2]["content"][0]["tool_use_id"],
        "toolu_1"
    );
}

/// A tool_result decoded inside an Anthropic user message used to reach a Chat
/// Completions provider as `content: ["output"]` — a bare string inside the
/// content array, which strict OpenAI-family validators reject with an HTTP 400
/// "invalid request error" (how `command-code` rejected this conversation).
#[test]
fn non_empty_tool_results_encode_as_text_parts() {
    let input = json!({
        "model":"mimo-v2.6-flash",
        "messages":[
            {"role":"user","content":"list the files"},
            {"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{}}]},
            {"role":"user","content":[
                {"type":"tool_result","tool_use_id":"toolu_1","content":"src\nCargo.toml"},
                {"type":"text","text":"now read Cargo.toml"}
            ]}
        ]
    });
    let request = decode_request(Protocol::Messages, &input).expect("Anthropic input parses");

    let chat = encode_upstream_request_with_thinking(
        UpstreamProtocol::ChatCompletions,
        &request,
        "mimo-v2.6-flash",
        ThinkingHandling::Preserve,
    )
    .expect("Anthropic history converts to Chat Completions");
    assert_chat_content_is_schema_valid(&chat);
    assert_eq!(
        chat["messages"][2]["content"],
        json!([
            {"type":"text","text":"src\nCargo.toml"},
            {"type":"text","text":"now read Cargo.toml"}
        ]),
        "tool_result output must encode as a text part object, not a bare string"
    );

    let messages = encode_upstream_request_with_thinking(
        UpstreamProtocol::Messages,
        &request,
        "mimo-v2.6-flash",
        ThinkingHandling::Preserve,
    )
    .expect("Anthropic history stays Anthropic");
    assert_messages_content_is_schema_valid(&messages);
    assert_eq!(
        messages["messages"][2]["content"][0]["content"],
        json!([{"type":"text","text":"src\nCargo.toml"}])
    );
}

/// A client that sends `content: ""` next to its tool calls, or a tool message
/// with no text, used to encode `[{"type":"text","text":""}]` and `[]`.
#[test]
fn empty_text_parts_are_dropped_from_encoded_content() {
    let input = json!({
        "model":"mimo-v2.6-flash",
        "messages":[
            {"role":"user","content":""},
            {"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_file","arguments":"{}"}}]},
            {"role":"tool","tool_call_id":"call_1","content":""}
        ]
    });
    let request = decode_request(Protocol::ChatCompletions, &input).expect("chat input parses");

    let chat = encode_upstream_request_with_thinking(
        UpstreamProtocol::ChatCompletions,
        &request,
        "mimo-v2.6-flash",
        ThinkingHandling::Preserve,
    )
    .expect("chat input stays Chat Completions");
    assert_chat_content_is_schema_valid(&chat);
    assert!(
        chat["messages"][1].get("content").is_none(),
        "the empty text part must not be encoded next to tool_calls"
    );
    assert_eq!(chat["messages"][1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(chat["messages"][2]["content"], json!(""));

    let messages = encode_upstream_request_with_thinking(
        UpstreamProtocol::Messages,
        &request,
        "mimo-v2.6-flash",
        ThinkingHandling::Preserve,
    )
    .expect("chat history converts to Anthropic");
    assert_messages_content_is_schema_valid(&messages);
    assert_eq!(
        messages["messages"][1]["content"],
        json!([{"type":"tool_use","id":"call_1","name":"read_file","input":{}}]),
        "the empty text block is dropped, leaving the tool call"
    );
    assert_eq!(
        messages["messages"][2]["content"][0]["content"],
        json!(""),
        "the empty tool result keeps its pairing"
    );
}

/// A canonical turn with no blocks at all must not reach either protocol as an
/// empty content array.
#[test]
fn empty_messages_never_encode_an_empty_content_array() {
    let messages_input = json!({
        "model":"mimo-v2.6-flash",
        "messages":[
            {"role":"user","content":[{"type":"text","text":"hello"}]},
            {"role":"assistant","content":[]}
        ]
    });
    let request = decode_request(Protocol::Messages, &messages_input).expect("input parses");
    let messages = encode_upstream_request_with_thinking(
        UpstreamProtocol::Messages,
        &request,
        "mimo-v2.6-flash",
        ThinkingHandling::Preserve,
    )
    .expect("empty Anthropic turn encodes");
    assert_messages_content_is_schema_valid(&messages);
    assert_eq!(messages["messages"][1]["content"], json!(""));

    let chat_input = json!({
        "model":"mimo-v2.6-flash",
        "messages":[
            {"role":"user","content":"hello"},
            {"role":"assistant","content":null}
        ]
    });
    let request = decode_request(Protocol::ChatCompletions, &chat_input).expect("input parses");
    let chat = encode_upstream_request_with_thinking(
        UpstreamProtocol::ChatCompletions,
        &request,
        "mimo-v2.6-flash",
        ThinkingHandling::Preserve,
    )
    .expect("empty chat turn encodes");
    assert_chat_content_is_schema_valid(&chat);
    assert_eq!(chat["messages"][1]["content"], json!(""));
}
