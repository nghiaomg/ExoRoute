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
