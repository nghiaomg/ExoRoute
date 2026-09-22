use super::*;

#[cfg(test)]
pub(super) fn decode_response(
    protocol: Protocol,
    value: &Value,
    requested_model: &str,
) -> Result<CanonicalResponse, String> {
    decode_upstream_response(protocol.into(), value, requested_model)
}

pub(super) fn decode_upstream_response(
    protocol: UpstreamProtocol,
    value: &Value,
    requested_model: &str,
) -> Result<CanonicalResponse, String> {
    let response = match protocol {
        UpstreamProtocol::GoogleGenerateContent => google::decode_response(value, requested_model),
        protocol => decode_openai_family_response(
            protocol
                .client_protocol()
                .ok_or("upstream protocol cannot decode this response")?,
            value,
            requested_model,
        ),
    }?;
    if !response_has_output(&response) {
        return Err("upstream completed the response without content".to_owned());
    }
    Ok(response)
}

/// Returns whether a decoded assistant response contains output that can be
/// represented to a client. A terminal status alone is not enough: providers
/// can send an empty completed response, and a tool-call finish reason can be
/// present even when the tool-call payload was lost.
pub(super) fn response_has_output(response: &CanonicalResponse) -> bool {
    response
        .message
        .content
        .iter()
        .any(content_block_has_output)
}

fn content_block_has_output(block: &ContentBlock) -> bool {
    match block {
        ContentBlock::Text { text } | ContentBlock::Reasoning { text } => !text.trim().is_empty(),
        ContentBlock::Thinking { thinking, .. } => !thinking.trim().is_empty(),
        ContentBlock::RedactedThinking { data, .. } => !data.trim().is_empty(),
        ContentBlock::Image { url, .. } => !url.trim().is_empty(),
        ContentBlock::Document { source, .. } => match source {
            DocumentSource::Url { url } | DocumentSource::Text { text: url } => {
                !url.trim().is_empty()
            }
            DocumentSource::Base64 { data, .. } => !data.trim().is_empty(),
        },
        ContentBlock::GeneratedImage { data, .. } => !data.trim().is_empty(),
        ContentBlock::ToolCall { name, .. } => !name.trim().is_empty(),
        ContentBlock::ToolResult { content, .. } => content.iter().any(content_block_has_output),
    }
}

fn decode_openai_family_response(
    protocol: Protocol,
    value: &Value,
    requested_model: &str,
) -> Result<CanonicalResponse, String> {
    match protocol {
        Protocol::ChatCompletions => decode_chat_response(value, requested_model),
        Protocol::Responses => decode_responses_response(value, requested_model),
        Protocol::Messages => decode_messages_response(value, requested_model),
    }
}

fn decode_chat_response(v: &Value, requested_model: &str) -> Result<CanonicalResponse, String> {
    let choice = v
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .ok_or("chat response has no choices")?;
    let raw = choice
        .get("message")
        .ok_or("chat response has no message")?;
    let mut content = Vec::new();
    if let Some(reasoning) = parse_reasoning_content(raw.get("reasoning_content"))? {
        content.push(ContentBlock::Reasoning { text: reasoning });
    }
    content.extend(parse_content(raw.get("content"))?);
    if let Some(calls) = raw.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let f = call.get("function").unwrap_or(call);
            let args = f.get("arguments").and_then(Value::as_str).unwrap_or("{}");
            content.push(ContentBlock::ToolCall {
                id: call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("call")
                    .to_owned(),
                name: f
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                arguments: serde_json::from_str(args).unwrap_or(json!({})),
            });
        }
    }
    Ok(CanonicalResponse {
        id: v
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("exo-response")
            .to_owned(),
        model: v
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(requested_model)
            .to_owned(),
        message: Message {
            role: Role::Assistant,
            content,
            name: None,
        },
        finish_reason: choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .unwrap_or("stop")
            .to_owned(),
        usage: parse_usage(v.get("usage"), "prompt_tokens", "completion_tokens"),
    })
}

fn decode_responses_response(
    v: &Value,
    requested_model: &str,
) -> Result<CanonicalResponse, String> {
    let mut content = Vec::new();
    if let Some(output) = v.get("output").and_then(Value::as_array) {
        for item in output {
            let item_type = item.get("type").and_then(Value::as_str);
            if item_type == Some("reasoning") {
                content.extend(parse_responses_reasoning_item(item)?);
                continue;
            }
            if item_type == Some("image_generation_call")
                && let Some(data) = item.get("result").and_then(Value::as_str)
            {
                content.push(ContentBlock::GeneratedImage {
                    data: data.to_owned(),
                    media_type: item
                        .get("media_type")
                        .and_then(Value::as_str)
                        .unwrap_or("image/png")
                        .to_owned(),
                });
            }
            if let Some(parts) = item.get("content").and_then(Value::as_array) {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                        content.push(ContentBlock::Text {
                            text: text.to_owned(),
                        });
                    }
                    if part.get("type").and_then(Value::as_str) == Some("image_generation_call")
                        && let Some(data) = part.get("result").and_then(Value::as_str)
                    {
                        content.push(ContentBlock::GeneratedImage {
                            data: data.to_owned(),
                            media_type: part
                                .get("media_type")
                                .and_then(Value::as_str)
                                .unwrap_or("image/png")
                                .to_owned(),
                        });
                    }
                }
            }
            // Codex freeform tools (for example apply_patch) arrive as
            // custom_tool_call items whose input is a freeform string that is not
            // required to be valid JSON, unlike function_call arguments.
            if item.get("type").and_then(Value::as_str) == Some("custom_tool_call") {
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                // Some custom tools send structured JSON input as a string; when
                // it parses, keep the object like function_call arguments do.
                // Otherwise retain the raw freeform payload (apply_patch diffs).
                let input = match item.get("input") {
                    Some(Value::String(text)) => {
                        serde_json::from_str(text).unwrap_or(Value::String(text.clone()))
                    }
                    Some(other) => other.clone(),
                    None => json!(""),
                };
                if !name.is_empty() {
                    content.push(ContentBlock::ToolCall {
                        id: item
                            .get("call_id")
                            .or_else(|| item.get("id"))
                            .and_then(Value::as_str)
                            .unwrap_or("call")
                            .to_owned(),
                        name,
                        arguments: input,
                    });
                }
            }
            if item.get("type").and_then(Value::as_str) == Some("function_call") {
                content.push(ContentBlock::ToolCall {
                    id: item
                        .get("call_id")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .unwrap_or("call")
                        .to_owned(),
                    name: item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    arguments: item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_else(|| item.get("arguments").cloned().unwrap_or(json!({}))),
                });
            }
        }
    }
    if !content
        .iter()
        .any(|block| matches!(block, ContentBlock::Text { .. }))
        && let Some(text) = v.get("output_text").and_then(Value::as_str)
    {
        content.push(ContentBlock::Text {
            text: text.to_owned(),
        });
    }
    let finish_reason = responses_finish_reason(v, &content)?;
    Ok(CanonicalResponse {
        id: v
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("exo-response")
            .to_owned(),
        model: v
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(requested_model)
            .to_owned(),
        message: Message {
            role: Role::Assistant,
            content,
            name: None,
        },
        finish_reason,
        usage: parse_usage(v.get("usage"), "input_tokens", "output_tokens"),
    })
}

fn responses_finish_reason(value: &Value, content: &[ContentBlock]) -> Result<String, String> {
    match value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("completed")
    {
        "completed" => Ok(
            if content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolCall { .. }))
            {
                "tool_calls".to_owned()
            } else {
                "stop".to_owned()
            },
        ),
        "incomplete" => Ok("length".to_owned()),
        "failed" | "cancelled" => {
            Err("Responses provider reported a failed or cancelled response".to_owned())
        }
        status => Err(format!(
            "Responses provider returned non-terminal status '{status}'"
        )),
    }
}

fn decode_messages_response(v: &Value, requested_model: &str) -> Result<CanonicalResponse, String> {
    let mut content = Vec::new();
    for block in v
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    content.push(ContentBlock::Text {
                        text: text.to_owned(),
                    });
                }
            }
            Some("thinking") | Some("redacted_thinking") => {
                content.extend(parse_content(Some(&json!([block])))?);
            }
            Some("image") | Some("document") => {
                content.extend(parse_content(Some(&json!([block])))?);
            }
            Some("tool_use") => content.push(ContentBlock::ToolCall {
                id: block
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("call")
                    .to_owned(),
                name: block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                arguments: block.get("input").cloned().unwrap_or(json!({})),
            }),
            _ => {}
        }
    }
    Ok(CanonicalResponse {
        id: v
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("exo-response")
            .to_owned(),
        model: v
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or(requested_model)
            .to_owned(),
        message: Message {
            role: Role::Assistant,
            content,
            name: None,
        },
        finish_reason: v
            .get("stop_reason")
            .and_then(Value::as_str)
            .unwrap_or("end_turn")
            .to_owned(),
        usage: parse_usage(v.get("usage"), "input_tokens", "output_tokens"),
    })
}
