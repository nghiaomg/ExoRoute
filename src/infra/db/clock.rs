//! UTC timestamp formatting and clock helpers for database records.
//!
//! Request logs, stream recovery, and retention pruning share this module so
//! clock handling stays in one place instead of being copied per domain.

use crate::infra::storage::StorageError;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn utc_timestamp_now() -> Result<String, StorageError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::Invalid("system clock is before the Unix epoch".to_owned()))?
        .as_secs();
    Ok(format_utc_timestamp(seconds))
}

pub fn utc_timestamp_before(duration: Duration) -> Result<String, StorageError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::Invalid("system clock is before the Unix epoch".to_owned()))?
        .as_secs();
    Ok(format_utc_timestamp(now.saturating_sub(duration.as_secs())))
}

pub(super) fn unix_minute_now() -> Result<i64, StorageError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::Invalid("system clock is before the Unix epoch".to_owned()))?
        .as_secs();
    let minute = seconds / 60;
    Ok(if minute > i64::MAX as u64 {
        i64::MAX
    } else {
        minute as i64
    })
}

pub(super) fn format_utc_timestamp(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let day_seconds = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let days = days_since_epoch + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::format_utc_timestamp;

    #[test]
    fn utc_timestamp_conversion_matches_epoch_and_leap_day() {
        assert_eq!(format_utc_timestamp(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc_timestamp(1_709_164_800), "2024-02-29 00:00:00");
    }
}
