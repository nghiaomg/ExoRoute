use super::MAX_PROVIDER_ERROR_BODY_BYTES;
use crate::provider_adapters;
use axum::http::StatusCode;
use serde_json::Value;

pub(super) async fn read_provider_error_detail(
    response: reqwest::Response,
    response_limit_bytes: usize,
) -> String {
    let max_bytes = response_limit_bytes.min(MAX_PROVIDER_ERROR_BODY_BYTES);
    match provider_adapters::read_limited_response(response, max_bytes).await {
        Ok(body) => sanitize_provider_error_body(&body),
        Err(error) => format!("provider error body unavailable: {error}"),
    }
}

pub(super) fn provider_http_error_message(
    provider_id: &str,
    status: StatusCode,
    detail: &str,
) -> String {
    let detail = detail.trim();
    if detail.is_empty() {
        format!("provider '{provider_id}' returned HTTP {}", status.as_u16())
    } else {
        format!(
            "provider '{provider_id}' returned HTTP {}: {detail}",
            status.as_u16()
        )
    }
}

/// How much of a provider's own diagnostic text is kept in a client-visible
/// decode-failure message. Long provider messages are truncated, never echoed
/// in full into an API response.
const MAX_PROVIDER_DIAGNOSTIC_CHARS: usize = 300;

/// Builds the client-visible message for a provider response that could not be
/// decoded: the decoder's own summary plus the provider's diagnostic when the
/// payload carries one. Providers answer an undecodable body with HTTP 200
/// often enough that the decode summary alone hides the real cause; the
/// diagnostic is redacted like every other provider error body and bounded, so
/// credential material and request content never reach the client.
pub(crate) fn provider_decode_failure_message(decode_error: &str, body: &Value) -> String {
    provider_failure_message(
        &format!("could not decode provider response: {decode_error}"),
        body,
    )
}

/// Appends the provider's own sanitized diagnostic to a provider failure
/// summary, so a body that arrives in the wrong shape still tells the caller
/// what the provider reported. Payloads without a diagnostic keep `summary`.
pub(crate) fn provider_failure_message(summary: &str, body: &Value) -> String {
    match provider_response_diagnostic(body) {
        Some(detail) => format!("{summary} (provider said: {detail})"),
        None => summary.to_owned(),
    }
}

/// The provider's own diagnostic from a decoded JSON body, redacted to one
/// whitespace-normalized line and bounded; `None` when the payload reports no
/// error of its own.
fn provider_response_diagnostic(body: &Value) -> Option<String> {
    let text = ["error", "detail", "message", "error_description"]
        .iter()
        .find_map(|field| provider_diagnostic_text(body.get(*field)))?;
    let text = redact_provider_error_text(text);
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut bounded: String = text.chars().take(MAX_PROVIDER_DIAGNOSTIC_CHARS).collect();
    if text.chars().nth(MAX_PROVIDER_DIAGNOSTIC_CHARS).is_some() {
        bounded.push_str("...");
    }
    Some(bounded)
}

/// The diagnostic a provider error field carries: a plain string, or the
/// `message` of an error object.
fn provider_diagnostic_text(value: Option<&Value>) -> Option<&str> {
    match value? {
        Value::String(text) => Some(text.as_str()),
        Value::Object(fields) => fields.get("message").and_then(Value::as_str),
        _ => None,
    }
}

pub(crate) fn sanitize_provider_error_body(body: &[u8]) -> String {
    if body.is_empty() {
        return String::new();
    }
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        return serde_json::to_string(&redact_provider_error_value(value))
            .unwrap_or_else(|_| "provider returned an unreadable JSON error body".to_owned());
    }
    std::str::from_utf8(body)
        .map(redact_provider_error_text)
        .unwrap_or_else(|_| {
            format!(
                "provider returned a non-text error body ({} bytes)",
                body.len()
            )
        })
}

fn redact_provider_error_value(value: Value) -> Value {
    match value {
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, value)| {
                    let value = if provider_error_field_is_sensitive(&key) {
                        Value::String("[redacted]".to_owned())
                    } else {
                        redact_provider_error_value(value)
                    };
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(redact_provider_error_value)
                .collect(),
        ),
        Value::String(text) => Value::String(redact_provider_error_text(&text)),
        value => value,
    }
}

fn provider_error_field_is_sensitive(field: &str) -> bool {
    let normalized = field
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "apikey"
            | "authorization"
            | "token"
            | "accesstoken"
            | "refreshtoken"
            | "password"
            | "secret"
            | "clientsecret"
            | "credential"
            | "credentials"
            | "cookie"
            | "cookies"
            | "setcookie"
            | "prompt"
            | "input"
            | "inputs"
            | "messages"
            | "content"
            | "contents"
            | "request"
            | "requestbody"
            | "body"
            | "headers"
    )
}

fn redact_provider_error_text(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut redact_next_tokens = 0usize;
    for (index, word) in text.split_whitespace().enumerate() {
        if index > 0 {
            result.push(' ');
        }
        if redact_next_tokens > 0 {
            result.push_str("[redacted]");
            redact_next_tokens -= 1;
            continue;
        }
        let separator = word.find('=').or_else(|| word.find(':'));
        if let Some(separator) = separator {
            let key = &word[..separator];
            if provider_error_field_is_sensitive(key) {
                result.push_str(key);
                result.push_str("=[redacted]");
                if word[separator + 1..].is_empty() {
                    redact_next_tokens = if key.eq_ignore_ascii_case("authorization") {
                        2
                    } else {
                        1
                    };
                }
                continue;
            }
        }
        if word.eq_ignore_ascii_case("bearer") || word.eq_ignore_ascii_case("basic") {
            result.push_str("[redacted]");
            redact_next_tokens = 1;
        } else {
            result.push_str(word);
        }
    }
    result
}
