//! Storage engine regression tests: transactions, capacity, resize, and
//! filesystem guards over the LMDB environment lifecycle.

use super::environment::{MAX_DATABASE_OPERATIONS, read_map_size_marker_for_test_support};
use super::*;
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
                transaction.put(Table::RequestLogs, &key, &payload)?;
            }
            Ok(())
        })
        .await
        .expect("automatic map growth");
    drop(database);

    let reopened = Database::open_for_test(&path, initial_map_size)
        .await
        .expect("open using persisted map size");
    let count = reopened
        .read(|transaction| transaction.scan_prefix::<Vec<u8>>(Table::RequestLogs, "", 901))
        .await
        .expect("read resized data")
        .len();
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
            transaction.put(Table::RequestLogs, "oversized", &oversized_value)
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
