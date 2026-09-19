use super::MAX_STREAM_ERROR_CHARS;
use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ResponsesTerminal {
    Completed,
    Incomplete,
    Failed,
}

pub(super) fn responses_terminal_state(event: &str, value: &Value) -> Option<ResponsesTerminal> {
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or(event);
    let is_completed_event = event == "response.completed" || event_type == "response.completed";
    let is_done_event = event == "response.done" || event_type == "response.done";
    let is_incomplete_event = event == "response.incomplete" || event_type == "response.incomplete";
    let is_failed_event = event == "response.failed" || event_type == "response.failed";
    let is_cancelled_event = event == "response.cancelled" || event_type == "response.cancelled";
    let is_error_event = event == "error" || event_type == "error";
    let is_terminal_event = is_completed_event
        || is_done_event
        || is_incomplete_event
        || is_failed_event
        || is_cancelled_event
        || is_error_event;
    if !is_terminal_event {
        return None;
    }
    if is_failed_event || is_cancelled_event || is_error_event {
        return Some(ResponsesTerminal::Failed);
    }
    if is_incomplete_event {
        return Some(ResponsesTerminal::Incomplete);
    }
    let response = value.get("response").unwrap_or(value);
    match response.get("status").and_then(Value::as_str) {
        Some("failed") | Some("cancelled") => Some(ResponsesTerminal::Failed),
        Some("incomplete") => Some(ResponsesTerminal::Incomplete),
        _ if is_completed_event || is_done_event => Some(ResponsesTerminal::Completed),
        _ => None,
    }
}

pub(super) fn is_responses_terminal_event(event: &str, value: &Value) -> bool {
    responses_terminal_state(event, value).is_some()
}

pub(super) fn extract_stream_error(event: &str, value: &Value) -> String {
    let event_type = value.get("type").and_then(Value::as_str).unwrap_or(event);
    if matches!(
        event_type,
        "error" | "response.failed" | "response.cancelled"
    ) || matches!(event, "error" | "response.failed" | "response.cancelled")
    {
        let error = value.get("error").or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("error"))
        });
        let mut details = Vec::new();
        if let Some(error) = error {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str());
            if let Some(message) = message {
                let message = sanitize_stream_error_detail(message);
                if !message.is_empty() {
                    details.push(format!("message: {message}"));
                }
            }
            for field in ["code", "type"] {
                if let Some(detail) = error.get(field).and_then(Value::as_str) {
                    let detail = sanitize_stream_error_detail(detail);
                    if !detail.is_empty() {
                        details.push(format!("{field}: {detail}"));
                    }
                }
            }
        }
        if !details.is_empty() {
            return format!(
                "provider reported a streaming response failure: {}",
                details.join(", ")
            );
        }
    }
    "provider reported a streaming response failure".to_owned()
}

pub(super) fn sanitize_stream_error_detail(value: &str) -> String {
    let bounded = value
        .chars()
        .take(MAX_STREAM_ERROR_CHARS)
        .collect::<String>();
    let sanitized = super::super::sanitize_provider_error_body(bounded.as_bytes());
    sanitized.chars().take(MAX_STREAM_ERROR_CHARS).collect()
}
