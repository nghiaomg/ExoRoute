//! Rate-limit policy, configuration, and decision types.
//!
//! Data only: these types describe what the limiter is allowed to do. The
//! stateful engine lives in `engine`.

use std::time::Duration;

pub const MAX_KEY_BYTES: usize = 256;

/// Fixed-window or token-bucket quota policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimitPolicy {
    /// Permit `max_requests` requests per `window`, aligned to the first
    /// request from each identity rather than to wall-clock boundaries.
    FixedWindow { max_requests: u32, window: Duration },
    /// Add `refill_tokens` tokens every `refill_interval`, up to `capacity`.
    /// A request consumes one token. Refill is applied in whole intervals.
    TokenBucket {
        capacity: u32,
        refill_tokens: u32,
        refill_interval: Duration,
    },
}

/// Configuration for a process-local limiter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimiterConfig {
    pub policy: RateLimitPolicy,
    /// Hard bound on identities held in memory.
    pub max_entries: usize,
    /// An entry is removable only after this much inactivity *and* after its
    /// quota has naturally reset/refilled. This prevents pruning from granting
    /// a fresh quota early.
    pub idle_ttl: Duration,
    /// Maximum interval between bounded incremental pruning batches.
    pub prune_interval: Duration,
}

impl RateLimiterConfig {
    pub const fn new(policy: RateLimitPolicy, max_entries: usize) -> Self {
        Self {
            policy,
            max_entries,
            idle_ttl: Duration::from_secs(15 * 60),
            prune_interval: Duration::from_secs(1),
        }
    }
}

/// Invalid limiter configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimiterConfigError {
    EmptyEntryLimit,
    MissingIdleTtl,
    MissingPruneInterval,
    ZeroRequestLimit,
    NoWindow,
    ZeroCapacity,
    ZeroRefill,
    ZeroRefillInterval,
}

/// Outcome of one rate-limit check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimitDecision {
    pub allowed: bool,
    /// Remaining requests/tokens after this check. Zero for rejected requests.
    pub remaining: u32,
    /// Suggested delay before retrying, present for quota and capacity denial.
    pub retry_after: Option<Duration>,
}

impl RateLimitDecision {
    pub(super) const fn allowed(remaining: u32) -> Self {
        Self {
            allowed: true,
            remaining,
            retry_after: None,
        }
    }

    pub(super) const fn rejected(retry_after: Duration) -> Self {
        Self {
            allowed: false,
            remaining: 0,
            retry_after: Some(retry_after),
        }
    }

    pub(super) const fn denied_without_estimate() -> Self {
        Self {
            allowed: false,
            remaining: 0,
            retry_after: None,
        }
    }
}
