//! HTTP boundary for the provider API-key callback.
//!
//! Request-size, origin, payload, and CORS concerns stay here; flow state
//! transitions are delegated to the sibling flow module.

use super::{
    ApiKeyCallbackAcceptance, CALLBACK_BODY_LIMIT, CALLBACK_KEY_LIMIT, accept_callback_state,
    pending_callback_adapter_id,
};
use crate::{
    provider_adapters,
    security::{encrypt_secret, token_hash},
    state::AppState,
};
use axum::{
    Json,
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, HeaderName, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub(crate) async fn callback(State(state): State<AppState>, request: Request<Body>) -> Response {
    let headers = request.headers().clone();
    let Some(origin) = callback_origin(&headers) else {
        return callback_response(
            StatusCode::FORBIDDEN,
            json!({"success":false,"error":"Origin not allowed"}),
            None,
        );
    };
    let body = match tokio::time::timeout(
        Duration::from_secs(10),
        to_bytes(request.into_body(), CALLBACK_BODY_LIMIT),
    )
    .await
    {
        Ok(Ok(body)) => body,
        Ok(Err(_)) => {
            return callback_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"success":false,"error":"Request body too large"}),
                Some(origin.clone()),
            );
        }
        Err(_) => {
            return callback_response(
                StatusCode::REQUEST_TIMEOUT,
                json!({"success":false,"error":"Request body timed out"}),
                Some(origin.clone()),
            );
        }
    };
    if body.len() > CALLBACK_BODY_LIMIT {
        return callback_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            json!({"success":false,"error":"Request body too large"}),
            Some(origin.clone()),
        );
    }
    let callback_value: Value = match serde_json::from_slice(&body) {
        Ok(payload) => payload,
        Err(_) => {
            return callback_response(
                StatusCode::BAD_REQUEST,
                json!({"success":false,"error":"Invalid callback payload"}),
                Some(origin.clone()),
            );
        }
    };
    let Some(callback_state) = callback_value.get("state").and_then(Value::as_str) else {
        return callback_response(
            StatusCode::BAD_REQUEST,
            json!({"success":false,"error":"Invalid callback payload"}),
            Some(origin.clone()),
        );
    };
    if !(32..=512).contains(&callback_state.len()) {
        return callback_response(
            StatusCode::BAD_REQUEST,
            json!({"success":false,"error":"Invalid callback payload"}),
            Some(origin.clone()),
        );
    }
    let submitted_state_hash = token_hash(callback_state);
    let adapter_id = {
        let mut flows = state.admin.provider_api_key_auth_flows.lock().await;
        pending_callback_adapter_id(&mut flows, &submitted_state_hash, Instant::now())
    };
    let Some(adapter_id) = adapter_id else {
        return callback_response(
            StatusCode::BAD_REQUEST,
            json!({"success":false,"error":"Invalid or expired state"}),
            Some(origin.clone()),
        );
    };
    if !provider_adapters::api_key_auth_assist_origin_allowed(&adapter_id, &origin) {
        return callback_response(
            StatusCode::FORBIDDEN,
            json!({"success":false,"error":"Origin not allowed for this provider"}),
            Some(origin),
        );
    }
    let payload = match provider_adapters::parse_api_key_auth_callback(&adapter_id, &body) {
        Ok(payload) => payload,
        Err(_) => {
            return callback_response(
                StatusCode::BAD_REQUEST,
                json!({"success":false,"error":"Invalid callback payload"}),
                Some(origin),
            );
        }
    };
    let api_key = payload.api_key.trim();
    if api_key.is_empty()
        || api_key.len() > CALLBACK_KEY_LIMIT
        || !(32..=512).contains(&payload.state.len())
        || payload
            .user_id
            .as_ref()
            .is_some_and(|value| value.len() > 256)
        || payload
            .user_name
            .as_ref()
            .is_some_and(|value| value.len() > 256)
        || payload
            .key_name
            .as_ref()
            .is_some_and(|value| value.len() > 256)
    {
        return callback_response(
            StatusCode::BAD_REQUEST,
            json!({"success":false,"error":"Invalid callback payload"}),
            Some(origin.clone()),
        );
    }
    let encrypted_api_key = match encrypt_secret(state.config.master_key.as_ref(), api_key) {
        Ok(Some(secret)) => secret,
        Ok(None) => {
            return callback_response(
                StatusCode::BAD_REQUEST,
                json!({"success":false,"error":"Invalid callback payload"}),
                Some(origin.clone()),
            );
        }
        Err(_) => {
            return callback_response(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"success":false,"error":"Provider key encryption is unavailable"}),
                Some(origin.clone()),
            );
        }
    };
    let now = Instant::now();
    let metadata = {
        let mut flows = state.admin.provider_api_key_auth_flows.lock().await;
        let Some(metadata) = accept_callback_state(
            &mut flows,
            ApiKeyCallbackAcceptance {
                adapter_id: &adapter_id,
                submitted_state_hash: &submitted_state_hash,
                encrypted_api_key,
                user_id: payload.user_id,
                key_name: payload.key_name,
                user_name: payload.user_name,
                now,
            },
        ) else {
            return callback_response(
                StatusCode::BAD_REQUEST,
                json!({"success":false,"error":"Invalid or expired state"}),
                Some(origin.clone()),
            );
        };
        metadata
    };
    callback_response(
        StatusCode::OK,
        json!({"success":true,"ok":true,"status":"received","metadata":metadata}),
        Some(origin),
    )
}

pub(crate) async fn preflight(headers: HeaderMap) -> Response {
    let origin = callback_origin(&headers);
    let Some(origin) = origin else {
        return callback_response(
            StatusCode::FORBIDDEN,
            json!({"success":false,"error":"Origin not allowed"}),
            None,
        );
    };
    let requested_headers = headers
        .get(header::ACCESS_CONTROL_REQUEST_HEADERS)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("content-type");
    if requested_headers
        .split(',')
        .map(str::trim)
        .any(|value| !value.eq_ignore_ascii_case("content-type"))
    {
        return callback_response(
            StatusCode::FORBIDDEN,
            json!({"success":false,"error":"Requested callback headers are not allowed"}),
            Some(origin),
        );
    }
    let mut response = callback_response(StatusCode::NO_CONTENT, Value::Null, Some(origin));
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("POST, OPTIONS"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("content-type"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("access-control-allow-private-network"),
        HeaderValue::from_static("true"),
    );
    response
}

pub(crate) fn callback_origin(headers: &HeaderMap) -> Option<String> {
    let origin = headers.get(header::ORIGIN)?.to_str().ok()?;
    provider_adapters::api_key_auth_assist_origin_registered(origin).then(|| origin.to_owned())
}

fn callback_response(status: StatusCode, body: Value, origin: Option<String>) -> Response {
    let mut response = if status == StatusCode::NO_CONTENT {
        status.into_response()
    } else {
        (status, Json(body)).into_response()
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::VARY,
        HeaderValue::from_static("Origin, Access-Control-Request-Headers"),
    );
    if let Some(origin) = origin.and_then(|origin| HeaderValue::from_str(&origin).ok()) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    }
    response
}
