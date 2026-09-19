use super::request_encode::{content_to_anthropic, content_to_chat};
use super::{CanonicalResponse, ContentBlock, Protocol};
use serde_json::{Value, json};

/// Encodes the canonical assistant response for the requested client wire
/// protocol. Request encoding lives in `encode.rs`; keeping this direction
/// separate prevents response-shape changes from coupling to request options.
pub fn encode_response(protocol: Protocol, response: &CanonicalResponse) -> Value {
    let text = response
        .message
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<String>();
    let reasoning_content = response
        .message
        .content
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Reasoning { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    match protocol {
        Protocol::ChatCompletions => {
            let tool_calls: Vec<Value> = response
                .message
                .content
                .iter()
                .filter_map(|block| {
                    if let ContentBlock::ToolCall {
                        id,
                        name,
                        arguments,
                    } = block
                    {
                        Some(json!({
                            "id": id,
                            "type": "function",
                            "function": {"name": name, "arguments": arguments.to_string()}
                        }))
                    } else {
                        None
                    }
                })
                .collect();
            let has_image = response.message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Image { .. } | ContentBlock::GeneratedImage { .. }
                )
            });
            let message_content = if has_image {
                json!(
                    response
                        .message
                        .content
                        .iter()
                        .filter_map(content_to_chat)
                        .collect::<Vec<_>>()
                )
            } else if text.is_empty() {
                Value::Null
            } else {
                json!(text)
            };
            let mut message = json!({"role":"assistant","content":message_content});
            if !reasoning_content.is_empty() {
                message["reasoning_content"] = json!(reasoning_content);
            }
            if !tool_calls.is_empty() {
                message["tool_calls"] = json!(tool_calls);
            }
            json!({
                "id": response.id,
                "object": "chat.completion",
                "created": 0,
                "model": response.model,
                "choices": [{
                    "index": 0,
                    "message": message,
                    "finish_reason": normalize_chat_finish(&response.finish_reason)
                }],
                "usage": response.usage.as_ref().map(|usage| json!({
                    "prompt_tokens": usage.input_tokens,
                    "completion_tokens": usage.output_tokens,
                    "total_tokens": usage.input_tokens.saturating_add(usage.output_tokens),
                    "cost": usage.cost_micro_usd.map(|value| value as f64 / 1_000_000.0)
                })).unwrap_or(Value::Null)
            })
        }
        Protocol::Responses => {
            let mut output = Vec::new();
            for block in &response.message.content {
                if let ContentBlock::Reasoning { text } = block
                    && !text.trim().is_empty()
                {
                    output.push(json!({
                        "id": format!("rs_{}", output.len()),
                        "type": "reasoning",
                        "content": [{"type":"reasoning_text","text":text}],
                        "summary": []
                    }));
                }
            }
            if !text.is_empty() {
                output.push(json!({
                    "id":"msg_0",
                    "type":"message",
                    "role":"assistant",
                    "content":[{"type":"output_text","text":text,"annotations":[]}]
                }));
            }
            for block in &response.message.content {
                if let ContentBlock::GeneratedImage { data, .. } = block {
                    output.push(json!({
                        "id": format!("ig_{}", output.len()),
                        "type": "image_generation_call",
                        "status": "completed",
                        "result": data
                    }));
                } else if let ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } = block
                {
                    output.push(json!({
                        "id": id,
                        "type": "function_call",
                        "call_id": id,
                        "name": name,
                        "arguments": arguments.to_string()
                    }));
                }
            }
            json!({
                "id": response.id,
                "object": "response",
                "status": if response.finish_reason.contains("length") { "incomplete" } else { "completed" },
                "model": response.model,
                "output": output,
                "output_text": text,
                "usage": response.usage.as_ref().map(|usage| json!({
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                    "total_tokens": usage.input_tokens.saturating_add(usage.output_tokens),
                    "cost": usage.cost_micro_usd.map(|value| value as f64 / 1_000_000.0)
                })).unwrap_or(Value::Null)
            })
        }
        Protocol::Messages => {
            let mut content: Vec<Value> = response
                .message
                .content
                .iter()
                .filter(|block| !matches!(block, ContentBlock::ToolCall { .. }))
                .filter_map(content_to_anthropic)
                .collect();
            if content.is_empty() {
                content.push(json!({"type":"text","text":text}));
            }
            for block in &response.message.content {
                if let ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } = block
                {
                    content.push(json!({
                        "type":"tool_use",
                        "id":id,
                        "name":name,
                        "input":arguments
                    }));
                }
            }
            json!({
                "id": response.id,
                "type": "message",
                "role": "assistant",
                "model": response.model,
                "content": content,
                "stop_reason": messages_finish_reason(&response.finish_reason),
                "stop_sequence": null,
                "usage": response.usage.as_ref().map(|usage| json!({
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                    "cost": usage.cost_micro_usd.map(|value| value as f64 / 1_000_000.0)
                })).unwrap_or(Value::Null)
            })
        }
    }
}

pub(crate) fn normalize_chat_finish(value: &str) -> &'static str {
    if value.contains("tool") {
        "tool_calls"
    } else if value.contains("length") || value.contains("max_tokens") {
        "length"
    } else {
        "stop"
    }
}

pub(crate) fn messages_finish_reason(value: &str) -> &'static str {
    if value.contains("tool") {
        "tool_use"
    } else if value.contains("length") || value.contains("max_tokens") {
        "max_tokens"
    } else if value.contains("stop_sequence") {
        "stop_sequence"
    } else {
        "end_turn"
    }
}
