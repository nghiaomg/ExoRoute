#[cfg(test)]
pub(super) use crate::provider_adapters;
pub(super) use crate::{
    config::{GatewayResourceLimits, OperationalSettings},
    infra::storage::{Field, Record, StorageError, Table},
    infra::telemetry::RequestAnalytics,
    protocol::{Protocol, UpstreamProtocol},
    security::rate_limit::{RateLimitPolicy, RateLimiter, RateLimiterConfig},
    security::{secure_eq, token_hash},
    state::{AppState, RequestLiveGuard, RequestLogRecord},
};
pub(super) use axum::{
    Json,
    body::Body,
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
pub(super) use bytes::Bytes;
pub(super) use futures_util::StreamExt;
pub(super) use serde_json::{Value, json};
pub(super) use std::{
    sync::atomic::Ordering,
    sync::{Arc, LazyLock},
    time::Duration,
};

mod auth;
mod continuity;
mod error_sanitizer;
mod errors;
mod execution;
mod flow;
mod logging;
mod request_preparation;
mod retry_policy;
mod routes;
mod streaming;

use auth::authenticate_client;
pub(crate) use error_sanitizer::sanitize_provider_error_body;
use error_sanitizer::{provider_http_error_message, read_provider_error_detail};
use execution::handle_request_inner_with_adapter_base_url_override_and_limits;
pub(super) use logging::{GatewayRequestLog, log_request};
use request_preparation::{PreparedGatewayRequest, prepare_gateway_request};
use retry_policy::{can_fail_over_after_provider_rejection, is_retryable, server_retry_deadline};
use streaming::{
    StreamLog, StreamOutcome, StreamTranslationConfig, preflight_stream,
    stream_translation_from_chunks,
};

const MAX_PUBLIC_MODEL_RECORDS: usize = 100_000;
// Provider error bodies are diagnostics for the admin request log, not
// pass-through payloads: keep them bounded and redact credential/request fields.
const MAX_PROVIDER_ERROR_BODY_BYTES: usize = 64 * 1024;
const MAX_REQUEST_LOG_ERROR_CHARS: usize = 8 * 1024;

#[derive(Clone)]
pub(crate) struct GatewayRequestContext {
    resource_limits: GatewayResourceLimits,
    operational_settings: OperationalSettings,
    api_key_id: String,
    analytics: Option<RequestAnalytics>,
}

#[derive(Clone)]
struct GatewayExecutionSettings {
    resource_limits: GatewayResourceLimits,
    operational_settings: OperationalSettings,
    api_key_id: Option<String>,
    analytics: Option<RequestAnalytics>,
    stream_continuity_retry: bool,
}

static API_KEY_LAST_USED_UPDATES: LazyLock<RateLimiter> = LazyLock::new(|| {
    RateLimiter::new(RateLimiterConfig::new(
        RateLimitPolicy::FixedWindow {
            max_requests: 1,
            window: Duration::from_secs(60),
        },
        100_000,
    ))
    .expect("static API key last-used limiter settings are valid")
});
static PROVIDER_KEY_LAST_USED_UPDATES: LazyLock<RateLimiter> = LazyLock::new(|| {
    RateLimiter::new(RateLimiterConfig::new(
        RateLimitPolicy::FixedWindow {
            max_requests: 1,
            window: Duration::from_secs(60),
        },
        100_000,
    ))
    .expect("static provider key last-used limiter settings are valid")
});

// The handler layer and the request pipeline share the gateway's configuration
// and types, so they are re-exported here as one namespace.
pub(in crate::gateway) use errors::{
    attach_request_live_guard, gateway_database_error, gateway_error, rate_limited_response,
    track_analytics_response,
};
pub(in crate::gateway) use flow::*;
pub(crate) use routes::*;

#[cfg(test)]
mod tests;
