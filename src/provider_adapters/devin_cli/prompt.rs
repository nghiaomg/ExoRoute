//! Request flattening and prompt synthesis for the Devin CLI transport.
//!
//! Converts OpenAI Chat Completions requests into the single text prompt
//! consumed by `devin acp`. Caller-side tools and structured response formats
//! are injected as system instructions, and multi-turn conversations (including
//! prior tool calls and tool results) are rendered as explicit message blocks.

use serde_json::Value;

/// Bound on the flattened prompt handed to the CLI.
pub(super) const MAX_PROMPT_TEXT_CHARS: usize = 1_000_000;

/// Flattens a Chat Completions request body into the single text prompt the
/// CLI turn expects. Caller-side tools and JSON schemas are noted in instructions;
/// media content is rejected rather than silently dropped.
pub(super) fn flatten_chat_messages(body: &Value) -> Result<String, String> {
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("the request body has no messages array")?;
    if messages.is_empty() {
        return Err("the request has no messages to send".to_owned());
    }

    let mut flattened = String::new();
    append_caller_tools(body, &mut flattened);
    append_response_format(body, &mut flattened);

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or("a request message is missing its role")?;
        let marker = match role {
            "system" | "developer" => "[System]",
            "user" => "[User]",
            "assistant" => "[Assistant]",
            "tool" => "[Tool]",
            other => {
                return Err(format!(
                    "the Devin CLI transport does not support the '{other}' message role"
                ));
            }
        };
        flattened.push_str(marker);
        flattened.push('\n');
        append_message_content(message, &mut flattened)?;
        if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                let name = call
                    .pointer("/function/name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                flattened.push_str("[Tool Call] ");
                flattened.push_str(name);
                flattened.push(' ');
                flattened.push_str(&truncate_chars(
                    call.pointer("/function/arguments")
                        .map(Value::to_string)
                        .unwrap_or_default()
                        .as_str(),
                    2048,
                ));
                flattened.push('\n');
            }
        }
        flattened.push_str("\n\n");
    }

    if flattened.chars().count() > MAX_PROMPT_TEXT_CHARS {
        return Err("the conversation is too large for the Devin CLI transport".to_owned());
    }
    Ok(flattened)
}

/// Extracts caller-side tool names declared in `tools` or `functions`.
pub(super) fn extract_caller_tool_names(body: &Value) -> Vec<String> {
    let mut names = Vec::new();
    let mut collect = |items: &[Value]| {
        for item in items {
            let name = item
                .pointer("/function/name")
                .and_then(Value::as_str)
                .or_else(|| item.get("name").and_then(Value::as_str))
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .filter(|n| !names.iter().any(|existing| existing == n));
            if let Some(name) = name {
                names.push(name.to_owned());
            }
        }
    };

    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        collect(tools);
    }
    if let Some(functions) = body.get("functions").and_then(Value::as_array) {
        collect(functions);
    }
    names
}

/// Whether the client passed tool definitions.
#[cfg(test)]
pub(super) fn has_client_tools(body: &Value) -> bool {
    !extract_caller_tool_names(body).is_empty()
}

fn append_caller_tools(body: &Value, out: &mut String) {
    let tool_names = extract_caller_tool_names(body);
    if !tool_names.is_empty() {
        out.push_str("[System]\nCaller-side tools are available externally: ");
        out.push_str(&tool_names.join(", "));
        out.push_str(
            ".\nTo use one, end your turn with exactly one envelope:\n\n<tool>{\"name\": \"<tool name>\", \"arguments\": {\"<argument>\": <value>}}</tool>\n\nThe envelope JSON must be valid and escape quotes inside string values. Use no other tag, do not mix narrative with the envelope, and make exactly one call per turn.\n\n",
        );
    }
}

fn append_response_format(body: &Value, out: &mut String) {
    let Some(format) = body.get("response_format").and_then(Value::as_object) else {
        return;
    };
    match format.get("type").and_then(Value::as_str) {
        Some("json_object") => {
            out.push_str("[System]\nReturn only valid JSON.\n\n");
        }
        Some("json_schema") => {
            if let Some(schema) = format.get("json_schema").and_then(|js| js.get("schema")) {
                let formatted =
                    serde_json::to_string_pretty(schema).unwrap_or_else(|_| schema.to_string());
                out.push_str("[System]\nReturn only valid JSON matching this schema:\n");
                out.push_str(&formatted);
                out.push_str("\n\n");
            }
        }
        _ => {}
    }
}

fn append_message_content(message: &Value, out: &mut String) -> Result<(), String> {
    match message.get("content") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(text)) => {
            out.push_str(text);
            Ok(())
        }
        Some(Value::Array(parts)) => {
            for part in parts {
                match part.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            out.push_str(text);
                        }
                    }
                    Some(kind) if kind.contains("image") || kind.contains("audio") => {
                        return Err(
                            "the Devin CLI transport does not support image or audio content"
                                .to_owned(),
                        );
                    }
                    other => {
                        return Err(format!(
                            "the Devin CLI transport does not understand the '{other:?}' content part"
                        ));
                    }
                }
            }
            Ok(())
        }
        Some(_) => Err("the request message content is not text".to_owned()),
    }
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
