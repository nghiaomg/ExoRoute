use super::aggregate::{AggregateKey, Counts, StatisticsEvent};
use crate::infra::storage::{Field, Record, StorageError, Table, validate_key};
use std::collections::HashMap;

pub(crate) fn add_aggregate(
    aggregates: &mut HashMap<AggregateKey, Counts>,
    minute: i64,
    dimension_kind: i64,
    dimension_id: &str,
    event: &StatisticsEvent,
) {
    let counts = aggregates
        .entry(AggregateKey {
            minute,
            dimension_kind,
            dimension_id: dimension_id.to_owned(),
        })
        .or_default();
    counts.add_event(event);
}

pub(crate) fn api_key_model_dimension_id(api_key_id: &str, model: &str) -> Option<String> {
    if api_key_id.is_empty()
        || api_key_id.len() > 128
        || api_key_id
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control() || byte == b'/')
    {
        return None;
    }
    let model = model.trim();
    if model.is_empty() || model.len() > 256 || model.bytes().any(|byte| byte == 0) {
        return None;
    }
    let dimension_id = format!("{api_key_id}/{model}");
    if dimension_id.len() > 400 {
        return None;
    }
    validate_key(&dimension_id).ok()?;
    Some(dimension_id)
}

pub(crate) fn minute_key(
    minute: i64,
    dimension_kind: i64,
    dimension_id: &str,
) -> Result<String, StorageError> {
    let key = format!("{minute:020}/{dimension_kind:02}/{dimension_id}");
    validate_key(&key)?;
    Ok(key)
}

pub(crate) fn window_key(
    window_minutes: i64,
    dimension_kind: i64,
    dimension_id: &str,
) -> Result<String, StorageError> {
    let key = format!("{window_minutes:05}/{dimension_kind:02}/{dimension_id}");
    validate_key(&key)?;
    Ok(key)
}

pub(crate) fn record_counts(record: &Record) -> Result<Counts, StorageError> {
    let value = |name: &str| -> Result<i64, StorageError> {
        match record.field(name) {
            Some(Field::I64(value)) if *value >= 0 => Ok(*value),
            Some(Field::I64(_)) => Err(StorageError::Invalid(format!(
                "statistics field '{name}' is negative"
            ))),
            None => Ok(0),
            _ => Err(StorageError::Invalid(format!(
                "statistics field '{name}' is malformed"
            ))),
        }
    };
    let input_tokens = value("input_tokens_total")?;
    let cache_input_tokens = value("cache_input_tokens_total")?.max(input_tokens);
    Ok(Counts {
        requests: value("request_count")?,
        succeeded: value("success_count")?,
        failed: value("failure_count")?,
        duration_ms: value("duration_ms_total")?,
        input_tokens,
        output_tokens: value("output_tokens_total")?,
        cached_tokens: value("cached_tokens_total")?.min(cache_input_tokens),
        cache_input_tokens,
    })
}

pub(crate) fn counts_record(counts: Counts) -> Record {
    Record::new()
        .with("request_count", Field::I64(counts.requests))
        .with("success_count", Field::I64(counts.succeeded))
        .with("failure_count", Field::I64(counts.failed))
        .with("duration_ms_total", Field::I64(counts.duration_ms))
        .with("input_tokens_total", Field::I64(counts.input_tokens))
        .with("output_tokens_total", Field::I64(counts.output_tokens))
        .with("cached_tokens_total", Field::I64(counts.cached_tokens))
        .with(
            "cache_input_tokens_total",
            Field::I64(counts.cache_input_tokens),
        )
}

pub(crate) fn cache_ratio_percent(cached_tokens: i64, cache_input_tokens: i64) -> f64 {
    if cache_input_tokens > 0 {
        cached_tokens.min(cache_input_tokens) as f64 / cache_input_tokens as f64 * 100.0
    } else {
        0.0
    }
}

pub(crate) fn add_counts(
    transaction: &mut crate::infra::storage::WriteTxn<'_, '_>,
    table: Table,
    key: &str,
    delta: Counts,
) -> Result<Counts, StorageError> {
    let current = transaction
        .get::<Record>(table, key)?
        .map(|record| record_counts(&record))
        .transpose()?
        .unwrap_or_default();
    let updated = current.add(delta);
    transaction.put(table, key, &counts_record(updated))?;
    Ok(updated)
}

pub(crate) fn top_index_key(
    window_minutes: i64,
    dimension_kind: i64,
    request_count: i64,
    dimension_id: &str,
) -> Result<String, StorageError> {
    let normalized = u64::try_from(request_count.max(0))
        .unwrap_or(i64::MAX as u64)
        .min(i64::MAX as u64);
    let inverse = (i64::MAX as u64).saturating_sub(normalized);
    let key = format!("top/{window_minutes:05}/{dimension_kind:02}/{inverse:019}/{dimension_id}");
    validate_key(&key)?;
    Ok(key)
}

pub(crate) fn update_window_counts(
    transaction: &mut crate::infra::storage::WriteTxn<'_, '_>,
    key: &str,
    window_minutes: i64,
    dimension_kind: i64,
    dimension_id: &str,
    delta: Counts,
) -> Result<(), StorageError> {
    let current_record = transaction.get::<Record>(Table::UsageWindows, key)?;
    let current = current_record
        .as_ref()
        .map(record_counts)
        .transpose()?
        .unwrap_or_default();
    if current.requests > 0 {
        let old_index = top_index_key(
            window_minutes,
            dimension_kind,
            current.requests,
            dimension_id,
        )?;
        transaction.delete(Table::StatisticsIndex, &old_index)?;
    }
    let updated = current.add(delta);
    transaction.put(Table::UsageWindows, key, &counts_record(updated))?;
    if updated.requests > 0 {
        let new_index = top_index_key(
            window_minutes,
            dimension_kind,
            updated.requests,
            dimension_id,
        )?;
        transaction.put(Table::StatisticsIndex, &new_index, &dimension_id.to_owned())?;
    }
    Ok(())
}
