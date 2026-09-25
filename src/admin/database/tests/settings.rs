use super::support::*;
use super::*;

#[tokio::test]
async fn corrupt_output_styles_backup_is_rejected_without_changing_live_state() {
    let (database, mut payload) = route_backup_fixture().await;
    crate::infra::db::save_output_styles(
        &database.db,
        vec![crate::support::output_styles::OutputStyleSelection {
            id: crate::support::output_styles::OutputStyleId::TerseProse,
            level: crate::support::output_styles::OutputStyleLevel::Full,
        }],
        0,
        true,
    )
    .await
    .expect("set live output styles");
    let live_styles = crate::infra::db::load_output_styles(&database.db)
        .await
        .expect("read live output styles");

    let styles_entry = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::OutputStyles)
        .expect("backup output styles entry");
    let mut record: Record =
        bincode::deserialize(&styles_entry.value).expect("decode output styles record");
    record.insert("format_version", Field::I64(999));
    styles_entry.value = bincode::serialize(&record).expect("encode corrupt styles record");
    let path = temporary_backup_path(&database, "corrupt-output-styles.exoroute");
    fs::write(
        &path,
        encode_backup(payload.entries).expect("encode corrupt backup"),
    )
    .expect("write corrupt backup");

    assert!(restore_application_data(&database.db, &path).await.is_err());
    assert_eq!(
        crate::infra::db::load_output_styles(&database.db)
            .await
            .expect("live output styles remain readable"),
        live_styles
    );
    assert!(
        database
            .db
            .read(|transaction| transaction.get::<Record>(Table::Providers, "backup-provider"))
            .await
            .expect("read live provider after rejected import")
            .is_some()
    );
}

#[tokio::test]
async fn legacy_backup_without_output_styles_preserves_current_value_and_advances_revision() {
    let database = TestDatabase::open().await;
    let password_hash =
        crate::admin::hash_admin_password("a-valid-test-password-for-legacy-output-style-backup")
            .expect("hash backup admin password");
    database
        .db
        .write(move |transaction| {
            transaction.put(
                Table::AdminAuth,
                "singleton",
                &Record::new()
                    .with("password_hash", Field::Text(password_hash.clone()))
                    .with("must_change_password", Field::Bool(false))
                    .with("updated_at", Field::Text("2026-01-01 00:00:00".to_owned())),
            )
        })
        .await
        .expect("seed admin auth state");

    crate::infra::db::save_output_styles(
        &database.db,
        vec![crate::support::output_styles::OutputStyleSelection {
            id: crate::support::output_styles::OutputStyleId::LessCode,
            level: crate::support::output_styles::OutputStyleLevel::Lite,
        }],
        0,
        true,
    )
    .await
    .expect("save source output styles");
    let mut legacy_entries = database
        .db
        .snapshot_entries(false, 1024 * 1024)
        .await
        .expect("snapshot legacy backup");
    legacy_entries.retain(|entry| entry.table != Table::OutputStyles);
    let path = temporary_backup_path(&database, "legacy-without-output-styles.exoroute");
    fs::write(
        &path,
        encode_backup(legacy_entries).expect("encode legacy backup"),
    )
    .expect("write legacy backup");

    let current_styles = vec![crate::support::output_styles::OutputStyleSelection {
        id: crate::support::output_styles::OutputStyleId::TerseProse,
        level: crate::support::output_styles::OutputStyleLevel::Ultra,
    }];
    crate::infra::db::save_output_styles(&database.db, current_styles.clone(), 1, true)
        .await
        .expect("save current output styles after backup");
    restore_application_data(&database.db, &path)
        .await
        .expect("restore legacy backup without output styles");

    let restored = crate::infra::db::load_output_styles(&database.db)
        .await
        .expect("read preserved output styles");
    assert_eq!(restored.styles, current_styles);
    assert_eq!(restored.revision, 3);
    assert!(restored.overridden);
}

#[tokio::test]
async fn request_log_snapshot_option_and_key_usage_redaction_are_applied() {
    let database = TestDatabase::open().await;
    crate::support::test_support::seed_api_key(&database.db, "key", "Key", "known-test-token")
        .await
        .expect("seed gateway key");
    let now = crate::infra::db::utc_timestamp_now().expect("timestamp");
    database
        .db
        .write(move |transaction| {
            transaction.put(
                Table::ApiKeys,
                "key",
                &Record::new()
                    .with("id", Field::Text("key".to_owned()))
                    .with("name", Field::Text("Key".to_owned()))
                    .with(
                        "token_hash",
                        Field::Bytes(crate::security::token_hash("known-test-token")),
                    )
                    .with("enabled", Field::Bool(true))
                    .with("created_at", Field::Text(now.clone()))
                    .with("last_used_at", Field::Null)
                    .with("request_count", Field::I64(41))
                    .with("request_count_generation", Field::I64(0)),
            )?;
            transaction.put(
                Table::RequestLogs,
                "log",
                &Record::new()
                    .with("id", Field::Text("log".to_owned()))
                    .with("request_id", Field::Text("request".to_owned()))
                    .with("route_alias", Field::Text("route".to_owned()))
                    .with("model", Field::Text("model".to_owned()))
                    .with(
                        "client_protocol",
                        Field::Text("chat_completions".to_owned()),
                    )
                    .with("status", Field::I64(200))
                    .with("duration_ms", Field::I64(1))
                    .with("created_at", Field::Text(now.clone())),
            )?;
            Ok(())
        })
        .await
        .expect("seed backup data");

    let without_logs = database
        .db
        .snapshot_entries(false, 1024 * 1024)
        .await
        .expect("snapshot without request logs");
    let with_logs = database
        .db
        .snapshot_entries(true, 1024 * 1024)
        .await
        .expect("snapshot with request logs");
    assert!(
        !without_logs
            .iter()
            .any(|entry| entry.table == Table::RequestLogs)
    );
    assert!(
        with_logs
            .iter()
            .any(|entry| entry.table == Table::RequestLogs)
    );
    let api_key = without_logs
        .iter()
        .find(|entry| entry.table == Table::ApiKeys && entry.key == "key")
        .expect("key record");
    let record: Record = bincode::deserialize(&api_key.value).expect("key record decode");
    assert_eq!(record.integer("request_count").expect("request count"), 0);
    assert!(
        !api_key
            .value
            .windows(b"known-test-token".len())
            .any(|window| window == b"known-test-token")
    );
}
