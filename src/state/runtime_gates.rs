use super::{AppState, MAX_TRACKED_PROVIDER_SEMAPHORES};
use crate::{
    config::OperationalSettings,
    state::{
        CircuitProbePermit, CircuitState, DynamicSemaphore, DynamicSemaphorePermit,
        LocalQuotaSnapshot,
    },
};
use std::sync::atomic::Ordering;
use std::{sync::Arc, time::Instant};

impl AppState {
    pub async fn try_provider_permit(&self, provider_id: &str) -> Option<DynamicSemaphorePermit> {
        let semaphore = {
            let mut semaphores = self.gates.provider_in_flight.lock().await;
            semaphores.retain(|_, semaphore| {
                Arc::strong_count(semaphore) > 1 || semaphore.active_count() > 0
            });
            if !semaphores.contains_key(provider_id)
                && semaphores.len() >= MAX_TRACKED_PROVIDER_SEMAPHORES
            {
                return None;
            }
            semaphores
                .entry(provider_id.to_owned())
                .or_insert_with(|| {
                    Arc::new(DynamicSemaphore::with_shared_limit(
                        self.runtime.gateway_provider_max_concurrency.clone(),
                    ))
                })
                .clone()
        };
        semaphore.try_acquire_owned()
    }

    #[cfg(test)]
    pub fn permit_provider(&self, provider_id: &str) -> Option<CircuitProbePermit> {
        let settings = self.operational_settings().settings;
        self.permit_provider_with_settings(provider_id, settings)
    }

    pub fn permit_provider_with_settings(
        &self,
        provider_id: &str,
        settings: OperationalSettings,
    ) -> Option<CircuitProbePermit> {
        let mut probe_id = None;
        let generation = if settings.circuit_breaker_enabled {
            let mut breakers = self
                .gates
                .circuit_breakers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !breakers.contains_key(provider_id) {
                let generation = self
                    .gates
                    .next_circuit_state_generation
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                        value.checked_add(1)
                    })
                    .ok()?;
                breakers.insert(
                    provider_id.to_owned(),
                    CircuitState {
                        generation,
                        ..CircuitState::default()
                    },
                );
            }
            let state = breakers.get_mut(provider_id)?;
            if let Some(open_until) = state.open_until {
                if Instant::now() < open_until || state.half_open_probe.is_some() {
                    return None;
                }
                let id = self
                    .gates
                    .next_circuit_probe_id
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
                    .ok()?;
                state.half_open_probe = Some(id);
                probe_id = Some(id);
            }
            Some(state.generation)
        } else {
            None
        };
        Some(CircuitProbePermit::new(
            self.gates.circuit_breakers.clone(),
            provider_id.to_owned(),
            generation,
            probe_id,
            self.gates.circuit_breaker_settings.clone(),
        ))
    }

    pub async fn clear_provider_runtime_state(&self, provider_id: &str) {
        self.gates
            .circuit_breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(provider_id);
        let mut semaphores = self.gates.provider_in_flight.lock().await;
        if semaphores.get(provider_id).is_some_and(|semaphore| {
            Arc::strong_count(semaphore) == 1 && semaphore.active_count() == 0
        }) {
            semaphores.remove(provider_id);
        }
        drop(semaphores);
        self.gates
            .provider_key_cursors
            .lock()
            .await
            .remove(provider_id);
        self.providers
            .local_provider_rpm_tracker
            .lock()
            .await
            .remove_provider(provider_id);
        self.providers
            .provider_usage
            .clear_antigravity_quota_provider(provider_id)
            .await;
        self.providers.freebuff_sessions.clear().await;
    }

    /// The fallback position that starts the next round-robin request of a
    /// provider, if one already served a request in this process.
    pub async fn provider_key_cursor(&self, provider_id: &str) -> Option<String> {
        self.gates
            .provider_key_cursors
            .lock()
            .await
            .get(provider_id)
            .cloned()
    }

    /// Continues the provider's round-robin rotation at `position`. The cursor
    /// is process-local, so concurrent requests can start at the same key and a
    /// restart begins again at the provider's preferred key.
    pub async fn set_provider_key_cursor(&self, provider_id: &str, position: &str) {
        self.gates
            .provider_key_cursors
            .lock()
            .await
            .insert(provider_id.to_owned(), position.to_owned());
    }

    pub async fn record_local_quota_attempt(&self, provider_id: &str) -> bool {
        self.providers
            .local_provider_rpm_tracker
            .lock()
            .await
            .record(provider_id, Instant::now())
    }

    pub async fn local_quota_snapshot(&self, provider_id: &str) -> LocalQuotaSnapshot {
        self.providers
            .local_provider_rpm_tracker
            .lock()
            .await
            .snapshot(provider_id, Instant::now())
    }

    pub async fn clear_route_runtime_state(&self, route_id: &str) {
        self.gates.round_robin.lock().await.remove(route_id);
    }

    /// Drop process-local state derived from provider and route records after
    /// an atomic database restore. Active provider permits remain attached to
    /// their existing semaphores so an import cannot bypass provider limits.
    pub async fn clear_restored_provider_runtime_state(&self) {
        self.gates
            .circuit_breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.gates.round_robin.lock().await.clear();
        self.gates.provider_key_cursors.lock().await.clear();
        self.invalidate_provider_usage_cache().await;
        self.providers.freebuff_sessions.clear().await;
        self.gates
            .provider_in_flight
            .lock()
            .await
            .retain(|_, semaphore| semaphore.active_count() > 0);
        self.providers
            .local_provider_rpm_tracker
            .lock()
            .await
            .reset(Instant::now());
    }
}
