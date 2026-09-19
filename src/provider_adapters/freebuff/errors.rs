//! Freebuff error shaping: session-error messages, conflict errors, and
//! bounded upstream-field extraction.
//!
//! Keeps user-facing diagnostics separate from the request lifecycle so
//! message wording can change without touching networking code.

use super::catalog::MAX_FREEBUFF_ID_BYTES;
use super::{AdapterRequestError, model_test_provider_response_body};
use http::StatusCode;
use serde_json::Value;

pub(super) fn freebuff_session_error(status: StatusCode, body: &[u8]) -> (String, Option<String>) {
    let provider_response_body = model_test_provider_response_body(body);
    let payload = serde_json::from_slice::<Value>(body).ok();
    let code = payload
        .as_ref()
        .and_then(|value| freebuff_error_field(value, &["code", "status", "error"]))
        .map(|value| value.to_ascii_lowercase());
    let current_model = payload
        .as_ref()
        .and_then(|value| freebuff_error_field(value, &["currentModel", "current_model"]));
    let requested_model = payload
        .as_ref()
        .and_then(|value| freebuff_error_field(value, &["requestedModel", "requested_model"]));
    let message = match code.as_deref() {
        Some("model_locked") | Some("session_model_mismatch") => {
            match (current_model, requested_model) {
                (Some(current), Some(requested)) => format!(
                    "Freebuff session is locked to model '{current}', but '{requested}' was requested (HTTP {})",
                    status.as_u16()
                ),
                (Some(current), None) => format!(
                    "Freebuff session is locked to model '{current}' (HTTP {})",
                    status.as_u16()
                ),
                _ => format!(
                    "Freebuff session model is already locked by the upstream (HTTP {})",
                    status.as_u16()
                ),
            }
        }
        Some("session_superseded") => format!(
            "Freebuff session was superseded by another client (HTTP {})",
            status.as_u16()
        ),
        Some("session_limit_reached") => format!(
            "Freebuff session limit was reached; finish the active session before retrying (HTTP {})",
            status.as_u16()
        ),
        _ => provider_response_body
            .as_deref()
            .filter(|detail| !detail.trim().is_empty())
            .map(|detail| {
                format!(
                    "Freebuff session returned HTTP {}: {detail}",
                    status.as_u16()
                )
            })
            .unwrap_or_else(|| format!("Freebuff session returned HTTP {}", status.as_u16())),
    };
    (message, provider_response_body)
}

pub(super) fn cached_freebuff_session_conflict(
    current_model: &str,
    requested_model: &str,
) -> AdapterRequestError {
    AdapterRequestError::new(
        Some(StatusCode::CONFLICT),
        None,
        format!(
            "Freebuff session is already locked to model '{current_model}', but '{requested_model}' was requested; finish the active session or use the locked model"
        ),
    )
}

pub(super) fn freebuff_error_field(value: &Value, keys: &[&str]) -> Option<String> {
    let object = value.as_object()?;
    for key in keys {
        if let Some(field) = object.get(*key).and_then(Value::as_str) {
            let field = field.trim();
            if !field.is_empty()
                && field.len() <= MAX_FREEBUFF_ID_BYTES
                && !field.chars().any(char::is_control)
            {
                return Some(field.to_owned());
            }
        }
    }
    object
        .get("error")
        .and_then(Value::as_object)
        .and_then(|error| {
            keys.iter().find_map(|key| {
                error.get(*key).and_then(Value::as_str).and_then(|field| {
                    let field = field.trim();
                    (!field.is_empty()
                        && field.len() <= MAX_FREEBUFF_ID_BYTES
                        && !field.chars().any(char::is_control))
                    .then(|| field.to_owned())
                })
            })
        })
}
