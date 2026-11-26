//! Small, process-local rate limiter primitives.
//!
//! Create separate `RateLimiter` instances for different identities/policies
//! (for example, admin login IPs and authenticated gateway client IDs). The
//! map has a hard entry limit. When full, new identities are rejected until
//! expired entries can be safely pruned; this avoids evicting an active
//! identity and letting an attacker bypass its quota by cycling identities.

mod engine;
mod policy;

pub use engine::RateLimiter;
pub use policy::{RateLimitDecision, RateLimitPolicy, RateLimiterConfig};
