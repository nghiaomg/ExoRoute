use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const MAX_FREEBUFF_SESSION_CACHE_ENTRIES: usize = 256;
const MAX_FREEBUFF_SESSION_LOCKS: usize = 256;
const FREEBUFF_SESSION_CACHE_MAX_AGE: Duration = Duration::from_secs(50 * 60);

#[derive(Clone)]
pub(crate) struct FreebuffSessionCacheEntry {
    pub(crate) model: String,
    pub(crate) instance_id: String,
    pub(crate) last_used_at: Instant,
}

pub(crate) struct FreebuffSessionRuntime {
    cache: Mutex<HashMap<[u8; 32], FreebuffSessionCacheEntry>>,
    locks: Mutex<HashMap<[u8; 32], Weak<Mutex<()>>>>,
    fallback_lock: Arc<Mutex<()>>,
}

impl FreebuffSessionRuntime {
    pub(crate) fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
            locks: Mutex::new(HashMap::new()),
            fallback_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) async fn lock(&self, key: [u8; 32]) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            return lock;
        }
        if locks.len() >= MAX_FREEBUFF_SESSION_LOCKS {
            return self.fallback_lock.clone();
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(key, Arc::downgrade(&lock));
        lock
    }

    pub(crate) async fn cached(&self, key: [u8; 32]) -> Option<FreebuffSessionCacheEntry> {
        let now = Instant::now();
        let mut cache = self.cache.lock().await;
        cache.retain(|_, entry| {
            now.saturating_duration_since(entry.last_used_at) < FREEBUFF_SESSION_CACHE_MAX_AGE
        });
        let entry = cache.get_mut(&key)?;
        entry.last_used_at = now;
        Some(entry.clone())
    }

    pub(crate) async fn insert(&self, key: [u8; 32], mut entry: FreebuffSessionCacheEntry) {
        let now = Instant::now();
        entry.last_used_at = now;
        let mut cache = self.cache.lock().await;
        cache.retain(|_, current| {
            now.saturating_duration_since(current.last_used_at) < FREEBUFF_SESSION_CACHE_MAX_AGE
        });
        if !cache.contains_key(&key) && cache.len() >= MAX_FREEBUFF_SESSION_CACHE_ENTRIES {
            let oldest_key = cache
                .iter()
                .min_by_key(|(_, current)| current.last_used_at)
                .map(|(key, _)| *key);
            if let Some(oldest_key) = oldest_key {
                cache.remove(&oldest_key);
            }
        }
        cache.insert(key, entry);
    }

    pub(crate) async fn invalidate(&self, key: [u8; 32]) {
        self.cache.lock().await.remove(&key);
    }

    pub(crate) async fn clear(&self) {
        self.cache.lock().await.clear();
    }
}
