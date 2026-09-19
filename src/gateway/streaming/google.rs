use super::*;

#[derive(Clone, Debug)]
pub(crate) struct GoogleStreamToolCall {
    pub(crate) id: String,
    pub(crate) source_id: Option<String>,
    pub(crate) name: String,
    pub(crate) arguments: Value,
}

#[derive(Debug)]
pub(super) struct GoogleStreamUpdate {
    pub(super) tool_calls: Vec<GoogleStreamToolCall>,
    pub(super) finish_reason: Option<String>,
}

pub(super) fn parse_google_stream_update(value: &Value) -> Result<GoogleStreamUpdate, String> {
    if let Some(reason) = value
        .get("promptFeedback")
        .and_then(|feedback| feedback.get("blockReason"))
        .and_then(Value::as_str)
    {
        return Err(format!("Google provider blocked the prompt ({reason})"));
    }

    let candidates = match value.get("candidates") {
        Some(Value::Array(candidates)) => candidates,
        Some(_) => return Err("Google stream candidates must be an array".to_owned()),
        None => {
            return Ok(GoogleStreamUpdate {
                tool_calls: Vec::new(),
                finish_reason: None,
            });
        }
    };
    if candidates.len() > 1 {
        return Err(
            "Google stream returned multiple candidates that cannot be represented".to_owned(),
        );
    }
    let Some(candidate) = candidates.first() else {
        return Ok(GoogleStreamUpdate {
            tool_calls: Vec::new(),
            finish_reason: None,
        });
    };

    let finish_reason = match candidate.get("finishReason") {
        None | Some(Value::Null) => None,
        Some(Value::String(reason)) if !reason.is_empty() => Some(reason.clone()),
        Some(_) => return Err("Google stream finishReason must be a string".to_owned()),
    };
    let parts = candidate
        .get("content")
        .and_then(|content| content.get("parts"));
    let mut tool_calls = Vec::new();
    match parts {
        None if finish_reason.is_some() => {}
        None => {}
        Some(Value::Array(parts)) => {
            for part in parts {
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    continue;
                }
                if [
                    "inlineData",
                    "fileData",
                    "videoMetadata",
                    "executableCode",
                    "codeExecutionResult",
                    "functionResponse",
                    "toolCall",
                    "toolResponse",
                ]
                .iter()
                .any(|field| part.get(*field).is_some())
                {
                    return Err("Google stream contains an unsupported content part".to_owned());
                }
                if let Some(function_call) = part.get("functionCall") {
                    let name = function_call
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|name| !name.trim().is_empty())
                        .ok_or("Google streamed functionCall has no name")?;
                    if name.len() > 128
                        || !name.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'_' | b'-' | b':' | b'.')
                        })
                    {
                        return Err("Google streamed functionCall has an invalid name".to_owned());
                    }
                    let arguments = function_call
                        .get("args")
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    if !arguments.is_object() {
                        return Err(format!(
                            "arguments for Google function '{name}' must be a JSON object"
                        ));
                    }
                    let source_id = match function_call.get("id") {
                        None | Some(Value::Null) => None,
                        Some(Value::String(id)) if !id.is_empty() && id.len() <= 256 => {
                            Some(id.clone())
                        }
                        Some(Value::String(_)) => {
                            return Err("Google streamed functionCall id is invalid".to_owned());
                        }
                        Some(_) => {
                            return Err(
                                "Google streamed functionCall id must be a string".to_owned()
                            );
                        }
                    };
                    tool_calls.push(GoogleStreamToolCall {
                        id: String::new(),
                        source_id,
                        name: name.to_owned(),
                        arguments,
                    });
                } else if let Some(text) = part.get("text") {
                    if !text.is_string() {
                        return Err("Google stream text part must be a string".to_owned());
                    }
                } else {
                    return Err("Google stream contains an unsupported content part".to_owned());
                }
            }
        }
        Some(_) => return Err("Google stream content parts must be an array".to_owned()),
    }

    Ok(GoogleStreamUpdate {
        tool_calls,
        finish_reason,
    })
}

pub(super) fn merge_google_tool_calls(
    current: &mut Vec<GoogleStreamToolCall>,
    updates: &[GoogleStreamToolCall],
    max_bytes: usize,
) -> Result<(), String> {
    let mut anonymous_ordinals = BTreeMap::<String, usize>::new();
    for update in updates {
        if let Some(source_id) = update.source_id.as_deref() {
            if let Some(existing) = current
                .iter_mut()
                .find(|call| call.source_id.as_deref() == Some(source_id))
            {
                if existing.name != update.name {
                    return Err("Google streamed functionCall changed its name".to_owned());
                }
                existing.arguments.clone_from(&update.arguments);
                continue;
            }
        } else {
            let ordinal = anonymous_ordinals.entry(update.name.clone()).or_default();
            let matching_index = current
                .iter()
                .enumerate()
                .filter(|(_, call)| call.source_id.is_none() && call.name == update.name)
                .nth(*ordinal)
                .map(|(index, _)| index);
            *ordinal = ordinal.saturating_add(1);
            if let Some(index) = matching_index {
                current[index].arguments.clone_from(&update.arguments);
                continue;
            }
        }
        if current.len() >= 128 {
            return Err("Google stream exceeded the 128 tool call limit".to_owned());
        }
        let mut call = update.clone();
        if call.source_id.is_none() {
            call.id = format!("google_call_{}", current.len());
        } else if let Some(source_id) = call.source_id.as_ref() {
            call.id = source_id.clone();
        }
        current.push(call);
    }

    let arguments_bytes = current.iter().try_fold(0usize, |total, call| {
        serde_json::to_vec(&call.arguments)
            .ok()
            .and_then(|encoded| total.checked_add(encoded.len()))
    });
    if arguments_bytes.is_none_or(|bytes| bytes > max_bytes) {
        return Err(
            "Google streamed tool arguments exceeded the configured response limit".to_owned(),
        );
    }
    Ok(())
}

pub(super) fn normalize_google_stream_finish(
    reason: &str,
    has_tool_calls: bool,
) -> Result<String, String> {
    match reason {
        "STOP" => Ok(if has_tool_calls { "tool_calls" } else { "stop" }.to_owned()),
        "MAX_TOKENS" => Ok("length".to_owned()),
        "SAFETY"
        | "RECITATION"
        | "BLOCKLIST"
        | "PROHIBITED_CONTENT"
        | "SPII"
        | "IMAGE_SAFETY"
        | "IMAGE_PROHIBITED_CONTENT"
        | "IMAGE_RECITATION"
        | "MODEL_ARMOR"
        | "LANGUAGE" => Ok("content_filter".to_owned()),
        "MALFORMED_FUNCTION_CALL" | "UNEXPECTED_TOOL_CALL" => {
            Err(format!("Google provider returned finish reason '{reason}'"))
        }
        "OTHER" | "NO_IMAGE" | "IMAGE_OTHER" => Ok("stop".to_owned()),
        _ => Err(format!(
            "Google provider returned unknown finish reason '{reason}'"
        )),
    }
}

pub(super) fn encode_google_tool_call_events(
    client_protocol: Protocol,
    tool_calls: &[GoogleStreamToolCall],
    response_id: &str,
    model: &str,
    has_response_text: bool,
    message_text_started: bool,
) -> Result<(Vec<Bytes>, Vec<Value>), String> {
    let mut events = Vec::with_capacity(tool_calls.len().saturating_mul(4));
    let mut response_output = Vec::with_capacity(tool_calls.len());
    for (index, call) in tool_calls.iter().enumerate() {
        let arguments = serde_json::to_string(&call.arguments)
            .map_err(|_| "Google streamed function arguments could not be encoded".to_owned())?;
        match client_protocol {
            Protocol::ChatCompletions => events.push(Bytes::from(format!(
                "data: {}\n\n",
                json!({"id":response_id,"object":"chat.completion.chunk","model":model,"choices":[{"index":0,"delta":{"tool_calls":[{"index":index,"id":call.id,"type":"function","function":{"name":call.name,"arguments":arguments}}]},"finish_reason":null}]})
            ))),
            Protocol::Responses => {
                let output_index = usize::from(has_response_text) + index;
                let item_id = format!("fc_{output_index}");
                let initial_item = json!({
                    "id":item_id,
                    "type":"function_call",
                    "status":"in_progress",
                    "call_id":call.id,
                    "name":call.name,
                    "arguments":""
                });
                let final_item = json!({
                    "id":item_id,
                    "type":"function_call",
                    "status":"completed",
                    "call_id":call.id,
                    "name":call.name,
                    "arguments":arguments
                });
                events.push(Bytes::from(format!(
                    "event: response.output_item.added\ndata: {}\n\n",
                    json!({"type":"response.output_item.added","output_index":output_index,"item":initial_item})
                )));
                events.push(Bytes::from(format!(
                    "event: response.function_call_arguments.delta\ndata: {}\n\n",
                    json!({"type":"response.function_call_arguments.delta","item_id":item_id,"output_index":output_index,"delta":arguments})
                )));
                events.push(Bytes::from(format!(
                    "event: response.function_call_arguments.done\ndata: {}\n\n",
                    json!({"type":"response.function_call_arguments.done","item_id":item_id,"output_index":output_index,"arguments":arguments})
                )));
                events.push(Bytes::from(format!(
                    "event: response.output_item.done\ndata: {}\n\n",
                    json!({"type":"response.output_item.done","output_index":output_index,"item":final_item})
                )));
                response_output.push(final_item);
            }
            Protocol::Messages => {
                let output_index = usize::from(message_text_started) + index;
                events.push(Bytes::from(format!(
                    "event: content_block_start\ndata: {}\n\n",
                    json!({"type":"content_block_start","index":output_index,"content_block":{"type":"tool_use","id":call.id,"name":call.name,"input":{}}})
                )));
                events.push(Bytes::from(format!(
                    "event: content_block_delta\ndata: {}\n\n",
                    json!({"type":"content_block_delta","index":output_index,"delta":{"type":"input_json_delta","partial_json":arguments}})
                )));
                events.push(Bytes::from(format!(
                    "event: content_block_stop\ndata: {}\n\n",
                    json!({"type":"content_block_stop","index":output_index})
                )));
            }
        }
    }
    Ok((events, response_output))
}

pub(super) fn google_messages_stream_start(id: &str, model: &str, input_tokens: u64) -> String {
    format!(
        "event: message_start\ndata: {}\n\n",
        json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":input_tokens,"output_tokens":0}}})
    )
}

pub(super) fn google_messages_text_start(index: usize) -> String {
    format!(
        "event: content_block_start\ndata: {}\n\n",
        json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":""}})
    )
}

pub(super) fn google_messages_stream_text(index: usize, text: &str) -> String {
    format!(
        "event: content_block_delta\ndata: {}\n\n",
        json!({"type":"content_block_delta","index":index,"delta":{"type":"text_delta","text":text}})
    )
}

pub(super) fn google_messages_stream_finish(
    reason: &str,
    input_tokens: u64,
    output_tokens: u64,
    cost_micro_usd: Option<i64>,
    text_started: bool,
) -> String {
    let stop_reason = if reason.contains("tool") {
        "tool_use"
    } else if reason.contains("length") || reason.contains("max_tokens") {
        "max_tokens"
    } else if reason.contains("stop_sequence") {
        "stop_sequence"
    } else {
        "end_turn"
    };
    let mut output = String::new();
    if text_started {
        output.push_str(&format!(
            "event: content_block_stop\ndata: {}\n\n",
            json!({"type":"content_block_stop","index":0})
        ));
    }
    output.push_str(&format!(
        "event: message_delta\ndata: {}\n\nevent: message_stop\ndata: {}\n\n",
        json!({"type":"message_delta","delta":{"stop_reason":stop_reason,"stop_sequence":null},"usage":{"input_tokens":input_tokens,"output_tokens":output_tokens,"cost":cost_micro_usd.map(|value| value as f64 / 1_000_000.0)}}),
        json!({"type":"message_stop"})
    ));
    output
}

pub(super) fn encode_responses_image_event(
    upstream_protocol: UpstreamProtocol,
    client_protocol: Protocol,
    event: &str,
    value: &Value,
) -> Option<String> {
    if upstream_protocol != UpstreamProtocol::Responses || client_protocol != Protocol::Responses {
        return None;
    }

    let event_type = value.get("type").and_then(Value::as_str).unwrap_or(event);
    let is_image_generation_event = event_type.starts_with("response.image_generation_call.");
    let is_image_output_item_event = matches!(
        event_type,
        "response.output_item.added" | "response.output_item.done"
    ) && value
        .get("item")
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
        == Some("image_generation_call");

    if !is_image_generation_event && !is_image_output_item_event {
        return None;
    }

    let data = serde_json::to_string(value).ok()?;
    Some(format!("event: {event_type}\ndata: {data}\n\n"))
}
