//! Process-local runtime state grouped by lifecycle and lock ownership.
//!
//! `AppState` is the application facade. `AdminRuntime`, `GatewayGates`, and
//! `ProviderRuntime` are the ownership slices; new mutable state belongs in
//! the smallest matching slice rather than as another unrelated AppState
//! field.

use crate::config::{Config, GatewayResourceLimits, OperationalSettings};
use crate::infra::db::OperationalSettingsRecord;
use crate::infra::storage::Database;
use crate::infra::telemetry::Telemetry;
use crate::security::rate_limit::{RateLimitPolicy, RateLimiter, RateLimiterConfig};
use crate::support::output_styles::{self, OutputStylesSnapshot};
use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock as StdRwLock, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Notify, RwLock, Semaphore, watch};

mod auth;
mod auth_flows;
mod freebuff;
mod local_rpm;
mod observability;
mod provider_locks;
mod provider_usage;
mod request_live;
mod request_log_record;
mod runtime;
mod runtime_controls;
mod runtime_gates;
mod settings;
pub(crate) use auth::{
    AdminRequestContext, AdminSession, AdminStepUpProof, AdminStepUpScope,
    MAX_ADMIN_STEP_UP_PROOFS, PendingProviderApiKeyAuthFlow, PendingProviderAuthFlow,
    ProviderApiKeyAuthFlowStatus, ProviderAuthFlowMethod, ProviderAuthFlowStatus,
    admin_token_digest, new_admin_token,
};
pub(crate) use freebuff::{FreebuffSessionCacheEntry, FreebuffSessionRuntime};
#[cfg(test)]
use local_rpm::MAX_LOCAL_PROVIDER_RPM_TRACKERS;
pub(crate) use local_rpm::{LocalQuotaSnapshot, LocalQuotaTrackingStatus, ProviderRpmTracker};
pub(crate) use provider_usage::{ProviderUsageCacheEntry, ProviderUsageRuntime};
pub(crate) use request_live::{
    RequestLiveEvent, RequestLiveGuard, RequestLiveRegistry, RequestLiveRow, RequestLiveSnapshot,
};
pub(crate) use request_log_record::RequestLogRecord;
pub(crate) use runtime::RuntimeState;
pub(crate) use runtime_controls::{
    CircuitBreakerRuntimeSettings, CircuitProbePermit, CircuitState, DynamicSemaphore,
    DynamicSemaphorePermit,
};

/// Admin dashboard runtime: sessions, step-up proofs, rate limits, database
/// operation gates, and provider OAuth/API-key auth flows.
#[derive(Clone)]
pub(crate) struct AdminRuntime {
    pub(crate) admin_sessions: Arc<RwLock<HashMap<[u8; 32], AdminSession>>>,
    pub(crate) admin_step_up_proofs: Arc<RwLock<HashMap<[u8; 32], AdminStepUpProof>>>,
    pub(crate) admin_password_change_required: Arc<AtomicBool>,
    pub(crate) admin_login_limiter: Arc<RateLimiter>,
    pub(crate) admin_refresh_limiter: Arc<RateLimiter>,
    pub(crate) admin_login_in_flight: Arc<Semaphore>,
    pub(crate) admin_auth_lock: Arc<Mutex<()>>,
    pub(crate) admin_provider_probe_in_flight: Arc<Semaphore>,
    pub(crate) admin_database_import_in_flight: Arc<Semaphore>,
    pub(crate) admin_database_export_in_flight: Arc<Semaphore>,
    pub(crate) admin_provider_usage_in_flight: Arc<Semaphore>,
    pub(crate) admin_api_limiter: Arc<RateLimiter>,
    pub(crate) admin_auth_failure_limiter: Arc<RateLimiter>,
    pub(crate) admin_upstream_event_streams: Arc<Semaphore>,
    pub(crate) provider_auth_flows: Arc<Mutex<HashMap<String, PendingProviderAuthFlow>>>,
    pub(crate) provider_auth_callback_started: Arc<AtomicBool>,
    pub(crate) provider_auth_callback_lock: Arc<Mutex<()>>,
    pub(crate) provider_api_key_auth_flows:
        Arc<Mutex<HashMap<String, PendingProviderApiKeyAuthFlow>>>,
    pub(crate) provider_api_key_auth_callback_started: Arc<AtomicBool>,
    pub(crate) provider_api_key_auth_callback_port: Arc<std::sync::atomic::AtomicU16>,
    pub(crate) provider_api_key_auth_callback_lock: Arc<Mutex<()>>,
}

/// Gateway admission and runtime gates: rate limits, bulkheads, round-robin
/// cursors, circuit breakers, and stream cancellation registry.
#[derive(Clone)]
pub(crate) struct GatewayGates {
    pub(crate) gateway_auth_failure_limiter: Arc<RateLimiter>,
    pub(crate) gateway_key_limiter: Arc<RateLimiter>,
    pub(crate) gateway_in_flight: Arc<DynamicSemaphore>,
    pub(crate) stream_continuity_in_flight: Arc<DynamicSemaphore>,
    pub(crate) gateway_body_processing: Arc<DynamicSemaphore>,
    pub(crate) gateway_sse_processing: Arc<DynamicSemaphore>,
    pub(crate) provider_in_flight: Arc<Mutex<HashMap<String, Arc<DynamicSemaphore>>>>,
    pub(crate) round_robin: Arc<Mutex<HashMap<String, usize>>>,
    /// Fallback position that starts the next round-robin request of a provider.
    /// Entries are dropped with the provider or by an atomic restore.
    pub(crate) provider_key_cursors: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) circuit_breakers: Arc<std::sync::Mutex<HashMap<String, CircuitState>>>,
    pub(crate) circuit_breaker_settings: Arc<StdRwLock<CircuitBreakerRuntimeSettings>>,
    pub(crate) next_circuit_state_generation: Arc<AtomicU64>,
    pub(crate) stream_cancellations: Arc<StdRwLock<HashMap<String, watch::Sender<bool>>>>,
    pub(crate) next_circuit_probe_id: Arc<AtomicU64>,
}

/// Provider-side runtime caches and locks: usage cache, Freebuff sessions,
/// local RPM tracking, and credential refresh serialization.
#[derive(Clone)]
pub(crate) struct ProviderRuntime {
    pub(crate) provider_usage: Arc<ProviderUsageRuntime>,
    pub(crate) freebuff_sessions: Arc<FreebuffSessionRuntime>,
    pub(crate) local_provider_rpm_tracker: Arc<Mutex<ProviderRpmTracker>>,
    pub(crate) codex_refresh_locks: Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>,
    pub(crate) codex_refresh_fallback_lock: Arc<Mutex<()>>,
    pub(crate) codex_account_save_lock: Arc<Mutex<()>>,
}

const MAX_ADMIN_ACCESS_TOKENS: usize = 1024;
const MAX_TRACKED_PROVIDER_SEMAPHORES: usize = 256;
const MAX_PENDING_PROVIDER_AUTH_FLOWS: usize = 16;
const MAX_PENDING_API_KEY_AUTH_FLOWS: usize = 16;
pub const PROVIDER_API_KEY_AUTH_APPLY_RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a completed (`Connected`/`Failed`) provider OAuth flow stays in RAM
/// so the dashboard can observe the terminal status once before cleanup.
pub const PROVIDER_AUTH_TERMINAL_FLOW_TTL: Duration = Duration::from_secs(2 * 60);
const MAX_CODEX_REFRESH_LOCKS: usize = 256;
const MAX_ADMIN_UPSTREAM_EVENT_STREAMS: usize = 8;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub(crate) runtime: RuntimeState,
    shutdown: watch::Sender<bool>,
    pub request_log_retention_notify: Arc<Notify>,
    pub provider_deletion_notify: Arc<Notify>,
    pub admin_key: Arc<RwLock<Option<String>>>,
    /// Admin dashboard runtime: sessions, limits, database gates, auth flows.
    pub(crate) admin: AdminRuntime,
    /// Gateway admission gates, bulkheads, and circuit breakers.
    pub(crate) gates: GatewayGates,
    /// Provider-side runtime caches and credential locks.
    pub(crate) providers: ProviderRuntime,
    request_live: Arc<RequestLiveRegistry>,
    pub(crate) upstream_live_count: watch::Sender<usize>,
    pub(crate) upstream_live_count_initialized: Arc<AtomicBool>,
    pub(crate) upstream_live_count_refresh_lock: Arc<Mutex<()>>,
    pub telemetry: Telemetry,
    pub db: Database,
    pub started_at: Instant,
    pub request_count: Arc<AtomicU64>,
}

impl AppState {
    #[cfg(test)]
    pub fn new(config: Config, db: Database) -> Self {
        Self::new_with_gateway_resource_limits(config, db, GatewayResourceLimits::default())
    }

    #[cfg(test)]
    pub fn new_with_gateway_resource_limits(
        config: Config,
        db: Database,
        gateway_resource_limits: GatewayResourceLimits,
    ) -> Self {
        let operational_settings = OperationalSettings {
            connect_timeout: config.connect_timeout,
            request_timeout: config.request_timeout,
            stream_idle_timeout: config.stream_idle_timeout,
            circuit_breaker_enabled: config.circuit_breaker_enabled,
            circuit_breaker_threshold: config.circuit_breaker_threshold,
            circuit_breaker_cooldown: config.circuit_breaker_cooldown,
            ..OperationalSettings::default()
        };
        Self::new_with_runtime_settings(
            config,
            db,
            gateway_resource_limits,
            OperationalSettingsRecord {
                settings: operational_settings,
                revision: 0,
                overridden: false,
            },
            Ok(operational_settings),
            OutputStylesSnapshot::default(),
        )
    }

    pub fn new_with_runtime_settings(
        config: Config,
        db: Database,
        gateway_resource_limits: GatewayResourceLimits,
        operational_record: OperationalSettingsRecord,
        operational_env_settings: Result<OperationalSettings, String>,
        output_styles_record: OutputStylesSnapshot,
    ) -> Self {
        debug_assert!(operational_record.settings.validate().is_ok());
        let operational = operational_record.settings;
        let admin_key = Arc::new(RwLock::new(config.admin_key.clone()));
        let password_change_required = config
            .admin_key
            .as_deref()
            .is_some_and(crate::admin::password_needs_change);
        let admin_login_limiter = RateLimiter::new(RateLimiterConfig::new(
            RateLimitPolicy::TokenBucket {
                capacity: 5,
                refill_tokens: 1,
                refill_interval: Duration::from_secs(30),
            },
            100_000,
        ))
        .expect("static admin login limiter settings are valid");
        let admin_refresh_limiter = RateLimiter::new(RateLimiterConfig::new(
            RateLimitPolicy::TokenBucket {
                capacity: 60,
                refill_tokens: 1,
                refill_interval: Duration::from_secs(2),
            },
            100_000,
        ))
        .expect("static admin refresh limiter settings are valid");
        let admin_api_limiter = RateLimiter::new(RateLimiterConfig::new(
            RateLimitPolicy::FixedWindow {
                max_requests: operational.admin_api_max_requests,
                window: operational.admin_api_window,
            },
            100_000,
        ))
        .expect("validated admin API limiter settings are valid");
        let admin_auth_failure_limiter = RateLimiter::new(RateLimiterConfig::new(
            RateLimitPolicy::FixedWindow {
                max_requests: 60,
                window: Duration::from_secs(60),
            },
            100_000,
        ))
        .expect("static admin authentication failure limiter settings are valid");
        let gateway_auth_failure_limiter = RateLimiter::new(RateLimiterConfig::new(
            RateLimitPolicy::FixedWindow {
                max_requests: 600,
                window: Duration::from_secs(60),
            },
            100_000,
        ))
        .expect("static gateway authentication failure limiter settings are valid");
        let gateway_key_limiter = RateLimiter::new(RateLimiterConfig::new(
            RateLimitPolicy::TokenBucket {
                capacity: operational.gateway_key_capacity,
                refill_tokens: operational.gateway_key_refill_tokens,
                refill_interval: operational.gateway_key_refill_interval,
            },
            100_000,
        ))
        .expect("validated gateway key limiter settings are valid");
        let (shutdown, _) = watch::channel(false);
        let telemetry = Telemetry::new(db.clone());
        let started_at = Instant::now();
        let local_provider_rpm_tracker = ProviderRpmTracker::new(started_at);
        let gateway_provider_max_concurrency = Arc::new(AtomicUsize::new(
            gateway_resource_limits.provider_max_concurrency,
        ));
        let (upstream_live_count, _upstream_live_count_receiver) = watch::channel(0_usize);
        Self {
            config: Arc::new(config),
            runtime: RuntimeState {
                operational_settings: Arc::new(StdRwLock::new(operational_record)),
                operational_env_settings: Arc::new(operational_env_settings),
                operational_settings_update_lock: Arc::new(Mutex::new(())),
                output_styles: Arc::new(StdRwLock::new(output_styles_record)),
                output_styles_update_lock: Arc::new(Mutex::new(())),
                output_styles_metrics: Arc::new(output_styles::Metrics::new()),
                gateway_resource_limits: Arc::new(StdRwLock::new(gateway_resource_limits)),
                gateway_resource_limits_update_lock: Arc::new(Mutex::new(())),
                gateway_provider_max_concurrency: gateway_provider_max_concurrency.clone(),
            },
            shutdown,
            request_log_retention_notify: Arc::new(Notify::new()),
            provider_deletion_notify: Arc::new(Notify::new()),
            admin_key,
            admin: AdminRuntime {
                admin_sessions: Arc::new(RwLock::new(HashMap::new())),
                admin_step_up_proofs: Arc::new(RwLock::new(HashMap::new())),
                admin_password_change_required: Arc::new(AtomicBool::new(password_change_required)),
                admin_login_limiter: Arc::new(admin_login_limiter),
                admin_refresh_limiter: Arc::new(admin_refresh_limiter),
                admin_login_in_flight: Arc::new(Semaphore::new(4)),
                admin_auth_lock: Arc::new(Mutex::new(())),
                admin_provider_probe_in_flight: Arc::new(Semaphore::new(8)),
                admin_database_import_in_flight: Arc::new(Semaphore::new(1)),
                admin_database_export_in_flight: Arc::new(Semaphore::new(1)),
                admin_provider_usage_in_flight: Arc::new(Semaphore::new(4)),
                admin_api_limiter: Arc::new(admin_api_limiter),
                admin_auth_failure_limiter: Arc::new(admin_auth_failure_limiter),
                admin_upstream_event_streams: Arc::new(Semaphore::new(
                    MAX_ADMIN_UPSTREAM_EVENT_STREAMS,
                )),
                provider_auth_flows: Arc::new(Mutex::new(HashMap::new())),
                provider_auth_callback_started: Arc::new(AtomicBool::new(false)),
                provider_auth_callback_lock: Arc::new(Mutex::new(())),
                provider_api_key_auth_flows: Arc::new(Mutex::new(HashMap::new())),
                provider_api_key_auth_callback_started: Arc::new(AtomicBool::new(false)),
                provider_api_key_auth_callback_port: Arc::new(std::sync::atomic::AtomicU16::new(0)),
                provider_api_key_auth_callback_lock: Arc::new(Mutex::new(())),
            },
            gates: GatewayGates {
                gateway_auth_failure_limiter: Arc::new(gateway_auth_failure_limiter),
                gateway_key_limiter: Arc::new(gateway_key_limiter),
                gateway_in_flight: Arc::new(DynamicSemaphore::new(
                    operational.gateway_max_in_flight,
                )),
                stream_continuity_in_flight: Arc::new(DynamicSemaphore::new(
                    gateway_resource_limits.stream_continuity_max_concurrency,
                )),
                gateway_body_processing: Arc::new(DynamicSemaphore::new(
                    gateway_resource_limits.gateway_body_processing_concurrency,
                )),
                gateway_sse_processing: Arc::new(DynamicSemaphore::new(
                    gateway_resource_limits.gateway_body_processing_concurrency,
                )),
                provider_in_flight: Arc::new(Mutex::new(HashMap::new())),
                round_robin: Arc::new(Mutex::new(HashMap::new())),
                provider_key_cursors: Arc::new(Mutex::new(HashMap::new())),
                circuit_breakers: Arc::new(std::sync::Mutex::new(HashMap::new())),
                circuit_breaker_settings: Arc::new(StdRwLock::new(CircuitBreakerRuntimeSettings {
                    failure_threshold: operational.circuit_breaker_threshold,
                    cooldown: operational.circuit_breaker_cooldown,
                })),
                next_circuit_state_generation: Arc::new(AtomicU64::new(1)),
                stream_cancellations: Arc::new(StdRwLock::new(HashMap::new())),
                next_circuit_probe_id: Arc::new(AtomicU64::new(1)),
            },
            providers: ProviderRuntime {
                provider_usage: Arc::new(ProviderUsageRuntime::new()),
                freebuff_sessions: Arc::new(FreebuffSessionRuntime::new()),
                local_provider_rpm_tracker: Arc::new(Mutex::new(local_provider_rpm_tracker)),
                codex_refresh_locks: Arc::new(Mutex::new(HashMap::new())),
                codex_refresh_fallback_lock: Arc::new(Mutex::new(())),
                codex_account_save_lock: Arc::new(Mutex::new(())),
            },
            request_live: Arc::new(RequestLiveRegistry::new()),
            upstream_live_count,
            upstream_live_count_initialized: Arc::new(AtomicBool::new(false)),
            upstream_live_count_refresh_lock: Arc::new(Mutex::new(())),
            telemetry,
            db,
            started_at,
            request_count: Arc::new(AtomicU64::new(0)),
        }
    }
}

#[cfg(test)]
mod tests;
