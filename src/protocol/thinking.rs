use super::*;

pub(super) const MAX_REASONING_CONTENT_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn parse_reasoning_content(value: Option<&Value>) -> Result<Option<String>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::String(text) => {
            if text.len() > MAX_REASONING_CONTENT_BYTES {
                return Err("reasoning_content exceeds the 4 MiB limit".to_owned());
            }
            if text.is_empty() {
                Ok(None)
            } else {
                Ok(Some(text.clone()))
            }
        }
        _ => Err("reasoning_content must be a string".to_owned()),
    }
}

pub(crate) fn parse_responses_reasoning_item(item: &Value) -> Result<Vec<ContentBlock>, String> {
    let mut content = Vec::new();
    for field in ["content", "summary"] {
        if let Some(parts) = item.get(field).and_then(Value::as_array) {
            for part in parts {
                if matches!(
                    part.get("type").and_then(Value::as_str),
                    Some("reasoning_text" | "summary_text")
                ) && let Some(text) = parse_reasoning_content(part.get("text"))?
                {
                    content.push(ContentBlock::Reasoning { text });
                }
            }
        }
    }
    Ok(content)
}

pub(crate) fn contains_thinking_block(block: &ContentBlock) -> bool {
    match block {
        ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => true,
        ContentBlock::ToolResult { content, .. } => content.iter().any(contains_thinking_block),
        _ => false,
    }
}

pub(crate) const INTERNAL_THINKING_BLOCK_TYPE: &str = "__exoroute_internal_thinking_block";

pub(crate) fn internal_thinking_block(block: &ContentBlock) -> Value {
    match block {
        ContentBlock::Thinking {
            thinking,
            signature,
            extra,
        } => {
            let mut marker = json!({
                "type": INTERNAL_THINKING_BLOCK_TYPE,
                "kind": "thinking",
                "thinking": thinking,
                "extra": extra,
            });
            if let Some(signature) = signature {
                marker["signature"] = json!(signature);
            }
            marker
        }
        ContentBlock::RedactedThinking { data, extra } => json!({
            "type": INTERNAL_THINKING_BLOCK_TYPE,
            "kind": "redacted_thinking",
            "data": data,
            "extra": extra,
        }),
        _ => Value::Null,
    }
}

pub(crate) fn finalize_thinking_payload(
    body: &mut Value,
    protocol: UpstreamProtocol,
    handling: ThinkingHandling<'_>,
) -> Result<(), String> {
    let request_is_empty = match protocol {
        UpstreamProtocol::ChatCompletions => rewrite_messages_field(
            body.get_mut("messages"),
            "content",
            Some("tool_calls"),
            protocol,
            handling,
        ),
        UpstreamProtocol::Messages => rewrite_messages_field(
            body.get_mut("messages"),
            "content",
            None,
            protocol,
            handling,
        ),
        UpstreamProtocol::Responses => {
            rewrite_messages_field(body.get_mut("input"), "content", None, protocol, handling)
        }
        UpstreamProtocol::GoogleGenerateContent => {
            let contents_empty = rewrite_messages_field(
                body.get_mut("contents"),
                "parts",
                Some("functionCall"),
                protocol,
                handling,
            );
            let remove_system_instruction =
                body.get_mut("systemInstruction")
                    .is_some_and(|instruction| {
                        let removed = rewrite_content_blocks(
                            instruction.get_mut("parts"),
                            protocol,
                            handling,
                            false,
                        );
                        removed
                            && instruction
                                .get("parts")
                                .and_then(Value::as_array)
                                .is_some_and(Vec::is_empty)
                    });
            if remove_system_instruction && let Some(object) = body.as_object_mut() {
                object.remove("systemInstruction");
            }
            contents_empty
        }
    };
    if matches!(handling, ThinkingHandling::Remove) && request_is_empty {
        return Err("removing thinking blocks left no provider messages".to_owned());
    }
    Ok(())
}

/// Rewrites only provider content-block arrays. Do not recursively walk a full
/// request: tool arguments are arbitrary user JSON and may contain objects that
/// happen to resemble the private marker.
pub(crate) fn rewrite_messages_field(
    messages: Option<&mut Value>,
    content_key: &str,
    preserve_empty_field: Option<&str>,
    protocol: UpstreamProtocol,
    handling: ThinkingHandling<'_>,
) -> bool {
    let Some(Value::Array(messages)) = messages else {
        return false;
    };
    let mut removed_any = false;
    messages.retain_mut(|message| {
        let Some(object) = message.as_object_mut() else {
            return true;
        };
        let assistant_message = protocol == UpstreamProtocol::Responses
            && object.get("role").and_then(Value::as_str) == Some("assistant");
        let removed = rewrite_content_blocks(
            object.get_mut(content_key),
            protocol,
            handling,
            assistant_message,
        );
        removed_any |= removed;
        let content_is_empty = removed
            && object
                .get(content_key)
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty);
        !(content_is_empty && preserve_empty_field.is_none_or(|field| !object.contains_key(field)))
    });
    removed_any && messages.is_empty()
}

pub(crate) fn rewrite_content_blocks(
    blocks: Option<&mut Value>,
    protocol: UpstreamProtocol,
    handling: ThinkingHandling<'_>,
    assistant_message: bool,
) -> bool {
    let Some(Value::Array(blocks)) = blocks else {
        return false;
    };
    let mut removed_any = false;
    blocks.retain_mut(|block| {
        if block.get("type").and_then(Value::as_str) != Some(INTERNAL_THINKING_BLOCK_TYPE) {
            return true;
        }
        let replacement = match handling {
            ThinkingHandling::Remove => None,
            ThinkingHandling::Preserve => match block.get("kind").and_then(Value::as_str) {
                Some("thinking") => {
                    let Some(thinking) = block.get("thinking").and_then(Value::as_str) else {
                        removed_any = true;
                        return false;
                    };
                    match protocol {
                        UpstreamProtocol::Messages => {
                            let mut preserved = json!({"type":"thinking","thinking":thinking});
                            if let Some(signature) = block.get("signature") {
                                preserved["signature"] = signature.clone();
                            }
                            copy_thinking_extra(block, &mut preserved);
                            Some(preserved)
                        }
                        UpstreamProtocol::Responses => Some(json!({
                            "type": if assistant_message { "output_text" } else { "input_text" },
                            "text": thinking
                        })),
                        UpstreamProtocol::GoogleGenerateContent => Some(json!({"text":thinking})),
                        UpstreamProtocol::ChatCompletions => {
                            Some(json!({"type":"text","text":thinking}))
                        }
                    }
                }
                Some("redacted_thinking") if protocol == UpstreamProtocol::Messages => {
                    block.get("data").and_then(Value::as_str).map(|data| {
                        let mut preserved = json!({"type":"redacted_thinking","data":data});
                        copy_thinking_extra(block, &mut preserved);
                        preserved
                    })
                }
                Some("redacted_thinking") | None | Some(_) => None,
            },
            ThinkingHandling::Override(thinking) => match protocol {
                // Replacement text must not be emitted as a signed thinking
                // block: upstream signatures are opaque and become invalid
                // as soon as their text changes.
                UpstreamProtocol::Messages => Some(json!({"type":"text","text":thinking})),
                UpstreamProtocol::Responses => Some(json!({
                    "type": if assistant_message { "output_text" } else { "input_text" },
                    "text": thinking
                })),
                UpstreamProtocol::GoogleGenerateContent => Some(json!({"text":thinking})),
                UpstreamProtocol::ChatCompletions => Some(json!({"type":"text","text":thinking})),
            },
        };
        match replacement {
            Some(replacement) => {
                *block = replacement;
                true
            }
            None => {
                removed_any = true;
                false
            }
        }
    });
    removed_any
}

pub(crate) fn copy_thinking_extra(source: &Value, target: &mut Value) {
    if let (Some(extra), Some(target)) = (
        source.get("extra").and_then(Value::as_object),
        target.as_object_mut(),
    ) {
        target.extend(
            extra
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }
}
