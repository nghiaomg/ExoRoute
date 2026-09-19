use super::super::fail;
use super::{MAX_DATABASE_IMPORT_BYTES, TempDirectory, create_private_temp_dir};
use crate::state::{AdminStepUpScope, AppState};
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use tokio::io::AsyncWriteExt;

pub(crate) async fn import_database(
    State(state): State<AppState>,
    mut request: axum::http::Request<Body>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let step_up_context = request
        .extensions()
        .get::<crate::state::AdminRequestContext>()
        .cloned();
    super::super::sessions::require_step_up(
        &state,
        request.headers(),
        step_up_context.as_ref(),
        AdminStepUpScope::DatabaseImport,
    )
    .await?;
    let clear_cookie_secure = super::super::cookie_policy::cookie_is_secure(&state, &request);
    let confirm_unlimited = request
        .headers()
        .get("x-exoroute-ack-unlimited-concurrency")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("true"));
    let _import_permit = state
        .admin
        .admin_database_import_in_flight
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "A database import is already in progress.",
            )
        })?;
    if request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > MAX_DATABASE_IMPORT_BYTES)
    {
        return Err(fail(
            StatusCode::PAYLOAD_TOO_LARGE,
            "The database file exceeds the 512 MB upload limit.",
        ));
    }

    let import_path = receive_import_upload(&state, &mut request).await?;
    let temp_dir = import_path
        .parent()
        .map(|parent| parent.to_path_buf())
        .unwrap_or_else(|| state.config.app_dir.clone());
    let _cleanup = TempDirectory(temp_dir);

    let auth_guard = state
        .admin
        .admin_auth_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "Admin authentication is busy; retry the database import shortly.",
            )
        })?;
    let family_is_active = if let Some(context) = step_up_context.as_ref() {
        super::super::sessions::family_is_active(&state, &context.family_id)
            .await
            .map_err(super::super::sessions::session_database_error)?
    } else {
        false
    };
    if !family_is_active {
        return Err(fail(StatusCode::UNAUTHORIZED, "admin session has expired"));
    }
    state.telemetry.flush().await.map_err(|error| {
        tracing::warn!(%error, "could not flush request history before LMDB import");
        fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "Could not save request history before importing the backup.",
        )
    })?;

    let previous_required = state
        .admin
        .admin_password_change_required
        .load(std::sync::atomic::Ordering::Acquire);
    state
        .admin
        .admin_password_change_required
        .store(true, std::sync::atomic::Ordering::Release);
    let import_state = state.clone();
    let import_permit = _import_permit;
    let cleanup = _cleanup;
    let import_task = super::spawn_restore_reconciliation_task(
        import_state,
        import_path.clone(),
        confirm_unlimited,
        previous_required,
        auth_guard,
        import_permit,
        cleanup,
    );
    match import_task.await {
        Ok(Ok(())) => {}
        Ok(Err((true, message))) => {
            return Err(fail(StatusCode::SERVICE_UNAVAILABLE, message));
        }
        Ok(Err((false, message))) => return Err(fail(StatusCode::BAD_REQUEST, message)),
        Err(error) => {
            tracing::error!(%error, "database restore task failed");
            return Err(fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "Database restore is still being reconciled. Retry login after the operation finishes.",
            ));
        }
    }
    let mut response = Json(json!({"ok":true,"authentication_reset":true})).into_response();
    super::super::cookie_policy::clear_refresh_cookie_for_scheme(
        &mut response,
        &state,
        clear_cookie_secure,
    )?;
    Ok(response)
}

async fn receive_import_upload(
    state: &AppState,
    request: &mut axum::http::Request<Body>,
) -> Result<PathBuf, (StatusCode, Json<Value>)> {
    let temp_dir =
        create_private_temp_dir(&state.config.app_dir, "lmdb-import").map_err(|error| {
            tracing::error!(%error, "could not create private LMDB import directory");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read the database upload.",
            )
        })?;
    let import_path = temp_dir.join("exoroute-import.exoroute");
    let mut options = tokio::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let mut file = options.open(&import_path).await.map_err(|error| {
        tracing::error!(%error, "could not create private LMDB import file");
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the database upload.",
        )
    })?;
    let mut uploaded_bytes = 0_u64;
    let upload_deadline = tokio::time::Instant::now() + Duration::from_secs(15 * 60);
    let body = std::mem::replace(request.body_mut(), Body::empty());
    let mut chunks = body.into_data_stream();
    loop {
        let next_chunk = tokio::time::timeout_at(
            upload_deadline,
            tokio::time::timeout(Duration::from_secs(60), chunks.next()),
        )
        .await;
        let chunk = match next_chunk {
            Err(_) => {
                return Err(fail(
                    StatusCode::REQUEST_TIMEOUT,
                    "The database upload exceeded the 15-minute time limit.",
                ));
            }
            Ok(Err(_)) => {
                return Err(fail(
                    StatusCode::REQUEST_TIMEOUT,
                    "The database upload was idle for more than 60 seconds.",
                ));
            }
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
        };
        let chunk = chunk.map_err(|error| {
            tracing::error!(%error, "error while receiving LMDB import upload");
            fail(
                StatusCode::BAD_REQUEST,
                "Could not read the database upload.",
            )
        })?;
        uploaded_bytes = uploaded_bytes.saturating_add(chunk.len() as u64);
        if uploaded_bytes > MAX_DATABASE_IMPORT_BYTES {
            return Err(fail(
                StatusCode::PAYLOAD_TOO_LARGE,
                "The database file exceeds the 512 MB upload limit.",
            ));
        }
        file.write_all(&chunk).await.map_err(|error| {
            tracing::error!(%error, "could not write temporary LMDB import file");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read the database upload.",
            )
        })?;
    }
    file.flush().await.map_err(|error| {
        tracing::error!(%error, "could not flush temporary LMDB import file");
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not read the database upload.",
        )
    })?;
    drop(file);
    if uploaded_bytes == 0 {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "The database backup is empty.",
        ));
    }
    Ok(import_path)
}
