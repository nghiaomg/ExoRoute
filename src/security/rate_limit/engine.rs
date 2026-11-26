//! Stateful rate-limit engine.
//!
//! Owns the per-identity quota map, its lock discipline, and the bounded
//! pruning work performed on each request.

use super::policy::{
    MAX_KEY_BYTES, RateLimitDecision, RateLimitPolicy, RateLimiterConfig, RateLimiterConfigError,
};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PRUNE_BATCH_SIZE: usize = 1_024;

#[derive(Debug)]
struct Entry {
    quota: QuotaState,
    last_seen: Instant,
}

#[derive(Debug)]
enum QuotaState {
    FixedWindow { started_at: Instant, used: u32 },
    TokenBucket { tokens: u32, refilled_at: Instant },
}

#[derive(Debug)]
struct State {
    entries: HashMap<String, Entry>,
    prune_queue: VecDeque<String>,
    next_prune_at: Instant,
    policy: RateLimitPolicy,
}

/// Thread-safe rate limiter with bounded memory and no external dependencies.
pub struct RateLimiter {
    config: RateLimiterConfig,
    state: Mutex<State>,
}

impl RateLimiter {
    pub fn new(config: RateLimiterConfig) -> Result<Self, RateLimiterConfigError> {
        if config.max_entries == 0 {
            return Err(RateLimiterConfigError::EmptyEntryLimit);
        }
        if config.idle_ttl.is_zero() {
            return Err(RateLimiterConfigError::MissingIdleTtl);
        }
        if config.prune_interval.is_zero() {
            return Err(RateLimiterConfigError::MissingPruneInterval);
        }
        match config.policy {
            RateLimitPolicy::FixedWindow {
                max_requests,
                window,
            } => {
                if max_requests == 0 {
                    return Err(RateLimiterConfigError::ZeroRequestLimit);
                }
                if window.is_zero() {
                    return Err(RateLimiterConfigError::NoWindow);
                }
            }
            RateLimitPolicy::TokenBucket {
                capacity,
                refill_tokens,
                refill_interval,
            } => {
                if capacity == 0 {
                    return Err(RateLimiterConfigError::ZeroCapacity);
                }
                if refill_tokens == 0 {
                    return Err(RateLimiterConfigError::ZeroRefill);
                }
                if refill_interval.is_zero() {
                    return Err(RateLimiterConfigError::ZeroRefillInterval);
                }
            }
        }

        let now = Instant::now();
        Ok(Self {
            config,
            state: Mutex::new(State {
                entries: HashMap::with_capacity(config.max_entries.min(1024)),
                prune_queue: VecDeque::with_capacity(config.max_entries.min(1024)),
                next_prune_at: now,
                policy: config.policy,
            }),
        })
    }

    /// Check and consume one request using the current monotonic time.
    pub fn check(&self, key: &str) -> RateLimitDecision {
        self.check_at(key, Instant::now())
    }

    /// Check and consume one request using a caller-supplied policy snapshot.
    /// This lets runtime settings change without replacing the bounded identity
    /// map or resetting active quotas.
    pub fn check_with_policy(&self, key: &str, policy: RateLimitPolicy) -> RateLimitDecision {
        self.check_at_with_policy(key, Instant::now(), policy)
    }

    /// Check at an explicit monotonic time. This is public to make integration
    /// deterministic in tests; production callers should normally use
    /// [`RateLimiter::check`].
    pub fn check_at(&self, key: &str, now: Instant) -> RateLimitDecision {
        self.check_at_internal(key, now, None)
    }

    pub fn check_at_with_policy(
        &self,
        key: &str,
        now: Instant,
        policy: RateLimitPolicy,
    ) -> RateLimitDecision {
        self.check_at_internal(key, now, Some(policy))
    }

    /// Change the policy without resetting active identities' quotas. Fixed
    /// windows retain the number of requests already used and begin a new
    /// window at the change instant. Token buckets are refilled under the old
    /// policy up to this instant, then clamped to the new capacity.
    pub(crate) fn reconfigure(&self, policy: RateLimitPolicy) {
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_policy = state.policy;
        if previous_policy == policy {
            return;
        }
        for entry in state.entries.values_mut() {
            match (previous_policy, policy, &mut entry.quota) {
                (
                    RateLimitPolicy::FixedWindow { .. },
                    RateLimitPolicy::FixedWindow { .. },
                    QuotaState::FixedWindow { started_at, .. },
                ) => *started_at = now,
                (
                    RateLimitPolicy::TokenBucket {
                        capacity,
                        refill_tokens,
                        refill_interval,
                    },
                    RateLimitPolicy::TokenBucket {
                        capacity: new_capacity,
                        ..
                    },
                    QuotaState::TokenBucket {
                        tokens,
                        refilled_at,
                    },
                ) => {
                    Self::refill_bucket(
                        tokens,
                        refilled_at,
                        now,
                        capacity,
                        refill_tokens,
                        refill_interval,
                    );
                    *tokens = (*tokens).min(new_capacity);
                    *refilled_at = now;
                }
                // The two runtime-configurable limiters keep their algorithm
                // fixed. If a future caller violates that invariant, do not
                // discard quota state or grant a fresh bucket.
                _ => {}
            }
        }
        state.policy = policy;
    }

    fn check_at_internal(
        &self,
        key: &str,
        now: Instant,
        requested_policy: Option<RateLimitPolicy>,
    ) -> RateLimitDecision {
        if key.is_empty() || key.len() > MAX_KEY_BYTES {
            return RateLimitDecision::denied_without_estimate();
        }

        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let policy = requested_policy.unwrap_or(state.policy);
        if now >= state.next_prune_at {
            let batch_len = state.prune_queue.len().min(PRUNE_BATCH_SIZE);
            for _ in 0..batch_len {
                let Some(key) = state.prune_queue.pop_front() else {
                    break;
                };
                let remove = state
                    .entries
                    .get(&key)
                    .is_some_and(|entry| self.is_prunable(entry, now, policy));
                if remove {
                    state.entries.remove(&key);
                } else if state.entries.contains_key(&key) {
                    state.prune_queue.push_back(key);
                }
            }
            state.next_prune_at = now.checked_add(self.config.prune_interval).unwrap_or(now);
        }

        if let Some(entry) = state.entries.get_mut(key) {
            entry.last_seen = now;
            return self.apply_policy(policy, &mut entry.quota, now);
        }

        if state.entries.len() >= self.config.max_entries {
            // Do not evict a live identity: doing so would reset its quota.
            return RateLimitDecision::denied_without_estimate();
        }

        let (quota, decision) = Self::initial_quota(policy, now);
        state.entries.insert(
            key.to_owned(),
            Entry {
                quota,
                last_seen: now,
            },
        );
        state.prune_queue.push_back(key.to_owned());
        decision
    }

    /// Current number of retained identities, useful for deterministic tests.
    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .len()
    }

    fn initial_quota(policy: RateLimitPolicy, now: Instant) -> (QuotaState, RateLimitDecision) {
        match policy {
            RateLimitPolicy::FixedWindow { max_requests, .. } => (
                QuotaState::FixedWindow {
                    started_at: now,
                    used: 1,
                },
                RateLimitDecision::allowed(max_requests - 1),
            ),
            RateLimitPolicy::TokenBucket { capacity, .. } => (
                QuotaState::TokenBucket {
                    tokens: capacity - 1,
                    refilled_at: now,
                },
                RateLimitDecision::allowed(capacity - 1),
            ),
        }
    }

    fn apply_policy(
        &self,
        policy: RateLimitPolicy,
        quota: &mut QuotaState,
        now: Instant,
    ) -> RateLimitDecision {
        match (policy, quota) {
            (
                RateLimitPolicy::FixedWindow {
                    max_requests,
                    window,
                },
                QuotaState::FixedWindow { started_at, used },
            ) => {
                let elapsed = now.saturating_duration_since(*started_at);
                if elapsed >= window {
                    *started_at = now;
                    *used = 1;
                    return RateLimitDecision::allowed(max_requests - 1);
                }
                if *used < max_requests {
                    *used += 1;
                    RateLimitDecision::allowed(max_requests - *used)
                } else {
                    RateLimitDecision::rejected(window.saturating_sub(elapsed))
                }
            }
            (
                RateLimitPolicy::TokenBucket {
                    capacity,
                    refill_tokens,
                    refill_interval,
                },
                QuotaState::TokenBucket {
                    tokens,
                    refilled_at,
                },
            ) => {
                Self::refill_bucket(
                    tokens,
                    refilled_at,
                    now,
                    capacity,
                    refill_tokens,
                    refill_interval,
                );

                if *tokens > 0 {
                    *tokens -= 1;
                    RateLimitDecision::allowed(*tokens)
                } else {
                    let elapsed = now.saturating_duration_since(*refilled_at);
                    RateLimitDecision::rejected(refill_interval.saturating_sub(elapsed))
                }
            }
            // Keep the identity blocked if a caller changes a limiter's
            // algorithm without migrating its quota representation.
            _ => RateLimitDecision::rejected(self.config.prune_interval),
        }
    }

    fn refill_bucket(
        tokens: &mut u32,
        refilled_at: &mut Instant,
        now: Instant,
        capacity: u32,
        refill_tokens: u32,
        refill_interval: Duration,
    ) {
        let elapsed = now.saturating_duration_since(*refilled_at);
        let intervals = elapsed.as_nanos() / refill_interval.as_nanos();
        if intervals == 0 {
            return;
        }

        let tokens_needed = capacity.saturating_sub(*tokens);
        let intervals_to_full = u128::from(tokens_needed / refill_tokens)
            + u128::from(!tokens_needed.is_multiple_of(refill_tokens));
        if intervals >= intervals_to_full {
            *tokens = capacity;
            *refilled_at = now;
            return;
        }

        // Since the bucket did not fill, this product is bounded by capacity.
        let added = (intervals as u32).saturating_mul(refill_tokens);
        *tokens = tokens.saturating_add(added).min(capacity);
        if let Some(whole_intervals) = refill_interval.checked_mul(intervals as u32) {
            if let Some(updated) = refilled_at.checked_add(whole_intervals) {
                *refilled_at = updated;
            } else {
                *refilled_at = now;
            }
        } else {
            *refilled_at = now;
        }
    }

    fn is_prunable(&self, entry: &Entry, now: Instant, policy: RateLimitPolicy) -> bool {
        if now.saturating_duration_since(entry.last_seen) < self.config.idle_ttl {
            return false;
        }
        match (policy, &entry.quota) {
            (
                RateLimitPolicy::FixedWindow { window, .. },
                QuotaState::FixedWindow { started_at, .. },
            ) => now.saturating_duration_since(*started_at) >= window,
            (
                RateLimitPolicy::TokenBucket {
                    capacity,
                    refill_tokens,
                    refill_interval,
                },
                QuotaState::TokenBucket {
                    tokens,
                    refilled_at,
                },
            ) => {
                if *tokens >= capacity {
                    true
                } else {
                    let elapsed = now.saturating_duration_since(*refilled_at);
                    let intervals = elapsed.as_nanos() / refill_interval.as_nanos();
                    let tokens_needed = capacity - *tokens;
                    let intervals_to_full = u128::from(tokens_needed / refill_tokens)
                        + u128::from(!tokens_needed.is_multiple_of(refill_tokens));
                    intervals >= intervals_to_full
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn config(policy: RateLimitPolicy, max_entries: usize) -> RateLimiterConfig {
        let mut config = RateLimiterConfig::new(policy, max_entries);
        config.idle_ttl = Duration::from_secs(10);
        config.prune_interval = Duration::from_secs(1);
        config
    }

    #[test]
    fn fixed_window_allows_limit_then_rejects_until_window_expires() {
        let limiter = RateLimiter::new(config(
            RateLimitPolicy::FixedWindow {
                max_requests: 2,
                window: Duration::from_secs(60),
            },
            10,
        ))
        .unwrap();
        let start = Instant::now();

        assert_eq!(
            limiter.check_at("client", start),
            RateLimitDecision::allowed(1)
        );
        assert_eq!(
            limiter.check_at("client", start + Duration::from_secs(1)),
            RateLimitDecision::allowed(0)
        );
        let denied = limiter.check_at("client", start + Duration::from_secs(2));
        assert!(!denied.allowed);
        assert_eq!(denied.retry_after, Some(Duration::from_secs(58)));
        assert_eq!(
            limiter.check_at("client", start + Duration::from_secs(60)),
            RateLimitDecision::allowed(1)
        );
    }

    #[test]
    fn token_bucket_refills_in_whole_intervals_and_caps_at_capacity() {
        let limiter = RateLimiter::new(config(
            RateLimitPolicy::TokenBucket {
                capacity: 2,
                refill_tokens: 1,
                refill_interval: Duration::from_secs(10),
            },
            10,
        ))
        .unwrap();
        let start = Instant::now();

        assert_eq!(
            limiter.check_at("client", start),
            RateLimitDecision::allowed(1)
        );
        assert_eq!(
            limiter.check_at("client", start + Duration::from_secs(1)),
            RateLimitDecision::allowed(0)
        );
        let denied = limiter.check_at("client", start + Duration::from_secs(2));
        assert_eq!(denied.retry_after, Some(Duration::from_secs(8)));
        assert_eq!(
            limiter.check_at("client", start + Duration::from_secs(10)),
            RateLimitDecision::allowed(0)
        );
        assert_eq!(
            limiter.check_at("client", start + Duration::from_secs(20)),
            RateLimitDecision::allowed(0)
        );
        assert_eq!(
            limiter.check_at("client", start + Duration::from_secs(100)),
            RateLimitDecision::allowed(1)
        );
    }

    #[test]
    fn full_map_denies_new_identities_without_evicting_existing_quota() {
        let limiter = RateLimiter::new(config(
            RateLimitPolicy::FixedWindow {
                max_requests: 1,
                window: Duration::from_secs(30),
            },
            1,
        ))
        .unwrap();
        let start = Instant::now();
        assert!(limiter.check_at("known", start).allowed);
        assert!(!limiter.check_at("attacker-1", start).allowed);
        assert_eq!(limiter.entry_count(), 1);
        assert!(
            !limiter
                .check_at("known", start + Duration::from_secs(1))
                .allowed
        );
        assert_eq!(limiter.entry_count(), 1);
    }

    #[test]
    fn pruning_removes_only_idle_entries_whose_quota_has_reset() {
        let limiter = RateLimiter::new(config(
            RateLimitPolicy::FixedWindow {
                max_requests: 1,
                window: Duration::from_secs(5),
            },
            1,
        ))
        .unwrap();
        let start = Instant::now();
        assert!(limiter.check_at("old", start).allowed);
        assert!(
            !limiter
                .check_at("new", start + Duration::from_secs(6))
                .allowed
        );
        // Once both the idle TTL and window have elapsed, pruning makes room.
        assert!(
            limiter
                .check_at("new", start + Duration::from_secs(11))
                .allowed
        );
        assert_eq!(limiter.entry_count(), 1);
    }

    #[test]
    fn pruning_work_is_bounded_to_one_batch_per_request() {
        let limiter = RateLimiter::new(config(
            RateLimitPolicy::FixedWindow {
                max_requests: 1,
                window: Duration::from_secs(5),
            },
            4096,
        ))
        .unwrap();
        let start = Instant::now();
        for index in 0..(PRUNE_BATCH_SIZE * 2) {
            assert!(limiter.check_at(&format!("idle-{index}"), start).allowed);
        }
        assert_eq!(limiter.entry_count(), PRUNE_BATCH_SIZE * 2);

        let first_prune = start + Duration::from_secs(11);
        assert!(limiter.check_at("new-one", first_prune).allowed);
        assert_eq!(limiter.entry_count(), PRUNE_BATCH_SIZE + 1);

        assert!(
            limiter
                .check_at("new-two", first_prune + Duration::from_secs(1))
                .allowed
        );
        assert_eq!(limiter.entry_count(), 2);
    }

    #[test]
    fn key_size_and_empty_key_are_rejected_without_allocating_state() {
        let limiter = RateLimiter::new(config(
            RateLimitPolicy::FixedWindow {
                max_requests: 3,
                window: Duration::from_secs(60),
            },
            4,
        ))
        .unwrap();
        assert!(!limiter.check("").allowed);
        assert!(!limiter.check(&"x".repeat(MAX_KEY_BYTES + 1)).allowed);
        assert_eq!(limiter.entry_count(), 0);
    }

    #[test]
    fn concurrent_checks_cannot_exceed_fixed_window_limit() {
        let limiter = Arc::new(
            RateLimiter::new(config(
                RateLimitPolicy::FixedWindow {
                    max_requests: 25,
                    window: Duration::from_secs(60),
                },
                100,
            ))
            .unwrap(),
        );
        let start = Instant::now();
        let mut workers = Vec::new();
        for _ in 0..100 {
            let limiter = Arc::clone(&limiter);
            workers.push(thread::spawn(move || {
                limiter.check_at("one-client", start).allowed
            }));
        }
        let allowed = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|allowed| *allowed)
            .count();
        assert_eq!(allowed, 25);
    }

    #[test]
    fn validates_configuration() {
        assert_eq!(
            RateLimiter::new(config(
                RateLimitPolicy::TokenBucket {
                    capacity: 0,
                    refill_tokens: 1,
                    refill_interval: Duration::from_secs(1),
                },
                1,
            ))
            .err(),
            Some(RateLimiterConfigError::ZeroCapacity)
        );
    }

    #[test]
    fn runtime_fixed_window_change_preserves_consumed_quota() {
        let limiter = RateLimiter::new(config(
            RateLimitPolicy::FixedWindow {
                max_requests: 3,
                window: Duration::from_secs(60),
            },
            10,
        ))
        .unwrap();
        let start = Instant::now();
        assert!(limiter.check_at("client", start).allowed);
        assert!(limiter.check_at("client", start).allowed);

        let reduced = RateLimitPolicy::FixedWindow {
            max_requests: 2,
            window: Duration::from_secs(60),
        };
        limiter.reconfigure(reduced);
        assert!(!limiter.check_at("client", Instant::now()).allowed);
        assert!(
            limiter
                .check_at("client", Instant::now() + Duration::from_secs(61))
                .allowed
        );
    }

    #[test]
    fn runtime_token_bucket_change_does_not_mint_tokens() {
        let limiter = RateLimiter::new(config(
            RateLimitPolicy::TokenBucket {
                capacity: 4,
                refill_tokens: 1,
                refill_interval: Duration::from_secs(3_600),
            },
            10,
        ))
        .unwrap();
        let start = Instant::now();
        assert!(limiter.check_at("client", start).allowed);
        assert!(limiter.check_at("client", start).allowed);

        let increased = RateLimitPolicy::TokenBucket {
            capacity: 8,
            refill_tokens: 1,
            refill_interval: Duration::from_secs(3_600),
        };
        limiter.reconfigure(increased);
        let changed = Instant::now();
        assert_eq!(
            limiter.check_at("client", changed),
            RateLimitDecision::allowed(1)
        );
        assert_eq!(
            limiter.check_at("client", changed),
            RateLimitDecision::allowed(0)
        );
        assert!(!limiter.check_at("client", changed).allowed);
    }
}
