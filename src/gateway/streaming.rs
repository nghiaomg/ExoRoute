use super::{GatewayRequestLog, log_request};
use crate::{
    config::GatewayResourceLimits,
    infra::telemetry::RequestAnalytics,
    protocol::{Protocol, UpstreamProtocol},
    provider_adapters::ResponsesStreamAccumulator,
    state::{AppState, DynamicSemaphore, DynamicSemaphorePermit, RequestLiveGuard},
};
use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, header},
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;

mod encode;
mod errors;
mod extract;
mod finalize;
mod google;
mod messages;
mod preflight;
mod reader;
mod translate;
mod translation;

use google::*;

#[cfg(test)]
use errors::sanitize_stream_error_detail;
use errors::{
    ResponsesTerminal, extract_stream_error, is_responses_terminal_event, responses_terminal_state,
};

pub(super) type UpstreamChunkStream =
    Pin<Box<dyn futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

const STREAM_OUTCOME_COMPLETED: u8 = 1;
const STREAM_OUTCOME_FAILED: u8 = 2;
const MAX_STREAM_ERROR_CHARS: usize = 2 * 1024;
const MAX_MESSAGES_STREAM_BLOCKS: usize = 128;

#[derive(Clone, Default)]
pub(super) struct StreamOutcome(Arc<AtomicU8>);

impl StreamOutcome {
    pub(super) fn complete(&self) {
        self.0.store(STREAM_OUTCOME_COMPLETED, Ordering::Release);
    }

    pub(super) fn fail(&self) {
        self.0.store(STREAM_OUTCOME_FAILED, Ordering::Release);
    }

    pub(super) fn failed(&self) -> bool {
        self.0.load(Ordering::Acquire) == STREAM_OUTCOME_FAILED
    }

    pub(super) fn completed(&self) -> bool {
        self.0.load(Ordering::Acquire) == STREAM_OUTCOME_COMPLETED
    }
}

pub(super) struct StreamLog {
    pub(super) state: AppState,
    pub(super) request_id: String,
    pub(super) route_alias: String,
    pub(super) provider_id: String,
    pub(super) model: String,
    pub(super) api_key_id: Option<String>,
    pub(super) provider_credential_id: Option<String>,
    pub(super) client_protocol: Protocol,
    pub(super) upstream_protocol: UpstreamProtocol,
    pub(super) started: Instant,
    pub(super) run_id: Option<String>,
    pub(super) live: RequestLiveGuard,
    // Keep the provider's bulkhead slot until the downstream consumes or
    // drops the full upstream stream.
    pub(super) _provider_permit: DynamicSemaphorePermit,
    pub(super) circuit_probe: crate::state::CircuitProbePermit,
    pub(super) sse_processing_slots: std::sync::Arc<DynamicSemaphore>,
}

pub(super) struct StreamTranslationConfig {
    pub(super) upstream_protocol: UpstreamProtocol,
    pub(super) client_protocol: Protocol,
    pub(super) request_id: String,
    pub(super) model: String,
    pub(super) idle_timeout: Duration,
    pub(super) continuity_enabled: bool,
    pub(super) overall_timeout: Option<Duration>,
    pub(super) outcome: StreamOutcome,
    pub(super) analytics: Option<RequestAnalytics>,
    pub(super) resource_limits: GatewayResourceLimits,
    pub(super) shutdown: watch::Receiver<bool>,
    pub(super) log: Option<StreamLog>,
}
// This tree is one implementation unit: the translation state machine, the
// Messages block encoder, the SSE preflight, the upstream decoders, and the
// client encoders compose directly, so they share a single namespace here.
pub(crate) use encode::*;
pub(crate) use extract::*;
pub(crate) use finalize::*;
pub(crate) use messages::*;
pub(crate) use preflight::*;
pub(crate) use reader::*;
pub(crate) use translate::*;

#[cfg(test)]
mod tests;
