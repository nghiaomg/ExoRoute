//! Admin authentication middleware.
//!
//! Owns login rate limiting, session validation, and per-request API quota
//! so route handlers never repeat the auth preamble.

use super::errors::admin_rate_limited_response;
use crate::state::AppState;
use axum::{
    Json,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;

pub(crate) async fn rate_limit_admin_login(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let peer_ip = crate::security::client_ip::client_ip(&request, state.config.trust_proxy_headers);
    let decision = state.admin.admin_login_limiter.check(&peer_ip);
    if !decision.allowed {
        return admin_rate_limited_response("admin login rate limit exceeded", decision);
    }
    next.run(request).await
}

pub(crate) async fn require_admin(
    State(state): State<AppState>,
    mut request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let peer_ip = crate::security::client_ip::client_ip(&request, state.config.trust_proxy_headers);

    let presented = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .or_else(|| {
            request
                .headers()
                .get("x-admin-key")
                .and_then(|h| h.to_str().ok())
        })
        .map(str::to_owned);
    let session = match presented.as_deref() {
        Some(token) => state.admin_session(token).await,
        None => None,
    };
    let Some(session) = session else {
        // Older unit tests exercise handler behavior without configuring admin auth.
        // Production requests always require a login-issued in-memory session.
        #[cfg(test)]
        if state.admin_key.read().await.is_none() {
            let has_stored_auth = crate::infra::db::has_admin_auth(&state.db)
                .await
                .unwrap_or(true);
            if !has_stored_auth {
                return next.run(request).await;
            }
        }
        let decision = state.admin.admin_auth_failure_limiter.check(&peer_ip);
        if !decision.allowed {
            return admin_rate_limited_response(
                "admin authentication failure rate limit exceeded",
                decision,
            );
        }
        return unauthorized_response();
    };
    let Some(session_identity) = presented.as_deref() else {
        return unauthorized_response();
    };
    // Access tokens are cached for ten minutes, but persisted family expiry and
    // revocation must take effect on the next admin request.
    match super::sessions::family_is_active(&state, &session.family_id).await {
        Ok(true) => {}
        Ok(false) => {
            state.revoke_admin_family(&session.family_id).await;
            return unauthorized_response();
        }
        Err(error) => {
            let (status, body) = super::sessions::session_database_error(error);
            return (status, body).into_response();
        }
    }
    request
        .extensions_mut()
        .insert(crate::state::AdminRequestContext {
            family_id: session.family_id.clone(),
        });
    let session_identity = URL_SAFE_NO_PAD.encode(super::sessions::token_digest(session_identity));
    let decision = {
        let runtime = state
            .runtime
            .operational_settings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.admin.admin_api_limiter.check_with_policy(
            &session_identity,
            crate::security::rate_limit::RateLimitPolicy::FixedWindow {
                max_requests: runtime.settings.admin_api_max_requests,
                window: runtime.settings.admin_api_window,
            },
        )
    };
    if !decision.allowed {
        return admin_rate_limited_response("admin API rate limit exceeded", decision);
    }
    if session.must_change_password
        && !(request.method() == axum::http::Method::PUT
            && request.uri().path() == "/api/v1/admin/settings/admin-password")
    {
        return (StatusCode::FORBIDDEN, Json(json!({"error":{"code":"admin_password_change_required","message":"change the admin password before using the dashboard","must_change_password":true}}))).into_response();
    }
    next.run(request).await
}

fn unauthorized_response() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({"error":{"code":"admin_session_expired","message":"admin authentication required","type":"authentication_error"}}))).into_response()
}
