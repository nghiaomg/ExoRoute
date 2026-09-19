use super::AppState;
use crate::state::CircuitBreakerRuntimeSettings;
use crate::{
    config::{GatewayResourceLimits, OperationalSettings},
    infra::db::OperationalSettingsRecord,
    infra::storage::StorageError,
    security::rate_limit::RateLimitPolicy,
    support::output_styles::{self, ApplyResult, OutputStyleSelection, OutputStylesSnapshot},
};
use std::{sync::atomic::Ordering, time::Instant};

impl AppState {
    pub fn gateway_resource_limits(&self) -> GatewayResourceLimits {
        *self
            .runtime
            .gateway_resource_limits
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn operational_settings(&self) -> OperationalSettingsRecord {
        *self
            .runtime
            .operational_settings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn output_styles(&self) -> OutputStylesSnapshot {
        self.runtime
            .output_styles
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub async fn save_output_styles(
        &self,
        styles: Vec<OutputStyleSelection>,
        expected_revision: i64,
        overridden: bool,
    ) -> Result<Option<OutputStylesSnapshot>, StorageError> {
        let styles = output_styles::validate_styles(&styles).map_err(StorageError::Invalid)?;
        let state = self.clone();
        // Keep the commit-and-publish operation in a detached task. If a
        // client disconnects after LMDB commits, the task still publishes the
        // same snapshot and releases the writer guard.
        tokio::spawn(async move {
            let _update_guard = state.runtime.output_styles_update_lock.lock().await;
            let result = crate::infra::db::save_output_styles(
                &state.db,
                styles,
                expected_revision,
                overridden,
            )
            .await?;
            if let Some(snapshot) = result.clone() {
                state.apply_output_styles(snapshot);
            }
            Ok(result)
        })
        .await
        .map_err(|error| StorageError::Task(format!("output styles update task failed: {error}")))?
    }

    pub(crate) fn apply_output_styles(&self, snapshot: OutputStylesSnapshot) {
        debug_assert!(output_styles::validate_styles(&snapshot.styles).is_ok());
        *self
            .runtime
            .output_styles
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot;
    }

    pub fn record_output_styles_result(&self, result: ApplyResult) {
        self.runtime.output_styles_metrics.record(result);
    }

    pub async fn save_operational_settings(
        &self,
        settings: OperationalSettings,
        expected_revision: i64,
        overridden: bool,
    ) -> Result<Option<OperationalSettingsRecord>, StorageError> {
        settings
            .validate()
            .map_err(|message| StorageError::Invalid(message.to_owned()))?;
        let state = self.clone();
        // The detached task finishes commit-and-publish even if the dashboard
        // disconnects after LMDB commits.
        tokio::spawn(async move {
            let _update_guard = state.runtime.operational_settings_update_lock.lock().await;
            let revision = crate::infra::db::save_operational_settings(
                &state.db,
                settings,
                expected_revision,
                overridden,
            )
            .await?;
            let Some(revision) = revision else {
                return Ok(None);
            };
            let record = OperationalSettingsRecord {
                settings,
                revision,
                overridden,
            };
            state.apply_operational_settings(record);
            Ok(Some(record))
        })
        .await
        .map_err(|error| {
            StorageError::Task(format!("operational settings update task failed: {error}"))
        })?
    }

    pub(crate) fn spawn_gateway_resource_limits_update(
        &self,
        limits: GatewayResourceLimits,
        update_guard: tokio::sync::OwnedMutexGuard<()>,
    ) -> tokio::task::JoinHandle<Result<(), StorageError>> {
        let state = self.clone();
        tokio::spawn(async move {
            let _update_guard = update_guard;
            crate::infra::db::save_gateway_resource_limits(&state.db, limits).await?;
            state.apply_gateway_resource_limits(limits);
            Ok(())
        })
    }

    pub(crate) fn apply_operational_settings(&self, record: OperationalSettingsRecord) {
        debug_assert!(record.settings.validate().is_ok());
        let mut active = self
            .runtime
            .operational_settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = active.settings;
        let next = record.settings;
        if previous.admin_api_max_requests != next.admin_api_max_requests
            || previous.admin_api_window != next.admin_api_window
        {
            self.admin
                .admin_api_limiter
                .reconfigure(RateLimitPolicy::FixedWindow {
                    max_requests: next.admin_api_max_requests,
                    window: next.admin_api_window,
                });
        }
        if previous.gateway_key_capacity != next.gateway_key_capacity
            || previous.gateway_key_refill_tokens != next.gateway_key_refill_tokens
            || previous.gateway_key_refill_interval != next.gateway_key_refill_interval
        {
            self.gates
                .gateway_key_limiter
                .reconfigure(RateLimitPolicy::TokenBucket {
                    capacity: next.gateway_key_capacity,
                    refill_tokens: next.gateway_key_refill_tokens,
                    refill_interval: next.gateway_key_refill_interval,
                });
        }
        self.gates
            .gateway_in_flight
            .set_limit(next.gateway_max_in_flight);
        let breaker_settings = CircuitBreakerRuntimeSettings {
            failure_threshold: next.circuit_breaker_threshold,
            cooldown: next.circuit_breaker_cooldown,
        };
        *self
            .gates
            .circuit_breaker_settings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = breaker_settings;
        let now = Instant::now();
        let mut breakers = self
            .gates
            .circuit_breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for circuit in breakers.values_mut() {
            if circuit.open_until.is_some() {
                let opened_at = circuit.opened_at.unwrap_or(now);
                circuit.opened_at = Some(opened_at);
                circuit.open_until = opened_at.checked_add(next.circuit_breaker_cooldown);
            }
        }
        drop(breakers);
        *active = record;
        drop(active);
        self.log_runtime_registry_sizes();
        self.request_log_retention_notify.notify_one();
    }

    /// Periodically surfaces the size of the runtime registries that grow with
    /// traffic so a leak pattern (entries that should have been reaped) is
    /// visible in the logs instead of silently accumulating. Uses try-lock so
    /// the synchronous settings path never blocks on a busy registry.
    fn log_runtime_registry_sizes(&self) {
        let provider_auth_flows = match self.admin.provider_auth_flows.try_lock() {
            Ok(flows) => flows.len(),
            Err(_) => return,
        };
        let stream_cancellations = self
            .gates
            .stream_cancellations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        let circuit_breakers = self
            .gates
            .circuit_breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        tracing::debug!(
            provider_auth_flows,
            stream_cancellations,
            circuit_breakers,
            "runtime registry sizes after operational settings update"
        );
    }

    /// Applies validated limits atomically for new work. Existing permits are
    /// allowed to finish; reductions stop new admissions until slots are free.
    /// Callers validate first and serialize this with database updates.
    pub(crate) fn apply_gateway_resource_limits(&self, limits: GatewayResourceLimits) {
        debug_assert!(limits.validate().is_ok());
        let mut active = self
            .runtime
            .gateway_resource_limits
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.gates
            .gateway_body_processing
            .set_limit(limits.gateway_body_processing_concurrency);
        self.gates
            .gateway_sse_processing
            .set_limit(limits.gateway_body_processing_concurrency);
        self.gates
            .stream_continuity_in_flight
            .set_limit(limits.stream_continuity_max_concurrency);
        self.runtime
            .gateway_provider_max_concurrency
            .store(limits.provider_max_concurrency, Ordering::Release);
        *active = limits;
    }
}
