//! Stream-run record storage: creation with retention, event journal
//! append/replay reads, completion, and bounded cleanup helpers.

use crate::{
    config::UpstreamSettings,
    infra::storage::{Database, Field, Record, StorageError, Table},
    infra::telemetry::RequestAnalytics,
};
use bytes::Bytes;
use std::time::Duration;

pub(in crate::gateway) struct BackgroundRequestSettings {
    /// `None` is reserved for stream continuity, which ends only on
    /// cancellation, shutdown, or a successful/terminal stream outcome.
    pub(in crate::gateway) request_timeout: Option<Duration>,
    pub(in crate::gateway) analytics: Option<RequestAnalytics>,
}

#[derive(Clone, Debug)]
pub(in crate::gateway) struct RunSnapshot {
    pub(in crate::gateway) status: String,
    pub(in crate::gateway) latest_sequence: i64,
    pub(in crate::gateway) retained_through: i64,
    pub(in crate::gateway) replay_complete: bool,
}

#[cfg(test)]
pub(in crate::gateway) async fn create_run(
    db: &Database,
    run_id: &str,
    api_key_id: &str,
) -> Result<(), StorageError> {
    create_run_with_settings(db, run_id, api_key_id, UpstreamSettings::default()).await
}

pub(in crate::gateway) async fn create_run_with_settings(
    db: &Database,
    run_id: &str,
    api_key_id: &str,
    upstream: UpstreamSettings,
) -> Result<(), StorageError> {
    let run_id = run_id.to_owned();
    let api_key_id = api_key_id.to_owned();
    let created_at = crate::infra::db::utc_timestamp_now()?;
    let cutoff = crate::infra::db::utc_timestamp_before(upstream.continuity_retention)?;
    db.write(move |transaction| {
        let runs = transaction.scan_prefix::<Record>(Table::StreamRuns, "", 1_000)?;
        if runs.len() >= 1_000 {
            return Err(StorageError::Busy);
        }
        let mut finished = Vec::new();
        for (id, record) in runs {
            let status = record.text("status")?;
            if status == "running" {
                continue;
            }
            let created_at = record.text("created_at")?.to_owned();
            if created_at < cutoff {
                remove_stream_run(transaction, &id, &record)?;
            } else {
                finished.push((created_at, id, record));
            }
        }
        finished.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
        while finished.len() >= upstream.continuity_retained_runs {
            let (_, id, record) = finished.remove(0);
            remove_stream_run(transaction, &id, &record)?;
        }
        let run = Record::new()
            .with("id", Field::Text(run_id.clone()))
            .with("api_key_id", Field::Text(api_key_id.clone()))
            .with("status", Field::Text("running".to_owned()))
            .with("created_at", Field::Text(created_at.clone()))
            .with("latest_sequence", Field::I64(0))
            .with("retained_through", Field::I64(0))
            .with("retained_bytes", Field::I64(0))
            .with("replay_complete", Field::Bool(true))
            .with("error", Field::Null)
            .with("finished_at", Field::Null);
        let api_key_index = stream_run_api_key_index_key(&api_key_id, &run_id)?;
        transaction.put_if_absent(Table::StreamRuns, &run_id, &run)?;
        transaction.put_if_absent(Table::StreamRunApiKeyIndex, &api_key_index, &run_id)
    })
    .await
}

#[cfg(test)]
pub(in crate::gateway) async fn load_run(
    db: &Database,
    run_id: &str,
    api_key_id: &str,
) -> Result<Option<RunSnapshot>, StorageError> {
    load_run_with_settings(db, run_id, api_key_id, UpstreamSettings::default()).await
}

pub(in crate::gateway) async fn load_run_with_settings(
    db: &Database,
    run_id: &str,
    api_key_id: &str,
    upstream: UpstreamSettings,
) -> Result<Option<RunSnapshot>, StorageError> {
    let run_id = run_id.to_owned();
    let api_key_id = api_key_id.to_owned();
    let cutoff = crate::infra::db::utc_timestamp_before(upstream.continuity_retention)?;
    db.read(move |transaction| {
        let Some(record) = transaction.get::<Record>(Table::StreamRuns, &run_id)? else {
            return Ok(None);
        };
        if record.text("api_key_id")? != api_key_id || record.text("created_at")? < cutoff.as_str()
        {
            return Ok(None);
        }
        Ok(Some(RunSnapshot {
            status: record.text("status")?.to_owned(),
            latest_sequence: record.integer("latest_sequence")?,
            retained_through: record.integer("retained_through")?,
            replay_complete: record.boolean("replay_complete")?,
        }))
    })
    .await
}

#[cfg(test)]
pub(in crate::gateway) async fn append_event(
    db: &Database,
    run_id: &str,
    payload: &Bytes,
) -> Result<i64, StorageError> {
    append_event_with_settings(db, run_id, payload, UpstreamSettings::default()).await
}

pub(in crate::gateway) async fn append_event_with_settings(
    db: &Database,
    run_id: &str,
    payload: &Bytes,
    upstream: UpstreamSettings,
) -> Result<i64, StorageError> {
    let payload_size = payload.len();
    let run_id = run_id.to_owned();
    let payload = payload.clone();
    db.write(move |transaction| {
        let mut run = transaction
            .get::<Record>(Table::StreamRuns, &run_id)?
            .ok_or(StorageError::NotFound)?;
        if run.text("status")? != "running" {
            return Err(StorageError::NotFound);
        }
        let latest_sequence = run.integer("latest_sequence")?;
        let sequence = latest_sequence.checked_add(1).ok_or_else(|| {
            StorageError::Invalid("stream event sequence exceeded the supported range".to_owned())
        })?;
        let retained_bytes = usize::try_from(run.integer("retained_bytes")?).map_err(|_| {
            StorageError::Invalid("stream retained-byte count is invalid".to_owned())
        })?;
        let replay_complete = run.boolean("replay_complete")?;
        let runs = transaction.scan_prefix::<Record>(Table::StreamRuns, "", 1_000)?;
        if runs.len() >= 1_000 {
            return Err(StorageError::Busy);
        }
        let total_retained = runs.iter().try_fold(0usize, |total, (_, record)| {
            let bytes = usize::try_from(record.integer("retained_bytes")?).map_err(|_| {
                StorageError::Invalid("stream retained-byte count is invalid".to_owned())
            })?;
            total.checked_add(bytes).ok_or_else(|| {
                StorageError::Invalid("stream retained-byte total overflowed".to_owned())
            })
        })?;
        let fits_run = retained_bytes
            .checked_add(payload_size)
            .is_some_and(|next| next <= upstream.continuity_replay_bytes_per_run);
        let fits_total = total_retained
            .checked_add(payload_size)
            .is_some_and(|next| next <= upstream.continuity_replay_bytes_total);
        let retain = replay_complete
            && sequence <= upstream.continuity_replay_events_per_run
            && payload_size <= upstream.continuity_replay_bytes_per_run
            && fits_run
            && fits_total;
        run.insert("latest_sequence", Field::I64(sequence));
        if retain {
            let key = stream_event_key(&run_id, sequence)?;
            let event = Record::new()
                .with("sequence", Field::I64(sequence))
                .with("payload", Field::Bytes(payload.to_vec()));
            transaction.put_if_absent(Table::StreamEvents, &key, &event)?;
            run.insert("retained_through", Field::I64(sequence));
            run.insert(
                "retained_bytes",
                Field::I64(
                    i64::try_from(retained_bytes.saturating_add(payload_size)).map_err(|_| {
                        StorageError::Invalid("stream retained-byte count overflowed".to_owned())
                    })?,
                ),
            );
        } else {
            run.insert("replay_complete", Field::Bool(false));
        }
        transaction.put(Table::StreamRuns, &run_id, &run)?;
        Ok(sequence)
    })
    .await
}

pub(in crate::gateway) async fn mark_replay_incomplete(
    db: &Database,
    run_id: &str,
    latest_sequence: i64,
) -> Result<(), StorageError> {
    let run_id = run_id.to_owned();
    db.write(move |transaction| {
        let Some(mut run) = transaction.get::<Record>(Table::StreamRuns, &run_id)? else {
            return Ok(());
        };
        if run.text("status")? == "running" {
            run.insert(
                "latest_sequence",
                Field::I64(run.integer("latest_sequence")?.max(latest_sequence)),
            );
            run.insert("replay_complete", Field::Bool(false));
            transaction.put(Table::StreamRuns, &run_id, &run)?;
        }
        Ok(())
    })
    .await
}

pub(in crate::gateway) async fn finish_run(
    db: &Database,
    run_id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), StorageError> {
    finish_run_with_settings(db, run_id, status, error, UpstreamSettings::default()).await
}

pub(in crate::gateway) async fn finish_run_with_settings(
    db: &Database,
    run_id: &str,
    status: &str,
    error: Option<&str>,
    upstream: UpstreamSettings,
) -> Result<(), StorageError> {
    debug_assert!(matches!(status, "completed" | "failed" | "cancelled"));
    let run_id = run_id.to_owned();
    let status = status.to_owned();
    let error = error.map(|message| truncate_error(message, upstream.continuity_setup_error_bytes));
    let finished_at = crate::infra::db::utc_timestamp_now()?;
    db.write(move |transaction| {
        let Some(mut run) = transaction.get::<Record>(Table::StreamRuns, &run_id)? else {
            return Ok(());
        };
        if run.text("status")? == "running" {
            run.insert("status", Field::Text(status.clone()));
            run.insert(
                "error",
                error.clone().map(Field::Text).unwrap_or(Field::Null),
            );
            run.insert("finished_at", Field::Text(finished_at.clone()));
            transaction.put(Table::StreamRuns, &run_id, &run)?;
        }
        Ok(())
    })
    .await
}

pub(in crate::gateway) async fn read_events_with_page_size(
    db: &Database,
    run_id: &str,
    after: i64,
    upstream: UpstreamSettings,
) -> Result<Vec<(i64, Vec<u8>)>, StorageError> {
    if after < 0 {
        return Err(StorageError::Invalid(
            "stream event cursor is invalid".to_owned(),
        ));
    }
    let run_id = run_id.to_owned();
    let prefix = format!("{run_id}/");
    let cursor = stream_event_key(&run_id, after)?;
    db.read(move |transaction| {
        let rows = transaction.scan_prefix_after::<Record>(
            Table::StreamEvents,
            &prefix,
            Some(&cursor),
            upstream.continuity_resume_page_size,
        )?;
        rows.into_iter()
            .map(|(_, record)| {
                Ok((
                    record.integer("sequence")?,
                    record.bytes("payload")?.to_vec(),
                ))
            })
            .collect()
    })
    .await
}

pub(in crate::gateway) fn stream_event_key(
    run_id: &str,
    sequence: i64,
) -> Result<String, StorageError> {
    if sequence < 0 {
        return Err(StorageError::Invalid(
            "stream event sequence is invalid".to_owned(),
        ));
    }
    let key = format!("{run_id}/{sequence:020}");
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(in crate::gateway) fn stream_run_api_key_index_key(
    api_key_id: &str,
    run_id: &str,
) -> Result<String, StorageError> {
    let key = format!("{api_key_id}/{run_id}");
    crate::infra::storage::validate_key(&key)?;
    Ok(key)
}

pub(in crate::gateway) fn remove_stream_run(
    transaction: &mut crate::infra::storage::WriteTxn<'_, '_>,
    id: &str,
    run: &Record,
) -> Result<(), StorageError> {
    transaction.delete(Table::StreamRuns, id)?;
    transaction.delete_prefix(Table::StreamEvents, &format!("{id}/"))?;
    let api_key_id = run.text("api_key_id")?;
    transaction.delete(
        Table::StreamRunApiKeyIndex,
        &stream_run_api_key_index_key(api_key_id, id)?,
    )?;
    Ok(())
}

pub(in crate::gateway) fn truncate_error(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}
