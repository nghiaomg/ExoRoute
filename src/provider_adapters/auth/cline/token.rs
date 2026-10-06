use super::*;

pub(super) fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty()
        || token.len() > MAX_TOKEN_BYTES
        || token.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err("Cline returned invalid token data".to_owned());
    }
    Ok(())
}

pub(super) fn bounded(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.chars().take(512).collect::<String>())
        .filter(|value| !value.trim().is_empty())
}

pub(super) fn parse_rfc3339_epoch(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || (bytes.get(10) != Some(&b'T') && bytes.get(10) != Some(&b't'))
    {
        return None;
    }
    let year = fixed_digits(bytes, 0, 4)? as i64;
    let month = fixed_digits(bytes, 5, 2)? as i64;
    let day = fixed_digits(bytes, 8, 2)? as i64;
    let hour = fixed_digits(bytes, 11, 2)? as i64;
    let minute = fixed_digits(bytes, 14, 2)? as i64;
    let second = fixed_digits(bytes, 17, 2)? as i64;
    if bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut index = 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    }
    let offset = match bytes.get(index) {
        Some(b'Z' | b'z') if index + 1 == bytes.len() => 0,
        Some(sign @ (b'+' | b'-'))
            if index + 6 == bytes.len() && bytes.get(index + 3) == Some(&b':') =>
        {
            let hours = fixed_digits(bytes, index + 1, 2)? as i64;
            let minutes = fixed_digits(bytes, index + 4, 2)? as i64;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let offset = hours * 3600 + minutes * 60;
            if *sign == b'+' { offset } else { -offset }
        }
        _ => return None,
    };
    let adjusted_year = year - i64::from(month <= 2);
    let era = adjusted_year.div_euclid(400);
    let year_of_era = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(
        (era * 146_097 + day_of_era - 719_468) * 86_400 + hour * 3600 + minute * 60 + second
            - offset,
    )
}

pub(super) fn parse_expiry_value(value: &Value) -> Option<i64> {
    match value {
        Value::String(value) => parse_rfc3339_epoch(value)
            .or_else(|| value.trim().parse::<i64>().ok().map(normalize_epoch)),
        Value::Number(value) => value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .map(normalize_epoch),
        _ => None,
    }
}

pub(super) fn normalize_epoch(value: i64) -> i64 {
    if value > 100_000_000_000 {
        value.saturating_div(1_000)
    } else {
        value
    }
}

pub(super) fn fixed_digits(bytes: &[u8], start: usize, length: usize) -> Option<u32> {
    bytes
        .get(start..start.checked_add(length)?)?
        .iter()
        .try_fold(0_u32, |value, digit| {
            if !digit.is_ascii_digit() {
                return None;
            }
            value.checked_mul(10)?.checked_add(u32::from(*digit - b'0'))
        })
}

pub(super) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}
