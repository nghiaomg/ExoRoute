//! Stream-run record storage: creation with retention, event journal
//! append/replay reads, completion, and bounded cleanup helpers.

use crate::{
    config::UpstreamSettings,
    infra::db::{
        add_stream_retained_bytes, deduct_stream_retained_bytes, set_stream_retained_bytes,
        stream_retained_bytes,
    },
    infra::storage::{Database, Field, Record, StorageError, Table, WriteTxn},
    infra::telemetry::RequestAnalytics,
};
use bytes::Bytes;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// Hard safety bound on the number of live runs. Pruning now runs *before*
/// this check so the bound is a backstop rather than a wedge.
const MAX_LIVE_RUNS: usize = 1_000;
const RUN_PAGE_SIZE: usize = 256;
const MAX_FINISHED_RUNS_TO_SORT: usize = 100_000;

/// Bounded continuity journal writer. A single bounded queue plus one flush
/// task coalesces appends from many concurrent stream workers into one
/// `RwTxn` per flush window instead of one fsynced transaction per SSE chunk.
const JOURNAL_QUEUE_CAPACITY: usize = 1024;
const JOURNAL_BATCH_SIZE: usize = 64;
const JOURNAL_FLUSH_INTERVAL: Duration = Duration::from_millis(8);

/// The only upstream fields the journal needs. Keeping this small struct in
/// queued messages avoids copying the whole `UpstreamSettings` into every
/// batch entry.
#[derive(Clone, Copy)]
struct ReplayLimits {
    bytes_per_run: usize,
    bytes_total: usize,
    events_per_run: i64,
}

impl From<UpstreamSettings> for ReplayLimits {
    fn from(value: UpstreamSettings) -> Self {
        Self {
            bytes_per_run: value.continuity_replay_bytes_per_run,
            bytes_total: value.continuity_replay_bytes_total,
            events_per_run: value.continuity_replay_events_per_run,
        }
    }
}

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

// ---------------------------------------------------------------------------
// Run creation and pruning
// ---------------------------------------------------------------------------

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
    let retained_runs = upstream.continuity_retained_runs;
    db.write(move |transaction| {
        // C-1: reclaim expired and excess finished runs BEFORE evaluating the
        // capacity bound. Previously the bound was checked first, which made
        // the pruning loop below unreachable once the table hit 1,000 rows.
        prune_expired_runs(transaction, &cutoff)?;
        prune_excess_finished_runs(transaction, retained_runs)?;
        let runs = transaction.scan_prefix::<Record>(Table::StreamRuns, "", MAX_LIVE_RUNS)?;
        if runs.len() >= MAX_LIVE_RUNS {
            return Err(StorageError::Busy);
        }
        // The counter is derived, but seed it on first use so every mutation
        // path can update it without a conditional.
        if transaction
            .get::<i64>(Table::Meta, crate::infra::db::STREAM_RETAINED_BYTES_KEY)?
            .is_none()
        {
            set_stream_retained_bytes(transaction, 0)?;
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

/// Deletes every run older than `cutoff`, in bounded pages. Running runs are
/// left for startup recovery so an in-flight stream is not cut short here.
fn prune_expired_runs(
    transaction: &mut WriteTxn<'_, '_>,
    cutoff: &str,
) -> Result<usize, StorageError> {
    let mut after = None;
    let mut removed = 0usize;
    loop {
        let page = transaction.scan_prefix_after::<Record>(
            Table::StreamRuns,
            "",
            after.as_deref(),
            RUN_PAGE_SIZE,
        )?;
        if page.is_empty() {
            break;
        }
        let page_len = page.len();
        let mut expired = Vec::new();
        for (id, record) in page {
            after = Some(id.clone());
            let status = record.text("status")?;
            let created_at = record.text("created_at")?;
            if status != "running" && created_at < cutoff {
                expired.push((id, record));
            }
        }
        for (id, record) in expired {
            remove_stream_run(transaction, &id, &record)?;
            removed += 1;
        }
        if page_len < RUN_PAGE_SIZE {
            break;
        }
    }
    Ok(removed)
}

/// Keeps only the newest `retained_runs` finished rows, deleting the rest.
/// Keys are not time-ordered, so the bounded page scans collect metadata and
/// a single sort decides which rows survive.
fn prune_excess_finished_runs(
    transaction: &mut WriteTxn<'_, '_>,
    retained_runs: usize,
) -> Result<usize, StorageError> {
    let mut finished = Vec::new();
    let mut after = None;
    loop {
        let page = transaction.scan_prefix_after::<Record>(
            Table::StreamRuns,
            "",
            after.as_deref(),
            RUN_PAGE_SIZE,
        )?;
        if page.is_empty() || finished.len() >= MAX_FINISHED_RUNS_TO_SORT {
            break;
        }
        let page_len = page.len();
        for (id, record) in page {
            after = Some(id.clone());
            if record.text("status")? == "running" {
                continue;
            }
            finished.push((record.text("created_at")?.to_owned(), id, record));
        }
        if page_len < RUN_PAGE_SIZE {
            break;
        }
    }
    if finished.len() <= retained_runs {
        return Ok(0);
    }
    finished.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    let excess = finished.len() - retained_runs;
    let mut removed = 0usize;
    for (_, id, record) in finished.into_iter().take(excess) {
        remove_stream_run(transaction, &id, &record)?;
        removed += 1;
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Run loading
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Event appending (single-row primitives, shared by direct callers and the
// batch writer)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(in crate::gateway) async fn append_event(
    db: &Database,
    run_id: &str,
    payload: &Bytes,
) -> Result<i64, StorageError> {
    append_event_with_settings(db, run_id, payload, UpstreamSettings::default()).await
}

/// Direct single-chunk append. Production stream workers use
/// [`JournalBatch`] instead; this wrapper keeps the domain primitive available
/// to tests and one-off callers.
#[cfg(test)]
pub(in crate::gateway) async fn append_event_with_settings(
    db: &Database,
    run_id: &str,
    payload: &Bytes,
    upstream: UpstreamSettings,
) -> Result<i64, StorageError> {
    let run_id = run_id.to_owned();
    let payload = payload.clone();
    db.write(move |transaction| {
        append_event_in_txn(transaction, &run_id, &payload, ReplayLimits::from(upstream))
    })
    .await
}

/// P-1: computes the global byte budget from the `Meta` counter, never by
/// scanning `StreamRuns`. All counter updates happen in the caller's RwTxn.
fn append_event_in_txn(
    transaction: &mut WriteTxn<'_, '_>,
    run_id: &str,
    payload: &[u8],
    upstream: ReplayLimits,
) -> Result<i64, StorageError> {
    let payload_size = payload.len();
    let mut run = transaction
        .get::<Record>(Table::StreamRuns, run_id)?
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
    let global_retained = stream_retained_bytes(transaction)?;
    let fits_run = retained_bytes
        .checked_add(payload_size)
        .is_some_and(|next| next <= upstream.bytes_per_run);
    let fits_total = usize::try_from(global_retained)
        .unwrap_or(usize::MAX)
        .checked_add(payload_size)
        .is_some_and(|next| next <= upstream.bytes_total);
    let retain = replay_complete
        && sequence <= upstream.events_per_run
        && payload_size <= upstream.bytes_per_run
        && fits_run
        && fits_total;
    run.insert("latest_sequence", Field::I64(sequence));
    if retain {
        let key = stream_event_key(run_id, sequence)?;
        let event = Record::new()
            .with("sequence", Field::I64(sequence))
            .with("payload", Field::Bytes(payload.to_vec()));
        transaction.put_if_absent(Table::StreamEvents, &key, &event)?;
        let added = i64::try_from(payload_size).map_err(|_| {
            StorageError::Invalid("stream event payload size overflowed".to_owned())
        })?;
        add_stream_retained_bytes(transaction, added)?;
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
    transaction.put(Table::StreamRuns, run_id, &run)?;
    Ok(sequence)
}

// ---------------------------------------------------------------------------
// Terminal run updates
// ---------------------------------------------------------------------------

fn mark_replay_incomplete_in_txn(
    transaction: &mut WriteTxn<'_, '_>,
    run_id: &str,
    latest_sequence: i64,
) -> Result<(), StorageError> {
    let Some(mut run) = transaction.get::<Record>(Table::StreamRuns, run_id)? else {
        return Ok(());
    };
    if run.text("status")? == "running" {
        run.insert(
            "latest_sequence",
            Field::I64(run.integer("latest_sequence")?.max(latest_sequence)),
        );
        run.insert("replay_complete", Field::Bool(false));
        transaction.put(Table::StreamRuns, run_id, &run)?;
    }
    Ok(())
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
    let run_id = run_id.to_owned();
    let status = status.to_owned();
    let error = error.map(|message| truncate_error(message, upstream.continuity_setup_error_bytes));
    let finished_at = crate::infra::db::utc_timestamp_now()?;
    db.write(move |transaction| {
        finish_run_in_txn(transaction, &run_id, &status, error.as_deref(), &finished_at)
    })
    .await
}

fn finish_run_in_txn(
    transaction: &mut WriteTxn<'_, '_>,
    run_id: &str,
    status: &str,
    error: Option<&str>,
    finished_at: &str,
) -> Result<(), StorageError> {
    debug_assert!(matches!(status, "completed" | "failed" | "cancelled"));
    let Some(mut run) = transaction.get::<Record>(Table::StreamRuns, run_id)? else {
        return Ok(());
    };
    if run.text("status")? == "running" {
        run.insert("status", Field::Text(status.to_owned()));
        run.insert("error", error.map(str::to_owned).map(Field::Text).unwrap_or(Field::Null));
        run.insert("finished_at", Field::Text(finished_at.to_owned()));
        transaction.put(Table::StreamRuns, run_id, &run)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Bounded mpsc batch writer
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum JournalOp {
    Append {
        run_id: String,
        payload: Bytes,
        upstream: ReplayLimits,
    },
    Finish {
        run_id: String,
        status: String,
        error: Option<String>,
        finished_at: String,
    },
    Incomplete {
        run_id: String,
        latest_sequence: i64,
    },
}

enum JournalOutcome {
    Sequence(i64),
    Applied,
}

struct JournalMessage {
    op: JournalOp,
    reply: oneshot::Sender<Result<JournalOutcome, StorageError>>,
}

/// One bounded queue shared by every stream worker, feeding a single flush
/// task so commits (and their fsyncs) are amortized across concurrent runs.
/// Each message carries its own oneshot reply: a logical error on one op is
/// reported only to that caller, while other ops in the same transaction still
/// commit. The task exits when the last handle is dropped and the channel
/// closes.
#[derive(Clone)]
pub(crate) struct JournalBatch {
    sender: mpsc::Sender<JournalMessage>,
    _writer: Arc<JoinHandle<()>>,
}

impl JournalBatch {
    pub(crate) fn start(db: Database) -> Self {
        let (sender, receiver) = mpsc::channel(JOURNAL_QUEUE_CAPACITY);
        let writer = tokio::spawn(journal_writer(db, receiver));
        Self {
            sender,
            _writer: Arc::new(writer),
        }
    }

    pub(in crate::gateway) async fn append(
        &self,
        run_id: &str,
        payload: &Bytes,
        upstream: UpstreamSettings,
    ) -> Result<i64, StorageError> {
        let outcome = self
            .request(JournalOp::Append {
                run_id: run_id.to_owned(),
                payload: payload.clone(),
                upstream: ReplayLimits::from(upstream),
            })
            .await?;
        match outcome {
            JournalOutcome::Sequence(sequence) => Ok(sequence),
            JournalOutcome::Applied => Err(StorageError::Task(
                "continuity journal returned an unexpected append outcome".to_owned(),
            )),
        }
    }

    pub(in crate::gateway) async fn finish(
        &self,
        run_id: &str,
        status: &str,
        error: Option<&str>,
        upstream: UpstreamSettings,
    ) -> Result<(), StorageError> {
        let finished_at = crate::infra::db::utc_timestamp_now()?;
        let error = error.map(|message| truncate_error(message, upstream.continuity_setup_error_bytes));
        match self
            .request(JournalOp::Finish {
                run_id: run_id.to_owned(),
                status: status.to_owned(),
                error,
                finished_at,
            })
            .await?
        {
            JournalOutcome::Applied => Ok(()),
            JournalOutcome::Sequence(_) => Err(StorageError::Task(
                "continuity journal returned an unexpected finish outcome".to_owned(),
            )),
        }
    }

    pub(in crate::gateway) async fn mark_incomplete(
        &self,
        run_id: &str,
        latest_sequence: i64,
    ) -> Result<(), StorageError> {
        match self
            .request(JournalOp::Incomplete {
                run_id: run_id.to_owned(),
                latest_sequence,
            })
            .await?
        {
            JournalOutcome::Applied => Ok(()),
            JournalOutcome::Sequence(_) => Err(StorageError::Task(
                "continuity journal returned an unexpected outcome".to_owned(),
            )),
        }
    }

    async fn request(&self, op: JournalOp) -> Result<JournalOutcome, StorageError> {
        let (reply, receiver) = oneshot::channel();
        self.sender
            .try_send(JournalMessage { op, reply })
            .map_err(|_| StorageError::Busy)?;
        receiver
            .await
            .map_err(|_| StorageError::Task("continuity journal writer stopped".to_owned()))?
    }
}

async fn journal_writer(db: Database, mut receiver: mpsc::Receiver<JournalMessage>) {
    let mut batch = Vec::with_capacity(JOURNAL_BATCH_SIZE);
    let mut interval = tokio::time::interval(JOURNAL_FLUSH_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval.tick().await;
    loop {
        tokio::select! {
            message = receiver.recv(), if batch.len() < JOURNAL_BATCH_SIZE => match message {
                Some(message) => batch.push(message),
                None => break,
            },
            _ = interval.tick(), if !batch.is_empty() => {}
        }
        // Coalesce everything already queued without adding latency when the
        // worker is idle; a full batch still flushes immediately.
        while batch.len() < JOURNAL_BATCH_SIZE {
            match receiver.try_recv() {
                Ok(message) => batch.push(message),
                Err(_) => break,
            }
        }
        if !batch.is_empty() {
            flush_journal_batch(&db, std::mem::take(&mut batch)).await;
        }
    }
    if !batch.is_empty() {
        flush_journal_batch(&db, batch).await;
    }
}

async fn flush_journal_batch(db: &Database, batch: Vec<JournalMessage>) {
    let ops: Vec<JournalOp> = batch.iter().map(|message| message.op.clone()).collect();
    let result = db
        .write(move |transaction| {
            let mut outcomes = Vec::with_capacity(ops.len());
            for op in &ops {
                outcomes.push(apply_journal_op(transaction, op));
            }
            Ok::<_, StorageError>(outcomes)
        })
        .await;
    match result {
        Ok(outcomes) => {
            for (message, outcome) in batch.into_iter().zip(outcomes) {
                let _ = message.reply.send(outcome);
            }
        }
        Err(error) => {
            tracing::warn!(%error, records = batch.len(), "continuity journal batch failed");
            for message in batch {
                let _ = message.reply.send(Err(StorageError::Backend(format!(
                    "continuity journal batch failed: {error}"
                ))));
            }
        }
    }
}

fn apply_journal_op(
    transaction: &mut WriteTxn<'_, '_>,
    op: &JournalOp,
) -> Result<JournalOutcome, StorageError> {
    match op {
        JournalOp::Append {
            run_id,
            payload,
            upstream,
        } => append_event_in_txn(transaction, run_id, payload, *upstream)
            .map(JournalOutcome::Sequence),
        JournalOp::Finish {
            run_id,
            status,
            error,
            finished_at,
        } => finish_run_in_txn(transaction, run_id, status, error.as_deref(), finished_at)
            .map(|()| JournalOutcome::Applied),
        JournalOp::Incomplete {
            run_id,
            latest_sequence,
        } => mark_replay_incomplete_in_txn(transaction, run_id, *latest_sequence)
            .map(|()| JournalOutcome::Applied),
    }
}

// ---------------------------------------------------------------------------
// Replay reads and key helpers
// ---------------------------------------------------------------------------

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

/// Deletes one run and all of its derived rows, keeping the retained-byte
/// counter in sync with the removed primary row.
pub(in crate::gateway) fn remove_stream_run(
    transaction: &mut WriteTxn<'_, '_>,
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
    let retained = run
        .optional_integer("retained_bytes")?
        .unwrap_or(0)
        .max(0);
    deduct_stream_retained_bytes(transaction, retained)?;
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
