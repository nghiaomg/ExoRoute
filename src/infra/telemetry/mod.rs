use crate::{
    infra::db,
    infra::storage::{Database, Field, Record, StorageError, Table, validate_key},
    state::RequestLogRecord,
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot, watch};

mod aggregate;
mod analytics;
mod maintenance;
mod persistence;
mod queries;
mod retention;
mod store;
mod windows;
pub(crate) use queries::{
    api_key_statistics_snapshot, provider_usage_meter_snapshot, statistics_snapshot,
};
pub(crate) use retention::{cursor_record, expire_provider_usage, expire_statistics};
use store::{persist_batch, provider_usage_counts, valid_provider_credential_id};
pub(crate) use windows::{
    add_aggregate, add_counts, api_key_model_dimension_id, cache_ratio_percent, counts_record,
    minute_key, record_counts, top_index_key, update_window_counts, window_key,
};

use aggregate::*;
use persistence::telemetry_writer;

pub const STATISTICS_RANGES: [(i64, &str); 3] =
    [(24 * 60, "1d"), (7 * 24 * 60, "7d"), (30 * 24 * 60, "30d")];

const TELEMETRY_QUEUE_CAPACITY: usize = 8_192;
const TELEMETRY_BATCH_SIZE: usize = 128;
const TELEMETRY_FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const STATISTICS_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(5);
const MAX_EXPIRY_ROWS_PER_RANGE: usize = 512;
const MINUTE_RETENTION: i64 = 31 * 24 * 60;
const PROVIDER_METER_RETENTION_MINUTES: i64 = 30 * 24 * 60;
const MAX_PROVIDER_METER_READ_ROWS: usize = PROVIDER_METER_RETENTION_MINUTES as usize + 1;
const MAX_PROVIDER_METER_EXPIRY_ROWS: usize = 2_048;
const STATISTICS_TOP_PREFIX: &str = "top/";

#[derive(Clone)]
pub struct Telemetry {
    sender: mpsc::Sender<TelemetryMessage>,
    counters: Arc<DropCounters>,
    dropped_notify: watch::Sender<u64>,
    drop_state_lock: Arc<AsyncMutex<()>>,
}

#[derive(Default)]
pub struct DropCounters {
    request_logs: AtomicU64,
    api_key_usage: AtomicU64,
    provider_usage: AtomicU64,
    statistics: AtomicU64,
    total: AtomicU64,
    persisted_this_process: AtomicU64,
    persisted_provider_usage_this_process: AtomicU64,
}

impl DropCounters {
    pub fn request_logs(&self) -> u64 {
        self.request_logs.load(Ordering::Relaxed)
    }

    pub fn api_key_usage(&self) -> u64 {
        self.api_key_usage.load(Ordering::Relaxed)
    }

    pub fn provider_usage(&self) -> u64 {
        self.provider_usage.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    pub fn unpersisted_total(&self) -> u64 {
        self.total()
            .saturating_sub(self.persisted_this_process.load(Ordering::Relaxed))
    }
}

#[derive(Clone)]
pub struct RequestAnalytics {
    inner: Arc<RequestAnalyticsInner>,
}

struct RequestAnalyticsInner {
    telemetry: Telemetry,
    api_key_id: String,
    started: Instant,
    requested_model: Mutex<Option<String>>,
    input_tokens: AtomicI64,
    output_tokens: AtomicI64,
    cached_tokens: AtomicI64,
    cache_input_tokens: AtomicI64,
    finished: AtomicBool,
    deferred: AtomicBool,
}

pub struct AnalyticsCompletionGuard {
    analytics: RequestAnalytics,
    finished: bool,
}

#[derive(Clone, Copy)]
enum DropKind {
    RequestLog,
    ApiKeyUsage,
    ProviderUsage,
    Statistics,
}

enum TelemetryMessage {
    RequestLog(Box<RequestLogRecord>),
    ApiKeyUsage(String),
    ProviderUsage(ProviderUsageEvent),
    Statistics(StatisticsEvent),
    FlushHistory(oneshot::Sender<Result<(), String>>),
    ClearHistory(oneshot::Sender<Result<i64, String>>),
}

struct ProviderUsageEvent {
    credential_id: String,
    minute: i64,
    cost_micro_usd: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
}

impl Telemetry {
    pub fn new(db: Database) -> Self {
        let (sender, receiver) = mpsc::channel(TELEMETRY_QUEUE_CAPACITY);
        let (dropped_notify, dropped_receiver) = watch::channel(0_u64);
        let counters = Arc::new(DropCounters::default());
        let drop_state_lock = Arc::new(AsyncMutex::new(()));
        tokio::spawn(telemetry_writer(
            db,
            receiver,
            dropped_receiver,
            dropped_notify.clone(),
            counters.clone(),
            drop_state_lock.clone(),
        ));
        Self {
            sender,
            counters,
            dropped_notify,
            drop_state_lock,
        }
    }

    pub fn drop_counters(&self) -> Arc<DropCounters> {
        self.counters.clone()
    }

    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }

    pub fn start_request(&self, api_key_id: String) -> RequestAnalytics {
        RequestAnalytics {
            inner: Arc::new(RequestAnalyticsInner {
                telemetry: self.clone(),
                api_key_id,
                started: Instant::now(),
                requested_model: Mutex::new(None),
                input_tokens: AtomicI64::new(0),
                output_tokens: AtomicI64::new(0),
                cached_tokens: AtomicI64::new(0),
                cache_input_tokens: AtomicI64::new(0),
                finished: AtomicBool::new(false),
                deferred: AtomicBool::new(false),
            }),
        }
    }

    pub fn enqueue_request_log(&self, record: RequestLogRecord) {
        self.enqueue(
            TelemetryMessage::RequestLog(Box::new(record)),
            DropKind::RequestLog,
        );
    }

    pub fn record_api_key_request(&self, api_key_id: String) {
        self.enqueue(
            TelemetryMessage::ApiKeyUsage(api_key_id),
            DropKind::ApiKeyUsage,
        );
    }

    /// Enqueues one bounded local-cost event. The event contains no secret and
    /// is deliberately separate from request-history logging so a dropped log
    /// cannot hide provider spend from the meter.
    pub fn record_provider_usage(
        &self,
        credential_id: String,
        cost_micro_usd: Option<i64>,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
    ) {
        let credential_id = credential_id.trim();
        if !valid_provider_credential_id(credential_id) {
            self.note_drop(DropKind::ProviderUsage);
            return;
        }
        let cost_micro_usd = cost_micro_usd.filter(|value| *value >= 0);
        let input_tokens = input_tokens.filter(|value| *value >= 0);
        let output_tokens = output_tokens.filter(|value| *value >= 0);
        self.enqueue(
            TelemetryMessage::ProviderUsage(ProviderUsageEvent {
                credential_id: credential_id.to_owned(),
                minute: unix_minute(),
                cost_micro_usd,
                input_tokens,
                output_tokens,
            }),
            DropKind::ProviderUsage,
        );
    }

    pub async fn clear_request_history(&self) -> Result<i64, String> {
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(TelemetryMessage::ClearHistory(sender))
            .await
            .map_err(|_| "The telemetry writer is unavailable.".to_owned())?;
        receiver.await.map_err(|_| {
            "The telemetry writer stopped before clearing request history.".to_owned()
        })?
    }

    pub async fn flush(&self) -> Result<(), String> {
        tokio::time::timeout(Duration::from_secs(10), async {
            let (sender, receiver) = oneshot::channel();
            self.sender
                .send(TelemetryMessage::FlushHistory(sender))
                .await
                .map_err(|_| "The telemetry writer is unavailable.".to_owned())?;
            receiver.await.map_err(|_| {
                "The telemetry writer stopped before saving request history.".to_owned()
            })?
        })
        .await
        .map_err(|_| "Saving request history timed out.".to_owned())?
    }

    pub async fn shutdown(&self) -> Result<(), String> {
        const SHUTDOWN_POLICY: &str =
            "bounded drain, oldest queued events may be dropped on timeout or writer failure";
        self.flush()
            .await
            .map_err(|error| format!("{error} ({SHUTDOWN_POLICY})."))
    }

    fn enqueue(&self, message: TelemetryMessage, kind: DropKind) {
        if self.sender.try_send(message).is_err() {
            self.note_drop(kind);
        }
    }

    fn note_drop(&self, kind: DropKind) {
        let counter = match kind {
            DropKind::RequestLog => &self.counters.request_logs,
            DropKind::ApiKeyUsage => &self.counters.api_key_usage,
            DropKind::ProviderUsage => &self.counters.provider_usage,
            DropKind::Statistics => &self.counters.statistics,
        };
        increment_saturating(counter);
        let total = increment_saturating(&self.counters.total);
        self.dropped_notify.send_replace(total);
    }
}

pub(super) fn increment_saturating(counter: &AtomicU64) -> u64 {
    let previous = counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_add(1))
        })
        .unwrap_or_else(|value| value);
    previous.saturating_add(1)
}

pub(super) fn unix_minute() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64 / 60)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
