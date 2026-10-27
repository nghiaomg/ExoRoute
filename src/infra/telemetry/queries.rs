use super::aggregate::{Counts, ProviderUsageBucketCounts};
use super::history::{HistoryScan, SeriesSpec, history_cutoff, scan_history, series_spec};
use super::{
    MAX_PROVIDER_METER_READ_ROWS, PROVIDER_METER_RETENTION_MINUTES, STATISTICS_TOP_PREFIX,
    Telemetry, cache_ratio_percent, provider_usage_counts, record_counts, unix_minute,
    valid_provider_credential_id, window_key,
};
use crate::infra::storage::{Database, Record, StorageError, Table, validate_key};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Builds the history-derived part of a statistics payload.
///
/// The grouping is by the provider that actually served a request, not by the
/// combo/route alias the client called: the same model routed through the same
/// alias still has to be attributable to one upstream, and a request that failed
/// before a provider was selected is grouped on its own.
///
/// `logged_requests` lets the caller compare these rows with the rollup totals.
/// Request history is bounded (retention days, retained rows, and the scan
/// limit) while the rolling totals are not, so the two can legitimately describe
/// different amounts of traffic. One client request can produce several attempt
/// logs, so the covered count is normally not smaller than the request count; a
/// smaller value means requests are missing from the history.
fn history_fields(
    scan: HistoryScan,
    series: SeriesSpec,
    summary: &Counts,
) -> serde_json::Map<String, serde_json::Value> {
    let ttft_samples = summary.first_output_events.max(0);
    let mut fields = serde_json::Map::new();
    fields.insert("model_breakdown".to_owned(), serde_json::json!(scan.models));
    fields.insert(
        "model_breakdown_truncated".to_owned(),
        serde_json::json!(scan.models_truncated),
    );
    fields.insert(
        "logged_requests".to_owned(),
        serde_json::json!(scan.logged_requests),
    );
    fields.insert(
        "model_breakdown_incomplete".to_owned(),
        serde_json::json!(scan.logged_requests < summary.requests),
    );
    fields.insert(
        "providers".to_owned(),
        serde_json::json!({ "rows": scan.providers, "truncated": scan.providers_truncated }),
    );
    fields.insert(
        "statuses".to_owned(),
        serde_json::json!({ "rows": scan.statuses, "truncated": scan.statuses_truncated }),
    );
    fields.insert(
        "series".to_owned(),
        serde_json::json!({
            "points": scan.series,
            "bucket_seconds": series.bucket_seconds,
            "truncated": scan.scan_truncated,
        }),
    );
    fields.insert("latency".to_owned(), scan.latency);
    fields.insert("cost".to_owned(), scan.cost);
    fields.insert(
        "history_truncated".to_owned(),
        serde_json::json!(scan.scan_truncated),
    );
    fields.insert(
        "average_time_to_first_token_ms".to_owned(),
        serde_json::json!(
            (ttft_samples > 0).then(|| summary.first_output_ms as f64 / ttft_samples as f64)
        ),
    );
    fields.insert(
        "time_to_first_token_samples".to_owned(),
        serde_json::json!(ttft_samples),
    );
    fields
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

pub(crate) use super::windows::parse_minute_key;

pub(crate) async fn statistics_snapshot(
    db: &Database,
    range: &'static str,
    window_minutes: i64,
    telemetry: &Telemetry,
) -> Result<serde_json::Value, StorageError> {
    let cutoff = history_cutoff(window_minutes);
    let series = series_spec(window_minutes);
    // Held deliberately across the database read below. The persistence path
    // (`persist_drop_delta`) and the history clear take this same lock, so a
    // report can never pair a pending in-memory drop delta with persisted totals
    // that already include it. Drop counts are therefore consistent rather than
    // approximately consistent; the cost is that statistics requests serialize
    // against each other and against the rollup writer, bounded by one scan and
    // one write because both run on the blocking pool.
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
        let scan = scan_history(transaction, cutoff, None, series)?;
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
        let mut payload = serde_json::json!({
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
        });
        if let Some(fields) = payload.as_object_mut() {
            fields.extend(history_fields(scan, series, &summary));
        }
        Ok(payload)
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
    let cutoff = history_cutoff(window_minutes);
    let series = series_spec(window_minutes);
    // Held deliberately across the database read below. The persistence path
    // (`persist_drop_delta`) and the history clear take this same lock, so a
    // report can never pair a pending in-memory drop delta with persisted totals
    // that already include it. Drop counts are therefore consistent rather than
    // approximately consistent; the cost is that statistics requests serialize
    // against each other and against the rollup writer, bounded by one scan and
    // one write because both run on the blocking pool.
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
        let scan = scan_history(transaction, cutoff, Some(&dimension_id), series)?;
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
        let mut payload = serde_json::json!({
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
        });
        if let Some(fields) = payload.as_object_mut() {
            fields.extend(history_fields(scan, series, &summary));
        }
        Ok(payload)
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
