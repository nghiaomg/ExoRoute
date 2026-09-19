//! Admin session HTTP handlers: login-adjacent logout, refresh rotation,
//! step-up reauthentication, and error mapping.
//!
//! The token state machine lives in `lifecycle.rs`; this module only adapts
//! it to Axum requests, cookies, and JSON responses.

use super::lifecycle::{family_is_active, revoke_family_in_database, rotate_refresh_token};
use super::records::digest_key;
use super::*;
use crate::{
    infra::storage::{Record, Table},
    state::{AdminRequestContext, AdminStepUpScope},
};
use axum::extract::Extension;

pub(crate) async fn logout(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    if !same_origin_browser_post(&state, &request) {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "same-origin browser request required",
        ));
    }
    let _auth_guard = state.admin.admin_auth_lock.lock().await;
    let mut family_ids = Vec::with_capacity(2);
    if let Some(token) = refresh_cookie(request.headers(), &state) {
        let hash = token_digest(&token);
        let hash_key = digest_key(&hash);
        if let Some(family_id) = state
            .db
            .read(move |transaction| {
                transaction
                    .get::<Record>(Table::AdminRefreshTokens, &hash_key)?
                    .map(|record| record.text("family_id").map(str::to_owned))
                    .transpose()
            })
            .await
            .map_err(session_database_error)?
        {
            family_ids.push(family_id);
        }
    }
    let access_token = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let access_session = if let Some(token) = access_token {
        state.admin_session(token).await
    } else {
        None
    };
    if let Some(session) = access_session {
        family_ids.push(session.family_id);
    }
    family_ids.sort_unstable();
    family_ids.dedup();
    for family_id in family_ids {
        revoke_family_in_database(&state, &family_id)
            .await
            .map_err(session_database_error)?;
    }
    let mut response = Json(json!({"ok":true})).into_response();
    clear_refresh_cookie(&mut response, &state, &request)?;
    Ok(response)
}

pub(crate) async fn refresh(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    if !same_origin_browser_post(&state, &request) {
        return Err(fail(
            StatusCode::FORBIDDEN,
            "same-origin browser request required",
        ));
    }
    let Some(refresh_token) = refresh_cookie(request.headers(), &state) else {
        let mut response = refresh_invalid_response();
        clear_refresh_cookie(&mut response, &state, &request)?;
        return Ok(response);
    };
    let peer_ip = crate::security::client_ip::client_ip(&request, state.config.trust_proxy_headers);
    let decision = state.admin.admin_refresh_limiter.check(&peer_ip);
    if !decision.allowed {
        return Ok(super::super::errors::admin_rate_limited_response(
            "admin session refresh rate limit exceeded",
            decision,
        ));
    }
    let _auth_guard = state.admin.admin_auth_lock.lock().await;
    match rotate_refresh_token(&state, &refresh_token)
        .await
        .map_err(session_database_error)?
    {
        RefreshResult::Rotated(session) => {
            let mut response = Json(json!({
                "authenticated": true,
                "access_token": session.access_token,
                "access_expires_in_seconds": ACCESS_TOKEN_TTL_SECONDS,
                "must_change_password": session.must_change_password
            }))
            .into_response();
            set_refresh_cookie(&mut response, &state, &request, &session)?;
            Ok(response)
        }
        RefreshResult::Invalid => {
            let mut response = refresh_invalid_response();
            clear_refresh_cookie(&mut response, &state, &request)?;
            Ok(response)
        }
    }
}

fn refresh_invalid_response() -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error":{"code":"admin_refresh_invalid","message":"admin refresh session is no longer valid","type":"authentication_error"}})),
    )
        .into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReauthInput {
    password: String,
    scope: String,
}

pub(crate) async fn reauthenticate(
    State(state): State<AppState>,
    Extension(context): Extension<AdminRequestContext>,
    mut request: Request<Body>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let input = super::super::auth::parse_auth_json::<ReauthInput>(&mut request).await?;
    let scope = match input.scope.as_str() {
        "database_export" => AdminStepUpScope::DatabaseExport,
        "database_import" => AdminStepUpScope::DatabaseImport,
        _ => {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "unsupported reauthentication scope",
            ));
        }
    };
    if input.password.is_empty() || input.password.chars().count() > 128 {
        return Err(reauth_password_invalid());
    }
    let _password_work_permit = state
        .admin
        .admin_login_in_flight
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "admin password service is busy; retry shortly",
            )
        })?;
    let _auth_guard = state.admin.admin_auth_lock.lock().await;
    let family_is_active = family_is_active(&state, &context.family_id)
        .await
        .map_err(session_database_error)?;
    if !family_is_active {
        return Err(fail(StatusCode::UNAUTHORIZED, "admin session has expired"));
    }
    let encoded_hash = state
        .db
        .read(|transaction| {
            transaction
                .get::<Record>(Table::AdminAuth, "singleton")?
                .map(|record| record.text("password_hash").map(str::to_owned))
                .transpose()
        })
        .await
        .map_err(session_database_error)?;
    let Some(encoded_hash) = encoded_hash else {
        return Err(reauth_password_invalid());
    };
    if !super::super::auth::verify_admin_password_async(encoded_hash, input.password)
        .await
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "admin password verification is busy",
            )
        })?
    {
        return Err(reauth_password_invalid());
    }
    let proof = state
        .issue_admin_step_up_proof(context.family_id, scope)
        .await
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "secure random source is unavailable",
            )
        })?;
    let mut response =
        Json(json!({"step_up_token":proof,"expires_in_seconds":300})).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn reauth_password_invalid() -> (StatusCode, Json<Value>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(
            json!({"error":{"code":"admin_password_invalid","message":"admin password is incorrect","type":"authentication_error"}}),
        ),
    )
}

pub(crate) async fn require_step_up(
    state: &AppState,
    headers: &HeaderMap,
    context: Option<&AdminRequestContext>,
    scope: AdminStepUpScope,
) -> Result<(), (StatusCode, Json<Value>)> {
    let Some(context) = context else {
        return Err(step_up_required());
    };
    let Some(proof) = headers
        .get("x-exoroute-step-up")
        .and_then(|value| value.to_str().ok())
    else {
        return Err(step_up_required());
    };
    if state
        .consume_admin_step_up_proof(&context.family_id, proof, scope)
        .await
    {
        Ok(())
    } else {
        Err(step_up_required())
    }
}

fn step_up_required() -> (StatusCode, Json<Value>) {
    (
        StatusCode::FORBIDDEN,
        Json(
            json!({"error":{"code":"admin_reauth_required","message":"reauthenticate before using this database operation","type":"authorization_error"}}),
        ),
    )
}

pub(crate) fn session_database_error(error: StorageError) -> (StatusCode, Json<Value>) {
    if matches!(error, StorageError::Busy | StorageError::MapFull) {
        tracing::warn!(%error, "LMDB is busy during admin session state update");
        fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "admin session storage is busy; retry shortly",
        )
    } else {
        internal(error)
    }
}
