use super::queries::parse_minute_key;
use super::{
    MAX_EXPIRY_ROWS_PER_RANGE, MAX_PROVIDER_METER_EXPIRY_ROWS, MINUTE_RETENTION,
    PROVIDER_METER_RETENTION_MINUTES, STATISTICS_RANGES, counts_record, minute_key, record_counts,
    top_index_key, window_key,
};
use crate::{
    infra::db,
    infra::storage::{Database, Field, Record, StorageError, Table},
};

pub(crate) fn cursor_record(minute: i64, dimension_kind: i64, dimension_id: &str) -> Record {
    Record::new()
        .with("cursor_minute", Field::I64(minute))
        .with("cursor_dimension_kind", Field::I64(dimension_kind))
        .with("cursor_dimension_id", Field::Text(dimension_id.to_owned()))
}

pub(crate) async fn expire_statistics(
    db: &Database,
    current_minute: i64,
) -> Result<(), StorageError> {
    for (window_minutes, _) in STATISTICS_RANGES {
        let target_minute = current_minute.saturating_sub(window_minutes);
        let cursor = db
            .read(move |transaction| {
                transaction.get::<Record>(Table::UsageExpiryCursor, &window_minutes.to_string())
            })
            .await?;
        let Some(cursor) = cursor else {
            continue;
        };
        let cursor_minute = cursor.integer("cursor_minute")?;
        let cursor_kind = cursor.integer("cursor_dimension_kind")?;
        let cursor_id = cursor.text("cursor_dimension_id")?.to_owned();
        let after = minute_key(cursor_minute, cursor_kind, &cursor_id)?;
        let expired = db
            .read(move |transaction| {
                let rows = transaction.scan_prefix_after::<Record>(
                    Table::UsageMinutes,
                    "",
                    Some(&after),
                    MAX_EXPIRY_ROWS_PER_RANGE,
                )?;
                let mut expired = Vec::with_capacity(rows.len());
                for (key, record) in rows {
                    let (minute, kind, dimension_id) = parse_minute_key(&key)?;
                    if minute > target_minute {
                        break;
                    }
                    expired.push((key, minute, kind, dimension_id, record_counts(&record)?));
                }
                Ok(expired)
            })
            .await?;
        if expired.is_empty() {
            db.write(move |transaction| {
                let current = transaction
                    .get::<Record>(Table::UsageExpiryCursor, &window_minutes.to_string())?
                    .ok_or_else(|| {
                        StorageError::Invalid("statistics expiry cursor is missing".to_owned())
                    })?;
                let current_minute = current.integer("cursor_minute")?;
                if target_minute < current_minute {
                    return Ok(());
                }
                transaction.put(
                    Table::UsageExpiryCursor,
                    &window_minutes.to_string(),
                    &cursor_record(target_minute, 4, ""),
                )
            })
            .await?;
            continue;
        }
        db.write(move |transaction| {
            let (last_minute, last_kind, last_id) = expired
                .last()
                .map(|(_, minute, kind, id, _)| (*minute, *kind, id.as_str()))
                .ok_or_else(|| {
                    StorageError::Invalid("statistics expiry batch is empty".to_owned())
                })?;
            for (key, _minute, kind, dimension_id, counts) in &expired {
                let aggregate_key = window_key(window_minutes, *kind, dimension_id)?;
                let current_record =
                    transaction.get::<Record>(Table::UsageWindows, &aggregate_key)?;
                if let Some(current_record) = current_record {
                    let current = record_counts(&current_record)?;
                    let updated = current.subtract(*counts);
                    if current.requests > 0 {
                        let old_index =
                            top_index_key(window_minutes, *kind, current.requests, dimension_id)?;
                        transaction.delete(Table::StatisticsIndex, &old_index)?;
                    }
                    if updated.requests == 0 {
                        transaction.delete(Table::UsageWindows, &aggregate_key)?;
                    } else {
                        transaction.put(
                            Table::UsageWindows,
                            &aggregate_key,
                            &counts_record(updated),
                        )?;
                        let new_index =
                            top_index_key(window_minutes, *kind, updated.requests, dimension_id)?;
                        transaction.put(
                            Table::StatisticsIndex,
                            &new_index,
                            &dimension_id.to_owned(),
                        )?;
                    }
                }
                let _ = key;
            }
            transaction.put(
                Table::UsageExpiryCursor,
                &window_minutes.to_string(),
                &cursor_record(last_minute, last_kind, last_id),
            )?;
            let mut state = transaction
                .get::<Record>(Table::StatisticsState, "singleton")?
                .ok_or_else(|| {
                    StorageError::Invalid("request statistics state is missing".to_owned())
                })?;
            state.insert("updated_at", Field::Text(db::utc_timestamp_now()?));
            transaction.put(Table::StatisticsState, "singleton", &state)
        })
        .await?;
    }

    let cutoff = current_minute.saturating_sub(MINUTE_RETENTION);
    let month_cursor = db
        .read(|transaction| {
            transaction.get::<Record>(Table::UsageExpiryCursor, &(30 * 24 * 60).to_string())
        })
        .await?;
    let can_remove_old_minutes = match month_cursor {
        Some(record) => record.integer("cursor_minute")? >= cutoff,
        None => false,
    };
    if can_remove_old_minutes {
        db.write(move |transaction| {
            let rows = transaction.scan_prefix(Table::UsageMinutes, "", 2_048)?;
            let mut removed = 0usize;
            for (key, record) in rows {
                let (minute, _, _) = parse_minute_key(&key)?;
                if minute >= cutoff {
                    break;
                }
                let _ = record_counts(&record)?;
                transaction.delete(Table::UsageMinutes, &key)?;
                removed += 1;
            }
            Ok(removed)
        })
        .await?;
    }
    Ok(())
}

pub(crate) async fn expire_provider_usage(
    db: &Database,
    current_minute: i64,
) -> Result<(), StorageError> {
    let cutoff = current_minute.saturating_sub(PROVIDER_METER_RETENTION_MINUTES);
    db.write(move |transaction| {
        let rows = transaction.scan_prefix::<Record>(
            Table::ProviderUsageMeters,
            "",
            MAX_PROVIDER_METER_EXPIRY_ROWS,
        )?;
        let mut removed = 0usize;
        for (key, record) in rows {
            let minute = record.integer("minute").or_else(|_| {
                key.rsplit('/')
                    .next()
                    .and_then(|value| value.parse::<i64>().ok())
                    .ok_or_else(|| {
                        StorageError::Invalid("provider usage meter minute is malformed".to_owned())
                    })
            })?;
            if minute < cutoff {
                transaction.delete(Table::ProviderUsageMeters, &key)?;
                removed = removed.saturating_add(1);
            }
        }
        Ok(removed)
    })
    .await?;
    Ok(())
}
