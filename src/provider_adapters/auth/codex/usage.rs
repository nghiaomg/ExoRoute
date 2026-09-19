//! Parsing for the OpenAI Codex usage and quota response.
//!
//! The HTTP/authentication lifecycle stays in [`super::codex`].  This module
//! owns only the provider response shape, normalization, and bounded quota
//! projection used by the dashboard.

use serde_json::{Value, json};

pub(crate) const MAX_USAGE_QUOTAS: usize = 128;

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn first_value<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| value.get(*key).filter(|candidate| !candidate.is_null()))
}

fn as_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
        .filter(|number| number.is_finite())
}

fn as_nonempty_string(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(128).collect())
}

fn parse_epoch_seconds(value: &Value) -> Option<i64> {
    let timestamp = as_number(value)?.trunc();
    if timestamp <= 0.0 || timestamp > i64::MAX as f64 {
        return None;
    }
    let timestamp = timestamp as i64;
    Some(if timestamp >= 1_000_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    })
}

fn rate_limit_payload(value: &Value) -> &Value {
    first_value(value, &["rate_limit", "rateLimit"]).unwrap_or(value)
}

fn quota_window_name(default_window: &str, duration: Option<i64>) -> &'static str {
    const SIX_HOURS: i64 = 6 * 60 * 60;
    const SIX_DAYS: i64 = 6 * 24 * 60 * 60;
    const TWENTY_DAYS: i64 = 20 * 24 * 60 * 60;
    match duration {
        Some(seconds) if seconds >= TWENTY_DAYS => "Monthly",
        Some(seconds) if seconds >= SIX_DAYS => "Weekly",
        Some(seconds) if seconds <= SIX_HOURS => "Session",
        _ if default_window == "Session" => "Session",
        _ => "Weekly",
    }
}

fn quota_id(prefix: &str, window: &str) -> String {
    let window_id = match window {
        "Session" => "session",
        "Monthly" => "monthly",
        _ => "weekly",
    };
    match (prefix, window_id) {
        ("", "session") => "session".to_owned(),
        ("", "weekly") => "weekly".to_owned(),
        ("", _) => "monthly".to_owned(),
        ("code_review", "session") => "code_review_session".to_owned(),
        ("code_review", "weekly") => "code_review_weekly".to_owned(),
        ("code_review", _) => "code_review_monthly".to_owned(),
        ("spark", "session") => "spark_session".to_owned(),
        ("spark", "weekly") => "spark_weekly".to_owned(),
        ("spark", _) => "spark_monthly".to_owned(),
        _ => format!("{prefix}_{window_id}"),
    }
}

fn quota_label(prefix: &str, window: &str) -> String {
    match prefix {
        "" => window.to_owned(),
        "code_review" => format!("Code review · {window}"),
        "spark" => format!("Spark · {window}"),
        other => format!("{other} · {window}"),
    }
}

fn parse_quota_window(
    value: &Value,
    id: String,
    label: String,
) -> Option<crate::provider_adapters::ProviderUsageQuota> {
    let used_percent = first_value(value, &["used_percent", "usedPercent", "percent_used"])
        .and_then(as_number)?
        .clamp(0.0, 100.0);
    let reset_at = first_value(value, &["reset_at", "resetAt", "resets_at"])
        .and_then(parse_epoch_seconds)
        .or_else(|| {
            first_value(value, &["reset_after_seconds", "resetAfterSeconds"])
                .and_then(as_number)
                .filter(|seconds| *seconds > 0.0)
                .map(|seconds| unix_now().saturating_add(seconds.trunc() as i64))
        });
    let window_seconds = first_value(
        value,
        &[
            "limit_window_seconds",
            "limitWindowSeconds",
            "window_seconds",
            "windowSeconds",
        ],
    )
    .and_then(as_number)
    .filter(|seconds| *seconds > 0.0 && *seconds <= i64::MAX as f64)
    .map(|seconds| seconds.trunc() as i64);

    Some(crate::provider_adapters::ProviderUsageQuota {
        id,
        label,
        used_percent,
        remaining_percent: 100.0 - used_percent,
        used_amount: None,
        limit_amount: None,
        unit: None,
        reset_at,
        window_seconds,
        uncapped: false,
        reset_period: None,
    })
}

fn append_rate_limit_quotas(
    quotas: &mut Vec<crate::provider_adapters::ProviderUsageQuota>,
    prefix: &str,
    value: &Value,
) {
    let rate_limit = rate_limit_payload(value);
    for (_window_name, keys, default_label) in [
        (
            "primary",
            &["primary_window", "primaryWindow", "primary"][..],
            "Session",
        ),
        (
            "secondary",
            &["secondary_window", "secondaryWindow", "secondary"][..],
            "Weekly",
        ),
    ] {
        if quotas.len() >= MAX_USAGE_QUOTAS {
            return;
        }
        let Some(window) = first_value(rate_limit, keys) else {
            continue;
        };
        let duration = first_value(
            window,
            &[
                "limit_window_seconds",
                "limitWindowSeconds",
                "window_seconds",
            ],
        )
        .and_then(as_number)
        .filter(|seconds| *seconds > 0.0 && *seconds <= i64::MAX as f64)
        .map(|seconds| seconds.trunc() as i64);
        let window_label = quota_window_name(default_label, duration);
        let id = quota_id(prefix, window_label);
        if quotas.iter().any(|quota| quota.id == id) {
            continue;
        }
        if let Some(quota) = parse_quota_window(window, id, quota_label(prefix, window_label)) {
            quotas.push(quota);
        }
    }
}

fn descriptor_matches(value: &Value, needle: &str) -> bool {
    [
        "limit_name",
        "limitName",
        "metered_feature",
        "meteredFeature",
        "limit_id",
        "limitId",
        "id",
        "name",
        "title",
        "model",
        "model_id",
        "modelId",
    ]
    .iter()
    .filter_map(|key| value.get(*key).and_then(Value::as_str))
    .any(|candidate| candidate.to_ascii_lowercase().contains(needle))
}

fn find_named_rate_limit<'a>(data: &'a Value, name: &str) -> Option<&'a Value> {
    let direct = match name {
        "code_review" => &["code_review_rate_limit", "review_rate_limit"][..],
        "spark" => &["spark_rate_limit", "gpt_5_3_codex_spark_rate_limit"][..],
        _ => &[][..],
    };
    if let Some(value) = first_value(data, direct) {
        return Some(value);
    }

    if let Some(by_limit_id) = data
        .get("rate_limits_by_limit_id")
        .and_then(Value::as_object)
    {
        let matched = by_limit_id.iter().find(|(key, _)| {
            let key = key.to_ascii_lowercase();
            if name == "code_review" {
                key.contains("review")
            } else {
                key.contains(name)
            }
        });
        if let Some((_, value)) = matched {
            return Some(value);
        }
    }

    data.get("additional_rate_limits")
        .and_then(Value::as_array)?
        .iter()
        .find(|entry| {
            descriptor_matches(
                entry,
                if name == "code_review" {
                    "review"
                } else {
                    name
                },
            )
        })
}

fn normalized_quota_prefix(value: &Value) -> Option<String> {
    let descriptor = first_value(
        value,
        &[
            "limit_name",
            "limitName",
            "metered_feature",
            "meteredFeature",
            "limit_id",
            "limitId",
            "id",
            "name",
        ],
    )
    .and_then(as_nonempty_string)?;
    let lower = descriptor.to_ascii_lowercase();
    if lower.contains("review") {
        return Some("code_review".to_owned());
    }
    if lower.contains("spark") {
        return Some("spark".to_owned());
    }
    let mut prefix = String::with_capacity(descriptor.len().min(48));
    let mut previous_separator = false;
    for character in lower.chars().take(48) {
        if character.is_ascii_alphanumeric() {
            prefix.push(character);
            previous_separator = false;
        } else if !previous_separator && !prefix.is_empty() {
            prefix.push('_');
            previous_separator = true;
        }
    }
    while prefix.ends_with('_') {
        prefix.pop();
    }
    (!prefix.is_empty()).then_some(prefix)
}

fn append_additional_rate_limits(
    quotas: &mut Vec<crate::provider_adapters::ProviderUsageQuota>,
    data: &Value,
) {
    if let Some(entries) = data.get("additional_rate_limits").and_then(Value::as_array) {
        for entry in entries {
            if quotas.len() >= MAX_USAGE_QUOTAS {
                return;
            }
            if let Some(prefix) = normalized_quota_prefix(entry) {
                append_rate_limit_quotas(quotas, &prefix, entry);
            }
        }
    }
    if let Some(entries) = data
        .get("rate_limits_by_limit_id")
        .and_then(Value::as_object)
    {
        for (key, value) in entries {
            if quotas.len() >= MAX_USAGE_QUOTAS {
                return;
            }
            if key.eq_ignore_ascii_case("codex") {
                continue;
            }
            let descriptor = json!({"id": key});
            if let Some(prefix) = normalized_quota_prefix(&descriptor) {
                append_rate_limit_quotas(quotas, &prefix, value);
            }
        }
    }
}

pub(crate) fn parse_usage_snapshot(
    data: &Value,
) -> crate::provider_adapters::ProviderUsageSnapshot {
    let rate_limit = first_value(data, &["rate_limit", "rate_limits"]).or_else(|| {
        data.get("rate_limits_by_limit_id")
            .and_then(|value| value.get("codex"))
    });
    let mut quotas = Vec::new();
    if let Some(rate_limit) = rate_limit {
        append_rate_limit_quotas(&mut quotas, "", rate_limit);
    }
    for prefix in ["code_review", "spark"] {
        if let Some(special) = find_named_rate_limit(data, prefix) {
            append_rate_limit_quotas(&mut quotas, prefix, special);
        }
    }
    append_additional_rate_limits(&mut quotas, data);

    let main_rate_limit = rate_limit.map(rate_limit_payload).unwrap_or(data);
    let limit_reached = first_value(main_rate_limit, &["limit_reached", "limitReached"])
        .is_some_and(|value| {
            value.as_bool().unwrap_or_else(|| {
                value
                    .as_str()
                    .is_some_and(|text| text.eq_ignore_ascii_case("true"))
            })
        });
    let plan = first_value(data, &["plan_type", "planType"])
        .and_then(as_nonempty_string)
        .or_else(|| {
            data.get("summary")
                .and_then(|summary| first_value(summary, &["plan", "plan_type"]))
                .and_then(as_nonempty_string)
        });
    let reset_credits_available = data
        .get("rate_limit_reset_credits")
        .and_then(|value| first_value(value, &["available_count", "availableCount"]))
        .and_then(as_number)
        .filter(|count| *count >= 0.0 && *count <= u32::MAX as f64)
        .map(|count| count.trunc() as u32);

    crate::provider_adapters::ProviderUsageSnapshot {
        plan,
        limit_reached,
        reset_credits_available,
        quotas,
        credit_balance: None,
    }
}
