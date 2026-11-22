use super::records::{JournalBatch, RunSnapshot, read_events_with_page_size};
use super::*;
use crate::{
    config::{GatewayResourceLimits, UpstreamSettings},
    gateway::{GatewayRequestContext, cancel_stream, continuity::BackgroundRequestSettings},
    infra::db::{STREAM_RETAINED_BYTES_KEY, bootstrap::recover_abandoned_stream_runs},
    infra::storage::{Field, Record, StorageError, Table},
    security::api_key_scope::ApiKeyScope,
    state::AppState,
    support::test_support::TestDatabase,
};
use axum::{
    body::{Body, to_bytes},
    extract::{Extension, Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;
async fn test_state() -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    crate::support::test_support::seed_api_key(
        &database.db,
        "owner",
        "owner",
        "continuity-test-key",
    )
    .await
    .expect("API key row");
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    config.request_timeout = Duration::from_secs(5);
    config.circuit_breaker_threshold = 3;
    let state = AppState::new(config, database.db.clone());
    (database, state)
}

async fn wait_for_status(state: &AppState, run_id: &str, expected: &str) -> RunSnapshot {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(snapshot) = load_run(&state.db, run_id, "owner")
                .await
                .expect("load stream run")
                && snapshot.status == expected
            {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("stream task changed status")
}

#[tokio::test]
async fn disconnected_live_client_does_not_cancel_upstream_and_can_resume() {
    let (_database, state) = test_state().await;
    create_run(&state.db, "resume-run", "owner")
        .await
        .expect("create stream run");
    assert!(
        load_run(&state.db, "resume-run", "another-owner")
            .await
            .expect("owner check")
            .is_none()
    );

    let upstream = async_stream::stream! {
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: first\n\n"));
        tokio::time::sleep(Duration::from_millis(10)).await;
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: second\n\n"));
    };
    let producer = Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from_stream(upstream))
        .expect("producer response");
    let (sender, receiver) = mpsc::channel(1);
    drop(receiver);
    let cancellation = state.register_stream_cancellation("resume-run").await;
    let permit = state
        .gates
        .stream_continuity_in_flight
        .clone()
        .try_acquire_owned()
        .expect("continuity slot");
    start_background_stream(
        state.clone(),
        "resume-run".to_owned(),
        producer,
        sender,
        cancellation,
        permit,
    )
    .await;

    let snapshot = wait_for_status(&state, "resume-run", "completed").await;
    assert_eq!(snapshot.latest_sequence, 2);
    assert_eq!(snapshot.retained_through, 2);
    assert!(snapshot.replay_complete);

    let response = resume_response(
        state.clone(),
        "resume-run".to_owned(),
        "owner".to_owned(),
        0,
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-exoroute-stream-id"),
        Some(&HeaderValue::from_static("resume-run"))
    );
    let body = to_bytes(response.into_body(), 4096)
        .await
        .expect("resumed body");
    let text = String::from_utf8(body.to_vec()).expect("SSE text");
    assert!(text.contains("id: 1\ndata: first\n\n"));
    assert!(text.contains("id: 2\ndata: second\n\n"));

    let response = resume_response(state, "resume-run".to_owned(), "owner".to_owned(), 1);
    let body = to_bytes(response.into_body(), 4096)
        .await
        .expect("resumed tail");
    let text = String::from_utf8(body.to_vec()).expect("tail SSE text");
    assert!(!text.contains("first"));
    assert!(text.contains("id: 2\ndata: second\n\n"));
}

#[tokio::test]
async fn explicit_cancel_stops_the_worker_and_releases_its_slot() {
    let (_database, state) = test_state().await;
    create_run(&state.db, "cancel-run", "owner")
        .await
        .expect("create stream run");
    let upstream = async_stream::stream! {
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: started\n\n"));
        tokio::time::sleep(Duration::from_secs(5)).await;
        yield Ok::<Bytes, std::io::Error>(Bytes::from_static(b"data: should-not-run\n\n"));
    };
    let producer = Response::new(Body::from_stream(upstream));
    let (sender, mut receiver) = mpsc::channel(1);
    let cancellation = state.register_stream_cancellation("cancel-run").await;
    let permit = state
        .gates
        .stream_continuity_in_flight
        .clone()
        .try_acquire_owned()
        .expect("continuity slot");
    start_background_stream(
        state.clone(),
        "cancel-run".to_owned(),
        producer,
        sender,
        cancellation,
        permit,
    )
    .await;

    tokio::time::timeout(Duration::from_secs(1), async {
        while receiver.is_empty() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("live event should fill the bounded client channel");
    let cancel_response = cancel_stream(
        State(state.clone()),
        Path("cancel-run".to_owned()),
        Extension(GatewayRequestContext {
            resource_limits: GatewayResourceLimits::default(),
            operational_settings: state.operational_settings().settings,
            api_key_id: "owner".to_owned(),
            api_key_scope: std::sync::Arc::new(ApiKeyScope::default()),
            analytics: None,
        }),
    )
    .await;
    assert_eq!(cancel_response.status(), StatusCode::ACCEPTED);
    let snapshot = wait_for_status(&state, "cancel-run", "cancelled").await;
    assert!(snapshot.latest_sequence >= 2);
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if state.gates.stream_continuity_in_flight.active_count() == 0
                && !state.request_stream_cancellation("cancel-run")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("continuity slot and cancellation registration are released");

    let first = receiver.recv().await.expect("first event remains buffered");
    assert!(String::from_utf8_lossy(&first).contains("started"));
    assert!(receiver.recv().await.is_none());
    let replay = resume_response(state, "cancel-run".to_owned(), "owner".to_owned(), 1);
    let terminal = to_bytes(replay.into_body(), 4096)
        .await
        .expect("terminal event remains replayable");
    assert!(String::from_utf8_lossy(&terminal).contains("cancelled by user"));
}

#[tokio::test]
async fn stream_id_is_available_and_user_cancel_stops_provider_setup() {
    let (_database, state) = test_state().await;
    create_run(&state.db, "pending-run", "owner")
        .await
        .expect("create stream run");
    let (sender, receiver) = mpsc::channel(1);
    let cancellation = state.register_stream_cancellation("pending-run").await;
    let permit = state
        .gates
        .stream_continuity_in_flight
        .clone()
        .try_acquire_owned()
        .expect("continuity slot");
    start_background_request(
        state.clone(),
        "pending-run".to_owned(),
        std::future::pending::<Response>(),
        sender,
        cancellation,
        permit,
        BackgroundRequestSettings {
            request_timeout: Some(Duration::from_secs(5)),
            analytics: None,
        },
    )
    .await;

    let response = live_response(
        receiver,
        "pending-run",
        "request-id",
        state.shutdown_receiver(),
    );
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("x-exoroute-stream-id"),
        Some(&HeaderValue::from_static("pending-run"))
    );
    let mut body = response.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_secs(1), body.next())
        .await
        .expect("immediate stream comment")
        .expect("body remains open")
        .expect("stream body chunk");
    assert!(String::from_utf8_lossy(&first).contains("stream active"));

    assert!(state.request_stream_cancellation("pending-run"));
    let snapshot = wait_for_status(&state, "pending-run", "cancelled").await;
    assert_eq!(snapshot.latest_sequence, 1);
    assert_eq!(state.gates.stream_continuity_in_flight.active_count(), 0);
    let cancellation_event = tokio::time::timeout(Duration::from_secs(1), body.next())
        .await
        .expect("cancellation event arrives")
        .expect("cancellation event is present")
        .expect("cancellation event body chunk");
    assert!(String::from_utf8_lossy(&cancellation_event).contains("cancelled by user"));
}

#[tokio::test]
async fn application_shutdown_stops_pending_continuity_work() {
    let (_database, state) = test_state().await;
    create_run(&state.db, "shutdown-run", "owner")
        .await
        .expect("create stream run");
    let (sender, receiver) = mpsc::channel(1);
    let cancellation = state.register_stream_cancellation("shutdown-run").await;
    let permit = state
        .gates
        .stream_continuity_in_flight
        .clone()
        .try_acquire_owned()
        .expect("continuity slot");
    start_background_request(
        state.clone(),
        "shutdown-run".to_owned(),
        std::future::pending::<Response>(),
        sender,
        cancellation,
        permit,
        BackgroundRequestSettings {
            request_timeout: Some(Duration::from_secs(60)),
            analytics: None,
        },
    )
    .await;

    let response = live_response(
        receiver,
        "shutdown-run",
        "request-id",
        state.shutdown_receiver(),
    );
    let mut body = response.into_body().into_data_stream();
    let first = body
        .next()
        .await
        .expect("initial stream comment")
        .expect("initial stream chunk");
    assert!(String::from_utf8_lossy(&first).contains("stream active"));

    state.request_shutdown();

    let next = tokio::time::timeout(Duration::from_secs(1), body.next())
        .await
        .expect("live stream stops during application shutdown");
    assert!(next.is_none());
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if state.gates.stream_continuity_in_flight.active_count() == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("continuity slot is released during application shutdown");
    assert!(!state.request_stream_cancellation("shutdown-run"));
}

#[tokio::test]
async fn provider_setup_failure_is_replayable_as_an_sse_error() {
    let (_database, state) = test_state().await;
    create_run(&state.db, "setup-failed-run", "owner")
        .await
        .expect("create stream run");
    let (sender, receiver) = mpsc::channel(1);
    let cancellation = state.register_stream_cancellation("setup-failed-run").await;
    let permit = state
        .gates
        .stream_continuity_in_flight
        .clone()
        .try_acquire_owned()
        .expect("continuity slot");
    start_background_request(
        state.clone(),
        "setup-failed-run".to_owned(),
        async {
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({
                    "error": {"message": "provider setup failed"}
                })),
            )
                .into_response()
        },
        sender,
        cancellation,
        permit,
        BackgroundRequestSettings {
            request_timeout: Some(Duration::from_secs(1)),
            analytics: None,
        },
    )
    .await;

    let body = to_bytes(
        live_response(
            receiver,
            "setup-failed-run",
            "request-id",
            state.shutdown_receiver(),
        )
        .into_body(),
        4096,
    )
    .await
    .expect("setup error stream body");
    let body = String::from_utf8_lossy(&body);
    assert!(body.contains("event: error"), "{body}");
    assert!(body.contains("provider setup failed"), "{body}");
    let snapshot = load_run(&state.db, "setup-failed-run", "owner")
        .await
        .expect("load run")
        .expect("run exists");
    assert_eq!(snapshot.status, "failed");
    let replay = to_bytes(
        resume_response(state, "setup-failed-run".to_owned(), "owner".to_owned(), 0).into_body(),
        4096,
    )
    .await
    .expect("setup error replay");
    assert!(String::from_utf8_lossy(&replay).contains("provider setup failed"));
}

#[tokio::test]
async fn replay_journal_marks_storage_limit_without_stopping_live_task() {
    let (_database, state) = test_state().await;
    create_run(&state.db, "bounded-run", "owner")
        .await
        .expect("create stream run");
    let payload = Bytes::from(vec![
        b'x';
        UpstreamSettings::default()
            .continuity_replay_bytes_per_run
            + 1
    ]);
    let sequence = append_event(&state.db, "bounded-run", &payload)
        .await
        .expect("sequence even when the event is not retained");
    assert_eq!(sequence, 1);
    let snapshot = load_run(&state.db, "bounded-run", "owner")
        .await
        .expect("load stream run")
        .expect("run exists");
    assert!(!snapshot.replay_complete);
    assert_eq!(snapshot.retained_through, 0);
    assert_eq!(snapshot.latest_sequence, 1);
}

#[tokio::test]
async fn replay_runs_expire_after_one_day() {
    let (_database, state) = test_state().await;
    create_run(&state.db, "expired-run", "owner")
        .await
        .expect("create stream run");
    let expired_at = crate::infra::db::utc_timestamp_before(Duration::from_secs(2 * 24 * 60 * 60))
        .expect("old stream timestamp");
    state
        .db
        .write(move |transaction| {
            let mut run = transaction
                .get::<Record>(Table::StreamRuns, "expired-run")?
                .ok_or(StorageError::NotFound)?;
            run.insert("created_at", Field::Text(expired_at.clone()));
            transaction.put(Table::StreamRuns, "expired-run", &run)
        })
        .await
        .expect("expire stream run fixture");

    assert!(
        load_run(&state.db, "expired-run", "owner")
            .await
            .expect("load expired stream run")
            .is_none()
    );
}

/// Seeds `count` derived stream rows with uniform metadata for the capacity
/// and recovery regressions. `status` and `age` control whether the rows are
/// eligible for age-based pruning or restart recovery.
async fn seed_stream_runs(
    db: &crate::infra::storage::Database,
    count: usize,
    api_key_id: &str,
    status: &str,
    age: Duration,
    retained_bytes: i64,
) {
    let created_at = if age.is_zero() {
        crate::infra::db::utc_timestamp_now().expect("now")
    } else {
        crate::infra::db::utc_timestamp_before(age).expect("aged timestamp")
    };
    let api_key_id = api_key_id.to_owned();
    let status = status.to_owned();
    db.write(move |transaction| {
        for index in 0..count {
            let run_id = format!("seed-{index:04}");
            let record = Record::new()
                .with("id", Field::Text(run_id.clone()))
                .with("api_key_id", Field::Text(api_key_id.clone()))
                .with("status", Field::Text(status.clone()))
                .with("created_at", Field::Text(created_at.clone()))
                .with("latest_sequence", Field::I64(1))
                .with("retained_through", Field::I64(1))
                .with("retained_bytes", Field::I64(retained_bytes))
                .with("replay_complete", Field::Bool(true))
                .with("error", Field::Null)
                .with("finished_at", Field::Text(created_at.clone()));
            transaction.put(Table::StreamRuns, &run_id, &record)?;
            transaction.put(
                Table::StreamEvents,
                &format!("{run_id}/00000000000000000001"),
                &Record::new().with("sequence", Field::I64(1)).with(
                    "payload",
                    Field::Bytes(vec![0; usize::try_from(retained_bytes).unwrap_or(0)]),
                ),
            )?;
            transaction.put(
                Table::StreamRunApiKeyIndex,
                &format!("{api_key_id}/{run_id}"),
                &run_id.to_owned(),
            )?;
        }
        Ok(())
    })
    .await
    .expect("seed stream runs");
}

#[tokio::test]
async fn create_run_prunes_over_cap_expired_runs_before_checking_capacity() {
    let (_database, state) = test_state().await;
    seed_stream_runs(
        &state.db,
        1_050,
        "owner",
        "completed",
        Duration::from_secs(2 * 24 * 60 * 60),
        16,
    )
    .await;

    create_run(&state.db, "fresh-run", "owner")
        .await
        .expect("over-cap expired runs are pruned before the capacity gate");
    let snapshot = load_run(&state.db, "fresh-run", "owner")
        .await
        .expect("load fresh run")
        .expect("fresh run exists");
    assert_eq!(snapshot.status, "running");

    let remaining = state
        .db
        .read(|transaction| transaction.scan_prefix::<Record>(Table::StreamRuns, "", 2_000))
        .await
        .expect("count remaining runs");
    assert!(remaining.len() < 1_000, "pruning ran, cap is no longer hit");
}

#[tokio::test]
async fn startup_recovery_processes_all_runs_and_reconciles_retained_bytes() {
    let (_database, state) = test_state().await;
    const RUNS: usize = 1_100;
    const BYTES: i64 = 32;
    seed_stream_runs(&state.db, RUNS, "owner", "running", Duration::ZERO, BYTES).await;

    recover_abandoned_stream_runs(&state.db)
        .await
        .expect("paginated startup recovery");

    let (runs, counter) = state
        .db
        .read(|transaction| {
            Ok((
                transaction.scan_prefix::<Record>(Table::StreamRuns, "seed-", RUNS + 10)?,
                transaction
                    .get::<i64>(Table::Meta, STREAM_RETAINED_BYTES_KEY)?
                    .unwrap_or(0),
            ))
        })
        .await
        .expect("read recovery results");
    assert_eq!(runs.len(), RUNS);
    assert!(
        runs.iter()
            .all(|(_, record)| record.text("status").expect("status") != "running"),
        "every run beyond the first page was recovered"
    );
    assert_eq!(counter, i64::try_from(RUNS).unwrap() * BYTES);
}

#[tokio::test]
async fn batched_appends_preserve_order_and_track_retained_bytes() {
    let (_database, state) = test_state().await;
    create_run(&state.db, "batch-a", "owner")
        .await
        .expect("run a");
    create_run(&state.db, "batch-b", "owner")
        .await
        .expect("run b");
    let upstream = UpstreamSettings::default();
    let journal_a = JournalBatch::start(state.db.clone());
    let journal_b = JournalBatch::start(state.db.clone());

    let payloads: Vec<Bytes> = (1..=20)
        .flat_map(|index| {
            [
                Bytes::from(format!("data: a{index}\n\n")),
                Bytes::from(format!("data: b{index}\n\n")),
            ]
        })
        .collect();
    let mut appends = Vec::with_capacity(payloads.len());
    for (index, payload) in payloads.iter().enumerate() {
        let (run_id, journal) = if index % 2 == 0 {
            ("batch-a", &journal_a)
        } else {
            ("batch-b", &journal_b)
        };
        appends.push(journal.append(run_id, payload, upstream));
    }
    for result in futures_util::future::join_all(appends).await {
        result.expect("batched append commits");
    }
    journal_a
        .finish("batch-a", "completed", None, upstream)
        .await
        .expect("finish run a");
    journal_b
        .finish("batch-b", "completed", None, upstream)
        .await
        .expect("finish run b");

    let mut expected_total = 0usize;
    for run_id in ["batch-a", "batch-b"] {
        let mut after = 0i64;
        let mut events = Vec::new();
        loop {
            let page = read_events_with_page_size(&state.db, run_id, after, upstream)
                .await
                .expect("read journal page");
            if page.is_empty() {
                break;
            }
            after = page.last().map(|(sequence, _)| *sequence).unwrap_or(after);
            events.extend(page);
        }
        assert_eq!(events.len(), 20);
        for (index, (sequence, payload)) in events.into_iter().enumerate() {
            assert_eq!(sequence, i64::try_from(index + 1).unwrap());
            expected_total = expected_total.saturating_add(payload.len());
        }
        let snapshot = load_run(&state.db, run_id, "owner")
            .await
            .expect("load run")
            .expect("run exists");
        assert_eq!(snapshot.status, "completed");
        assert_eq!(snapshot.latest_sequence, 20);
        assert_eq!(snapshot.retained_through, 20);
    }
    let counter = state
        .db
        .read(|transaction| {
            transaction
                .get::<i64>(Table::Meta, STREAM_RETAINED_BYTES_KEY)
                .map(|value| value.unwrap_or(0))
        })
        .await
        .expect("read counter");
    assert_eq!(
        usize::try_from(counter).expect("counter fits usize"),
        expected_total,
        "Meta counter equals the sum of all retained event payloads"
    );
}

#[tokio::test]
async fn shared_journal_coalesces_across_streams_and_isolates_a_bad_op() {
    let (_database, state) = test_state().await;
    create_run(&state.db, "shared-a", "owner")
        .await
        .expect("run a");
    create_run(&state.db, "shared-b", "owner")
        .await
        .expect("run b");
    let journal = state.continuity_journal.clone();
    let upstream = UpstreamSettings::default();

    let mut workers = Vec::new();
    for run in ["shared-a", "shared-b"] {
        let journal = journal.clone();
        workers.push(tokio::spawn(async move {
            let mut sequences = Vec::new();
            for index in 1..=8 {
                let payload = Bytes::from(format!("data: {run}-{index}\n\n"));
                sequences.push(
                    journal
                        .append(run, &payload, upstream)
                        .await
                        .expect("append through the shared journal"),
                );
            }
            sequences
        }));
    }

    // A logical failure for one run must be reported only to this caller and
    // must not abort healthy ops queued around it.
    let rejected = journal
        .append(
            "missing-run",
            &Bytes::from_static(b"data: orphan\n\n"),
            upstream,
        )
        .await;
    assert!(rejected.is_err(), "a missing run yields a per-op error");

    let mut handles = Vec::new();
    for worker in workers {
        let sequences = worker.await.expect("worker completes");
        assert_eq!(
            sequences,
            (1..=8).collect::<Vec<i64>>(),
            "per-run FIFO sequence numbers remain contiguous"
        );
        handles.push(sequences.len());
    }
    assert_eq!(handles, vec![8, 8]);

    journal
        .finish("shared-a", "completed", None, upstream)
        .await
        .expect("finish run a");
    journal
        .finish("shared-b", "completed", None, upstream)
        .await
        .expect("finish run b");

    for run in ["shared-a", "shared-b"] {
        let mut after = 0i64;
        let mut events = Vec::new();
        loop {
            let page = read_events_with_page_size(&state.db, run, after, upstream)
                .await
                .expect("read replay page");
            if page.is_empty() {
                break;
            }
            after = page.last().map(|(sequence, _)| *sequence).unwrap_or(after);
            events.extend(page);
        }
        assert_eq!(events.len(), 8, "both runs retained every event");
        assert_eq!(
            events
                .iter()
                .map(|(sequence, _)| *sequence)
                .collect::<Vec<_>>(),
            (1..=8).collect::<Vec<i64>>()
        );
        let snapshot = load_run(&state.db, run, "owner")
            .await
            .expect("load run")
            .expect("run exists");
        assert_eq!(snapshot.status, "completed");
        assert_eq!(snapshot.latest_sequence, 8);
    }
}
