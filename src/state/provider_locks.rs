use super::{AppState, MAX_CODEX_REFRESH_LOCKS};
use crate::state::{FreebuffSessionCacheEntry, ProviderUsageCacheEntry};
use std::{
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::sync::Mutex;

impl AppState {
    pub async fn codex_refresh_lock(&self, credential_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.providers.codex_refresh_locks.lock().await;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(credential_id).and_then(Weak::upgrade) {
            return lock;
        }
        if locks.len() >= MAX_CODEX_REFRESH_LOCKS {
            return self.providers.codex_refresh_fallback_lock.clone();
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(credential_id.to_owned(), Arc::downgrade(&lock));
        lock
    }

    /// Serializes Codex OAuth account check-and-save operations so concurrent
    /// callbacks cannot both inspect the same old key set and overwrite or
    /// duplicate the wrong account.
    pub fn codex_account_save_lock(&self) -> Arc<Mutex<()>> {
        self.providers.codex_account_save_lock.clone()
    }

    /// Serializes token refreshes by credential for OAuth adapters whose token
    /// format is not OpenAI Codex. The shared bounded lock map has a process
    /// wide fallback when its weak-entry capacity is reached.
    pub async fn provider_token_refresh_lock(&self, credential_id: &str) -> Arc<Mutex<()>> {
        self.codex_refresh_lock(credential_id).await
    }

    pub async fn provider_usage_lock(&self, credential_id: &str) -> Arc<Mutex<()>> {
        self.providers.provider_usage.lock(credential_id).await
    }

    pub(crate) async fn antigravity_quota_is_blocked(
        &self,
        provider_id: &str,
        credential_id: &str,
        model: &str,
    ) -> bool {
        self.providers
            .provider_usage
            .antigravity_quota_is_blocked(provider_id, credential_id, model)
            .await
    }

    pub(crate) async fn record_antigravity_quota_rejection(
        &self,
        provider_id: &str,
        credential_id: &str,
        model: &str,
        retry_after: Option<Duration>,
    ) {
        self.providers
            .provider_usage
            .record_antigravity_quota_rejection(provider_id, credential_id, model, retry_after)
            .await;
    }

    pub(crate) async fn clear_antigravity_quota_breaker(
        &self,
        provider_id: &str,
        credential_id: &str,
        model: &str,
    ) {
        self.providers
            .provider_usage
            .clear_antigravity_quota_breaker(provider_id, credential_id, model)
            .await;
    }

    pub(crate) async fn freebuff_session_lock(&self, key: [u8; 32]) -> Arc<Mutex<()>> {
        self.providers.freebuff_sessions.lock(key).await
    }

    pub(crate) async fn cached_freebuff_session(
        &self,
        key: [u8; 32],
    ) -> Option<FreebuffSessionCacheEntry> {
        self.providers.freebuff_sessions.cached(key).await
    }

    pub(crate) async fn cache_freebuff_session(
        &self,
        key: [u8; 32],
        entry: FreebuffSessionCacheEntry,
    ) {
        self.providers.freebuff_sessions.insert(key, entry).await;
    }

    pub(crate) async fn invalidate_freebuff_session(&self, key: [u8; 32]) {
        self.providers.freebuff_sessions.invalidate(key).await;
    }

    pub async fn cached_provider_usage(
        &self,
        credential_id: &str,
    ) -> Option<ProviderUsageCacheEntry> {
        self.providers.provider_usage.cached(credential_id).await
    }

    pub(crate) fn provider_usage_cache_epoch(&self) -> Option<u64> {
        self.providers.provider_usage.epoch()
    }

    /// Invalidate quota snapshots after provider keys are removed or restored.
    /// Advancing the epoch also prevents an already-running refresh from
    /// repopulating the cache with data for a deleted credential.
    pub async fn invalidate_provider_usage_cache(&self) {
        self.providers.provider_usage.invalidate().await;
    }

    pub async fn cache_provider_usage(
        &self,
        credential_id: String,
        entry: ProviderUsageCacheEntry,
        epoch: Option<u64>,
    ) {
        self.providers
            .provider_usage
            .insert(credential_id, entry, epoch)
            .await;
    }
}
