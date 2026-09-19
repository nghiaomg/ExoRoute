use super::*;

pub(super) async fn persist_batch(
    db: &Database,
    batch: &[TelemetryMessage],
) -> Result<(), StorageError> {
    let now = db::utc_timestamp_now()?;
    let mut logs = Vec::new();
    let mut api_key_counts = HashMap::<String, i64>::new();
    let mut aggregates = HashMap::<AggregateKey, Counts>::new();
    let mut provider_usage = HashMap::<ProviderUsageBucketKey, ProviderUsageBucketCounts>::new();
    for message in batch {
        match message {
            TelemetryMessage::RequestLog(record) => {
                let stored = request_log_record(record, &now)?;
                let index = db::request_log_index_key(&now, &record.id)?;
                let descending_index = db::request_log_desc_index_key(&now, &record.id)?;
                logs.push((record.id.clone(), stored, index, descending_index));
            }
            TelemetryMessage::ApiKeyUsage(api_key_id) => {
                let count = api_key_counts.entry(api_key_id.clone()).or_default();
                *count = count.saturating_add(1);
            }
            TelemetryMessage::ProviderUsage(event) => {
                provider_usage
                    .entry(ProviderUsageBucketKey {
                        credential_id: event.credential_id.clone(),
                        minute: event.minute,
                    })
                    .or_default()
                    .add(event);
            }
            TelemetryMessage::Statistics(event) => {
                add_aggregate(&mut aggregates, event.minute, 0, "", event);
                add_aggregate(&mut aggregates, event.minute, 1, &event.api_key_id, event);
                if let Some(model_dimension) = crate::infra::telemetry::api_key_model_dimension_id(
                    &event.api_key_id,
                    &event.requested_model,
                ) {
                    add_aggregate(&mut aggregates, event.minute, 3, &model_dimension, event);
                }
                add_aggregate(
                    &mut aggregates,
                    event.minute,
                    2,
                    &event.requested_model,
                    event,
                );
            }
            TelemetryMessage::FlushHistory(_) | TelemetryMessage::ClearHistory(_) => {}
        }
    }
    if logs.is_empty()
        && api_key_counts.is_empty()
        && aggregates.is_empty()
        && provider_usage.is_empty()
    {
        return Ok(());
    }

    db.write(move |transaction| {
        for (id, record, index_key, descending_index) in &logs {
            transaction.put_if_absent(Table::RequestLogs, id, record)?;
            transaction.put_if_absent(Table::RequestLogIndex, index_key, id)?;
            transaction.put_if_absent(Table::RequestLogIndex, descending_index, id)?;
        }
        if !logs.is_empty() {
            let current = transaction
                .get::<i64>(Table::Meta, "request_log_count")?
                .ok_or_else(|| {
                    StorageError::Invalid("request log counter is missing".to_owned())
                })?;
            let added = i64::try_from(logs.len()).unwrap_or(i64::MAX);
            transaction.put(
                Table::Meta,
                "request_log_count",
                &current.saturating_add(added),
            )?;
        }

        let generation = transaction
            .get::<i64>(Table::Meta, "statistics_generation")?
            .unwrap_or(0);
        for (api_key_id, count) in &api_key_counts {
            let Some(mut record) = transaction.get::<Record>(Table::ApiKeys, api_key_id)? else {
                continue;
            };
            let current_generation = match record.field("request_count_generation") {
                Some(Field::I64(value)) => *value,
                None => generation,
                _ => {
                    return Err(StorageError::Invalid(
                        "API key usage generation is invalid".to_owned(),
                    ));
                }
            };
            let current_count = if current_generation == generation {
                match record.field("request_count") {
                    Some(Field::I64(value)) if *value >= 0 => *value,
                    Some(Field::I64(_)) => {
                        return Err(StorageError::Invalid(
                            "API key request count is negative".to_owned(),
                        ));
                    }
                    None => 0,
                    _ => {
                        return Err(StorageError::Invalid(
                            "API key request count is invalid".to_owned(),
                        ));
                    }
                }
            } else {
                0
            };
            record.insert(
                "request_count",
                Field::I64(current_count.saturating_add(*count)),
            );
            record.insert("request_count_generation", Field::I64(generation));
            transaction.put(Table::ApiKeys, api_key_id, &record)?;
        }

        for (key, counts) in &aggregates {
            let minute_key = minute_key(key.minute, key.dimension_kind, &key.dimension_id)?;
            add_counts(transaction, Table::UsageMinutes, &minute_key, *counts)?;
            for (window_minutes, _) in STATISTICS_RANGES {
                let window_key = window_key(window_minutes, key.dimension_kind, &key.dimension_id)?;
                update_window_counts(
                    transaction,
                    &window_key,
                    window_minutes,
                    key.dimension_kind,
                    &key.dimension_id,
                    *counts,
                )?;
            }
        }
        for (key, delta) in &provider_usage {
            // A credential can be deleted while usage events are still queued.
            // Check the primary record in the same LMDB write transaction so a
            // late telemetry batch cannot recreate a meter that deletion just
            // removed. This also keeps the meter namespace bounded by live
            // credentials without imposing a key-count cap.
            if transaction
                .get::<Record>(Table::ProviderApiKeys, &key.credential_id)?
                .is_none()
            {
                continue;
            }
            let meter_key = provider_usage_key(&key.credential_id, key.minute)?;
            let current = transaction
                .get::<Record>(Table::ProviderUsageMeters, &meter_key)?
                .map(provider_usage_counts)
                .transpose()?
                .unwrap_or_default();
            let mut updated = current;
            updated.add_counts(delta);
            transaction.put(
                Table::ProviderUsageMeters,
                &meter_key,
                &provider_usage_record(&key.credential_id, key.minute, updated),
            )?;
        }
        if !aggregates.is_empty() {
            let mut state = transaction
                .get::<Record>(Table::StatisticsState, "singleton")?
                .ok_or_else(|| {
                    StorageError::Invalid("request statistics state is missing".to_owned())
                })?;
            if state.optional_text("collection_started_at")?.is_none() {
                state.insert("collection_started_at", Field::Text(now.clone()));
            }
            state.insert("updated_at", Field::Text(now.clone()));
            transaction.put(Table::StatisticsState, "singleton", &state)?;
        }
        Ok(())
    })
    .await
}

pub(super) fn request_log_record(
    record: &RequestLogRecord,
    created_at: &str,
) -> Result<Record, StorageError> {
    Ok(Record::new()
        .with("id", Field::Text(record.id.clone()))
        .with("request_id", Field::Text(record.request_id.clone()))
        .with("route_alias", Field::Text(record.route_alias.clone()))
        .with("provider_id", optional_text(&record.provider_id))
        .with(
            "provider_credential_id",
            optional_text(&record.provider_credential_id),
        )
        .with("api_key_id", optional_text(&record.api_key_id))
        .with("model", Field::Text(record.model.clone()))
        .with(
            "client_protocol",
            Field::Text(record.client_protocol.clone()),
        )
        .with(
            "upstream_protocol",
            optional_text(&record.upstream_protocol),
        )
        .with("status", Field::I64(record.status))
        .with("duration_ms", Field::I64(record.duration_ms))
        .with("input_tokens", optional_i64(record.input_tokens))
        .with("output_tokens", optional_i64(record.output_tokens))
        .with("cached_tokens", optional_i64(record.cached_tokens))
        .with(
            "cache_input_tokens",
            optional_i64(record.cache_input_tokens),
        )
        .with("cost_micro_usd", optional_i64(record.cost_micro_usd))
        .with("error", optional_text(&record.error))
        .with("created_at", Field::Text(created_at.to_owned())))
}

pub(super) fn optional_text(value: &Option<String>) -> Field {
    value
        .as_ref()
        .map(|value| Field::Text(value.clone()))
        .unwrap_or(Field::Null)
}

pub(super) fn optional_i64(value: Option<i64>) -> Field {
    value.map(Field::I64).unwrap_or(Field::Null)
}

pub(super) fn provider_usage_key(credential_id: &str, minute: i64) -> Result<String, StorageError> {
    if !valid_provider_credential_id(credential_id) || minute < 0 {
        return Err(StorageError::Invalid(
            "provider usage meter key is invalid".to_owned(),
        ));
    }
    let key = format!("{credential_id}/{minute:020}");
    validate_key(&key)?;
    Ok(key)
}

pub(super) fn valid_provider_credential_id(credential_id: &str) -> bool {
    !credential_id.is_empty()
        && credential_id.len() <= 256
        && !credential_id.contains('/')
        && !credential_id.bytes().any(|byte| byte.is_ascii_control())
}

pub(super) fn provider_usage_record(
    credential_id: &str,
    minute: i64,
    counts: ProviderUsageBucketCounts,
) -> Record {
    Record::new()
        .with("credential_id", Field::Text(credential_id.to_owned()))
        .with("minute", Field::I64(minute))
        .with("request_count", Field::I64(counts.requests))
        .with("cost_micro_usd", Field::I64(counts.cost_micro_usd))
        .with("cost_events", Field::I64(counts.cost_events))
        .with(
            "missing_cost_events",
            Field::I64(counts.missing_cost_events),
        )
        .with("input_tokens", Field::I64(counts.input_tokens))
        .with("output_tokens", Field::I64(counts.output_tokens))
}

pub(super) fn provider_usage_counts(
    record: Record,
) -> Result<ProviderUsageBucketCounts, StorageError> {
    let nonnegative = |name: &str| -> Result<i64, StorageError> {
        match record.field(name) {
            Some(Field::I64(value)) if *value >= 0 => Ok(*value),
            Some(Field::I64(_)) => Err(StorageError::Invalid(format!(
                "provider usage field '{name}' is negative"
            ))),
            None => Ok(0),
            _ => Err(StorageError::Invalid(format!(
                "provider usage field '{name}' is malformed"
            ))),
        }
    };
    Ok(ProviderUsageBucketCounts {
        requests: nonnegative("request_count")?,
        cost_micro_usd: nonnegative("cost_micro_usd")?,
        cost_events: nonnegative("cost_events")?,
        missing_cost_events: nonnegative("missing_cost_events")?,
        input_tokens: nonnegative("input_tokens")?,
        output_tokens: nonnegative("output_tokens")?,
    })
}
