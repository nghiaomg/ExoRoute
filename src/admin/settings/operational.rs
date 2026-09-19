use super::{
    ApiResult, AppState, OperationalSettingsInput, ResetOperationalSettingsInput, fail,
    internal_message,
};
use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};

pub(crate) fn operational_settings_response(
    record: crate::infra::db::OperationalSettingsRecord,
) -> Value {
    let settings = record.settings;
    let upstream = json!({
        "server_retry_max_attempts": settings.upstream.server_retry_max_attempts,
        "server_retry_delay_first_ms": settings.upstream.server_retry_delay_first.as_millis(),
        "server_retry_delay_second_ms": settings.upstream.server_retry_delay_second.as_millis(),
        "continuity_retry_base_delay_ms": settings.upstream.continuity_retry_base_delay.as_millis(),
        "continuity_retry_max_delay_ms": settings.upstream.continuity_retry_max_delay.as_millis(),
        "gateway_body_processing_queue_timeout_ms": settings.upstream.gateway_body_processing_queue_timeout.as_millis(),
        "continuity_replay_bytes_per_run_mib": settings.upstream.continuity_replay_bytes_per_run / (1024 * 1024),
        "continuity_replay_bytes_total_mib": settings.upstream.continuity_replay_bytes_total / (1024 * 1024),
        "continuity_replay_events_per_run": settings.upstream.continuity_replay_events_per_run,
        "continuity_retained_runs": settings.upstream.continuity_retained_runs,
        "continuity_resume_page_size": settings.upstream.continuity_resume_page_size,
        "continuity_poll_interval_ms": settings.upstream.continuity_poll_interval.as_millis(),
        "continuity_heartbeat_interval_seconds": settings.upstream.continuity_heartbeat_interval.as_secs(),
        "continuity_setup_error_kib": settings.upstream.continuity_setup_error_bytes / 1024,
        "continuity_retention_seconds": settings.upstream.continuity_retention.as_secs(),
        "continuity_request_timeout_seconds": settings.upstream.continuity_request_timeout.as_secs(),
        "provider_live_events_max_duration_seconds": settings.upstream.provider_live_events_max_duration.as_secs(),
        "provider_live_events_keepalive_seconds": settings.upstream.provider_live_events_keepalive.as_secs(),
        "model_discovery_max_pages": settings.upstream.model_discovery_max_pages,
        "discovery_request_timeout_seconds": settings.upstream.discovery_request_timeout.as_secs(),
        "usage_response_limit_kib": settings.upstream.usage_response_max_bytes / 1024,
        "opencode_usage_timeout_seconds": settings.upstream.opencode_usage_timeout.as_secs(),
        "command_code_usage_timeout_seconds": settings.upstream.command_code_usage_timeout.as_secs(),
        "command_code_optional_usage_timeout_seconds": settings.upstream.command_code_optional_usage_timeout.as_secs(),
        "freebuff_auxiliary_timeout_seconds": settings.upstream.freebuff_auxiliary_timeout.as_secs(),
        "freebuff_auxiliary_response_limit_kib": settings.upstream.freebuff_auxiliary_response_max_bytes / 1024,
        "remote_image_max_count": settings.upstream.remote_image_max_count,
        "remote_image_max_mib": settings.upstream.remote_image_max_bytes / (1024 * 1024),
        "remote_image_total_max_mib": settings.upstream.remote_image_total_max_bytes / (1024 * 1024),
        "remote_image_connect_timeout_seconds": settings.upstream.remote_image_connect_timeout.as_secs(),
        "remote_image_request_timeout_seconds": settings.upstream.remote_image_request_timeout.as_secs(),
        "codex_image_preparation_timeout_seconds": settings.upstream.codex_image_preparation_timeout.as_secs(),
        "provider_client_cache_ttl_seconds": settings.upstream.provider_client_cache_ttl.as_secs(),
        "provider_client_cache_max_entries": settings.upstream.provider_client_cache_max_entries,
        "provider_client_max_resolved_addresses": settings.upstream.provider_client_max_resolved_addresses,
    });
    json!({
        "settings": {
            "connect_timeout_ms": settings.connect_timeout.as_millis(),
            "request_timeout_ms": settings.request_timeout.as_millis(),
            "stream_idle_timeout_ms": settings.stream_idle_timeout.as_millis(),
            "circuit_breaker_enabled": settings.circuit_breaker_enabled,
            "circuit_breaker_threshold": settings.circuit_breaker_threshold,
            "circuit_breaker_cooldown_seconds": settings.circuit_breaker_cooldown.as_secs(),
            "gateway_max_in_flight": settings.gateway_max_in_flight,
            "upstream_response_limit_mib": settings.upstream_response_limit_bytes / (1024 * 1024),
            "admin_api_max_requests": settings.admin_api_max_requests,
            "admin_api_window_seconds": settings.admin_api_window.as_secs(),
            "gateway_key_capacity": settings.gateway_key_capacity,
            "gateway_key_refill_tokens": settings.gateway_key_refill_tokens,
            "gateway_key_refill_interval_ms": settings.gateway_key_refill_interval.as_millis(),
            "request_log_retention_days": settings.request_log_retention_days,
            "request_log_max_rows": settings.request_log_max_rows,
            "upstream": upstream,
        },
        "revision": record.revision,
        "overridden": record.overridden,
        "source": if record.overridden { "database" } else { "environment" },
    })
}

pub(crate) async fn get_operational_settings(State(state): State<AppState>) -> ApiResult {
    Ok(Json(operational_settings_response(
        state.operational_settings(),
    )))
}

pub(crate) async fn update_operational_settings(
    State(state): State<AppState>,
    Json(input): Json<OperationalSettingsInput>,
) -> ApiResult {
    let current_upstream = state.operational_settings().settings.upstream;
    let (settings, expected_revision, confirm_unlimited) = input
        .into_settings(current_upstream)
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    if settings.gateway_max_in_flight == 0 && !confirm_unlimited {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "Confirm unlimited gateway concurrency before saving.",
        ));
    }
    let record = state
        .save_operational_settings(settings, expected_revision, true)
        .await
        .map_err(internal_message)?
        .ok_or_else(|| {
            fail(
                StatusCode::CONFLICT,
                "Operational settings changed in another session. Reload the latest values before saving.",
            )
        })?;
    Ok(Json(operational_settings_response(record)))
}

pub(crate) async fn reset_operational_settings(
    State(state): State<AppState>,
    Json(input): Json<ResetOperationalSettingsInput>,
) -> ApiResult {
    if input.expected_revision < 0 || input.expected_revision == i64::MAX {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "operational settings revision is invalid",
        ));
    }
    let settings = state
        .runtime
        .operational_env_settings
        .as_ref()
        .as_ref()
        .copied()
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message.clone()))?;
    let record = state
        .save_operational_settings(settings, input.expected_revision, false)
        .await
        .map_err(internal_message)?
        .ok_or_else(|| {
            fail(
                StatusCode::CONFLICT,
                "Operational settings changed in another session. Reload the latest values before resetting.",
            )
        })?;
    Ok(Json(operational_settings_response(record)))
}
