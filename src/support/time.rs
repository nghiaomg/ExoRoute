//! Shared wall-clock helpers.
//!
//! Provider auth, telemetry, and key bookkeeping all need "now" as Unix
//! seconds/millis/minutes. Keeping one implementation avoids the drift between
//! saturating, defaulting, and fallible variants that used to be copied per
//! module.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch, saturating at `i64::MAX` and returning 0 when
/// the system clock is before the epoch.
pub(crate) fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

/// Milliseconds since the Unix epoch with the same saturation rules as
/// [`unix_seconds`].
pub(crate) fn unix_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

/// Minutes since the Unix epoch, derived from [`unix_seconds`].
pub(crate) fn unix_minutes() -> i64 {
    unix_seconds() / 60
}
