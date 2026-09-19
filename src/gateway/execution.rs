use super::request_preparation::GatewayRequestPreparation;
use super::{
    GatewayExecutionSettings, GatewayRequestLog, GatewayResourceLimits, OperationalSettings,
    PreparedGatewayRequest, RequestAnalytics, RequestLiveGuard, StreamLog, StreamOutcome,
    StreamTranslationConfig, attach_request_live_guard, can_fail_over_after_provider_rejection,
    gateway_database_error, gateway_error, is_retryable, log_request, preflight_stream,
    prepare_gateway_request, provider_http_error_message, read_provider_error_detail,
    stream_translation_from_chunks,
};
use crate::{
    infra::storage::{Record, StorageError, Table},
    protocol::{self, Protocol, UpstreamProtocol},
    provider_adapters::{self, AdapterSseError, ProviderAdapter},
    security::egress,
    state::{AppState, DynamicSemaphorePermit},
};
use axum::{
    Json,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

mod classify;
mod credential_rotation;
mod dispatch;
mod errors;
mod log_payload;
mod orchestrator;
mod outcome;
mod provider;
mod respond;
mod send;
mod target_preparation;

use super::request_preparation::Target;
use credential_rotation::CredentialRotation;
use errors::TargetLoopFailures;
use log_payload::RequestLogPayload;
use orchestrator::Orchestrator;
use outcome::DispatchOutcome;
use provider::{ProviderUpstreamResult, mark_provider_key_invalid, mark_provider_key_used};
use provider_adapters::keys::ProviderApiKeyCursor;
use target_preparation::AttemptContext;

fn retry_after_duration(value: &HeaderValue) -> Option<Duration> {
    value
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|seconds| Duration::from_secs(seconds.min(24 * 60 * 60)))
}

fn is_quota_rejection(adapter_id: &str, status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || (adapter_id == provider_adapters::ANTIGRAVITY_ADAPTER_ID
            && status == StatusCode::CONFLICT)
}

/// Facade for the gateway execution pipeline. The request lifecycle is
/// intentionally kept here as a small boundary; preparation, credential
/// rotation, dispatch, response decoding, and logging live in dedicated
/// execution stages.
pub(super) async fn handle_request_inner_with_adapter_base_url_override_and_limits(
    state: AppState,
    headers: HeaderMap,
    body: Arc<Value>,
    client_protocol: Protocol,
    adapter_base_url_override: Option<&str>,
    execution_settings: GatewayExecutionSettings,
    target_rotation_offset: usize,
) -> Response {
    Orchestrator {
        state,
        headers,
        body,
        client_protocol,
        adapter_base_url_override: adapter_base_url_override.map(str::to_owned),
        resource_limits: execution_settings.resource_limits,
        operational_settings: execution_settings.operational_settings,
        api_key_id: execution_settings.api_key_id,
        analytics: execution_settings.analytics,
        target_rotation_offset,
    }
    .run()
    .await
}
