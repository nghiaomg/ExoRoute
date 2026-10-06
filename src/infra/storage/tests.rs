//! Storage engine regression tests: transactions, capacity, resize, and
//! filesystem guards over the LMDB environment lifecycle.

use super::environment::{MAX_DATABASE_OPERATIONS, read_map_size_marker_for_test_support};
use super::*;
use crate::infra::storage::codec;
use crate::infra::storage::fs_guard::{resize_with_marker_cleanup, write_map_size_marker};
use crate::infra::storage::{Field, Record, Table};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "exoroute-storage-{label}-{}",
        uuid::Uuid::new_v4().simple()
    ))
}

#[tokio::test]
async fn aborted_write_is_invisible_and_named_databases_reopen() {
    let root = test_path("abort");
    let path = root.join("exoroute.lmdb");
    let database = Database::open_for_test(&path, 4 * 1024 * 1024)
        .await
        .expect("open environment");
    let rejected: Result<(), StorageError> = database
        .write(|transaction| {
            transaction.put(Table::Providers, "aborted", &Record::new())?;
            Err(StorageError::Invalid("abort test transaction".to_owned()))
        })
        .await;
    assert!(matches!(rejected, Err(StorageError::Invalid(_))));
    assert!(
        database
            .read(|transaction| transaction.get::<Record>(Table::Providers, "aborted"))
            .await
            .expect("read aborted key")
            .is_none()
    );
    drop(database);

    let reopened = Database::open_for_test(&path, 4 * 1024 * 1024)
        .await
        .expect("reopen environment");
    assert!(
        reopened
            .read(|transaction| transaction.get::<u32>(Table::Meta, "storage_format_version"))
            .await
            .expect("read storage version")
            .is_some()
    );
    drop(reopened);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn opening_custom_environment_does_not_restrict_parent_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let root = test_path("shared-parent");
    fs::create_dir_all(&root).expect("create shared parent");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755))
        .expect("set shared parent permissions");
    let environment = root.join("custom.lmdb");
    let database = Database::open_for_test(&environment, 4 * 1024 * 1024)
        .await
        .expect("open custom environment");

    let parent_mode = fs::metadata(&root)
        .expect("read shared parent permissions")
        .permissions()
        .mode()
        & 0o777;
    let environment_mode = fs::metadata(&environment)
        .expect("read environment permissions")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(parent_mode, 0o755);
    assert_eq!(environment_mode, 0o700);

    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn reverse_prefix_scan_returns_newest_entries_and_resumes_after_cursor() {
    let root = test_path("reverse-prefix-scan");
    let database = Database::open_for_test(&root.join("exoroute.lmdb"), 4 * 1024 * 1024)
        .await
        .expect("open environment");
    let entries = [
        ("request_log_by_time/2026-09-15 07:08:00/log-1", "log-1"),
        ("request_log_by_time/2026-09-15 07:09:00/log-2", "log-2"),
        ("request_log_by_time/2026-09-15 07:10:00/log-3", "log-3"),
    ];
    database
        .write(move |transaction| {
            for (key, value) in entries {
                transaction.put(Table::RequestLogIndex, key, &value)?;
            }
            Ok(())
        })
        .await
        .expect("seed time index");

    let first_page = database
        .read(|transaction| {
            transaction.scan_prefix_reverse_after::<String>(
                Table::RequestLogIndex,
                "request_log_by_time/",
                None,
                2,
            )
        })
        .await
        .expect("read newest entries");
    assert_eq!(
        first_page
            .iter()
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>(),
        ["log-3", "log-2"]
    );

    let cursor = first_page.last().map(|(key, _)| key.clone());
    let second_page = database
        .read(move |transaction| {
            transaction.scan_prefix_reverse_after::<String>(
                Table::RequestLogIndex,
                "request_log_by_time/",
                cursor.as_deref(),
                2,
            )
        })
        .await
        .expect("resume newest-entry scan");
    assert_eq!(
        second_page
            .iter()
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>(),
        ["log-1"]
    );

    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn concurrent_writers_commit_all_records_without_exposing_partial_state() {
    let root = test_path("writers");
    let database = Database::open_for_test(&root.join("exoroute.lmdb"), 4 * 1024 * 1024)
        .await
        .expect("open environment");
    let mut writers = tokio::task::JoinSet::new();
    for index in 0..24 {
        let database = database.clone();
        writers.spawn(async move {
            let key = format!("provider-{index:02}");
            database
                .write(move |transaction| {
                    let record = Record::new()
                        .with("id", Field::Text(key.clone()))
                        .with("name", Field::Text(key.clone()));
                    transaction.put(Table::Providers, &key, &record)
                })
                .await
        });
    }
    while let Some(result) = writers.join_next().await {
        result.expect("writer task").expect("writer commit");
    }
    let records = database
        .read(|transaction| transaction.scan_prefix::<Record>(Table::Providers, "", 25))
        .await
        .expect("read providers");
    assert_eq!(records.len(), 24);
    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn operation_capacity_fails_fast_and_releases_slots_after_cancellation() {
    let root = test_path("operation-capacity");
    let database = Database::open_for_test(&root.join("exoroute.lmdb"), 4 * 1024 * 1024)
        .await
        .expect("open environment");
    let barrier = Arc::new(std::sync::Barrier::new(MAX_DATABASE_OPERATIONS + 1));
    let finished = Arc::new(AtomicUsize::new(0));
    let (entered_sender, mut entered_receiver) =
        tokio::sync::mpsc::channel(MAX_DATABASE_OPERATIONS);
    let mut tasks = tokio::task::JoinSet::new();
    let mut cancelled_task = None;

    for index in 0..MAX_DATABASE_OPERATIONS {
        let database = database.clone();
        let barrier = barrier.clone();
        let finished = finished.clone();
        let entered_sender = entered_sender.clone();
        let abort_handle = tasks.spawn(async move {
            database
                .read(move |_| {
                    entered_sender.blocking_send(()).map_err(|_| {
                        StorageError::Task("test entry signal was closed".to_owned())
                    })?;
                    barrier.wait();
                    finished.fetch_add(1, Ordering::Release);
                    Ok(())
                })
                .await
        });
        if index == 0 {
            cancelled_task = Some(abort_handle);
        }
    }
    drop(entered_sender);
    for _ in 0..MAX_DATABASE_OPERATIONS {
        entered_receiver
            .recv()
            .await
            .expect("all operation slots are held");
    }

    assert!(matches!(
        database.read(|_| Ok(())).await,
        Err(StorageError::Busy)
    ));
    cancelled_task
        .expect("first operation abort handle")
        .abort();
    let release_barrier = barrier.clone();
    tokio::task::spawn_blocking(move || release_barrier.wait())
        .await
        .expect("release held LMDB operations");

    let mut observed_cancellation = false;
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok(Ok(())) => {}
            Err(error) if error.is_cancelled() => observed_cancellation = true,
            Err(error) => panic!("operation task failed: {error}"),
            Ok(Err(error)) => panic!("LMDB read failed: {error}"),
        }
    }
    assert!(observed_cancellation);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while finished.load(Ordering::Acquire) != MAX_DATABASE_OPERATIONS {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled blocking operation releases its slot");
    database
        .read(|_| Ok(()))
        .await
        .expect("capacity is available after operations finish");
    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn active_operations_counts_database_work_in_flight() {
    let root = test_path("active-operations");
    let path = root.join("exoroute.lmdb");
    let database = Database::open_for_test(&path, 8 * 1024 * 1024)
        .await
        .expect("open test database");
    assert_eq!(database.active_operations(), 0, "an idle database is empty");

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let (entered_sender, mut entered_receiver) = tokio::sync::mpsc::channel(1);
    let reader = tokio::spawn({
        let database = database.clone();
        let barrier = barrier.clone();
        async move {
            database
                .read(move |_| {
                    entered_sender.blocking_send(()).map_err(|_| {
                        StorageError::Task("test entry signal was closed".to_owned())
                    })?;
                    barrier.wait();
                    Ok(())
                })
                .await
        }
    });
    entered_receiver.recv().await.expect("reader entered");
    assert_eq!(database.active_operations(), 1, "the reader holds one slot");

    let release = barrier.clone();
    tokio::task::spawn_blocking(move || release.wait())
        .await
        .expect("release the held reader");
    reader.await.expect("join reader").expect("read completes");
    assert_eq!(database.active_operations(), 0, "the slot is released");
    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn map_growth_survives_reopen_and_stays_within_the_limit() {
    let root = test_path("map-growth");
    let path = root.join("exoroute.lmdb");
    let initial_map_size = 4 * 1024 * 1024;
    let database = Database::open_for_test(&path, initial_map_size)
        .await
        .expect("open small test map");
    let payload = vec![0x5a; 8 * 1024];
    database
        .write(move |transaction| {
            for index in 0..900 {
                let key = format!("payload-{index:04}");
                // Raw bytes: this test measures automatic map growth, and a
                // compressible payload would shrink before the map fills.
                transaction.put_raw(Table::RequestLogs, &key, &payload)?;
            }
            Ok(())
        })
        .await
        .expect("automatic map growth");
    drop(database);

    let reopened = Database::open_for_test(&path, initial_map_size)
        .await
        .expect("open using persisted map size");
    // Values were written raw above, so counting through the raw reader is
    // the layout-neutral way to verify every row survived the reopen.
    let count = reopened
        .read(|transaction| {
            let mut count = 0usize;
            for index in 0..900 {
                let key = format!("payload-{index:04}");
                if transaction.get_raw(Table::RequestLogs, &key)?.is_some() {
                    count += 1;
                }
            }
            Ok(count)
        })
        .await
        .expect("read resized data");
    assert_eq!(count, 900);
    drop(reopened);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn stale_map_full_waiter_does_not_grow_the_map_twice() {
    let root = test_path("stale-map-growth");
    let initial_map_size = 4 * 1024 * 1024;
    let database = Database::open_for_test(&root.join("exoroute.lmdb"), initial_map_size)
        .await
        .expect("open small test environment");

    database
        .grow_map(initial_map_size)
        .await
        .expect("first MapFull result grows the map");
    let grown_size = database.inner_for_test().map_size.load(Ordering::Acquire);
    assert_eq!(grown_size, initial_map_size * 2);

    database
        .grow_map(initial_map_size)
        .await
        .expect("stale MapFull result is coalesced");
    assert_eq!(
        database.inner_for_test().map_size.load(Ordering::Acquire),
        grown_size
    );

    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn failed_resize_removes_the_uncommitted_map_size_marker() {
    let root = test_path("failed-map-resize");
    fs::create_dir_all(&root).expect("create marker directory");
    let initial_map_size = 4 * 1024 * 1024;
    let next_map_size = initial_map_size * 2;
    write_map_size_marker(&root, next_map_size).expect("write resize-intent marker");

    let result = resize_with_marker_cleanup(&root, next_map_size, || {
        Err(StorageError::Io(io::Error::other(
            "simulated resize failure",
        )))
    });

    assert!(matches!(result, Err(StorageError::Io(_))));
    assert!(
        !root
            .join(format!(".exoroute-map-size-{next_map_size}"))
            .exists()
    );
    assert_eq!(
        read_map_size_marker_for_test_support(&root, initial_map_size, next_map_size)
            .expect("read map size after failed resize"),
        initial_map_size
    );
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn map_full_at_the_hard_limit_aborts_the_entire_write() {
    let root = test_path("map-limit");
    let path = root.join("exoroute.lmdb");
    let database = Database::open_with_map_bounds(&path, 4 * 1024 * 1024, 8 * 1024 * 1024)
        .await
        .expect("open bounded test environment");
    database
        .write(|transaction| transaction.put(Table::Providers, "retained", &"value"))
        .await
        .expect("seed retained value");

    let oversized_value = vec![0x5a; 12 * 1024 * 1024];
    let result = database
        .write(move |transaction| {
            // Raw bytes: this test measures map capacity, and a compressible
            // payload would shrink to a frame instead of exercising MapFull.
            transaction.put_raw(Table::RequestLogs, "oversized", &oversized_value)
        })
        .await;
    assert!(matches!(result, Err(StorageError::MapFull)));
    assert_eq!(
        database
            .read(|transaction| transaction.get::<String>(Table::Providers, "retained"))
            .await
            .expect("read retained value"),
        Some("value".to_owned())
    );
    assert!(
        database
            .read(|transaction| transaction.get::<Vec<u8>>(Table::RequestLogs, "oversized"))
            .await
            .expect("check aborted record")
            .is_none()
    );
    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_a_symlink_as_the_environment_directory() {
    use std::os::unix::fs::symlink;

    let root = test_path("symlink");
    fs::create_dir_all(&root).expect("create test root");
    let target = root.join("target");
    fs::create_dir(&target).expect("create target directory");
    let linked_environment = root.join("linked-environment");
    symlink(&target, &linked_environment).expect("create environment symlink");

    let result = Database::open_for_test(&linked_environment, 4 * 1024 * 1024).await;
    assert!(matches!(result, Err(StorageError::Invalid(_))));
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_a_symlink_in_an_ancestor_of_the_environment_directory() {
    use std::os::unix::fs::symlink;

    let root = test_path("symlink-ancestor");
    fs::create_dir_all(&root).expect("create test root");
    let target = root.join("target");
    fs::create_dir(&target).expect("create target directory");
    let linked_parent = root.join("linked-parent");
    symlink(&target, &linked_parent).expect("create ancestor symlink");

    let linked_environment = linked_parent.join("exoroute.lmdb");
    let result = Database::open_for_test(&linked_environment, 4 * 1024 * 1024).await;
    assert!(matches!(
        result,
        Err(StorageError::Io(_)) | Err(StorageError::Invalid(_))
    ));
    assert!(!target.join("exoroute.lmdb").exists());
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn refuses_symlinked_lmdb_files_before_opening_the_environment() {
    use std::os::unix::fs::symlink;

    for file_name in ["data.mdb", "lock.mdb"] {
        let root = test_path(file_name);
        let environment = root.join("exoroute.lmdb");
        fs::create_dir_all(&environment).expect("create environment directory");
        let target = root.join("outside-target");
        fs::write(&target, b"must remain untouched").expect("create external target");
        symlink(&target, environment.join(file_name)).expect("create database file symlink");

        let result = Database::open_for_test(&environment, 4 * 1024 * 1024).await;
        assert!(matches!(result, Err(StorageError::Invalid(_))));
        assert_eq!(
            fs::read(&target).expect("read external target"),
            b"must remain untouched"
        );
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn snapshot_validation_rejects_unbounded_records_and_fields() {
    let oversized = SnapshotEntry {
        table: Table::Providers,
        key: "provider".to_owned(),
        value: vec![0; MAX_SNAPSHOT_RECORD_BYTES + 1],
    };
    assert!(validate_snapshot_entries(&[oversized]).is_err());

    let mut record = Record::new();
    for index in 0..=MAX_RECORD_FIELDS {
        record.insert(format!("field-{index}"), Field::Null);
    }
    let too_many_fields = SnapshotEntry {
        table: Table::Providers,
        key: "provider".to_owned(),
        value: bincode::serialize(&record).expect("serialize record"),
    };
    assert!(validate_snapshot_entries(&[too_many_fields]).is_err());
}

#[test]
fn malformed_snapshot_keys_and_runtime_only_tables_are_rejected() {
    let malformed_key = SnapshotEntry {
        table: Table::Providers,
        key: "".to_owned(),
        value: bincode::serialize(&Record::new()).expect("serialize record"),
    };
    assert!(validate_snapshot_entries(&[malformed_key]).is_err());

    let runtime_session = SnapshotEntry {
        table: Table::AdminSessionFamilies,
        key: "family".to_owned(),
        value: bincode::serialize(&Record::new()).expect("serialize record"),
    };
    assert!(validate_snapshot_entries(&[runtime_session]).is_err());

    let runtime_stream_index = SnapshotEntry {
        table: Table::StreamRunApiKeyIndex,
        key: "stream_run_by_api_key/key/run".to_owned(),
        value: bincode::serialize(&"run".to_owned()).expect("serialize index"),
    };
    assert!(validate_snapshot_entries(&[runtime_stream_index]).is_err());
}

#[tokio::test]
async fn request_log_writes_compress_and_reads_decode_both_layouts() {
    let root = test_path("request-log-compression");
    let path = root.join("exoroute.lmdb");
    let database = Database::open_for_test(&path, 4 * 1024 * 1024)
        .await
        .expect("open environment");

    // A realistic request-log row: repeated field names plus a long error.
    let mut record = Record::new();
    for index in 0..20 {
        record.insert(format!("field-{index}"), Field::Text("value".repeat(16)));
    }
    record.insert(
        "error",
        Field::Text("upstream 500: internal error".repeat(20)),
    );
    let legacy_bytes = bincode::serialize(&record).expect("serialize legacy row");
    assert_ne!(legacy_bytes[0], codec::COMPRESSED_RECORD_PREFIX);
    assert!(legacy_bytes.len() > codec::COMPRESSED_RECORD_MIN_BYTES);

    let first_row = record.clone();
    database
        .write(move |transaction| {
            transaction.put(Table::RequestLogs, "compressed", &first_row)?;
            Ok(())
        })
        .await
        .expect("write compressed row");

    let stored = database
        .read(|transaction| {
            transaction
                .get_raw(Table::RequestLogs, "compressed")?
                .map(|row| vec![row])
                .ok_or_else(|| StorageError::Invalid("compressed row is missing".to_owned()))
        })
        .await
        .expect("read raw row");
    // The compressed row must be a frame and must be smaller than the raw
    // bincode payload for this repetitive record.
    assert!(
        stored[0][0] == codec::COMPRESSED_RECORD_PREFIX,
        "expected a compressed frame"
    );
    assert!(
        stored[0].len() < legacy_bytes.len(),
        "compression must shrink the row"
    );

    // Reads go through the same typed path the dashboard uses.
    let decoded_compressed = database
        .read(|transaction| transaction.get::<Record>(Table::RequestLogs, "compressed"))
        .await
        .expect("decode rows")
        .expect("compressed decode");
    assert_eq!(decoded_compressed, record);

    // A pre-compression raw bincode value must remain readable: the decode
    // path accepts the legacy layout without any rewrite.
    database
        .write(move |transaction| {
            transaction.put_raw(Table::RequestLogs, "old-build", &legacy_bytes)?;
            Ok(())
        })
        .await
        .expect("write legacy layout");
    let decoded_old = database
        .read(|transaction| transaction.get::<Record>(Table::RequestLogs, "old-build"))
        .await
        .expect("decode legacy layout")
        .expect("row present");
    assert_eq!(decoded_old, record);

    // The compressed row and the legacy row are the same logical record.
    assert_eq!(decoded_old, decoded_compressed);

    // Corrupt frames fail with a codec diagnostic, never a success fallback.
    let corrupt_frame = [codec::COMPRESSED_RECORD_PREFIX, 0x00, 0x01];
    database
        .write(move |transaction| {
            transaction.put_raw(Table::RequestLogs, "corrupt", &corrupt_frame)?;
            Ok(())
        })
        .await
        .expect("write corrupt frame");
    let corrupt = database
        .read(|transaction| transaction.get::<Record>(Table::RequestLogs, "corrupt"))
        .await;
    assert!(matches!(corrupt, Err(StorageError::Codec(_))));

    // Small payloads stay raw so tiny rows do not pay frame overhead.
    database
        .write(move |transaction| transaction.put(Table::RequestLogs, "tiny", &Record::new()))
        .await
        .expect("write tiny row");
    let tiny = database
        .read(|transaction| transaction.get_raw(Table::RequestLogs, "tiny"))
        .await
        .expect("read tiny row")
        .expect("tiny row present");
    assert_ne!(tiny[0], codec::COMPRESSED_RECORD_PREFIX);

    drop(database);
    let _ = fs::remove_dir_all(root);
}

fn repetitive_request_log_row(index: usize) -> Record {
    let mut row = Record::new();
    row.insert("id", Field::Text(format!("log-{index:06}")));
    row.insert("request_id", Field::Text(format!("request-{index:06}")));
    row.insert("route_alias", Field::Text("coding".to_owned()));
    row.insert(
        "model",
        Field::Text("anthropic/claude-sonnet-4.5".to_owned()),
    );
    row.insert(
        "client_protocol",
        Field::Text("chat_completions".to_owned()),
    );
    row.insert(
        "upstream_protocol",
        Field::Text("anthropic_messages".to_owned()),
    );
    row.insert("status", Field::I64(200));
    row.insert("duration_ms", Field::I64(800 + (index % 40) as i64 * 37));
    row.insert(
        "input_tokens",
        Field::I64(9_000 + (index % 90) as i64 * 111),
    );
    row.insert(
        "output_tokens",
        Field::I64(1_200 + (index % 50) as i64 * 83),
    );
    row.insert(
        "cost_micro_usd",
        Field::I64(50_000 + (index % 70) as i64 * 911),
    );
    row.insert("error", Field::Null);
    row.insert("created_at", Field::Text("2026-10-05 13:45:12".to_owned()));
    row
}

#[test]
fn dictionary_frames_require_the_dictionary_and_legacy_frames_stay_decodable() {
    let sample_record = repetitive_request_log_row(1);
    let payload = bincode::serialize(&sample_record).expect("serialize sample");
    let samples: Vec<&[u8]> = (0..8).map(|_| payload.as_slice()).collect();
    let dictionary = zstd::dict::from_samples(&samples, 8 * 1024).expect("train test dictionary");
    let leaked: &'static [u8] = Box::leak(dictionary.into_boxed_slice());

    let frame = codec::compress_with_dictionary(&payload, leaked).expect("dict compress");
    assert!(
        frame.len() < payload.len(),
        "dictionary must shrink the row"
    );

    // With the dictionary loaded the frame decodes...
    let restored = codec::decompress_frame(&frame, Some(leaked)).expect("dict decompress");
    assert_eq!(restored, payload);
    // ...and without it the same frame is rejected, never mis-decoded.
    assert!(codec::decompress_frame(&frame, None).is_err());
    // The full stored layout (marker + frame) fails the same way through the
    // public decode path when no dictionary is loaded.
    let mut stored = Vec::with_capacity(frame.len() + 1);
    stored.push(codec::COMPRESSED_RECORD_PREFIX);
    stored.extend_from_slice(&frame);
    assert!(codec::decode_request_log_bytes::<Record>(&stored, None).is_err());

    // A legacy dictionary-free frame still decodes when a dictionary is
    // loaded: zstd consults the dictionary only when a frame references it.
    let legacy_frame =
        zstd::bulk::compress(&payload, codec::COMPRESSED_RECORD_LEVEL).expect("plain compress");
    let restored_legacy =
        codec::decompress_frame(&legacy_frame, Some(leaked)).expect("legacy frame with dict");
    assert_eq!(restored_legacy, payload);
}

#[tokio::test]
async fn request_log_dictionary_trains_once_after_enough_rows() {
    let root = test_path("request-log-dictionary");
    let path = root.join("exoroute.lmdb");
    let database = Database::open_for_test(&path, 4 * 1024 * 1024)
        .await
        .expect("open environment");

    // Below the row threshold nothing is trained and nothing is stored.
    database
        .write(|transaction| {
            for index in 0..10 {
                transaction.put(
                    Table::RequestLogs,
                    &format!("log-{index:06}"),
                    &repetitive_request_log_row(index),
                )?;
            }
            Ok(())
        })
        .await
        .expect("seed few rows");
    assert!(
        !super::dictionary::train_and_store(&database)
            .await
            .expect("train gate")
    );
    let stored = database
        .read(|transaction| {
            transaction.get::<Vec<u8>>(Table::Meta, super::REQUEST_LOG_DICTIONARY_KEY)
        })
        .await
        .expect("read dictionary row")
        .is_none();
    assert!(stored, "no dictionary below the threshold");

    // Production keeps this counter in Meta alongside the logs; training
    // consults it instead of scanning the table for a row count.
    database
        .write(|transaction| {
            for index in 10..300 {
                transaction.put(
                    Table::RequestLogs,
                    &format!("log-{index:06}"),
                    &repetitive_request_log_row(index),
                )?;
            }
            transaction.put(Table::Meta, "request_log_count", &300_i64)?;
            Ok(())
        })
        .await
        .expect("seed rows");
    assert!(
        super::dictionary::train_and_store(&database)
            .await
            .expect("train")
    );
    let stored_dictionary = database
        .read(|transaction| {
            transaction.get::<Vec<u8>>(Table::Meta, super::REQUEST_LOG_DICTIONARY_KEY)
        })
        .await
        .expect("read dictionary")
        .expect("dictionary stored");
    assert!(!stored_dictionary.is_empty());
    assert!(
        stored_dictionary.len() <= super::REQUEST_LOG_DICTIONARY_MAX_BYTES,
        "dictionary stays within the size bound"
    );
    // The dictionary is trained once: a second call must not retrain or
    // rewrite it, so frames written with the first dictionary stay readable.
    assert!(
        !super::dictionary::train_and_store(&database)
            .await
            .expect("no retrain")
    );

    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn marker_valued_non_request_log_records_round_trip() {
    let root = test_path("marker-valued-values");
    let path = root.join("exoroute.lmdb");
    let database = Database::open_for_test(&path, 4 * 1024 * 1024)
        .await
        .expect("open environment");

    // Each of these raw bincode encodings starts with the compressed-frame
    // marker byte 0x5A. Only request logs may be decompressed, so every value
    // below must survive a round trip unchanged.
    database
        .write(|transaction| {
            transaction.put(Table::Meta, "request_log_count", &90_i64)?;
            transaction.put(Table::Meta, "statistics_generation", &346_i64)?;
            transaction.put(Table::Meta, "sentinel_u32", &90_u32)?;
            transaction.put(Table::Meta, "sentinel_blob", &vec![7_u8; 90])?;
            transaction.put(Table::ApiKeyIndex, "sentinel-index", &"x".repeat(90))?;
            Ok(())
        })
        .await
        .expect("write marker-valued raw records");

    let stored = database
        .read(|transaction| {
            Ok((
                transaction
                    .get::<i64>(Table::Meta, "request_log_count")?
                    .expect("counter"),
                transaction
                    .get::<i64>(Table::Meta, "statistics_generation")?
                    .expect("generation"),
                transaction
                    .get::<u32>(Table::Meta, "sentinel_u32")?
                    .expect("u32"),
                transaction
                    .get::<Vec<u8>>(Table::Meta, "sentinel_blob")?
                    .expect("blob"),
                transaction
                    .get::<String>(Table::ApiKeyIndex, "sentinel-index")?
                    .expect("index string"),
            ))
        })
        .await
        .expect("read marker-valued raw records");

    assert_eq!(stored.0, 90);
    assert_eq!(stored.1, 346);
    assert_eq!(stored.2, 90);
    assert_eq!(stored.3, vec![7_u8; 90]);
    assert_eq!(stored.4, "x".repeat(90));

    let raw_first_bytes = database
        .read(|transaction| {
            Ok((
                transaction
                    .get_raw(Table::Meta, "request_log_count")?
                    .expect("counter bytes")[0],
                transaction
                    .get_raw(Table::ApiKeyIndex, "sentinel-index")?
                    .expect("index bytes")[0],
            ))
        })
        .await
        .expect("read raw bytes");
    assert_eq!(raw_first_bytes.0, codec::COMPRESSED_RECORD_PREFIX);
    assert_eq!(raw_first_bytes.1, codec::COMPRESSED_RECORD_PREFIX);

    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn request_log_record_with_marker_field_count_is_compressed() {
    let root = test_path("marker-field-count");
    let path = root.join("exoroute.lmdb");
    let database = Database::open_for_test(&path, 4 * 1024 * 1024)
        .await
        .expect("open environment");

    // Exactly 90 fields makes the raw bincode map-length prefix equal to the
    // frame marker. The writer must compress instead of storing ambiguous raw
    // bytes, and the read path must return the original record.
    let mut record = Record::new();
    for index in 0..90 {
        record.insert(format!("f{index}"), Field::Null);
    }
    let expected = record.clone();
    database
        .write(move |transaction| transaction.put(Table::RequestLogs, "log-90", &record))
        .await
        .expect("write 90-field request log");

    let stored_first = database
        .read(|transaction| {
            Ok(transaction
                .get_raw(Table::RequestLogs, "log-90")?
                .expect("stored log")[0])
        })
        .await
        .expect("read stored log");
    assert_eq!(stored_first, codec::COMPRESSED_RECORD_PREFIX);

    let decoded = database
        .read(|transaction| transaction.get::<Record>(Table::RequestLogs, "log-90"))
        .await
        .expect("decode 90-field request log")
        .expect("log present");
    assert_eq!(decoded, expected);

    drop(database);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn snapshot_validation_decodes_request_logs_with_the_snapshots_own_dictionary() {
    let sample = repetitive_request_log_row(1);
    let payload = bincode::serialize(&sample).expect("serialize sample");
    let samples: Vec<&[u8]> = (0..8).map(|_| payload.as_slice()).collect();
    let dictionary = zstd::dict::from_samples(&samples, 8 * 1024).expect("train test dictionary");
    let leaked: &'static [u8] = Box::leak(dictionary.clone().into_boxed_slice());
    let frame = codec::compress_with_dictionary(&payload, leaked).expect("dict compress");
    let mut stored = Vec::with_capacity(frame.len() + 1);
    stored.push(codec::COMPRESSED_RECORD_PREFIX);
    stored.extend_from_slice(&frame);

    let with_dictionary = vec![
        SnapshotEntry {
            table: Table::Meta,
            key: super::REQUEST_LOG_DICTIONARY_KEY.to_owned(),
            value: bincode::serialize(&dictionary).expect("encode dictionary"),
        },
        SnapshotEntry {
            table: Table::RequestLogs,
            key: "log-1".to_owned(),
            value: stored.clone(),
        },
    ];
    validate_snapshot_entries(&with_dictionary)
        .expect("the snapshot's own dictionary decodes its frames");

    let without_dictionary = vec![SnapshotEntry {
        table: Table::RequestLogs,
        key: "log-1".to_owned(),
        value: stored,
    }];
    assert!(
        validate_snapshot_entries(&without_dictionary).is_err(),
        "a dictionary-required frame must not validate without its dictionary"
    );
}
