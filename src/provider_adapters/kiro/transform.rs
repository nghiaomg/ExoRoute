//! OpenAI Chat Completions → Kiro `conversationState` translation.
//!
//! Ported from OmniRoute's `openai-to-kiro` translator: system/tool messages
//! fold into user turns, consecutive same-role content merges, assistant tool
//! calls become `toolUses`, tool results become `toolResults` on the next user
//! message, and `tools` attach as `toolSpecification` entries on the first
//! user turn. The last user turn becomes `currentMessage`; the rest is
//! `history`. The conversation id is a deterministic UUID derived from the
//! first content so AWS Builder ID context caching keeps working.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const KIRO_NAMESPACE: Uuid = Uuid::from_u128(0x34f7193f_561d_4050_bc84_9547d953d6bf);

/// Translates an OpenAI chat completions body into the Kiro request payload.
pub(super) fn build_payload(upstream_body: &Value, model: &str) -> Result<Value, String> {
    let messages = upstream_body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("Kiro request has no messages")?;
    if messages.is_empty() || messages.len() > 10_000 {
        return Err("Kiro request has an invalid message list".to_owned());
    }
    let tools = upstream_body.get("tools").and_then(Value::as_array);
    let (history, current_message) = convert_messages(messages, tools, model)?;

    let mut final_content = current_message
        .get("userInputMessage")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    final_content = format!(
        "[Context: Current time is {}]\n\n{}",
        rfc3339_now(),
        final_content
    );

    let first_content = history
        .first()
        .and_then(|item| item.pointer("/userInputMessage/content"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| final_content.clone());
    let conversation_id = deterministic_uuid(
        first_content
            .chars()
            .take(4000)
            .collect::<String>()
            .as_bytes(),
    )
    .to_string();

    let mut user_input = json!({
        "content": final_content,
        "modelId": model,
        "origin": "AI_EDITOR",
    });
    if let Some(context) = current_message
        .get("userInputMessage")
        .and_then(|message| message.get("userInputMessageContext"))
    {
        user_input["userInputMessageContext"] = context.clone();
    }

    let mut payload = json!({
        "conversationState": {
            "chatTriggerType": "MANUAL",
            "conversationId": conversation_id,
            "currentMessage": { "userInputMessage": user_input },
            "history": history,
        },
    });

    if let Some(arn) = upstream_body
        .get("_kiro_profile_arn")
        .and_then(Value::as_str)
        .filter(|arn| !arn.is_empty())
    {
        payload["profileArn"] = json!(arn);
    }

    let mut inference = serde_json::Map::new();
    if let Some(max_tokens) = upstream_body
        .get("max_completion_tokens")
        .or_else(|| upstream_body.get("max_tokens"))
        .and_then(Value::as_u64)
    {
        inference.insert("maxTokens".to_owned(), json!(max_tokens));
    }
    if let Some(temperature) = upstream_body.get("temperature").and_then(Value::as_f64) {
        inference.insert("temperature".to_owned(), json!(temperature));
    }
    if let Some(top_p) = upstream_body.get("top_p").and_then(Value::as_f64) {
        inference.insert("topP".to_owned(), json!(top_p));
    }
    if !inference.is_empty() {
        payload["inferenceConfig"] = Value::Object(inference);
    }
    Ok(payload)
}

/// RFC 3339 UTC timestamp without a datetime dependency.
fn rfc3339_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let secs_of_day = secs % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60,
        secs_of_day % 60
    )
}

/// Deterministic 128-bit id derived from content. AWS only needs a stable id
/// for context caching; SHA-256 truncated to 16 bytes with v5/variant bits set
/// keeps the `uuid` feature set unchanged.
fn deterministic_uuid(name: &[u8]) -> Uuid {
    let mut hasher_input = Vec::with_capacity(KIRO_NAMESPACE.as_bytes().len() + name.len());
    hasher_input.extend_from_slice(KIRO_NAMESPACE.as_bytes());
    hasher_input.extend_from_slice(name);
    let digest = Sha256::digest(&hasher_input);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[derive(Default)]
struct Pending {
    user: Vec<String>,
    assistant: Vec<String>,
    tool_results: Vec<Value>,
    role: Option<&'static str>,
}

fn flush(pending: &mut Pending, history: &mut Vec<Value>, current: &mut Option<Value>) {
    match pending.role {
        Some("user") => {
            let content = pending.user.join("\n\n").trim().to_owned();
            let content = if content.is_empty() {
                "continue".to_owned()
            } else {
                content
            };
            let mut message = json!({
                "userInputMessage": { "content": content, "modelId": "" }
            });
            if !pending.tool_results.is_empty() {
                message["userInputMessage"]["userInputMessageContext"] =
                    json!({ "toolResults": pending.tool_results });
            }
            history.push(message.clone());
            *current = Some(message);
        }
        Some("assistant") => {
            let content = pending.assistant.join("\n\n").trim().to_owned();
            let content = if content.is_empty() {
                "...".to_owned()
            } else {
                content
            };
            history.push(json!({ "assistantResponseMessage": { "content": content } }));
        }
        _ => {}
    }
    pending.user.clear();
    pending.assistant.clear();
    pending.tool_results.clear();
    pending.role = None;
}

fn message_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        part.get("content")
                            .and_then(|inner| inner.get("text").and_then(Value::as_str))
                    })
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn tool_specs(tools: Option<&Vec<Value>>) -> Vec<Value> {
    tools
        .map(|list| {
            list.iter()
                .take(128)
                .filter_map(|tool| {
                    let function = tool.get("function");
                    let name = function
                        .and_then(|f| f.get("name"))
                        .and_then(Value::as_str)
                        .or_else(|| tool.get("name").and_then(Value::as_str))?;
                    let description = function
                        .and_then(|f| f.get("description"))
                        .and_then(Value::as_str)
                        .or_else(|| tool.get("description").and_then(Value::as_str))
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("Tool: {name}"));
                    let parameters = function
                        .and_then(|f| f.get("parameters"))
                        .or_else(|| tool.get("parameters"))
                        .or_else(|| tool.get("input_schema"))
                        .cloned()
                        .unwrap_or_else(|| json!({}));
                    Some(json!({
                        "toolSpecification": {
                            "name": name,
                            "description": description,
                            "inputSchema": { "json": parameters },
                        }
                    }))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn convert_messages(
    messages: &[Value],
    tools: Option<&Vec<Value>>,
    model: &str,
) -> Result<(Vec<Value>, Value), String> {
    let specs = tool_specs(tools);
    let mut history: Vec<Value> = Vec::new();
    let mut current: Option<Value> = None;
    let mut pending = Pending::default();
    let mut specs_attached = specs.is_empty();
    let mut pending_tool_specs: Option<Vec<Value>> = None;

    for message in messages {
        let raw_role = message.get("role").and_then(Value::as_str).unwrap_or("");
        // Kiro folds system and tool roles into user turns.
        let role = if raw_role == "assistant" {
            "assistant"
        } else {
            "user"
        };
        if pending.role.is_some() && pending.role != Some(role) {
            flush(&mut pending, &mut history, &mut current);
        }
        pending.role = Some(role);

        if role == "user" {
            let content = message.get("content");
            let mut text = content.map(message_text).unwrap_or_default();
            if let Some(parts) = content.and_then(Value::as_array) {
                for part in parts {
                    if part.get("type").and_then(Value::as_str) == Some("tool_result") {
                        let tool_text = part
                            .get("content")
                            .map(|inner| {
                                if let Some(inner_text) = inner.as_str() {
                                    inner_text.to_owned()
                                } else {
                                    message_text(inner)
                                }
                            })
                            .unwrap_or_default();
                        pending.tool_results.push(json!({
                            "toolUseId": part
                                .get("tool_use_id")
                                .and_then(Value::as_str)
                                .unwrap_or(""),
                            "status": "success",
                            "content": [{ "text": tool_text }],
                        }));
                    }
                }
            }
            if raw_role == "tool" {
                pending.tool_results.push(json!({
                    "toolUseId": message
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .unwrap_or(""),
                    "status": "success",
                    "content": [{ "text": text }],
                }));
                text.clear();
            }
            if !text.trim().is_empty() {
                pending.user.push(text);
            }
        } else {
            let text = message.get("content").map(message_text).unwrap_or_default();
            if !text.trim().is_empty() {
                pending.assistant.push(text.trim().to_owned());
            }
            if let Some(calls) = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .filter(|calls| !calls.is_empty())
            {
                flush(&mut pending, &mut history, &mut current);
                let tool_uses: Vec<Value> = calls
                    .iter()
                    .take(64)
                    .map(|call| {
                        let function = call.get("function");
                        let name = function
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .or_else(|| call.get("name").and_then(Value::as_str))
                            .unwrap_or("");
                        let arguments = function
                            .and_then(|f| f.get("arguments"))
                            .cloned()
                            .unwrap_or_else(|| call.get("input").cloned().unwrap_or(json!({})));
                        let input = match arguments {
                            Value::String(text) => serde_json::from_str::<Value>(&text)
                                .unwrap_or_else(|_| json!({ "_raw": text })),
                            other => other,
                        };
                        json!({
                            "toolUseId": call.get("id").and_then(Value::as_str).unwrap_or(""),
                            "name": name,
                            "input": input,
                        })
                    })
                    .collect();
                if let Some(last) = history.last_mut() {
                    if last.get("assistantResponseMessage").is_some() {
                        last["assistantResponseMessage"]["toolUses"] = json!(tool_uses);
                    }
                }
            }
        }

        // Attach tool specifications to the first user turn.
        if !specs_attached && role == "user" {
            specs_attached = true;
            if !specs.is_empty() {
                if let Some(last) = history.last_mut() {
                    attach_tool_specs(last, &specs);
                } else {
                    pending_tool_specs = Some(specs.clone());
                }
            }
        }
    }
    if pending.role.is_some() {
        flush(&mut pending, &mut history, &mut current);
    }
    if let Some(specs) = pending_tool_specs.take() {
        if let Some(first_user) = history
            .iter_mut()
            .find(|item| item.get("userInputMessage").is_some())
        {
            attach_tool_specs(first_user, &specs);
        }
    }

    // The last user turn becomes the current message.
    if let Some(position) = history
        .iter()
        .rposition(|item| item.get("userInputMessage").is_some())
    {
        let message = history.remove(position);
        current = Some(message);
    }
    let mut current = current.ok_or("Kiro request has no user message")?;

    // OmniRoute moves tool specs onto the current message when the first user
    // turn became the current message; mirror that so tools still reach Kiro.
    if !specs.is_empty()
        && current
            .pointer("/userInputMessage/userInputMessageContext/tools")
            .is_none()
    {
        let has_specs_in_history = history.iter().any(|item| {
            item.pointer("/userInputMessage/userInputMessageContext/tools")
                .is_some()
        });
        if !has_specs_in_history {
            current["userInputMessage"]["userInputMessageContext"]["tools"] = json!(specs);
        }
    }

    // Fill modelId on history turns and drop empty contexts.
    for item in history.iter_mut() {
        if let Some(message) = item.get_mut("userInputMessage") {
            if message.get("modelId").and_then(Value::as_str) == Some("") {
                message["modelId"] = json!(model);
            }
            if let Some(context) = message.get_mut("userInputMessageContext") {
                if context
                    .get("tools")
                    .and_then(Value::as_array)
                    .is_some_and(|list| list.is_empty())
                {
                    if let Some(map) = context.as_object_mut() {
                        map.remove("tools");
                    }
                }
                if context.as_object().is_some_and(|map| map.is_empty()) {
                    if let Some(map) = message.as_object_mut() {
                        map.remove("userInputMessageContext");
                    }
                }
            }
        }
    }
    // Current message modelId/origin are set by the caller.
    Ok((history, current))
}

fn attach_tool_specs(message: &mut Value, specs: &[Value]) {
    if let Some(user) = message.get_mut("userInputMessage") {
        if user.get("userInputMessageContext").is_none() {
            user["userInputMessageContext"] = json!({});
        }
        user["userInputMessageContext"]["tools"] = json!(specs);
    }
}
