use super::parse::{CanonicalToolChoice, parse_tool_choice};
use super::request_metadata::apply_responses_metadata;
use super::*;
pub(crate) fn encode_openai_family_request(
    protocol: Protocol,
    request: &CanonicalRequest,
    model: &str,
) -> Result<Value, String> {
    if protocol != request.source_protocol
        && protocol != Protocol::Responses
        && !request.metadata.is_empty()
    {
        return Err(
            "request contains protocol-specific options that cannot be translated".to_owned(),
        );
    }
    match protocol {
        Protocol::ChatCompletions => {
            if request.messages.iter().any(|m| {
                m.content.iter().any(contains_document)
                    || matches!(m.role, Role::System | Role::Developer)
                        && m.content.iter().any(is_non_text_media)
            }) {
                return Err("Chat Completions cannot represent document inputs or media in system/developer messages".to_owned());
            }
            let messages: Vec<Value> = request.messages.iter().map(encode_chat_message).collect();
            let tools: Vec<Value> = request.tools.iter().map(|tool| json!({"type":"function","function":{"name":tool.name,"description":tool.description,"parameters":tool.parameters}})).collect();
            let mut result = json!({"model":model,"messages":messages,"stream":request.stream});
            for (key, value) in &request.metadata {
                let lower = key.to_ascii_lowercase();
                if lower.contains("key")
                    || lower.contains("token")
                    || lower.contains("authorization")
                    || lower.contains("secret")
                {
                    return Err(format!("request option '{key}' cannot be forwarded safely"));
                }
                result[key] = value.clone();
            }
            copy_options(request, &mut result, false);
            if !tools.is_empty() {
                result["tools"] = json!(tools);
            }
            if let Some(choice) = &request.tool_choice {
                result["tool_choice"] = encode_tool_choice(choice, Protocol::ChatCompletions)?;
            }
            Ok(result)
        }
        Protocol::Responses => {
            if request.messages.iter().any(|m| {
                matches!(m.role, Role::System | Role::Developer)
                    && m.content.iter().any(is_non_text_media)
            }) {
                return Err("Responses instructions cannot contain image or document blocks; place them in a user message".to_owned());
            }
            let mut input = Vec::new();
            for message in request
                .messages
                .iter()
                .filter(|m| !matches!(m.role, Role::System | Role::Developer))
            {
                let assistant_message = matches!(message.role, Role::Assistant);
                let mut content = Vec::new();
                for block in &message.content {
                    match block {
                        ContentBlock::ToolCall { id, name, arguments } => input.push(json!({"type":"function_call","call_id":id,"name":name,"arguments":arguments.to_string()})),
                        ContentBlock::ToolResult { tool_call_id, content: result } => input.push(json!({"type":"function_call_output","call_id":tool_call_id,"output":result.iter().filter_map(|b| if let ContentBlock::Text{text}=b {Some(text.as_str())} else {None}).collect::<Vec<_>>().join("\n")})),
                        ContentBlock::Reasoning { text } if assistant_message => {
                            if !text.trim().is_empty() {
                                input.push(json!({
                                    "type":"reasoning",
                                    "content":[{"type":"reasoning_text","text":text}],
                                    "summary":[]
                                }));
                            }
                        }
                        other => {
                            if let Some(value) =
                                content_to_responses_input(other, assistant_message)
                            {
                                content.push(value);
                            }
                        }
                    }
                }
                if !content.is_empty() {
                    let role = if matches!(message.role, Role::Tool) {
                        "user"
                    } else {
                        message.role.as_str()
                    };
                    input.push(json!({"role":role,"content":content}));
                }
            }
            let instructions = request
                .messages
                .iter()
                .filter(|m| matches!(m.role, Role::System | Role::Developer))
                .flat_map(|m| m.content.iter())
                .filter_map(|b| {
                    if let ContentBlock::Text { text } = b {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let mut result = json!({"model":model,"input":input,"stream":request.stream});
            if !instructions.is_empty() {
                result["instructions"] = json!(instructions);
            }
            if !request.tools.is_empty() {
                result["tools"] = json!(request.tools.iter().map(|tool| json!({"type":"function","name":tool.name,"description":tool.description,"parameters":tool.parameters})).collect::<Vec<_>>());
            }
            if let Some(choice) = &request.tool_choice {
                result["tool_choice"] = encode_tool_choice(choice, Protocol::Responses)?;
            }
            apply_responses_metadata(&request.metadata, &mut result, model)?;
            copy_options(request, &mut result, true);
            Ok(result)
        }
        Protocol::Messages => {
            if request.messages.iter().any(|m| {
                matches!(m.role, Role::System | Role::Developer)
                    && m.content.iter().any(is_non_text_media)
            }) {
                return Err("Anthropic system prompts cannot contain image or document blocks; place them in a user message".to_owned());
            }
            let system = request
                .messages
                .iter()
                .filter(|m| matches!(m.role, Role::System | Role::Developer))
                .flat_map(|m| m.content.iter())
                .filter_map(|b| {
                    if let ContentBlock::Text { text } = b {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let messages: Vec<Value> = request.messages.iter().filter(|m| matches!(m.role, Role::User | Role::Assistant | Role::Tool)).map(|m| json!({"role":if matches!(m.role, Role::Tool) {"user"} else {m.role.as_str()},"content":m.content.iter().filter_map(content_to_anthropic_request).collect::<Vec<_>>()})).collect();
            let mut result = json!({"model":model,"messages":messages,"max_tokens":request.max_tokens.unwrap_or(1024),"stream":request.stream});
            if !system.is_empty() {
                result["system"] = json!(system);
            }
            if !request.tools.is_empty() {
                result["tools"] = json!(request.tools.iter().map(|t| json!({"name":t.name,"description":t.description,"input_schema":t.parameters})).collect::<Vec<_>>());
            }
            if !request.stop.is_empty() {
                result["stop_sequences"] = json!(request.stop);
            }
            if let Some(choice) = &request.tool_choice {
                result["tool_choice"] = encode_tool_choice(choice, Protocol::Messages)?;
            }
            if let Some(v) = request.temperature {
                result["temperature"] = json!(v);
            }
            if let Some(v) = request.top_p {
                result["top_p"] = json!(v);
            }
            Ok(result)
        }
    }
}

pub(crate) fn contains_document(block: &ContentBlock) -> bool {
    match block {
        ContentBlock::Document { .. } => true,
        ContentBlock::ToolResult { content, .. } => content.iter().any(contains_document),
        _ => false,
    }
}

pub(crate) fn is_non_text_media(block: &ContentBlock) -> bool {
    matches!(
        block,
        ContentBlock::Image { .. }
            | ContentBlock::Document { .. }
            | ContentBlock::GeneratedImage { .. }
    )
}

pub(crate) fn copy_options(request: &CanonicalRequest, target: &mut Value, responses: bool) {
    if let Some(v) = request.temperature {
        target["temperature"] = json!(v);
    }
    if let Some(v) = request.top_p {
        target["top_p"] = json!(v);
    }
    if let Some(v) = request.max_tokens {
        target[if responses {
            "max_output_tokens"
        } else {
            "max_tokens"
        }] = json!(v);
    }
    if !request.stop.is_empty() && !responses {
        target["stop"] = json!(request.stop);
    }
}

pub(crate) fn encode_chat_message(message: &Message) -> Value {
    if matches!(message.role, Role::Tool)
        && let Some(ContentBlock::ToolResult {
            tool_call_id,
            content,
        }) = message.content.first()
    {
        return json!({
            "role":"tool",
            "tool_call_id":tool_call_id,
            "content":content.iter().filter_map(|b| if let ContentBlock::Text{text}=b {Some(text.as_str())} else {None}).collect::<Vec<_>>().join("\n")
        });
    }
    let assistant_message = matches!(message.role, Role::Assistant);
    let visible_content: Vec<&ContentBlock> = message
        .content
        .iter()
        .filter(|block| !assistant_message || !matches!(block, ContentBlock::Reasoning { .. }))
        .collect();
    let mut result = json!({
        "role":message.role.as_str(),
        "content":visible_content.iter().filter_map(|block| content_to_chat_request(block)).collect::<Vec<_>>()
    });
    if visible_content.len() == 1
        && let ContentBlock::Text { text } = visible_content[0]
    {
        result["content"] = json!(text);
    }
    if assistant_message {
        let reasoning_content = message
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
        if !reasoning_content.is_empty() {
            result["reasoning_content"] = json!(reasoning_content);
        }
    }
    let calls: Vec<Value> = message.content.iter().filter_map(|block| match block {
        ContentBlock::ToolCall {id,name,arguments} => Some(json!({"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}})), _ => None
    }).collect();
    if !calls.is_empty() {
        result["tool_calls"] = json!(calls);
    }
    if let Some(name) = &message.name {
        result["name"] = json!(name);
    }
    result
}

pub(crate) fn content_to_chat(block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Text { text } => Some(json!({"type":"text","text":text})),
        ContentBlock::Image { url, detail } => {
            let mut image_url = json!({"url":url});
            if let Some(detail) = detail {
                image_url["detail"] = json!(detail);
            }
            Some(json!({"type":"image_url","image_url":image_url}))
        }
        ContentBlock::GeneratedImage { data, media_type } => Some(
            json!({"type":"image_url","image_url":{"url":format!("data:{media_type};base64,{data}")}}),
        ),
        ContentBlock::Document { .. } => None,
        ContentBlock::ToolResult { content, .. } => Some(json!(
            content
                .iter()
                .filter_map(|b| if let ContentBlock::Text { text } = b {
                    Some(text.as_str())
                } else {
                    None
                })
                .collect::<Vec<_>>()
                .join("\n")
        )),
        ContentBlock::Reasoning { text } => Some(json!({"type":"text","text":text})),
        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => None,
        ContentBlock::ToolCall { .. } => None,
    }
}

pub(crate) fn content_to_chat_request(block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
            Some(internal_thinking_block(block))
        }
        _ => content_to_chat(block),
    }
}

pub(crate) fn content_to_responses_input(
    block: &ContentBlock,
    assistant_message: bool,
) -> Option<Value> {
    match block {
        ContentBlock::Text { text } => Some(json!({
            "type": if assistant_message { "output_text" } else { "input_text" },
            "text": text
        })),
        ContentBlock::Image { url, detail } => {
            if assistant_message {
                return Some(json!({"type":"output_text","text":format!("[Image: {url}]"),}));
            }
            let mut value = json!({"type":"input_image","image_url":url});
            if let Some(detail) = detail {
                value["detail"] = json!(detail);
            }
            Some(value)
        }
        ContentBlock::GeneratedImage { data, media_type } => {
            if assistant_message {
                return Some(json!({"type":"output_text","text":"[Generated image]"}));
            }
            Some(
                json!({"type":"input_image","image_url":format!("data:{media_type};base64,{data}")}),
            )
        }
        ContentBlock::Document { source, filename } => {
            if assistant_message {
                let label = filename
                    .as_deref()
                    .map(|filename| format!("[Document: {filename}]"))
                    .unwrap_or_else(|| "[Document]".to_owned());
                return Some(json!({"type":"output_text","text":label}));
            }
            let mut value = match source {
                DocumentSource::Url { url } => json!({"type":"input_file","file_url":url}),
                DocumentSource::Base64 { media_type, data } => {
                    json!({"type":"input_file","file_data":format!("data:{media_type};base64,{data}")})
                }
                DocumentSource::Text { text } => {
                    let encoded =
                        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, text);
                    json!({"type":"input_file","file_data":format!("data:text/plain;base64,{encoded}")})
                }
            };
            if let Some(filename) = filename {
                value["filename"] = json!(filename);
            }
            Some(value)
        }
        ContentBlock::ToolResult { .. } => None,
        ContentBlock::Reasoning { text } => Some(json!({
            "type": if assistant_message { "output_text" } else { "input_text" },
            "text": text
        })),
        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
            Some(internal_thinking_block(block))
        }
        ContentBlock::ToolCall { .. } => None,
    }
}

pub(crate) fn content_to_anthropic_request(block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => {
            Some(internal_thinking_block(block))
        }
        _ => content_to_anthropic(block),
    }
}

pub(crate) fn content_to_anthropic(block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Text { text } => Some(json!({"type":"text","text":text})),
        ContentBlock::Image { url, .. } if url.starts_with("data:") => {
            let (kind, data) = url.split_once(',')?;
            Some(
                json!({"type":"image","source":{"type":"base64","media_type":kind.strip_prefix("data:")?.strip_suffix(";base64")?,"data":data}}),
            )
        }
        ContentBlock::Image { url, .. } => {
            Some(json!({"type":"image","source":{"type":"url","url":url}}))
        }
        ContentBlock::GeneratedImage { data, media_type } => Some(
            json!({"type":"image","source":{"type":"base64","media_type":media_type,"data":data}}),
        ),
        ContentBlock::Document { source, filename } => {
            let mut value = match source {
                DocumentSource::Url { url } => {
                    json!({"type":"document","source":{"type":"url","url":url}})
                }
                DocumentSource::Base64 { media_type, data } => {
                    json!({"type":"document","source":{"type":"base64","media_type":media_type,"data":data}})
                }
                DocumentSource::Text { text } => {
                    json!({"type":"document","source":{"type":"text","media_type":"text/plain","data":text}})
                }
            };
            if let Some(filename) = filename {
                value["title"] = json!(filename);
            }
            Some(value)
        }
        ContentBlock::ToolResult {
            tool_call_id,
            content,
        } => Some(
            json!({"type":"tool_result","tool_use_id":tool_call_id,"content":content.iter().filter_map(|b| if let ContentBlock::Text{text}=b {Some(json!({"type":"text","text":text}))} else {None}).collect::<Vec<_>>()}),
        ),
        ContentBlock::ToolCall {
            id,
            name,
            arguments,
        } => Some(json!({"type":"tool_use","id":id,"name":name,"input":arguments})),
        ContentBlock::Reasoning { text } => Some(json!({"type":"text","text":text})),
        ContentBlock::Thinking {
            thinking,
            signature,
            extra,
        } => {
            let mut value = json!({"type":"thinking","thinking":thinking});
            if let Some(signature) = signature {
                value["signature"] = json!(signature);
            }
            extend_object_fields(&mut value, extra);
            Some(value)
        }
        ContentBlock::RedactedThinking { data, extra } => {
            let mut value = json!({"type":"redacted_thinking","data":data});
            extend_object_fields(&mut value, extra);
            Some(value)
        }
    }
}

pub(crate) fn extend_object_fields(target: &mut Value, extra: &BTreeMap<String, Value>) {
    if let Some(target) = target.as_object_mut() {
        target.extend(
            extra
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
}

pub(crate) fn encode_tool_choice(value: &Value, target: Protocol) -> Result<Value, String> {
    match (target, parse_tool_choice(value)?) {
        (_, CanonicalToolChoice::Auto) if target != Protocol::Messages => Ok(json!("auto")),
        (_, CanonicalToolChoice::None) if target != Protocol::Messages => Ok(json!("none")),
        (_, CanonicalToolChoice::Required) if target != Protocol::Messages => Ok(json!("required")),
        (Protocol::Messages, CanonicalToolChoice::Auto) => Ok(json!({"type":"auto"})),
        (Protocol::Messages, CanonicalToolChoice::None) => {
            Err("Anthropic Messages cannot represent tool_choice 'none'".to_owned())
        }
        (Protocol::Messages, CanonicalToolChoice::Required) => Ok(json!({"type":"any"})),
        (Protocol::ChatCompletions, CanonicalToolChoice::Named(name)) => {
            Ok(json!({"type":"function","function":{"name":name}}))
        }
        (Protocol::Responses, CanonicalToolChoice::Named(name)) => {
            Ok(json!({"type":"function","name":name}))
        }
        (Protocol::Messages, CanonicalToolChoice::Named(name)) => {
            Ok(json!({"type":"tool","name":name}))
        }
        _ => Err("tool_choice cannot be represented by the target protocol".to_owned()),
    }
}
