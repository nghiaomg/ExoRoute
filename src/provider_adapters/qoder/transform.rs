//! Request transformation from OpenAI Chat Completions format to Qoder's native JSON shape.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn extract_text_from_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => {
            let mut out = Vec::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    out.push(text);
                } else if let Some(s) = part.as_str() {
                    out.push(s);
                }
            }
            out.join("\n")
        }
        _ => String::new(),
    }
}

/// Normalizes messages by hoisting system messages into a single string.
fn normalize_messages(incoming: &[Value]) -> (Vec<Value>, String) {
    let mut system_parts = Vec::new();
    let mut filtered = Vec::new();

    for msg in incoming {
        let role = msg.get("role").and_then(Value::as_str).unwrap_or("");
        if role == "system" {
            let text = extract_text_from_content(msg.get("content").unwrap_or(&Value::Null));
            if !text.is_empty() {
                system_parts.push(text);
            }
        } else {
            let mut clean_msg = msg.clone();
            if let Some(obj) = clean_msg.as_object_mut() {
                // Ensure content is simplified text if not array of image parts
                if let Some(content) = obj.get("content") {
                    let text = extract_text_from_content(content);
                    if !content.is_array() {
                        obj.insert("content".to_owned(), Value::String(text));
                    }
                }
            }
            filtered.push(clean_msg);
        }
    }

    (filtered, system_parts.join("\n\n"))
}

fn last_user_text(messages: &[Value]) -> String {
    for msg in messages.iter().rev() {
        if msg.get("role").and_then(Value::as_str) == Some("user") {
            return extract_text_from_content(msg.get("content").unwrap_or(&Value::Null));
        }
    }
    String::new()
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_owned()
    } else {
        let mut truncated: String = s.chars().take(max_len).collect();
        truncated.push_str("...");
        truncated
    }
}

fn stable_hash(prefix: &str, a: &str, b: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix.as_bytes());
    hasher.update(b"\0");
    hasher.update(a.as_bytes());
    hasher.update(b"\0");
    hasher.update(b.as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    hex.chars().take(16).collect()
}

pub(crate) fn canonical_model_key(model: &str) -> &str {
    model
        .strip_prefix("qd/")
        .or_else(|| model.strip_prefix("qdcn/"))
        .or_else(|| model.strip_prefix("qoder/"))
        .unwrap_or(model)
}

pub(crate) fn transform_to_qoder_payload(
    body: &Value,
    model: &str,
    user_id: &str,
) -> Result<Value, String> {
    let model_key = canonical_model_key(model);

    let empty_vec = Vec::new();
    let incoming_messages = body
        .get("messages")
        .and_then(Value::as_array)
        .unwrap_or(&empty_vec);

    let (messages, system_text) = normalize_messages(incoming_messages);
    let last_user = last_user_text(&messages);

    let max_tokens = body
        .get("max_tokens")
        .or_else(|| body.get("max_completion_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(32768);

    let tools = body
        .get("tools")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let is_reasoning = model_key.contains("r1")
        || model_key.contains("reasoning")
        || model_key.contains("thinking");

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let req_id = uuid::Uuid::new_v4().to_string();
    let session_id = stable_hash("qoder-session", user_id, model_key);
    let record_id = stable_hash("qoder-record", model_key, &req_id);

    let qoder_payload = json!({
        "request_id": req_id,
        "request_set_id": record_id,
        "chat_record_id": record_id,
        "session_id": session_id,
        "stream": true,
        "chat_task": "FREE_INPUT",
        "is_reply": true,
        "is_retry": false,
        "source": 1,
        "version": "3",
        "session_type": "qodercli",
        "agent_id": "agent_common",
        "task_id": "common",
        "code_language": "",
        "chat_prompt": "",
        "image_urls": null,
        "aliyun_user_type": "",
        "system": system_text,
        "messages": messages,
        "tools": tools,
        "parameters": {
            "max_tokens": max_tokens
        },
        "chat_context": {
            "chatPrompt": "",
            "imageUrls": null,
            "extra": {
                "context": [],
                "modelConfig": {
                    "key": model_key,
                    "is_reasoning": is_reasoning
                },
                "originalContent": last_user
            },
            "features": [],
            "text": last_user
        },
        "model_config": {
            "key": model_key,
            "source": "system",
            "is_reasoning": is_reasoning
        },
        "business": {
            "product": "cli",
            "version": "1.0.0",
            "type": "agent",
            "stage": "start",
            "id": uuid::Uuid::new_v4().to_string(),
            "name": truncate_str(&last_user, 30),
            "begin_at": now_ms
        }
    });

    Ok(qoder_payload)
}
