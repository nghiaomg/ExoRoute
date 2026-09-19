use super::aggregate::ProviderUsageBucketCounts;
use super::{
    MAX_PROVIDER_METER_READ_ROWS, PROVIDER_METER_RETENTION_MINUTES, STATISTICS_TOP_PREFIX,
    Telemetry, cache_ratio_percent, provider_usage_counts, record_counts, unix_minute,
    valid_provider_credential_id, window_key,
};
use crate::infra::{
    db,
    storage::{Database, Record, StorageError, Table, validate_key},
};
use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MAX_MODEL_BREAKDOWN_SCAN_ROWS: usize = 100_000;
const MAX_MODEL_BREAKDOWN_ROWS: usize = 256;

/// Reads bounded rolling aggregates for one provider credential. This is
/// intentionally independent from request-history tables so the dashboard
/// never scans individual request logs to render spend.
pub(crate) async fn provider_usage_meter_snapshot(
    db: &Database,
    credential_id: &str,
) -> Result<serde_json::Value, StorageError> {
    if !valid_provider_credential_id(credential_id) {
        return Err(StorageError::Invalid(
            "provider usage meter credential ID is invalid".to_owned(),
        ));
    }
    let prefix = format!("{credential_id}/");
    validate_prefix_for_meter(&prefix)?;
    let current_minute = unix_minute();
    let cutoff = current_minute.saturating_sub(PROVIDER_METER_RETENTION_MINUTES);
    let rows = db
        .read(move |transaction| {
            transaction.scan_prefix::<Record>(
                Table::ProviderUsageMeters,
                &prefix,
                MAX_PROVIDER_METER_READ_ROWS.saturating_add(1),
            )
        })
        .await?;
    let partial_rows = rows.len() > MAX_PROVIDER_METER_READ_ROWS;
    let mut buckets = Vec::with_capacity(rows.len().min(MAX_PROVIDER_METER_READ_ROWS));
    for (_key, record) in rows.into_iter().take(MAX_PROVIDER_METER_READ_ROWS) {
        let minute = record.integer("minute")?;
        if minute < cutoff || minute > current_minute {
            continue;
        }
        buckets.push((minute, provider_usage_counts(record)?));
    }
    let dropped_events = db
        .read(|transaction| {
            Ok(transaction
                .get::<Record>(Table::StatisticsState, "singleton")?
                .map(|record| record.optional_integer("provider_usage_dropped_events"))
                .transpose()?
                .flatten()
                .unwrap_or(0))
        })
        .await?
        .max(0);
    let aggregate = |window_minutes: i64| {
        let threshold = current_minute.saturating_sub(window_minutes.saturating_sub(1));
        let mut counts = ProviderUsageBucketCounts::default();
        for (minute, bucket) in &buckets {
            if *minute >= threshold {
                counts.add_counts(bucket);
            }
        }
        let cost = (counts.missing_cost_events == 0 && counts.cost_events > 0)
            .then_some(counts.cost_micro_usd);
        serde_json::json!({
            "cost_micro_usd": cost,
            "requests": counts.requests,
            "input_tokens": counts.input_tokens,
            "output_tokens": counts.output_tokens,
            "cost_events": counts.cost_events,
            "missing_cost_events": counts.missing_cost_events,
        })
    };
    let latest_minute = buckets.iter().map(|(minute, _)| *minute).max();
    let has_data = !buckets.is_empty();
    let status = if !has_data {
        "unknown"
    } else if partial_rows
        || dropped_events > 0
        || buckets
            .iter()
            .any(|(_, bucket)| bucket.missing_cost_events > 0)
    {
        "partial"
    } else if latest_minute.is_some_and(|minute| current_minute.saturating_sub(minute) > 5) {
        "stale"
    } else {
        "fresh"
    };
    Ok(serde_json::json!({
        "source": "exoroute_local_meter",
        "status": status,
        "updated_at_ms": unix_time_millis(),
        "dropped_events": dropped_events,
        "windows": {
            "5h": aggregate(5 * 60),
            "7d": aggregate(7 * 24 * 60),
            "30d": aggregate(30 * 24 * 60),
        }
    }))
}

#[derive(Default)]
struct ModelBreakdownCounts {
    requests: i64,
    successes: i64,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    cache_input_tokens: i64,
}

fn model_breakdown_cutoff(window_minutes: i64) -> Result<String, StorageError> {
    let minutes = u64::try_from(window_minutes)
        .map_err(|_| StorageError::Invalid("statistics window is invalid".to_owned()))?;
    db::utc_timestamp_before(Duration::from_secs(minutes.saturating_mul(60)))
}

fn non_negative_log_value(record: &Record, name: &str) -> Result<i64, StorageError> {
    match record.optional_integer(name)? {
        Some(value) if value >= 0 => Ok(value),
        Some(_) => Err(StorageError::Invalid(format!(
            "request log field '{name}' is negative"
        ))),
        None => Ok(0),
    }
}

fn request_log_created_at(index_key: &str) -> Result<&str, StorageError> {
    index_key
        .strip_prefix(db::REQUEST_LOG_INDEX_PREFIX)
        .and_then(|suffix| suffix.split_once('/'))
        .map(|(created_at, _)| created_at)
        .ok_or_else(|| StorageError::Invalid("request log time index is malformed".to_owned()))
}

fn model_breakdown(
    transaction: &crate::infra::storage::ReadTxn<'_, '_>,
    cutoff: &str,
    api_key_id: Option<&str>,
) -> Result<(Vec<serde_json::Value>, bool), StorageError> {
    let mut groups = BTreeMap::<(String, Option<String>), ModelBreakdownCounts>::new();
    let mut after = None::<String>;
    let mut scanned = 0usize;
    let mut reached_cutoff = false;

    while scanned < MAX_MODEL_BREAKDOWN_SCAN_ROWS && !reached_cutoff {
        let batch_limit = (MAX_MODEL_BREAKDOWN_SCAN_ROWS - scanned).min(256);
        let page = transaction.scan_prefix_reverse_after::<String>(
            Table::RequestLogIndex,
            db::REQUEST_LOG_INDEX_PREFIX,
            after.as_deref(),
            batch_limit,
        )?;
        if page.is_empty() {
            break;
        }
        for (index_key, id) in page {
            scanned = scanned.saturating_add(1);
            after = Some(index_key.clone());
            if request_log_created_at(&index_key)? < cutoff {
                reached_cutoff = true;
                break;
            }
            let record = transaction
                .get::<Record>(Table::RequestLogs, &id)?
                .ok_or_else(|| {
                    StorageError::Invalid("request log index points to a missing record".to_owned())
                })?;
            if let Some(expected) = api_key_id
                && record.optional_text("api_key_id")? != Some(expected)
            {
                continue;
            }
            let model = record.text("model")?.to_owned();
            if model.is_empty() {
                continue;
            }
            let route_alias = record.text("route_alias")?;
            let combo = (route_alias != model).then(|| route_alias.to_owned());
            let input_tokens = non_negative_log_value(&record, "input_tokens")?;
            let output_tokens = non_negative_log_value(&record, "output_tokens")?;
            let cache_input_tokens =
                non_negative_log_value(&record, "cache_input_tokens")?.max(input_tokens);
            let cached_tokens =
                non_negative_log_value(&record, "cached_tokens")?.min(cache_input_tokens);
            let counts = groups.entry((model, combo)).or_default();
            counts.requests = counts.requests.saturating_add(1);
            if (200..300).contains(&record.integer("status")?) {
                counts.successes = counts.successes.saturating_add(1);
            }
            counts.input_tokens = counts.input_tokens.saturating_add(input_tokens);
            counts.output_tokens = counts.output_tokens.saturating_add(output_tokens);
            counts.cached_tokens = counts.cached_tokens.saturating_add(cached_tokens);
            counts.cache_input_tokens =
                counts.cache_input_tokens.saturating_add(cache_input_tokens);
        }
    }

    let scan_truncated = !reached_cutoff && scanned >= MAX_MODEL_BREAKDOWN_SCAN_ROWS;
    let mut rows = groups
        .into_iter()
        .map(|((model, combo), counts)| {
            let success_rate = if counts.requests > 0 {
                counts.successes as f64 / counts.requests as f64 * 100.0
            } else {
                0.0
            };
            serde_json::json!({
                "model": model,
                "combo": combo,
                "input_tokens": counts.input_tokens,
                "output_tokens": counts.output_tokens,
                "cache_hit_rate": cache_ratio_percent(counts.cached_tokens, counts.cache_input_tokens),
                "requests": counts.requests,
                "successes": counts.successes,
                "success_rate": success_rate,
            })
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        right["requests"]
            .as_i64()
            .cmp(&left["requests"].as_i64())
            .then_with(|| left["model"].as_str().cmp(&right["model"].as_str()))
    });
    let output_truncated = rows.len() > MAX_MODEL_BREAKDOWN_ROWS;
    rows.truncate(MAX_MODEL_BREAKDOWN_ROWS);
    Ok((rows, scan_truncated || output_truncated))
}

pub(crate) fn validate_prefix_for_meter(prefix: &str) -> Result<(), StorageError> {
    validate_key(prefix.trim_end_matches('/'))
}

pub(crate) fn unix_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

pub(crate) fn parse_minute_key(key: &str) -> Result<(i64, i64, String), StorageError> {
    let mut parts = key.splitn(3, '/');
    let minute = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| StorageError::Invalid("statistics minute key is malformed".to_owned()))?;
    let kind = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| StorageError::Invalid("statistics dimension key is malformed".to_owned()))?;
    let id = parts
        .next()
        .ok_or_else(|| StorageError::Invalid("statistics dimension id is missing".to_owned()))?;
    if minute < 0 || !(0..=3).contains(&kind) {
        return Err(StorageError::Invalid(
            "statistics minute key is outside its valid range".to_owned(),
        ));
    }
    Ok((minute, kind, id.to_owned()))
}

pub(crate) async fn statistics_snapshot(
    db: &Database,
    range: &'static str,
    window_minutes: i64,
    telemetry: &Telemetry,
) -> Result<serde_json::Value, StorageError> {
    let cutoff = model_breakdown_cutoff(window_minutes)?;
    let _drop_state_guard = telemetry.drop_state_lock.lock().await;
    let unpersisted_drops = telemetry.drop_counters().unpersisted_total();
    db.read(move |transaction| {
        let summary_key = window_key(window_minutes, 0, "")?;
        let summary = transaction
            .get::<Record>(Table::UsageWindows, &summary_key)?
            .map(|record| record_counts(&record))
            .transpose()?
            .unwrap_or_default();
        let (api_keys, api_key_sum) = top_dimensions(transaction, window_minutes, 1, summary.requests)?;
        let (models, model_sum) = top_dimensions(transaction, window_minutes, 2, summary.requests)?;
        let (model_breakdown, model_breakdown_truncated) =
            model_breakdown(transaction, &cutoff, None)?;
        let state = transaction.get::<Record>(Table::StatisticsState, "singleton")?;
        let (collection_started_at, persisted_drops, updated_at) = match state {
            Some(record) => (
                record.optional_text("collection_started_at")?.map(str::to_owned),
                record.integer("dropped_events")?,
                record.text("updated_at")?.to_owned(),
            ),
            None => (None, 0, String::new()),
        };
        if persisted_drops < 0 {
            return Err(StorageError::Invalid("persisted telemetry drop count is negative".to_owned()));
        }
        let target_minute = unix_minute().saturating_sub(window_minutes);
        let expiry_cursor = transaction.get::<Record>(Table::UsageExpiryCursor, &window_minutes.to_string())?;
        let aggregation_current = match expiry_cursor {
            Some(record) => {
                let minute = record.integer("cursor_minute")?;
                let kind = record.integer("cursor_dimension_kind")?;
                minute > target_minute || (minute == target_minute && kind == 4)
            }
            None => true,
        };
        let dropped_events = u64::try_from(persisted_drops)
            .unwrap_or(u64::MAX)
            .saturating_add(unpersisted_drops);
        Ok(serde_json::json!({
            "range": range,
            "total_requests": summary.requests,
            "successes": summary.succeeded,
            "failures": summary.failed,
            "success_rate": if summary.requests > 0 { (summary.succeeded as f64 / summary.requests as f64) * 100.0 } else { 0.0 },
            "average_latency_ms": if summary.requests > 0 { summary.duration_ms as f64 / summary.requests as f64 } else { 0.0 },
            "input_tokens": summary.input_tokens,
            "output_tokens": summary.output_tokens,
            "cached_tokens": summary.cached_tokens,
            "cache_ratio": cache_ratio_percent(summary.cached_tokens, summary.cache_input_tokens),
            "as_of": updated_at,
            "collection_started_at": collection_started_at,
            "complete": dropped_events == 0,
            "dropped_events": dropped_events,
            "aggregation_current": aggregation_current,
            "api_keys": { "top": api_keys, "others": summary.requests.saturating_sub(api_key_sum) },
            "models": { "top": models, "others": summary.requests.saturating_sub(model_sum) },
            "model_breakdown": model_breakdown,
            "model_breakdown_truncated": model_breakdown_truncated
        }))
    }).await
}

pub(crate) async fn api_key_statistics_snapshot(
    db: &Database,
    api_key_id: &str,
    range: &'static str,
    window_minutes: i64,
    telemetry: &Telemetry,
) -> Result<serde_json::Value, StorageError> {
    validate_api_key_dimension_id(api_key_id)?;
    let dimension_id = api_key_id.to_owned();
    let cutoff = model_breakdown_cutoff(window_minutes)?;
    let _drop_state_guard = telemetry.drop_state_lock.lock().await;
    let unpersisted_drops = telemetry.drop_counters().unpersisted_total();
    db.read(move |transaction| {
        let summary = transaction
            .get::<Record>(
                Table::UsageWindows,
                &window_key(window_minutes, 1, &dimension_id)?,
            )?
            .map(|record| record_counts(&record))
            .transpose()?
            .unwrap_or_default();
        let (models, model_sum) =
            top_api_key_models(transaction, window_minutes, &dimension_id, summary.requests)?;
        let (model_breakdown, model_breakdown_truncated) =
            model_breakdown(transaction, &cutoff, Some(&dimension_id))?;
        let api_key_name = transaction
            .get::<Record>(Table::ApiKeys, &dimension_id)?
            .map(|record| record.text("name").map(str::to_owned))
            .transpose()?
            .unwrap_or_else(|| dimension_id.clone());
        let state = transaction.get::<Record>(Table::StatisticsState, "singleton")?;
        let (collection_started_at, persisted_drops, updated_at) = match state {
            Some(record) => (
                record.optional_text("collection_started_at")?.map(str::to_owned),
                record.integer("dropped_events")?,
                record.text("updated_at")?.to_owned(),
            ),
            None => (None, 0, String::new()),
        };
        if persisted_drops < 0 {
            return Err(StorageError::Invalid(
                "persisted telemetry drop count is negative".to_owned(),
            ));
        }
        let target_minute = unix_minute().saturating_sub(window_minutes);
        let aggregation_current = match transaction
            .get::<Record>(Table::UsageExpiryCursor, &window_minutes.to_string())?
        {
            Some(record) => {
                let minute = record.integer("cursor_minute")?;
                let kind = record.integer("cursor_dimension_kind")?;
                minute > target_minute || (minute == target_minute && kind == 4)
            }
            None => true,
        };
        let dropped_events = u64::try_from(persisted_drops)
            .unwrap_or(u64::MAX)
            .saturating_add(unpersisted_drops);
        Ok(serde_json::json!({
            "api_key_id": dimension_id,
            "api_key_name": api_key_name,
            "range": range,
            "total_requests": summary.requests,
            "successes": summary.succeeded,
            "failures": summary.failed,
            "success_rate": if summary.requests > 0 { (summary.succeeded as f64 / summary.requests as f64) * 100.0 } else { 0.0 },
            "average_latency_ms": if summary.requests > 0 { summary.duration_ms as f64 / summary.requests as f64 } else { 0.0 },
            "input_tokens": summary.input_tokens,
            "output_tokens": summary.output_tokens,
            "cached_tokens": summary.cached_tokens,
            "cache_ratio": cache_ratio_percent(summary.cached_tokens, summary.cache_input_tokens),
            "as_of": updated_at,
            "collection_started_at": collection_started_at,
            "complete": dropped_events == 0,
            "dropped_events": dropped_events,
            "aggregation_current": aggregation_current,
            "models": { "top": models, "others": summary.requests.saturating_sub(model_sum) },
            "model_breakdown": model_breakdown,
            "model_breakdown_truncated": model_breakdown_truncated
        }))
    })
    .await
}

fn validate_api_key_dimension_id(api_key_id: &str) -> Result<(), StorageError> {
    if api_key_id.is_empty()
        || api_key_id.len() > 128
        || api_key_id
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control() || byte == b'/')
    {
        return Err(StorageError::Invalid(
            "API key ID is invalid or too long".to_owned(),
        ));
    }
    validate_key(api_key_id)
}

fn top_api_key_models(
    transaction: &crate::infra::storage::ReadTxn<'_, '_>,
    window_minutes: i64,
    api_key_id: &str,
    total_requests: i64,
) -> Result<(Vec<serde_json::Value>, i64), StorageError> {
    let prefix = format!("{window_minutes:05}/03/{api_key_id}/");
    let rows = transaction.scan_prefix::<Record>(Table::UsageWindows, &prefix, 512)?;
    let mut models = Vec::new();
    for (key, record) in rows {
        let Some(model) = key.strip_prefix(&prefix).filter(|model| !model.is_empty()) else {
            continue;
        };
        let counts = record_counts(&record)?;
        if counts.requests <= 0 {
            continue;
        }
        models.push((
            counts.requests,
            serde_json::json!({
                "id": model,
                "label": model,
                "requests": counts.requests,
                "percentage": if total_requests > 0 { counts.requests as f64 / total_requests as f64 * 100.0 } else { 0.0 }
            }),
        ));
    }
    models.sort_by_key(|model| std::cmp::Reverse(model.0));
    models.truncate(5);
    let total = models.iter().map(|(requests, _)| *requests).sum();
    let output = models.into_iter().map(|(_, value)| value).collect();
    Ok((output, total))
}

pub(crate) fn top_dimensions(
    transaction: &crate::infra::storage::ReadTxn<'_, '_>,
    window_minutes: i64,
    dimension_kind: i64,
    total_requests: i64,
) -> Result<(Vec<serde_json::Value>, i64), StorageError> {
    let prefix = format!("{STATISTICS_TOP_PREFIX}{window_minutes:05}/{dimension_kind:02}/");
    let rows = transaction.scan_prefix::<String>(Table::StatisticsIndex, &prefix, 5)?;
    let mut output = Vec::with_capacity(rows.len());
    let mut total = 0_i64;
    for (_index, dimension_id) in rows {
        let key = window_key(window_minutes, dimension_kind, &dimension_id)?;
        let record = transaction
            .get::<Record>(Table::UsageWindows, &key)?
            .ok_or_else(|| StorageError::Invalid("statistics top index is stale".to_owned()))?;
        let counts = record_counts(&record)?;
        let label = if dimension_kind == 1 {
            match transaction.get::<Record>(Table::ApiKeys, &dimension_id)? {
                Some(key) => key
                    .optional_text("name")?
                    .map(str::to_owned)
                    .unwrap_or_else(|| dimension_id.clone()),
                None => dimension_id.clone(),
            }
        } else {
            dimension_id.clone()
        };
        total = total.saturating_add(counts.requests);
        output.push(serde_json::json!({
                "id": dimension_id,
                "label": label,
                "requests": counts.requests,
                "percentage": if total_requests > 0 { counts.requests as f64 / total_requests as f64 * 100.0 } else { 0.0 }
            }));
    }
    Ok((output, total))
}
