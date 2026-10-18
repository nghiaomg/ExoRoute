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

/// Parses the `YYYY-MM-DD HH:MM:SS` shape [`format_utc_timestamp`] writes.
///
/// Returns `None` for anything else, so a malformed timestamp is rejected
/// instead of being read as the epoch. Timestamps are UTC with no offset, so
/// the result is directly comparable with [`utc_timestamp_now`].
pub fn parse_utc_timestamp(value: &str) -> Option<u64> {
    if value.len() != 19 {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b' '
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let year = value.get(0..4)?.parse::<i64>().ok()?;
    let number = |start: usize| -> Option<i64> { value.get(start..start + 2)?.parse::<i64>().ok() };
    let month = number(5)?;
    let day = number(8)?;
    let hour = number(11)?;
    let minute = number(14)?;
    let second = number(17)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour * 3_600 + minute * 60 + second)?;
    u64::try_from(seconds).ok()
}

/// Inverse of [`civil_from_days`]: days since the Unix epoch for a civil date.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = (month + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// Formats seconds since the Unix epoch as the `YYYY-MM-DD HH:MM:SS` shape the
/// request-log time index is keyed by. Kept public so callers that bucket
/// history by second can label their buckets with the same shape
/// [`parse_utc_timestamp`] reads back.
pub fn format_utc_timestamp(seconds: u64) -> String {
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
    use super::{format_utc_timestamp, parse_utc_timestamp};

    #[test]
    fn utc_timestamp_conversion_matches_epoch_and_leap_day() {
        assert_eq!(format_utc_timestamp(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc_timestamp(1_709_164_800), "2024-02-29 00:00:00");
    }

    #[test]
    fn parsing_a_formatted_timestamp_round_trips() {
        for seconds in [0_u64, 1, 1_709_164_800, 1_767_225_599, 4_102_444_800] {
            let formatted = format_utc_timestamp(seconds);
            assert_eq!(
                parse_utc_timestamp(&formatted),
                Some(seconds),
                "{formatted}"
            );
        }
    }

    #[test]
    fn malformed_timestamps_are_rejected() {
        for value in [
            "",
            "1970-01-01",
            "1970-01-01T00:00:00",
            "1970-01-01 00:00:0",
            "1970-13-01 00:00:00",
            "1970-01-32 00:00:00",
            "1970-01-01 24:00:00",
            "1970-01-01 00:60:00",
            "1970-01-01 00:00:60",
            "19x0-01-01 00:00:00",
        ] {
            assert_eq!(parse_utc_timestamp(value), None, "{value}");
        }
    }
}
