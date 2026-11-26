use super::super::fail;
use super::{BackupExportError, DatabaseExportQuery, MAX_DATABASE_IMPORT_BYTES};
use crate::state::{AdminStepUpScope, AppState};
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Query, State},
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use serde_json::Value;
use std::io;
use tokio::io::AsyncReadExt;

pub(crate) async fn export_database(
    State(state): State<AppState>,
    Query(options): Query<DatabaseExportQuery>,
    request: axum::http::Request<Body>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    export_database_inner(State(state), Query(options), request).await
}

async fn export_database_inner(
    State(state): State<AppState>,
    Query(options): Query<DatabaseExportQuery>,
    request: axum::http::Request<Body>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let step_up_context = request
        .extensions()
        .get::<crate::state::AdminRequestContext>();
    super::super::sessions::require_step_up(
        &state,
        request.headers(),
        step_up_context,
        AdminStepUpScope::DatabaseExport,
    )
    .await?;
    let export_permit = state
        .admin
        .admin_database_export_in_flight
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "A database export is already in progress.",
            )
        })?;
    if options.include_request_logs {
        state.telemetry.flush().await.map_err(|error| {
            tracing::warn!(%error, "could not flush request history before LMDB export");
            fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "Could not save request history before creating the backup.",
            )
        })?;
    }

    let mut entries = state
        .db
        .snapshot_entries(
            options.include_request_logs,
            MAX_DATABASE_IMPORT_BYTES as usize,
        )
        .await
        .map_err(|error| {
            tracing::warn!(%error, "could not read a bounded LMDB backup snapshot");
            fail(
                StatusCode::PAYLOAD_TOO_LARGE,
                "The database exceeds the 512 MB backup limit.",
            )
        })?;
    let temp_dir =
        super::create_private_temp_dir(&state.config.app_dir, "lmdb-backup").map_err(|error| {
            tracing::error!(%error, "could not create private LMDB backup directory");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not create database backup.",
            )
        })?;
    let snapshot_path = temp_dir.join("exoroute-backup.exoroute");
    let writer_path = snapshot_path.clone();
    let (archive_len, cleanup_dir, export_permit) = tokio::task::spawn_blocking(move || {
        let cleanup_dir = super::TempDirectory(temp_dir);
        super::normalize_provider_defaults(&mut entries).map_err(BackupExportError::Normalize)?;
        let archive_len =
            super::write_backup_archive(&writer_path, entries).map_err(BackupExportError::Write)?;
        Ok::<_, BackupExportError>((archive_len, cleanup_dir, export_permit))
    })
    .await
    .map_err(|error| {
        tracing::error!(%error, "LMDB backup writer task failed");
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not create database backup.",
        )
    })?
    .map_err(|error| match error {
        BackupExportError::Normalize(message) => {
            tracing::error!(
                error = message,
                "could not normalize provider defaults for LMDB backup"
            );
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not create database backup.",
            )
        }
        BackupExportError::Write(message) => fail(StatusCode::PAYLOAD_TOO_LARGE, message),
    })?;
    let file = tokio::fs::File::open(&snapshot_path)
        .await
        .map_err(|error| {
            tracing::error!(%error, "could not open LMDB backup archive for download");
            fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not create database backup.",
            )
        })?;
    let stream = async_stream::stream! {
        let _export_permit = export_permit;
        let _cleanup = cleanup_dir;
        let mut file = file;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            match file.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => yield Ok::<Bytes, io::Error>(Bytes::copy_from_slice(&buffer[..read])),
                Err(error) => {
                    tracing::error!(%error, "error while streaming LMDB backup");
                    yield Err(error);
                    break;
                }
            }
        }
    };
    let mut response = Response::new(Body::from_stream(stream));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.exoroute.lmdb-backup"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"exoroute-backup.exoroute\""),
    );
    if let Ok(value) = HeaderValue::from_str(&archive_len.to_string()) {
        headers.insert(header::CONTENT_LENGTH, value);
    }
    Ok(response)
}
