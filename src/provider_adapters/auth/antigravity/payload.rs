use super::{ProviderUsageCreditBalance, Value, antigravity_metadata, as_number, bounded_text};
use serde_json::json;

pub(crate) fn extract_credit_balance(
    values: Option<&Value>,
    second: Option<&Value>,
    third: Option<&Value>,
) -> Option<ProviderUsageCreditBalance> {
    for value in [values, second, third].into_iter().flatten() {
        for key in [
            "remainingCredits",
            "remaining_credits",
            "creditBalance",
            "credit_balance",
        ] {
            if let Some(amount) = value.get(key).and_then(extract_credit_amount) {
                return Some(ProviderUsageCreditBalance {
                    unit: "credits".to_owned(),
                    monthly_remaining: Some(amount.max(0.0)),
                    purchased_remaining: None,
                    free_remaining: None,
                    period_used: None,
                    period_ends_at: None,
                });
            }
        }
    }
    None
}

pub(crate) fn extract_credit_amount(value: &Value) -> Option<f64> {
    extract_credit_amount_inner(value, 0)
}

pub(crate) fn extract_credit_amount_inner(value: &Value, depth: usize) -> Option<f64> {
    if depth > 3 {
        return None;
    }
    if let Some(amount) = as_number(value) {
        return Some(amount);
    }
    if let Some(object) = value.as_object() {
        for key in [
            "remaining",
            "remainingCredits",
            "remaining_credits",
            "creditAmount",
            "credit_amount",
            "amount",
        ] {
            if let Some(amount) = object.get(key).and_then(as_number) {
                return Some(amount);
            }
        }
        return object
            .get("credits")
            .or_else(|| object.get("items"))
            .and_then(Value::as_array)
            .and_then(|items| {
                items.iter().take(32).find_map(|item| {
                    let credit_type = item
                        .get("creditType")
                        .or_else(|| item.get("credit_type"))
                        .and_then(Value::as_str);
                    if credit_type.is_some_and(|kind| kind != "GOOGLE_ONE_AI") {
                        return None;
                    }
                    extract_credit_amount_inner(item, depth + 1)
                })
            });
    }
    value.as_array().and_then(|items| {
        items.iter().take(32).find_map(|item| {
            let credit_type = item
                .get("creditType")
                .or_else(|| item.get("credit_type"))
                .and_then(Value::as_str);
            if credit_type.is_some_and(|kind| kind != "GOOGLE_ONE_AI") {
                return None;
            }
            extract_credit_amount_inner(item, depth + 1)
        })
    })
}

pub(crate) fn extract_plan(value: &Value) -> Option<String> {
    [
        value.get("plan"),
        value.get("tier"),
        value.get("currentTier"),
        value.get("current_tier"),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| {
        value
            .as_str()
            .or_else(|| value.get("name").and_then(Value::as_str))
            .or_else(|| value.get("id").and_then(Value::as_str))
            .map(bounded_text)
    })
}

pub(crate) fn extract_project_id(value: &Value) -> Option<String> {
    value
        .get("cloudaicompanionProject")
        .or_else(|| value.get("cloudaicompanion_project"))
        .and_then(|project| {
            project.as_str().map(ToOwned::to_owned).or_else(|| {
                project
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
        })
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && value.len() <= 256)
}

pub(crate) fn extract_onboard_project_id(value: &Value) -> Option<String> {
    value
        .get("response")
        .and_then(extract_project_id)
        .or_else(|| extract_project_id(value))
}

pub(crate) fn onboard_user_body(tier: Option<&str>) -> Value {
    json!({
        "tierId": tier.unwrap_or("legacy-tier"),
        "metadata": antigravity_metadata(),
    })
}

pub(crate) fn extract_tier(value: &Value) -> Option<String> {
    let direct_tier = value
        .get("paidTier")
        .and_then(|tier| tier.get("id"))
        .or_else(|| value.get("paid_tier").and_then(|tier| tier.get("id")))
        .or_else(|| value.get("tierId"))
        .or_else(|| value.get("tier_id"))
        .or_else(|| value.get("tier").and_then(|tier| tier.get("id")))
        .or_else(|| value.get("currentTier").and_then(|tier| tier.get("id")))
        .and_then(Value::as_str);
    direct_tier
        .or_else(|| {
            value
                .get("allowedTiers")
                .or_else(|| value.get("allowed_tiers"))
                .and_then(Value::as_array)
                .and_then(|tiers| {
                    tiers
                        .iter()
                        .find(|tier| tier.get("isDefault").and_then(Value::as_bool) == Some(true))
                        .or_else(|| tiers.first())
                        .and_then(|tier| {
                            tier.get("id")
                                .or_else(|| tier.get("tierId"))
                                .or_else(|| tier.get("tier_id"))
                                .and_then(Value::as_str)
                        })
                })
        })
        .map(bounded_text)
}

pub(crate) fn extract_reset_at(value: &Value) -> Option<i64> {
    for key in ["resetAt", "reset_at", "resetTime", "reset_time"] {
        if let Some(number) = value.get(key).and_then(Value::as_i64)
            && (1..=i64::MAX).contains(&number)
        {
            return Some(if number > 10_000_000_000 {
                number / 1000
            } else {
                number
            });
        }
        if let Some(text) = value.get(key).and_then(Value::as_str)
            && let Ok(parsed) = chrono_like_timestamp(text)
        {
            return Some(parsed);
        }
    }
    None
}

pub(crate) fn chrono_like_timestamp(value: &str) -> Result<i64, ()> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<i64>()
        && (1..=i64::MAX).contains(&seconds)
    {
        return Ok(if seconds > 10_000_000_000 {
            seconds / 1000
        } else {
            seconds
        });
    }

    let (date, time) = value
        .strip_suffix('Z')
        .or_else(|| value.strip_suffix("+00:00"))
        .and_then(|value| value.split_once('T'))
        .ok_or(())?;
    let mut date_parts = date.split('-');
    let year = date_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or(())?;
    let month = date_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or(())?;
    let day = date_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or(())?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(());
    }
    let time = time.split('.').next().ok_or(())?;
    let mut time_parts = time.split(':');
    let hour = time_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or(())?;
    let minute = time_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or(())?;
    let second = time_parts
        .next()
        .and_then(|part| part.parse::<i64>().ok())
        .ok_or(())?;
    if time_parts.next().is_some() || hour > 23 || minute > 59 || second > 60 {
        return Err(());
    }

    // Howard Hinnant's civil-date conversion, bounded to the valid i64 range.
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year / 400
    } else {
        (adjusted_year - 399) / 400
    };
    let year_of_era = adjusted_year - era * 400;
    let month_index = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    days.checked_mul(86_400)
        .and_then(|seconds| seconds.checked_add(hour * 3_600 + minute * 60 + second))
        .filter(|seconds| *seconds > 0)
        .ok_or(())
}
