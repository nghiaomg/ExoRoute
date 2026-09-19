use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex as StdMutex, RwLock as StdRwLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// A cancellation-safe concurrency gate whose limit can change at runtime.
/// Existing permits are allowed to finish when the limit is reduced; no new
/// permit is granted until the active count falls below the new limit.
pub struct DynamicSemaphore {
    pub(crate) active: AtomicUsize,
    limit: Arc<AtomicUsize>,
    waiters: Notify,
}

pub struct DynamicSemaphorePermit {
    semaphore: Arc<DynamicSemaphore>,
}

#[derive(Clone, Copy)]
pub(crate) struct CircuitBreakerRuntimeSettings {
    pub(crate) failure_threshold: u32,
    pub(crate) cooldown: Duration,
}

impl DynamicSemaphore {
    pub fn new(limit: usize) -> Self {
        Self::with_shared_limit(Arc::new(AtomicUsize::new(limit)))
    }

    pub fn with_shared_limit(limit: Arc<AtomicUsize>) -> Self {
        Self {
            active: AtomicUsize::new(0),
            limit,
            waiters: Notify::new(),
        }
    }

    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }

    pub fn limit(&self) -> usize {
        self.limit.load(Ordering::Acquire)
    }

    pub fn try_acquire_owned(self: &Arc<Self>) -> Option<DynamicSemaphorePermit> {
        let mut active = self.active.load(Ordering::Acquire);
        loop {
            let limit = self.limit.load(Ordering::Acquire);
            if limit != 0 && active >= limit {
                return None;
            }
            let next_active = active.checked_add(1)?;
            match self.active.compare_exchange_weak(
                active,
                next_active,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let current_limit = self.limit.load(Ordering::Acquire);
                    if current_limit == 0 || active < current_limit {
                        return Some(DynamicSemaphorePermit {
                            semaphore: self.clone(),
                        });
                    }
                    self.release_slot();
                    return None;
                }
                Err(current) => active = current,
            }
        }
    }

    pub async fn acquire_owned(self: &Arc<Self>) -> DynamicSemaphorePermit {
        loop {
            let notified = self.waiters.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(permit) = self.try_acquire_owned() {
                return permit;
            }
            notified.await;
        }
    }

    pub fn set_limit(&self, limit: usize) {
        let previous = self.limit.swap(limit, Ordering::AcqRel);
        if limit == 0 || (previous != 0 && limit > previous) {
            self.waiters.notify_waiters();
        }
    }

    fn release_slot(&self) {
        let previous = self.active.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "dynamic semaphore permit count underflowed");
        self.waiters.notify_one();
    }
}

impl Drop for DynamicSemaphorePermit {
    fn drop(&mut self) {
        self.semaphore.release_slot();
    }
}

#[derive(Clone, Default)]
pub struct CircuitState {
    pub(crate) generation: u64,
    pub consecutive_failures: u32,
    pub open_until: Option<Instant>,
    pub(crate) opened_at: Option<Instant>,
    pub half_open_probe: Option<u64>,
}

/// A circuit-breaker admission lease. Dropping a half-open lease releases the
/// single probe slot synchronously, including when an async request is cancelled.
pub struct CircuitProbePermit {
    breakers: Arc<StdMutex<HashMap<String, CircuitState>>>,
    provider_id: String,
    generation: Option<u64>,
    probe_id: Option<u64>,
    runtime_settings: Arc<StdRwLock<CircuitBreakerRuntimeSettings>>,
}

impl CircuitProbePermit {
    pub(crate) fn new(
        breakers: Arc<StdMutex<HashMap<String, CircuitState>>>,
        provider_id: String,
        generation: Option<u64>,
        probe_id: Option<u64>,
        runtime_settings: Arc<StdRwLock<CircuitBreakerRuntimeSettings>>,
    ) -> Self {
        Self {
            breakers,
            provider_id,
            generation,
            probe_id,
            runtime_settings,
        }
    }

    pub fn succeeded(&mut self) {
        let Some(generation) = self.generation else {
            self.probe_id = None;
            return;
        };
        let mut breakers = self
            .breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(state) = breakers
            .get_mut(&self.provider_id)
            .filter(|state| state.generation == generation)
        {
            match self.probe_id {
                Some(probe_id) if state.half_open_probe == Some(probe_id) => {
                    state.consecutive_failures = 0;
                    state.open_until = None;
                    state.opened_at = None;
                    state.half_open_probe = None;
                }
                Some(_) => {}
                None if state.open_until.is_none() && state.half_open_probe.is_none() => {
                    // A request admitted while closed may finish after another
                    // request opens the circuit. Its late success must not
                    // close the circuit or release the half-open probe.
                    state.consecutive_failures = 0;
                    state.opened_at = None;
                }
                None => {}
            }
        }
        self.probe_id = None;
    }

    pub fn failed(&mut self) {
        let Some(generation) = self.generation else {
            self.probe_id = None;
            return;
        };
        let runtime_settings = *self
            .runtime_settings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut breakers = self
            .breakers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(state) = breakers
            .get_mut(&self.provider_id)
            .filter(|state| state.generation == generation)
        {
            let now = Instant::now();
            state.consecutive_failures = state.consecutive_failures.saturating_add(1);
            if state.consecutive_failures >= runtime_settings.failure_threshold {
                state.opened_at = Some(now);
                state.open_until = now.checked_add(runtime_settings.cooldown);
            }
            if self.probe_id.is_some() && state.half_open_probe == self.probe_id {
                state.half_open_probe = None;
            }
        }
        self.probe_id = None;
    }

    pub fn abort(&mut self) {
        self.release_probe();
    }

    fn release_probe(&mut self) {
        if let Some(probe_id) = self.probe_id.take() {
            let Some(generation) = self.generation else {
                return;
            };
            let mut breakers = self
                .breakers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(state) = breakers
                .get_mut(&self.provider_id)
                .filter(|state| state.generation == generation)
                && state.half_open_probe == Some(probe_id)
            {
                state.half_open_probe = None;
            }
        }
    }
}

impl Drop for CircuitProbePermit {
    fn drop(&mut self) {
        self.release_probe();
    }
}
