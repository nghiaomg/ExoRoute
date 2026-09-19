use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const MAX_PROVIDER_USAGE_LOCKS: usize = 256;
const MAX_PROVIDER_USAGE_CACHE_ENTRIES: usize = 512;
const PROVIDER_USAGE_CACHE_MAX_AGE: Duration = Duration::from_secs(30 * 60);
const MAX_ANTIGRAVITY_QUOTA_BREAKERS: usize = 512;
const ANTIGRAVITY_QUOTA_STRIKE_WINDOW: Duration = Duration::from_secs(60);
const ANTIGRAVITY_QUOTA_BLOCK: Duration = Duration::from_secs(15 * 60);
const MAX_ANTIGRAVITY_RETRY_AFTER: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone)]
pub struct ProviderUsageCacheEntry {
    pub snapshot: Option<Value>,
    pub fetched_at_ms: Option<i64>,
    pub fetched_at: Option<Instant>,
    pub last_attempt_at: Instant,
    pub last_error: Option<String>,
}

#[derive(Clone, Copy)]
struct AntigravityQuotaBreakerEntry {
    window_started: Instant,
    last_seen: Instant,
    strikes: u8,
    blocked_until: Option<Instant>,
}

pub(crate) struct ProviderUsageRuntime {
    cache: Mutex<HashMap<String, ProviderUsageCacheEntry>>,
    epoch: AtomicU64,
    disabled: AtomicBool,
    locks: Mutex<HashMap<String, Weak<Mutex<()>>>>,
    fallback_lock: Arc<Mutex<()>>,
    antigravity_quota_breakers: Mutex<HashMap<String, AntigravityQuotaBreakerEntry>>,
}

impl ProviderUsageRuntime {
    pub(crate) fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            epoch: AtomicU64::new(1),
            disabled: AtomicBool::new(false),
            locks: Mutex::new(HashMap::new()),
            fallback_lock: Arc::new(Mutex::new(())),
            antigravity_quota_breakers: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn lock(&self, credential_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(credential_id).and_then(Weak::upgrade) {
            return lock;
        }
        if locks.len() >= MAX_PROVIDER_USAGE_LOCKS {
            return self.fallback_lock.clone();
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(credential_id.to_owned(), Arc::downgrade(&lock));
        lock
    }

    pub(crate) async fn cached(&self, credential_id: &str) -> Option<ProviderUsageCacheEntry> {
        let now = Instant::now();
        let mut cache = self.cache.lock().await;
        cache.retain(|_, entry| {
            now.saturating_duration_since(entry.last_attempt_at) <= PROVIDER_USAGE_CACHE_MAX_AGE
        });
        cache.get(credential_id).cloned()
    }

    pub(crate) fn epoch(&self) -> Option<u64> {
        if self.disabled.load(Ordering::Acquire) {
            None
        } else {
            Some(self.epoch.load(Ordering::Acquire))
        }
    }

    /// Invalidate quota snapshots after provider keys are removed or restored.
    /// Advancing the epoch also prevents an already-running refresh from
    /// repopulating the cache with data for a deleted credential.
    pub(crate) async fn invalidate(&self) {
        if self
            .epoch
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                epoch.checked_add(1)
            })
            .is_err()
        {
            // Disable caching rather than allow an ancient in-flight result to
            // match the epoch again after theoretical integer wraparound.
            self.disabled.store(true, Ordering::Release);
        }
        self.cache.lock().await.clear();
        self.antigravity_quota_breakers.lock().await.clear();
    }

    pub(crate) async fn clear_antigravity_quota_provider(&self, provider_id: &str) {
        let prefix = format!("{provider_id}\u{0}");
        self.antigravity_quota_breakers
            .lock()
            .await
            .retain(|key, _| !key.starts_with(&prefix));
    }

    pub(crate) async fn antigravity_quota_is_blocked(
        &self,
        provider_id: &str,
        credential_id: &str,
        model: &str,
    ) -> bool {
        let key = antigravity_quota_key(provider_id, credential_id, model);
        let now = Instant::now();
        let mut breakers = self.antigravity_quota_breakers.lock().await;
        breakers.retain(|_, entry| {
            entry.blocked_until.is_some_and(|until| until > now)
                || now.saturating_duration_since(entry.window_started)
                    <= ANTIGRAVITY_QUOTA_STRIKE_WINDOW
        });
        breakers
            .get_mut(&key)
            .map(|entry| {
                entry.last_seen = now;
                entry.blocked_until.is_some_and(|until| until > now)
            })
            .unwrap_or(false)
    }

    pub(crate) async fn record_antigravity_quota_rejection(
        &self,
        provider_id: &str,
        credential_id: &str,
        model: &str,
        retry_after: Option<Duration>,
    ) {
        let key = antigravity_quota_key(provider_id, credential_id, model);
        let now = Instant::now();
        let mut breakers = self.antigravity_quota_breakers.lock().await;
        breakers.retain(|_, entry| {
            entry.blocked_until.is_some_and(|until| until > now)
                || now.saturating_duration_since(entry.window_started)
                    <= ANTIGRAVITY_QUOTA_STRIKE_WINDOW
        });
        if !breakers.contains_key(&key)
            && breakers.len() >= MAX_ANTIGRAVITY_QUOTA_BREAKERS
            && let Some(oldest_key) = breakers
                .iter()
                .min_by_key(|(_, entry)| entry.last_seen)
                .map(|(key, _)| key.clone())
        {
            breakers.remove(&oldest_key);
        }
        let entry = breakers.entry(key).or_insert(AntigravityQuotaBreakerEntry {
            window_started: now,
            last_seen: now,
            strikes: 0,
            blocked_until: None,
        });
        if now.saturating_duration_since(entry.window_started) > ANTIGRAVITY_QUOTA_STRIKE_WINDOW {
            entry.window_started = now;
            entry.strikes = 0;
        }
        entry.last_seen = now;
        entry.strikes = entry.strikes.saturating_add(1).min(3);
        let block_for = retry_after
            .map(|duration| duration.min(MAX_ANTIGRAVITY_RETRY_AFTER))
            .filter(|duration| !duration.is_zero())
            .or_else(|| (entry.strikes >= 3).then_some(ANTIGRAVITY_QUOTA_BLOCK));
        if let Some(duration) = block_for {
            entry.blocked_until = Some(now + duration);
        }
    }

    pub(crate) async fn clear_antigravity_quota_breaker(
        &self,
        provider_id: &str,
        credential_id: &str,
        model: &str,
    ) {
        self.antigravity_quota_breakers
            .lock()
            .await
            .remove(&antigravity_quota_key(provider_id, credential_id, model));
    }

    pub(crate) async fn insert(
        &self,
        credential_id: String,
        entry: ProviderUsageCacheEntry,
        epoch: Option<u64>,
    ) {
        let Some(epoch) = epoch else {
            return;
        };
        if self.disabled.load(Ordering::Acquire) || self.epoch.load(Ordering::Acquire) != epoch {
            return;
        }
        let now = Instant::now();
        let mut cache = self.cache.lock().await;
        if self.disabled.load(Ordering::Acquire) || self.epoch.load(Ordering::Acquire) != epoch {
            return;
        }
        cache.retain(|_, cached| {
            now.saturating_duration_since(cached.last_attempt_at) <= PROVIDER_USAGE_CACHE_MAX_AGE
        });
        if !cache.contains_key(&credential_id)
            && cache.len() >= MAX_PROVIDER_USAGE_CACHE_ENTRIES
            && let Some(oldest_id) = cache
                .iter()
                .min_by_key(|(_, cached)| cached.last_attempt_at)
                .map(|(id, _)| id.clone())
        {
            cache.remove(&oldest_id);
        }
        cache.insert(credential_id, entry);
    }
}

fn antigravity_quota_key(provider_id: &str, credential_id: &str, model: &str) -> String {
    let mut key = String::with_capacity(provider_id.len() + credential_id.len() + model.len() + 2);
    key.push_str(provider_id);
    key.push('\0');
    key.push_str(credential_id);
    key.push('\0');
    key.push_str(model);
    key
}
