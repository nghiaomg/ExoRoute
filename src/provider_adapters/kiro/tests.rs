use super::device::{KiroDeviceContext, decode_device_code, encode_device_code};
use super::transform;
use serde_json::json;

#[test]
fn device_code_context_round_trips() {
    let context = KiroDeviceContext {
        device_code: "device-code".to_owned(),
        client_id: "client-id".to_owned(),
        client_secret: "client-secret".to_owned(),
        region: "us-east-1".to_owned(),
    };
    let encoded = encode_device_code(&context).expect("encodes");
    let decoded = decode_device_code(&encoded).expect("decodes");
    assert_eq!(decoded.device_code, "device-code");
    assert_eq!(decoded.client_id, "client-id");
    assert_eq!(decoded.client_secret, "client-secret");
}

#[test]
fn device_code_rejects_garbage_and_empty_fields() {
    assert!(decode_device_code("").is_err());
    assert!(decode_device_code("!!!not-base64!!!").is_err());
    let encoded = encode_device_code(&KiroDeviceContext {
        device_code: "".to_owned(),
        client_id: "id".to_owned(),
        client_secret: "secret".to_owned(),
        region: "us-east-1".to_owned(),
    })
    .expect("encodes");
    assert!(decode_device_code(&encoded).is_err());
}

#[test]
fn build_payload_shapes_a_single_turn_request() {
    let body = json!({
        "messages": [{"role": "user", "content": "hello"}],
        "model": "claude-sonnet-4.5",
        "max_tokens": 1024,
        "temperature": 0.5,
    });
    let payload = transform::build_payload(&body, "claude-sonnet-4.5").expect("payload builds");
    let state = &payload["conversationState"];
    assert_eq!(state["chatTriggerType"], json!("MANUAL"));
    assert!(
        state["conversationId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(state["history"], json!([]));
    let current = &state["currentMessage"]["userInputMessage"];
    assert_eq!(current["modelId"], json!("claude-sonnet-4.5"));
    assert_eq!(current["origin"], json!("AI_EDITOR"));
    assert!(current["content"].as_str().unwrap().contains("hello"));
    assert_eq!(payload["inferenceConfig"]["maxTokens"], json!(1024));
    assert_eq!(payload["inferenceConfig"]["temperature"], json!(0.5));
}

#[test]
fn build_payload_folds_system_and_history() {
    let body = json!({
        "messages": [
            {"role": "system", "content": "be brief"},
            {"role": "user", "content": "first"},
            {"role": "assistant", "content": "answer"},
            {"role": "user", "content": "second"},
        ],
    });
    let payload = transform::build_payload(&body, "claude-haiku-4.5").expect("payload builds");
    let state = &payload["conversationState"];
    let history = state["history"].as_array().expect("history array");
    // system folds into the first user turn; the last user turn is current.
    assert_eq!(history.len(), 2);
    assert!(
        history[0]["userInputMessage"]["content"]
            .as_str()
            .unwrap()
            .contains("be brief")
    );
    assert!(
        history[0]["userInputMessage"]["content"]
            .as_str()
            .unwrap()
            .contains("first")
    );
    assert!(history[1].get("assistantResponseMessage").is_some());
    assert!(
        state["currentMessage"]["userInputMessage"]["content"]
            .as_str()
            .unwrap()
            .contains("second")
    );
    assert_eq!(
        history[0]["userInputMessage"]["modelId"],
        json!("claude-haiku-4.5")
    );
}

#[test]
fn build_payload_maps_tool_calls_and_tool_results() {
    let body = json!({
        "messages": [
            {"role": "user", "content": "weather?"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"},
                }],
            },
            {"role": "tool", "tool_call_id": "call_1", "content": "sunny"},
            {"role": "user", "content": "thanks"},
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "object"},
            },
        }],
    });
    let payload = transform::build_payload(&body, "claude-sonnet-4.5").expect("payload builds");
    let state = &payload["conversationState"];
    let history = state["history"].as_array().expect("history array");
    // First user turn carries the tool specification.
    let tools = history[0].pointer("/userInputMessage/userInputMessageContext/tools");
    assert!(tools.is_some(), "tool specs attach to first user turn");
    // Assistant tool call becomes toolUses.
    let tool_uses = history
        .iter()
        .find_map(|item| item.pointer("/assistantResponseMessage/toolUses"))
        .expect("assistant toolUses present");
    assert_eq!(tool_uses[0]["name"], json!("get_weather"));
    assert_eq!(tool_uses[0]["toolUseId"], json!("call_1"));
    // The tool result lands on a user turn's toolResults context — here the
    // final "thanks" turn is the current message, which is also valid.
    let has_tool_result = history.iter().any(|item| {
        item.pointer("/userInputMessage/userInputMessageContext/toolResults")
            .is_some()
    }) || state
        .pointer("/currentMessage/userInputMessage/userInputMessageContext/toolResults")
        .is_some();
    assert!(has_tool_result, "tool results reach a user turn");
}

#[test]
fn build_payload_rejects_empty_message_lists() {
    assert!(transform::build_payload(&json!({"messages": []}), "m").is_err());
    assert!(transform::build_payload(&json!({}), "m").is_err());
}
