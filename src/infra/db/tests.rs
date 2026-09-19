//! Domain regression tests for `infra::db`: bootstrap idempotency, index
//! repair, revision compare-and-swap, and process-lock behavior.

use super::settings_records::gateway_limits_from_record;
use super::*;
use crate::{
    config::{GatewayResourceLimits, OperationalSettings},
    infra::storage::{Database, Field, Record, StorageError, Table},
    support::output_styles::{self, OutputStyleSelection, OutputStylesSnapshot},
};
use std::{fs, io, time::Duration};

#[test]
fn legacy_gateway_limits_without_continuity_capacity_use_the_bounded_default() {
    let defaults = GatewayResourceLimits::default();
    let legacy = Record::new()
        .with(
            "gateway_body_limit_bytes",
            Field::I64(defaults.gateway_body_limit_bytes as i64),
        )
        .with(
            "gateway_body_processing_concurrency",
            Field::I64(defaults.gateway_body_processing_concurrency as i64),
        )
        .with(
            "sse_frame_limit_bytes",
            Field::I64(defaults.sse_frame_limit_bytes as i64),
        )
        .with(
            "sse_buffer_limit_bytes",
            Field::I64(defaults.sse_buffer_limit_bytes as i64),
        )
        .with(
            "provider_max_concurrency",
            Field::I64(defaults.provider_max_concurrency as i64),
        )
        .with(
            "stream_continuity_enabled",
            Field::Bool(defaults.stream_continuity_enabled),
        );

    assert_eq!(gateway_limits_from_record(&legacy).unwrap(), defaults);
}

fn temp_db_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("exoroute-lmdb-db-test-{}", uuid::Uuid::new_v4()))
}

#[tokio::test]
async fn initializes_lmdb_defaults_idempotently_and_reopens() {
    let path = temp_db_path();
    let database = connect(&path).await.expect("open LMDB");
    bootstrap::initialize_storage(&database)
        .await
        .expect("repeat initialization");
    assert_eq!(
        load_gateway_resource_limits(&database).await.unwrap(),
        GatewayResourceLimits::default()
    );
    assert_eq!(
        load_operational_settings(&database).await.unwrap().revision,
        0
    );
    assert_eq!(
        load_output_styles(&database).await.unwrap(),
        OutputStylesSnapshot::default()
    );
    drop(database);
    let reopened = connect(&path).await.expect("reopen LMDB");
    assert_eq!(
        load_operational_settings(&reopened).await.unwrap().revision,
        0
    );
    assert_eq!(
        load_output_styles(&reopened).await.unwrap(),
        OutputStylesSnapshot::default()
    );
    drop(reopened);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn repairs_request_log_indexes_before_the_first_requests_read() {
    let path = temp_db_path();
    let database = Database::open_for_test(&path, 4 * 1024 * 1024)
        .await
        .expect("open LMDB");
    bootstrap::initialize_storage(&database)
        .await
        .expect("initialize LMDB");
    database
        .write(|transaction| {
            transaction.put(
                Table::RequestLogs,
                "log-legacy",
                &Record::new()
                    .with("id", Field::Text("log-legacy".to_owned()))
                    .with("created_at", Field::Text("2026-09-15 07:10:00".to_owned())),
            )
        })
        .await
        .expect("seed legacy request record without indexes");
    drop(database);

    let reopened = connect_for_test(&path).await.expect("reopen LMDB");
    let indexes = reopened
        .read(|transaction| {
            Ok((
                transaction.get::<i64>(Table::Meta, "request_log_count")?,
                transaction.get::<String>(
                    Table::RequestLogIndex,
                    &request_log_index_key("2026-09-15 07:10:00", "log-legacy")?,
                )?,
                transaction.get::<String>(
                    Table::RequestLogIndex,
                    &request_log_desc_index_key("2026-09-15 07:10:00", "log-legacy")?,
                )?,
            ))
        })
        .await
        .expect("read repaired request indexes");
    assert_eq!(indexes.0, Some(1));
    assert_eq!(indexes.1.as_deref(), Some("log-legacy"));
    assert_eq!(indexes.2.as_deref(), Some("log-legacy"));

    drop(reopened);
    let _ = fs::remove_dir_all(path);
}

#[tokio::test]
async fn repairs_missing_request_log_indexes_even_when_format_marker_is_current() {
    let path = temp_db_path();
    let database = Database::open_for_test(&path, 4 * 1024 * 1024)
        .await
        .expect("open LMDB");
    bootstrap::initialize_storage(&database)
        .await
        .expect("initialize LMDB");
    database
        .write(|transaction| {
            transaction.put(
                Table::RequestLogs,
                "log-stale-index",
                &Record::new()
                    .with("id", Field::Text("log-stale-index".to_owned()))
                    .with("created_at", Field::Text("2026-09-15 07:11:00".to_owned())),
            )?;
            transaction.put(Table::Meta, "request_log_count", &1_i64)?;
            transaction.put(
                Table::Meta,
                request_logs::REQUEST_LOG_INDEX_FORMAT_KEY_FOR_TEST,
                &request_logs::REQUEST_LOG_INDEX_FORMAT_VERSION_FOR_TEST,
            )
        })
        .await
        .expect("seed request record with stale index metadata");
    drop(database);

    let reopened = connect_for_test(&path).await.expect("reopen LMDB");
    let indexes = reopened
        .read(|transaction| {
            Ok((
                transaction.get::<String>(
                    Table::RequestLogIndex,
                    &request_log_index_key("2026-09-15 07:11:00", "log-stale-index")?,
                )?,
                transaction.get::<String>(
                    Table::RequestLogIndex,
                    &request_log_desc_index_key("2026-09-15 07:11:00", "log-stale-index")?,
                )?,
            ))
        })
        .await
        .expect("read reconciled request indexes");
    assert_eq!(indexes.0.as_deref(), Some("log-stale-index"));
    assert_eq!(indexes.1.as_deref(), Some("log-stale-index"));

    drop(reopened);
    let _ = fs::remove_dir_all(path);
}

#[tokio::test]
async fn storage_v3_migration_creates_default_output_styles_record() {
    let path = temp_db_path();
    let database = connect_for_test(&path).await.expect("open initial LMDB");
    database
        .write(|transaction| {
            transaction.delete(Table::OutputStyles, SINGLETON_KEY)?;
            transaction.put(Table::Meta, "storage_format_version", &3_u32)
        })
        .await
        .expect("prepare v3 storage fixture");
    drop(database);

    let migrated = connect_for_test(&path).await.expect("migrate v3 storage");
    assert_eq!(
        migrated
            .read(|transaction| { transaction.get::<u32>(Table::Meta, "storage_format_version") })
            .await
            .expect("read migrated storage version"),
        Some(4)
    );
    assert_eq!(
        load_output_styles(&migrated).await.unwrap(),
        OutputStylesSnapshot::default()
    );
    drop(migrated);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn operational_settings_use_revision_compare_and_swap() {
    let path = temp_db_path();
    let database = connect(&path).await.expect("open LMDB");
    let updated = OperationalSettings {
        request_timeout: Duration::from_secs(600),
        ..OperationalSettings::default()
    };
    assert_eq!(
        save_operational_settings(&database, updated, 0, true)
            .await
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        save_operational_settings(&database, updated, 0, true)
            .await
            .unwrap(),
        None
    );
    let current = load_operational_settings(&database).await.unwrap();
    assert_eq!(current.revision, 1);
    assert!(current.overridden);
    assert_eq!(current.settings.request_timeout, Duration::from_secs(600));
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn output_styles_persist_and_use_revision_compare_and_swap() {
    let path = temp_db_path();
    let database = connect_for_test(&path).await.expect("open LMDB");
    let styles = vec![OutputStyleSelection {
        id: output_styles::OutputStyleId::Ponytail,
        level: output_styles::OutputStyleLevel::Ultra,
    }];
    let saved = save_output_styles(&database, styles.clone(), 0, true)
        .await
        .expect("save output styles")
        .expect("revision matches");
    assert_eq!(saved.revision, 1);
    assert!(saved.overridden);
    assert_eq!(saved.styles[0].id, output_styles::OutputStyleId::Ponytail);
    assert!(
        save_output_styles(&database, styles, 0, true)
            .await
            .expect("compare and swap")
            .is_none()
    );
    assert_eq!(
        load_output_styles(&database)
            .await
            .expect("load output styles"),
        saved
    );

    let reset = save_output_styles(&database, Vec::new(), 1, false)
        .await
        .expect("reset output styles")
        .expect("reset revision matches");
    assert_eq!(reset.revision, 2);
    assert!(!reset.overridden);
    assert!(reset.styles.is_empty());
    drop(database);
    let reopened = connect_for_test(&path)
        .await
        .expect("reopen output styles LMDB");
    assert_eq!(
        load_output_styles(&reopened)
            .await
            .expect("load reset output styles after reopen"),
        reset
    );
    drop(reopened);
    let _ = fs::remove_dir_all(&path);
}

#[tokio::test]
async fn corrupt_output_styles_record_fails_closed_on_load() {
    let path = temp_db_path();
    let database = connect_for_test(&path).await.expect("open LMDB");
    database
        .write(|transaction| {
            transaction.put(
                Table::OutputStyles,
                SINGLETON_KEY,
                &Record::new().with("format_version", Field::I64(999)),
            )
        })
        .await
        .expect("write corrupt fixture");
    assert!(matches!(
        load_output_styles(&database).await,
        Err(StorageError::Invalid(_))
    ));
    drop(database);
    let _ = fs::remove_dir_all(&path);
}

#[test]
fn database_process_lock_excludes_a_second_owner_and_releases_on_drop() {
    let root = temp_db_path();
    let path = root.join("exoroute.lmdb");
    let first = lock_database(&path).expect("acquire process lock");
    let second = lock_database(&path);
    assert!(matches!(
        second,
        Err(ref error) if error.kind() == io::ErrorKind::WouldBlock
    ));
    drop(first);
    let after_release = lock_database(&path).expect("reacquire released process lock");
    drop(after_release);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn database_process_lock_rejects_a_symlink_lock_file() {
    use std::os::unix::fs::symlink;

    let root = temp_db_path();
    fs::create_dir_all(&root).expect("create test root");
    let path = root.join("exoroute.lmdb");
    fs::create_dir_all(&path).expect("create environment directory");
    let target = root.join("target");
    std::fs::File::create(&target).expect("create symlink target");
    let lock_path = std::path::PathBuf::from(format!("{}.lock", path.display()));
    symlink(&target, &lock_path).expect("create symlink lock file");

    let result = lock_database(&path);
    assert!(matches!(
        result,
        Err(ref error) if error.kind() == io::ErrorKind::InvalidInput
    ));
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn database_lock_does_not_restrict_custom_path_parent_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_db_path();
    fs::create_dir_all(&root).expect("create custom database parent");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755))
        .expect("make parent intentionally shared");
    let path = root.join("custom.lmdb");

    let lock = lock_database(&path).expect("acquire custom database process lock");
    let parent_mode = fs::metadata(&root)
        .expect("read parent permissions")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(parent_mode, 0o755);
    assert!(Path::new(&format!("{}.lock", path.display())).is_file());
    assert!(!path.join(".exoroute-process.lock").exists());
    drop(lock);
    let _ = fs::remove_dir_all(root);
}
