use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const LOCAL_PROVIDER_RPM_WINDOW_SECONDS: u64 = 60;
const LOCAL_PROVIDER_RPM_BUCKET_COUNT: usize = 60;
pub(crate) const MAX_LOCAL_PROVIDER_RPM_TRACKERS: usize = 512;
const LOCAL_PROVIDER_RPM_PRUNE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalQuotaTrackingStatus {
    Ready,
    WarmingUp,
    CapacityLimited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalQuotaSnapshot {
    pub requests_last_60s: u64,
    pub coverage_seconds: u64,
    pub status: LocalQuotaTrackingStatus,
}

#[derive(Clone, Copy, Debug, Default)]
struct LocalRpmBucket {
    second: Option<u64>,
    requests: u64,
}

struct ProviderRpmWindow {
    buckets: [LocalRpmBucket; LOCAL_PROVIDER_RPM_BUCKET_COUNT],
    last_activity_at: Instant,
}

impl ProviderRpmWindow {
    fn new(now: Instant) -> Self {
        Self {
            buckets: [LocalRpmBucket::default(); LOCAL_PROVIDER_RPM_BUCKET_COUNT],
            last_activity_at: now,
        }
    }

    fn record(&mut self, second: u64, now: Instant) {
        let bucket = &mut self.buckets[(second % LOCAL_PROVIDER_RPM_WINDOW_SECONDS) as usize];
        if bucket.second != Some(second) {
            bucket.second = Some(second);
            bucket.requests = 0;
        }
        bucket.requests = bucket.requests.saturating_add(1);
        self.last_activity_at = now;
    }

    fn requests_last_60s(&self, current_second: u64) -> u64 {
        self.buckets.iter().fold(0_u64, |total, bucket| {
            if bucket.second.is_some_and(|second| {
                current_second.saturating_sub(second) < LOCAL_PROVIDER_RPM_WINDOW_SECONDS
            }) {
                total.saturating_add(bucket.requests)
            } else {
                total
            }
        })
    }
}

pub(crate) struct ProviderRpmTracker {
    windows: HashMap<String, ProviderRpmWindow>,
    epoch_started_at: Instant,
    last_pruned_at: Instant,
}

impl ProviderRpmTracker {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            windows: HashMap::new(),
            epoch_started_at: now,
            last_pruned_at: now,
        }
    }

    pub(crate) fn record(&mut self, provider_id: &str, now: Instant) -> bool {
        self.prune(now);
        if !self.windows.contains_key(provider_id) {
            if self.windows.len() >= MAX_LOCAL_PROVIDER_RPM_TRACKERS {
                return false;
            }
            self.windows
                .insert(provider_id.to_owned(), ProviderRpmWindow::new(now));
        }
        let second = now
            .saturating_duration_since(self.epoch_started_at)
            .as_secs();
        if let Some(window) = self.windows.get_mut(provider_id) {
            window.record(second, now);
            true
        } else {
            false
        }
    }

    pub(crate) fn snapshot(&mut self, provider_id: &str, now: Instant) -> LocalQuotaSnapshot {
        self.prune(now);
        let elapsed = now.saturating_duration_since(self.epoch_started_at);
        let coverage_seconds = elapsed.as_secs().min(LOCAL_PROVIDER_RPM_WINDOW_SECONDS);
        let capacity_limited = !self.windows.contains_key(provider_id)
            && self.windows.len() >= MAX_LOCAL_PROVIDER_RPM_TRACKERS;
        let current_second = elapsed.as_secs();
        let requests_last_60s = self
            .windows
            .get(provider_id)
            .map_or(0, |window| window.requests_last_60s(current_second));
        let status = if capacity_limited {
            LocalQuotaTrackingStatus::CapacityLimited
        } else if coverage_seconds < LOCAL_PROVIDER_RPM_WINDOW_SECONDS {
            LocalQuotaTrackingStatus::WarmingUp
        } else {
            LocalQuotaTrackingStatus::Ready
        };
        LocalQuotaSnapshot {
            requests_last_60s,
            coverage_seconds,
            status,
        }
    }

    pub(crate) fn remove_provider(&mut self, provider_id: &str) {
        self.windows.remove(provider_id);
    }

    pub(crate) fn reset(&mut self, now: Instant) {
        self.windows.clear();
        self.epoch_started_at = now;
        self.last_pruned_at = now;
    }

    fn prune(&mut self, now: Instant) {
        if now.saturating_duration_since(self.last_pruned_at) < LOCAL_PROVIDER_RPM_PRUNE_INTERVAL {
            return;
        }
        self.last_pruned_at = now;
        self.windows.retain(|_, window| {
            now.saturating_duration_since(window.last_activity_at)
                < Duration::from_secs(LOCAL_PROVIDER_RPM_WINDOW_SECONDS)
        });
    }
}
