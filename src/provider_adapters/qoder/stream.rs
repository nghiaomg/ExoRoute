//! SSE event-stream decoding and envelope unwrapping for Qoder.
//!
//! Qoder wraps streaming events inside `{statusCodeValue: 200, body: "..."}`.
//! This module provides unwrapping for both live streaming and non-streaming responses.

use crate::provider_adapters::sse::{AdapterSseError, read_limited_response};
use bytes::Bytes;
use serde_json::{Value, json};

/// Unwraps a Qoder `{statusCodeValue, body}` SSE envelope into standard OpenAI SSE payload.
pub(crate) fn unwrap_qoder_envelope(value: &Value) -> Result<Option<Value>, String> {
    let Some(obj) = value.as_object() else {
        return Ok(Some(value.clone()));
    };

    if let Some(status_val) = obj.get("statusCodeValue").and_then(Value::as_i64) {
        if status_val != 200 {
            let msg = obj
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or("Qoder upstream error");
            return Err(format!("Qoder upstream returned HTTP {status_val}: {msg}"));
        }
        if let Some(body) = obj.get("body") {
            if let Some(body_str) = body.as_str() {
                let trimmed = body_str.trim();
                if trimmed == "[DONE]" {
                    return Ok(None);
                }
                match serde_json::from_str::<Value>(trimmed) {
                    Ok(inner) => return Ok(Some(inner)),
                    Err(_) => return Ok(None),
                }
            } else if body.is_object() {
                return Ok(Some(body.clone()));
            }
        }
    }

    Ok(Some(value.clone()))
}

/// Reads a full SSE stream from Qoder and aggregates it into a single non-streaming completion response.
pub(crate) async fn read_qoder_event_stream(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<Value, AdapterSseError> {
    let bytes: Bytes = read_limited_response(response, max_bytes)
        .await
        .map_err(|e| AdapterSseError::new(e, false))?;

    let text = std::str::from_utf8(&bytes)
        .map_err(|_| AdapterSseError::new("Qoder stream contains invalid UTF-8", false))?;

    let mut accumulated_content = String::new();
    let mut finish_reason = None;
    let mut last_id = None;
    let mut last_model = None;
    let mut last_created = None;
    let mut accumulated_usage = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("data:") {
            continue;
        }
        let payload = trimmed.strip_prefix("data:").unwrap_or("").trim_start();
        if payload == "[DONE]" {
            break;
        }
        let Ok(parsed) = serde_json::from_str::<Value>(payload) else {
            continue;
        };
        let unwrapped = match unwrap_qoder_envelope(&parsed) {
            Ok(Some(v)) => v,
            Ok(None) => break,
            Err(err) => return Err(AdapterSseError::new(err, false)),
        };

        if let Some(id) = unwrapped.get("id").and_then(Value::as_str) {
            last_id = Some(id.to_owned());
        }
        if let Some(model) = unwrapped.get("model").and_then(Value::as_str) {
            last_model = Some(model.to_owned());
        }
        if let Some(created) = unwrapped.get("created").and_then(Value::as_u64) {
            last_created = Some(created);
        }
        if let Some(usage) = unwrapped.get("usage").filter(|u| u.is_object()) {
            accumulated_usage = Some(usage.clone());
        }

        if let Some(first) = unwrapped
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        {
            if let Some(delta) = first.get("delta") {
                if let Some(content) = delta.get("content").and_then(Value::as_str) {
                    accumulated_content.push_str(content);
                }
                if let Some(reason) = delta.get("finish_reason").and_then(Value::as_str) {
                    finish_reason = Some(reason.to_owned());
                }
            }
            if let Some(reason) = first.get("finish_reason").and_then(Value::as_str) {
                finish_reason = Some(reason.to_owned());
            }
        }
    }

    let id = last_id.unwrap_or_else(|| format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()));
    let model = last_model.unwrap_or_else(|| "qoder".to_owned());
    let created = last_created.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });

    let completion = json!({
        "id": id,
        "object": "chat.completion",
        "created": created,
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": accumulated_content
            },
            "finish_reason": finish_reason.unwrap_or_else(|| "stop".to_owned())
        }],
        "usage": accumulated_usage.unwrap_or_else(|| json!({
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        }))
    });

    Ok(completion)
}
