use super::{
    AppState, MAX_TRACKED_PROVIDER_SEMAPHORES, MAX_TRACKED_PROXY_CHECKS, ProxyCheckPermitOutcome,
    ProxyCheckRecord, ProxyCheckSummary, ProxyEndpointCheckOutcome,
};
use crate::{
    config::OperationalSettings,
    state::{
        CircuitProbePermit, CircuitState, DynamicSemaphore, DynamicSemaphorePermit,
        LocalQuotaSnapshot,
    },
};
use std::collections::HashMap;
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

    pub async fn proxy_endpoint_cursor(&self, proxy_id: &str) -> usize {
        self.gates
            .proxy_endpoint_cursors
            .lock()
            .await
            .get(proxy_id)
            .copied()
            .unwrap_or_default()
    }

    pub async fn advance_proxy_endpoint_cursor(&self, proxy_id: &str, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let mut cursors = self.gates.proxy_endpoint_cursors.lock().await;
        let cursor = cursors.entry(proxy_id.to_owned()).or_default();
        let current = *cursor % len;
        *cursor = cursor.wrapping_add(1);
        current
    }

    pub async fn clear_proxy_runtime_state(&self, proxy_id: &str) {
        self.gates
            .proxy_endpoint_cursors
            .lock()
            .await
            .remove(proxy_id);
        self.gates
            .proxy_traffic
            .lock()
            .await
            .entries
            .remove(proxy_id);
        self.clear_proxy_check_state(proxy_id).await;
    }

    pub async fn record_proxy_traffic(
        &self,
        proxy_id: &str,
        received_bytes: u64,
        sent_bytes: u64,
        elapsed_millis: u64,
    ) {
        let mut traffic = self.gates.proxy_traffic.lock().await;
        Self::add_proxy_traffic(
            &mut traffic,
            proxy_id,
            received_bytes,
            sent_bytes,
            elapsed_millis,
        );
    }

    pub(crate) fn try_record_proxy_traffic(
        &self,
        proxy_id: &str,
        received_bytes: u64,
        sent_bytes: u64,
        elapsed_millis: u64,
    ) -> bool {
        let Ok(mut traffic) = self.gates.proxy_traffic.try_lock() else {
            return false;
        };
        Self::add_proxy_traffic(
            &mut traffic,
            proxy_id,
            received_bytes,
            sent_bytes,
            elapsed_millis,
        );
        true
    }

    fn add_proxy_traffic(
        registry: &mut super::ProxyTrafficRegistry,
        proxy_id: &str,
        received_bytes: u64,
        sent_bytes: u64,
        elapsed_millis: u64,
    ) {
        registry.clock = registry.clock.saturating_add(1);
        let clock = registry.clock;
        if !registry.entries.contains_key(proxy_id) {
            if registry.entries.len() >= super::MAX_TRACKED_PROXY_TRAFFIC {
                registry.truncated = true;
                if let Some(stale_id) = registry
                    .entries
                    .iter()
                    .min_by_key(|(_, totals)| totals.last_used)
                    .map(|(id, _)| id.clone())
                {
                    registry.entries.remove(&stale_id);
                }
            }
            registry
                .entries
                .insert(proxy_id.to_owned(), Default::default());
        }
        if let Some(totals) = registry.entries.get_mut(proxy_id) {
            totals.received_bytes = totals.received_bytes.saturating_add(received_bytes);
            totals.sent_bytes = totals.sent_bytes.saturating_add(sent_bytes);
            totals.requests = totals.requests.saturating_add(1);
            totals.elapsed_millis = totals.elapsed_millis.saturating_add(elapsed_millis);
            totals.last_used = clock;
        }
    }

    pub async fn proxy_traffic_snapshot(
        &self,
        proxy_ids: &[String],
    ) -> (
        std::collections::HashMap<String, super::ProxyTrafficTotals>,
        bool,
    ) {
        let registry = self.gates.proxy_traffic.lock().await;
        let stats = proxy_ids
            .iter()
            .filter_map(|id| {
                registry
                    .entries
                    .get(id)
                    .copied()
                    .map(|totals| (id.clone(), totals))
            })
            .collect();
        (stats, registry.truncated)
    }

    /// Reserves the right to run one live-check of `proxy_id`. At most one check
    /// per proxy runs at a time; the permit is released on drop, so a cancelled
    /// handler or a dropped response frees the flag.
    pub(crate) async fn try_proxy_check_permit(&self, proxy_id: &str) -> ProxyCheckPermitOutcome {
        let semaphore = {
            let mut checks = self.gates.proxy_checks.lock().await;
            // Drop flags for proxies whose check already finished.
            checks.in_flight.retain(|_, semaphore| {
                Arc::strong_count(semaphore) > 1 || semaphore.available_permits() == 0
            });
            if checks.in_flight.contains_key(proxy_id) {
                return ProxyCheckPermitOutcome::Busy;
            }
            if checks.in_flight.len() >= MAX_TRACKED_PROXY_CHECKS {
                return ProxyCheckPermitOutcome::AtCapacity;
            }
            let semaphore = Arc::new(tokio::sync::Semaphore::new(1));
            checks
                .in_flight
                .insert(proxy_id.to_owned(), semaphore.clone());
            semaphore
        };
        match semaphore.try_acquire_owned() {
            Ok(permit) => ProxyCheckPermitOutcome::Acquired(permit),
            Err(_) => ProxyCheckPermitOutcome::Busy,
        }
    }

    /// Stores the outcome of one completed live-check. Results are process-local
    /// and capped; the oldest proxy result is evicted at the cap.
    pub async fn record_proxy_check(
        &self,
        proxy_id: &str,
        checked_at: u64,
        results: Vec<ProxyEndpointCheckOutcome>,
    ) {
        let mut checks = self.gates.proxy_checks.lock().await;
        checks.clock = checks.clock.saturating_add(1);
        let clock = checks.clock;
        if !checks.entries.contains_key(proxy_id)
            && checks.entries.len() >= MAX_TRACKED_PROXY_CHECKS
        {
            if let Some(stale_id) = checks
                .entries
                .iter()
                .min_by_key(|(_, record)| record.recorded_clock)
                .map(|(id, _)| id.clone())
            {
                checks.entries.remove(&stale_id);
            }
        }
        checks.entries.insert(
            proxy_id.to_owned(),
            ProxyCheckRecord {
                checked_at,
                recorded_clock: clock,
                results,
            },
        );
    }

    /// The last live-check summary for each requested proxy that has one.
    pub async fn proxy_check_snapshot(
        &self,
        proxy_ids: &[String],
    ) -> HashMap<String, ProxyCheckSummary> {
        let checks = self.gates.proxy_checks.lock().await;
        proxy_ids
            .iter()
            .filter_map(|id| {
                checks.entries.get(id).map(|record| {
                    (
                        id.clone(),
                        ProxyCheckSummary {
                            checked_at: record.checked_at,
                            live_count: record.results.iter().filter(|r| r.live).count(),
                            die_count: record.results.iter().filter(|r| !r.live).count(),
                        },
                    )
                })
            })
            .collect()
    }

    /// Drops one proxy's stored live-check result. An in-flight check keeps its
    /// flag until the check finishes, so an update cannot admit a second
    /// concurrent check of the same proxy.
    pub async fn clear_proxy_check_state(&self, proxy_id: &str) {
        let mut checks = self.gates.proxy_checks.lock().await;
        checks.entries.remove(proxy_id);
        checks.in_flight.retain(|id, semaphore| {
            id != proxy_id || Arc::strong_count(semaphore) > 1 || semaphore.available_permits() == 0
        });
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
        self.gates.proxy_endpoint_cursors.lock().await.clear();
        self.gates.provider_key_cursors.lock().await.clear();
        // Restored proxy records replace the endpoints the stored check
        // results described; the results are process-local, so drop them.
        self.gates.proxy_checks.lock().await.entries.clear();
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
