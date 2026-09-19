use super::{ApiResult, AppState, GatewayResourceLimitsInput, fail, internal_message};
use crate::config::GatewayResourceLimits;
use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};

fn gateway_resource_limits_values(limits: GatewayResourceLimits) -> Value {
    json!({
        "gateway_body_limit_mib": limits.gateway_body_limit_bytes / (1024 * 1024),
        "gateway_body_processing_concurrency": limits.gateway_body_processing_concurrency,
        "sse_frame_limit_kib": limits.sse_frame_limit_bytes / 1024,
        "sse_buffer_limit_kib": limits.sse_buffer_limit_bytes / 1024,
        "provider_max_concurrency": limits.provider_max_concurrency,
        "stream_continuity_enabled": limits.stream_continuity_enabled,
        "stream_continuity_max_concurrency": limits.stream_continuity_max_concurrency,
    })
}

fn gateway_resource_limits_response(
    active: GatewayResourceLimits,
    saved: GatewayResourceLimits,
) -> Value {
    json!({
        "active": gateway_resource_limits_values(active),
        "saved": gateway_resource_limits_values(saved),
        "restart_required": active != saved,
    })
}

pub(crate) async fn get_gateway_resource_limits(State(state): State<AppState>) -> ApiResult {
    let _update_guard = state
        .runtime
        .gateway_resource_limits_update_lock
        .lock()
        .await;
    let saved = crate::infra::db::load_gateway_resource_limits(&state.db)
        .await
        .map_err(internal_message)?;
    Ok(Json(gateway_resource_limits_response(
        state.gateway_resource_limits(),
        saved,
    )))
}

pub(crate) async fn update_gateway_resource_limits(
    State(state): State<AppState>,
    Json(input): Json<GatewayResourceLimitsInput>,
) -> ApiResult {
    let update_guard = state
        .runtime
        .gateway_resource_limits_update_lock
        .clone()
        .lock_owned()
        .await;
    let saved = crate::infra::db::load_gateway_resource_limits(&state.db)
        .await
        .map_err(internal_message)?;
    let (limits, expected_saved, confirm_unlimited_provider_concurrency) = input
        .into_limits(
            saved.stream_continuity_enabled,
            saved.stream_continuity_max_concurrency,
        )
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    if expected_saved.is_some_and(|expected| expected != saved) {
        return Err(fail(
            StatusCode::CONFLICT,
            "Gateway resource limits changed in another session. Reload the latest values before saving.",
        ));
    }
    if limits.provider_max_concurrency == GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY
        && saved.provider_max_concurrency
            != GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY
        && !confirm_unlimited_provider_concurrency
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "Confirm unlimited per-provider concurrency before saving.",
        ));
    }
    state
        .spawn_gateway_resource_limits_update(limits, update_guard)
        .await
        .map_err(|error| {
            internal_message(crate::infra::storage::StorageError::Task(error.to_string()))
        })?
        .map_err(internal_message)?;
    Ok(Json(gateway_resource_limits_response(
        state.gateway_resource_limits(),
        limits,
    )))
}
