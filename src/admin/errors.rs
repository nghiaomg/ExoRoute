//! Shared admin error constructors.
//!
//! All dashboard handlers map domain failures through these helpers so
//! status codes and JSON shapes stay consistent across modules.

use crate::infra::storage::StorageError;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

pub(crate) type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

pub(crate) fn fail(status: StatusCode, message: impl Into<String>) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error":{"message":message.into()}})))
}

pub(crate) fn internal<E>(error: E) -> (StatusCode, Json<Value>)
where
    E: std::error::Error + 'static,
{
    tracing::error!(%error, "admin database operation failed");
    if let Some(response) = storage_error_response(&error) {
        return response;
    }
    fail(
        StatusCode::INTERNAL_SERVER_ERROR,
        "An internal database operation failed.",
    )
}

pub(crate) fn internal_message<E>(error: E) -> (StatusCode, Json<Value>)
where
    E: std::fmt::Display + std::any::Any,
{
    tracing::error!(%error, "admin operation failed");
    if let Some(response) = storage_error_response(&error) {
        return response;
    }
    fail(
        StatusCode::INTERNAL_SERVER_ERROR,
        "An internal database operation failed.",
    )
}

pub(crate) fn storage_error_response(
    error: &dyn std::any::Any,
) -> Option<(StatusCode, Json<Value>)> {
    let error = error.downcast_ref::<StorageError>()?;
    let (status, message) = match error {
        StorageError::Busy => (
            StatusCode::SERVICE_UNAVAILABLE,
            "The database is busy. Retry this request shortly.",
        ),
        StorageError::MapFull => (
            StatusCode::SERVICE_UNAVAILABLE,
            "The database has reached its configured storage capacity.",
        ),
        StorageError::Io(_) | StorageError::Backend(_) | StorageError::Task(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Database storage is temporarily unavailable. Retry this request shortly.",
        ),
        StorageError::Conflict => (
            StatusCode::CONFLICT,
            "The database record changed concurrently.",
        ),
        _ => return None,
    };
    Some(fail(status, message))
}

pub(crate) fn resource_id(name: &str) -> String {
    let slug = name
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        format!("resource-{}", uuid::Uuid::new_v4().simple())
    } else {
        slug
    }
}

pub(crate) fn admin_rate_limited_response(
    message: &str,
    decision: crate::security::rate_limit::RateLimitDecision,
) -> Response {
    use axum::http::{HeaderValue, header};
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({"error":{"code":"rate_limited","message":message,"type":"rate_limit_error"}})),
    )
        .into_response();
    if let Some(delay) = decision.retry_after {
        let seconds = delay
            .as_secs()
            .saturating_add(u64::from(delay.subsec_nanos() > 0))
            .max(1);
        if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_errors_map_consistently_without_exposing_backend_details() {
        let responses = [
            (
                internal(StorageError::Busy),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                internal(StorageError::MapFull),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                internal(StorageError::Io(std::io::Error::other(
                    "sensitive file system detail",
                ))),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                internal_message(StorageError::Task("sensitive worker detail".to_owned())),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (internal(StorageError::Conflict), StatusCode::CONFLICT),
            (
                internal_message(StorageError::Busy),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                internal_message(StorageError::MapFull),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                internal_message(StorageError::Conflict),
                StatusCode::CONFLICT,
            ),
        ];

        for ((status, Json(body)), expected_status) in responses {
            assert_eq!(status, expected_status);
            let message = body["error"]["message"]
                .as_str()
                .expect("an API error message is returned");
            assert!(!message.contains("LMDB"));
            assert!(!message.contains("configured map limit"));
            assert!(!body.to_string().contains("sensitive file system detail"));
            assert!(!body.to_string().contains("sensitive worker detail"));
        }
    }

    #[test]
    fn unknown_admin_errors_keep_the_generic_server_error_response() {
        let (status, Json(body)) =
            internal_message("sensitive storage backend diagnostic".to_owned());

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            body["error"]["message"],
            "An internal database operation failed."
        );
        assert!(
            !body
                .to_string()
                .contains("sensitive storage backend diagnostic")
        );
    }
}
