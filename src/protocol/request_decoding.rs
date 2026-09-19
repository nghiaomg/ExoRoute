use super::*;

pub(super) fn decode_request(protocol: Protocol, body: &Value) -> Result<CanonicalRequest, String> {
    match protocol {
        Protocol::ChatCompletions => decode_chat_request(body),
        Protocol::Responses => decode_responses_request(body),
        Protocol::Messages => decode_messages_request(body),
    }
}

fn decode_chat_request(body: &Value) -> Result<CanonicalRequest, String> {
    let model = string_field(body, "model")?;
    let raw_messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("messages must be an array")?;
    let mut messages = Vec::with_capacity(raw_messages.len());
    for raw in raw_messages {
        let role = parse_role(
            raw.get("role")
                .and_then(Value::as_str)
                .ok_or("message role is required")?,
        )?;
        let mut content = Vec::new();
        if matches!(role, Role::Assistant)
            && let Some(reasoning) = parse_reasoning_content(raw.get("reasoning_content"))?
        {
            content.push(ContentBlock::Reasoning { text: reasoning });
        }
        content.extend(parse_content(raw.get("content"))?);
        if let Some(calls) = raw.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let function = call.get("function").unwrap_or(call);
                let args = function
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let arguments = match args {
                    Value::String(text) => {
                        serde_json::from_str(&text).unwrap_or(Value::String(text))
                    }
                    other => other,
                };
                content.push(ContentBlock::ToolCall {
                    id: call
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("call")
                        .to_owned(),
                    name: function
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    arguments,
                });
            }
        }
        if role.as_str() == "tool" {
            content = vec![ContentBlock::ToolResult {
                tool_call_id: raw
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                content,
            }];
        }
        messages.push(Message {
            role,
            content,
            name: raw.get("name").and_then(Value::as_str).map(str::to_owned),
        });
    }
    let tools = parse_chat_tools(body.get("tools"))?;
    let mut metadata = BTreeMap::new();
    if let Some(object) = body.as_object() {
        for (key, value) in object {
            if ![
                "model",
                "messages",
                "tools",
                "tool_choice",
                "temperature",
                "top_p",
                "max_tokens",
                "max_completion_tokens",
                "stop",
                "stream",
            ]
            .contains(&key.as_str())
            {
                metadata.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(CanonicalRequest {
        source_protocol: Protocol::ChatCompletions,
        model,
        messages,
        tools,
        tool_choice: body.get("tool_choice").cloned(),
        temperature: optional_f32(body.get("temperature"), "temperature", 0.0, 2.0)?,
        top_p: optional_f32(body.get("top_p"), "top_p", 0.0, 1.0)?,
        max_tokens: optional_u32(
            body.get("max_tokens")
                .or_else(|| body.get("max_completion_tokens")),
            "max_tokens",
        )?,
        stop: parse_string_list(body.get("stop")),
        stream: optional_bool(body.get("stream"), "stream")?.unwrap_or(false),
        metadata,
        output_styles_applied: false,
    })
}

fn decode_responses_request(body: &Value) -> Result<CanonicalRequest, String> {
    let model = string_field(body, "model")?;
    let mut messages = Vec::new();
    if let Some(instructions) = body.get("instructions").and_then(Value::as_str) {
        messages.push(Message {
            role: Role::System,
            content: vec![ContentBlock::Text {
                text: instructions.to_owned(),
            }],
            name: None,
        });
    }
    let input = body.get("input").ok_or("input is required")?;
    if let Some(text) = input.as_str() {
        messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_owned(),
            }],
            name: None,
        });
    } else if let Some(items) = input.as_array() {
        for item in items {
            let item_type = item.get("type").and_then(Value::as_str);
            let inferred_role = match item_type {
                Some("function_call") => "assistant",
                Some("function_call_output") => "tool",
                Some("reasoning") => "assistant",
                _ => "user",
            };
            let role = parse_role(
                item.get("role")
                    .and_then(Value::as_str)
                    .unwrap_or(inferred_role),
            )?;
            let mut content = if item_type == Some("reasoning") {
                parse_responses_reasoning_item(item)?
            } else {
                parse_content(item.get("content"))?
            };
            if matches!(
                item.get("type").and_then(Value::as_str),
                Some(
                    "function_call"
                        | "function_call_output"
                        | "input_text"
                        | "input_image"
                        | "input_file"
                )
            ) {
                content.extend(parse_content(Some(&json!([item])))?);
            }
            messages.push(Message {
                role,
                content,
                name: item.get("name").and_then(Value::as_str).map(str::to_owned),
            });
        }
    } else {
        return Err("input must be a string or array".to_owned());
    }
    let tools = parse_response_tools(body.get("tools"))?;
    Ok(CanonicalRequest {
        source_protocol: Protocol::Responses,
        model,
        messages,
        tools,
        tool_choice: body.get("tool_choice").cloned(),
        temperature: optional_f32(body.get("temperature"), "temperature", 0.0, 2.0)?,
        top_p: optional_f32(body.get("top_p"), "top_p", 0.0, 1.0)?,
        max_tokens: optional_u32(body.get("max_output_tokens"), "max_output_tokens")?,
        stop: Vec::new(),
        stream: optional_bool(body.get("stream"), "stream")?.unwrap_or(false),
        metadata: BTreeMap::new(),
        output_styles_applied: false,
    })
}

fn decode_messages_request(body: &Value) -> Result<CanonicalRequest, String> {
    let model = string_field(body, "model")?;
    let mut messages = Vec::new();
    if let Some(system) = body.get("system") {
        let content = parse_content(Some(system))?;
        if content.iter().any(|block| {
            matches!(
                block,
                ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
            )
        }) {
            return Err("thinking blocks are only valid in assistant messages".to_owned());
        }
        messages.push(Message {
            role: Role::System,
            content,
            name: None,
        });
    }
    for raw in body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("messages must be an array")?
    {
        let role = parse_role(
            raw.get("role")
                .and_then(Value::as_str)
                .ok_or("message role is required")?,
        )?;
        let content = parse_content(raw.get("content"))?;
        if !matches!(role, Role::Assistant)
            && content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. }
                )
            })
        {
            return Err("thinking blocks are only valid in assistant messages".to_owned());
        }
        messages.push(Message {
            role,
            content,
            name: None,
        });
    }
    let tools = parse_messages_tools(body.get("tools"))?;
    Ok(CanonicalRequest {
        source_protocol: Protocol::Messages,
        model,
        messages,
        tools,
        tool_choice: body.get("tool_choice").cloned(),
        temperature: optional_f32(body.get("temperature"), "temperature", 0.0, 2.0)?,
        top_p: optional_f32(body.get("top_p"), "top_p", 0.0, 1.0)?,
        max_tokens: optional_u32(body.get("max_tokens"), "max_tokens")?,
        stop: parse_string_list(body.get("stop_sequences")),
        stream: optional_bool(body.get("stream"), "stream")?.unwrap_or(false),
        metadata: BTreeMap::new(),
        output_styles_applied: false,
    })
}
