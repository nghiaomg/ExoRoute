//! Freebuff request-body shaping: Buffy system prompt, model/stream
//! fields, and cost/client metadata.
//!
//! The gateway, not the client, owns these fields so prompts and internal
//! metadata cannot be overridden from outside.

use super::{AdapterRequestError, catalog::FREEBUFF_PROMPT};
use serde_json::Value;

pub(super) fn prepare_freebuff_body(
    body: &mut Value,
    model: &str,
    streaming: bool,
    client_id: &str,
    instance_id: &str,
    run_id: Option<&str>,
) -> Result<(), AdapterRequestError> {
    let object = body.as_object_mut().ok_or_else(|| {
        AdapterRequestError::new(None, None, "Freebuff request body must be a JSON object")
    })?;
    if !matches!(object.get("messages"), Some(Value::Array(_))) {
        object.insert("messages".to_owned(), Value::Array(Vec::new()));
    }
    let messages = object
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| AdapterRequestError::new(None, None, "Freebuff messages are invalid"))?;
    messages.retain(Value::is_object);
    let has_buffy_prompt = messages
        .first()
        .and_then(Value::as_object)
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .and_then(|message| message.get("content").and_then(Value::as_str))
        .is_some_and(|content| content.starts_with(FREEBUFF_PROMPT));
    if !has_buffy_prompt {
        messages.insert(
            0,
            serde_json::json!({"role":"system","content":FREEBUFF_PROMPT}),
        );
    }
    object.insert("model".to_owned(), Value::String(model.to_owned()));
    object.insert("stream".to_owned(), Value::Bool(streaming));
    if !matches!(object.get("codebuff_metadata"), Some(Value::Object(_))) {
        object.insert(
            "codebuff_metadata".to_owned(),
            Value::Object(serde_json::Map::new()),
        );
    }
    let metadata = object
        .get_mut("codebuff_metadata")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| AdapterRequestError::new(None, None, "Freebuff metadata is invalid"))?;
    metadata.insert("cost_mode".to_owned(), Value::String("free".to_owned()));
    metadata.insert("client_id".to_owned(), Value::String(client_id.to_owned()));
    metadata.insert(
        "freebuff_instance_id".to_owned(),
        Value::String(instance_id.to_owned()),
    );
    metadata.remove("run_id");
    if let Some(run_id) = run_id {
        metadata.insert("run_id".to_owned(), Value::String(run_id.to_owned()));
    }
    Ok(())
}
