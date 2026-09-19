use super::{
    MAX_QUOTA_ROWS, MAX_WEEKLY_ROWS, as_number, bounded_text, extract_reset_at, is_supported_model,
    model_id,
};
use crate::provider_adapters::{ProviderUsageQuota, ProviderUsageResetPeriod};
use serde_json::Value;
use std::collections::HashSet;

const FIVE_HOUR_WINDOW_SECONDS: i64 = 5 * 60 * 60;
const WEEKLY_WINDOW_SECONDS: i64 = 7 * 24 * 60 * 60;

pub(super) fn parse_model_quotas(
    available: Option<&Value>,
    quota: Option<&Value>,
) -> Vec<ProviderUsageQuota> {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    if let Some(value) = quota {
        append_bucket_quotas(&mut rows, &mut seen, value);
    }
    if let Some(value) = available {
        append_bucket_quotas(&mut rows, &mut seen, value);
    }
    rows
}

fn append_bucket_quotas(
    output: &mut Vec<ProviderUsageQuota>,
    seen: &mut HashSet<String>,
    value: &Value,
) {
    if let Some(buckets) = value.get("buckets").and_then(Value::as_array) {
        for bucket in buckets.iter().take(MAX_QUOTA_ROWS) {
            append_quota_entry(output, seen, bucket, None);
        }
    }
    let Some(container) = value.get("models").or_else(|| value.get("data")) else {
        return;
    };
    if let Some(models) = container.as_object() {
        for (id, model) in models.iter().take(MAX_QUOTA_ROWS) {
            append_quota_entry(output, seen, model, Some(id));
        }
    } else if let Some(models) = container.as_array() {
        for model in models.iter().take(MAX_QUOTA_ROWS) {
            append_quota_entry(output, seen, model, None);
        }
    }
}

fn append_quota_entry(
    output: &mut Vec<ProviderUsageQuota>,
    seen: &mut HashSet<String>,
    bucket: &Value,
    fallback_id: Option<&str>,
) {
    let Some(id) = model_id(bucket, fallback_id).filter(|id| is_supported_model(id)) else {
        return;
    };
    let quota_info = bucket.get("quotaInfo").unwrap_or(bucket);
    let Some(fraction) = quota_info
        .get("remainingFraction")
        .or_else(|| quota_info.get("remaining_fraction"))
        .and_then(as_number)
    else {
        return;
    };
    let fraction = fraction.clamp(0.0, 1.0);
    if seen.contains(&id) {
        return;
    }
    seen.insert(id.clone());
    let reset_at = extract_reset_at(quota_info);
    let label = bucket
        .get("displayName")
        .or_else(|| bucket.get("display_name"))
        .and_then(Value::as_str)
        .filter(|label| !label.trim().is_empty())
        .unwrap_or(&id)
        .chars()
        .take(256)
        .collect();
    output.push(ProviderUsageQuota {
        id,
        label,
        used_percent: (1.0 - fraction) * 100.0,
        remaining_percent: fraction * 100.0,
        used_amount: Some((1.0 - fraction) * 1000.0),
        limit_amount: Some(1000.0),
        unit: Some("normalized upstream units".to_owned()),
        reset_at,
        window_seconds: Some(FIVE_HOUR_WINDOW_SECONDS),
        uncapped: fraction >= 1.0 && reset_at.is_none(),
        reset_period: None,
    });
}

pub(super) fn summarize_antigravity_quotas(
    model_quotas: &[ProviderUsageQuota],
    weekly_quotas: &[ProviderUsageQuota],
    tier: Option<&str>,
) -> Vec<ProviderUsageQuota> {
    let mut output = Vec::with_capacity(4);

    // Antigravity's free tier exposes model-shaped rows that are not a
    // reliable five-hour window. Keep those rows out of the dashboard
    // aggregates, matching the paid-tier guard used by 9router.
    if has_paid_model_quota(tier)
        && let Some(quota) = aggregate_quota(
            model_quotas,
            |_| true,
            "five_hour",
            "5h",
            FIVE_HOUR_WINDOW_SECONDS,
            None,
        )
    {
        output.push(quota);
    }

    if let Some(quota) = aggregate_quota(
        weekly_quotas,
        |_| true,
        "weekly",
        "7d",
        WEEKLY_WINDOW_SECONDS,
        Some(ProviderUsageResetPeriod::Weekly),
    ) {
        output.push(quota);
    }

    if has_paid_model_quota(tier)
        && let Some(quota) = aggregate_quota(
            model_quotas,
            |quota| is_model_family(&quota.id, ModelFamily::Gpt),
            "gpt",
            "GPT",
            FIVE_HOUR_WINDOW_SECONDS,
            None,
        )
    {
        output.push(quota);
    }
    if has_paid_model_quota(tier)
        && let Some(quota) = aggregate_quota(
            model_quotas,
            |quota| is_model_family(&quota.id, ModelFamily::Claude),
            "claude",
            "Claude",
            FIVE_HOUR_WINDOW_SECONDS,
            None,
        )
    {
        output.push(quota);
    }

    output
}

#[derive(Clone, Copy)]
enum ModelFamily {
    Gpt,
    Claude,
}

fn has_paid_model_quota(tier: Option<&str>) -> bool {
    tier.is_some_and(|value| {
        let value = value.trim();
        !value.is_empty()
            && !value.eq_ignore_ascii_case("free-tier")
            && !value.eq_ignore_ascii_case("legacy-tier")
            && !value.eq_ignore_ascii_case("unknown")
    })
}

fn is_model_family(id: &str, family: ModelFamily) -> bool {
    let id = id.to_ascii_lowercase();
    match family {
        ModelFamily::Gpt => {
            id.starts_with("gpt-") || id.starts_with("gpt_") || id.starts_with("openai-")
        }
        ModelFamily::Claude => id.starts_with("claude-") || id.starts_with("cloud-"),
    }
}

fn aggregate_quota<F>(
    quotas: &[ProviderUsageQuota],
    matches: F,
    id: &str,
    label: &str,
    window_seconds: i64,
    reset_period: Option<ProviderUsageResetPeriod>,
) -> Option<ProviderUsageQuota>
where
    F: Fn(&ProviderUsageQuota) -> bool,
{
    let mut selected = None;
    let mut all_uncapped = true;
    for quota in quotas.iter().filter(|quota| matches(quota)) {
        all_uncapped &= quota.uncapped;
        if selected.is_none_or(|current: &ProviderUsageQuota| {
            quota.remaining_percent < current.remaining_percent
                || (quota.remaining_percent == current.remaining_percent
                    && quota.reset_at.unwrap_or(i64::MAX) < current.reset_at.unwrap_or(i64::MAX))
        }) {
            selected = Some(quota);
        }
    }
    let selected = selected?;
    let remaining_percent = selected.remaining_percent.clamp(0.0, 100.0);
    Some(ProviderUsageQuota {
        id: id.to_owned(),
        label: label.to_owned(),
        used_percent: 100.0 - remaining_percent,
        remaining_percent,
        used_amount: Some((100.0 - remaining_percent) * 10.0),
        limit_amount: Some(1000.0),
        unit: Some("normalized upstream units".to_owned()),
        reset_at: selected.reset_at,
        window_seconds: Some(window_seconds),
        uncapped: all_uncapped,
        reset_period,
    })
}

fn merge_lower_quota(output: &mut Vec<ProviderUsageQuota>, quota: ProviderUsageQuota) {
    if let Some(existing) = output.iter_mut().find(|existing| existing.id == quota.id) {
        let should_replace = quota.remaining_percent < existing.remaining_percent
            || (quota.remaining_percent == existing.remaining_percent
                && quota.reset_at.unwrap_or(i64::MAX) < existing.reset_at.unwrap_or(i64::MAX));
        if should_replace {
            *existing = quota;
        }
    } else {
        output.push(quota);
    }
}

pub(super) fn append_weekly_quotas(output: &mut Vec<ProviderUsageQuota>, summary: Option<&Value>) {
    let Some(summary) = summary else { return };
    let groups = summary
        .get("groups")
        .or_else(|| {
            summary
                .get("quotaSummary")
                .and_then(|value| value.get("groups"))
        })
        .and_then(Value::as_array);
    let Some(groups) = groups else { return };
    for group in groups.iter().take(MAX_WEEKLY_ROWS) {
        let group_label = group
            .get("displayName")
            .or_else(|| group.get("display_name"))
            .or_else(|| group.get("bucketId"))
            .or_else(|| group.get("bucket_id"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
            continue;
        };
        for bucket in buckets.iter().take(MAX_WEEKLY_ROWS) {
            if group.get("disabled").and_then(Value::as_bool) == Some(true)
                || bucket.get("disabled").and_then(Value::as_bool) == Some(true)
            {
                continue;
            }
            let bucket_label = bucket
                .get("displayName")
                .or_else(|| bucket.get("display_name"))
                .or_else(|| bucket.get("bucketId"))
                .or_else(|| bucket.get("bucket_id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let combined_label = format!("{group_label} {bucket_label}");
            if !combined_label.to_ascii_lowercase().contains("weekly") {
                continue;
            }
            let Some(fraction) = bucket
                .get("remainingFraction")
                .or_else(|| bucket.get("remaining_fraction"))
                .and_then(as_number)
            else {
                continue;
            };
            let fraction = fraction.clamp(0.0, 1.0);
            let id = if combined_label.to_ascii_lowercase().contains("gemini") {
                "gemini_weekly"
            } else if combined_label.to_ascii_lowercase().contains("claude")
                || combined_label.to_ascii_lowercase().contains("gpt")
            {
                "claude_gpt_weekly"
            } else {
                continue;
            };
            let reset_at = extract_reset_at(bucket).or_else(|| extract_reset_at(group));
            merge_lower_quota(
                output,
                ProviderUsageQuota {
                    id: id.to_owned(),
                    label: bounded_text(bucket_label),
                    used_percent: (1.0 - fraction) * 100.0,
                    remaining_percent: fraction * 100.0,
                    used_amount: Some((1.0 - fraction) * 1000.0),
                    limit_amount: Some(1000.0),
                    unit: Some("normalized upstream units".to_owned()),
                    reset_at,
                    window_seconds: Some(WEEKLY_WINDOW_SECONDS),
                    uncapped: fraction >= 1.0 && reset_at.is_none(),
                    reset_period: Some(ProviderUsageResetPeriod::Weekly),
                },
            );
        }
    }
}
