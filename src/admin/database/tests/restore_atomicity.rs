use super::support::*;
use super::*;

#[test]
fn backup_restore_rebuilds_secondary_indexes_from_primary_records() {
    let token_hash = vec![0x4a; 32];
    let api_key = Record::new()
        .with("id", Field::Text("key-id".to_owned()))
        .with("name", Field::Text("Backup key".to_owned()))
        .with("token_hash", Field::Bytes(token_hash.clone()))
        .with("enabled", Field::Bool(true))
        .with("created_at", Field::Text("2026-09-14T00:00:00Z".to_owned()));
    let mut entries = vec![
        SnapshotEntry {
            table: Table::ApiKeys,
            key: "key-id".to_owned(),
            value: bincode::serialize(&api_key).expect("serialize API key"),
        },
        SnapshotEntry {
            table: Table::ApiKeyTokenIndex,
            key: "token/stale".to_owned(),
            value: bincode::serialize(&"wrong-id".to_owned()).expect("serialize stale index"),
        },
    ];

    rebuild_backup_secondary_indexes(&mut entries).expect("rebuild indexes");

    let token_index = super::api_keys::api_key_token_index_key(&token_hash);
    assert!(entries.iter().any(|entry| {
        entry.table == Table::ApiKeyTokenIndex
            && entry.key == token_index
            && bincode::deserialize::<String>(&entry.value)
                .map(|value| value == "key-id")
                .unwrap_or(false)
    }));
    assert!(
        !entries
            .iter()
            .any(|entry| { entry.table == Table::ApiKeyTokenIndex && entry.key == "token/stale" })
    );
    assert!(entries.iter().any(|entry| {
        entry.table == Table::ApiKeyIndex
            && bincode::deserialize::<String>(&entry.value)
                .map(|value| value == "key-id")
                .unwrap_or(false)
    }));
}

#[test]
fn backup_restore_rebuilds_pending_provider_deletion_marker() {
    let provider = Record::new()
        .with("id", Field::Text("provider-1".to_owned()))
        .with("name", Field::Text("Provider".to_owned()))
        .with("model_prefix", Field::Text("provider-1".to_owned()))
        .with("enabled", Field::Bool(false))
        .with("deleting", Field::Bool(true))
        .with("deletion_phase", Field::Text("models".to_owned()));
    let expected_key = super::providers::provider_deletion_index_key("provider-1")
        .expect("valid provider deletion index key");
    let mut entries = vec![
        SnapshotEntry {
            table: Table::Providers,
            key: "provider-1".to_owned(),
            value: bincode::serialize(&provider).expect("serialize provider"),
        },
        SnapshotEntry {
            table: Table::Indexes,
            key: "provider_deleting/stale".to_owned(),
            value: bincode::serialize(&"wrong-provider".to_owned()).expect("serialize stale index"),
        },
    ];

    rebuild_backup_secondary_indexes(&mut entries).expect("rebuild secondary indexes");

    assert!(entries.iter().any(|entry| {
        entry.table == Table::Indexes
            && entry.key == expected_key
            && bincode::deserialize::<String>(&entry.value)
                .is_ok_and(|provider_id| provider_id == "provider-1")
    }));
    assert!(
        !entries.iter().any(|entry| {
            entry.table == Table::Indexes && entry.key == "provider_deleting/stale"
        })
    );
}

#[tokio::test]
async fn backup_restore_is_atomic_and_excludes_sessions_and_stream_state() {
    let database = TestDatabase::open().await;
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "backup-provider",
            name: "Backup provider",
            base_url: "https://1.1.1.1/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "backup-provider",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed source provider");
    let backup_password_hash =
        crate::admin::hash_admin_password("backup-password-is-only-a-test-fixture")
            .expect("hash test admin password");
    database
        .db
        .write(move |transaction| {
            transaction.put(
                Table::AdminAuth,
                "singleton",
                &Record::new()
                    .with("password_hash", Field::Text(backup_password_hash.clone()))
                    .with("must_change_password", Field::Bool(false))
                    .with("updated_at", Field::Text("2026-01-01 00:00:00".to_owned())),
            )?;
            transaction.put(
                Table::AdminSessionFamilies,
                "active-family",
                &Record::new().with("id", Field::Text("active-family".to_owned())),
            )?;
            transaction.put(
                Table::StreamRuns,
                "temporary-run",
                &Record::new().with("id", Field::Text("temporary-run".to_owned())),
            )?;
            transaction.put(
                Table::StreamRunApiKeyIndex,
                "stream_run_by_api_key/owner/temporary-run",
                &"temporary-run".to_owned(),
            )?;
            Ok(())
        })
        .await
        .expect("seed runtime-only records");
    crate::infra::db::save_output_styles(
        &database.db,
        vec![crate::support::output_styles::OutputStyleSelection {
            id: crate::support::output_styles::OutputStyleId::LessCode,
            level: crate::support::output_styles::OutputStyleLevel::Full,
        }],
        0,
        true,
    )
    .await
    .expect("save output styles for backup");

    let entries = database
        .db
        .snapshot_entries(false, 1024 * 1024)
        .await
        .expect("snapshot without logs");
    assert!(entries.iter().all(|entry| !matches!(
        entry.table,
        Table::AdminSessionFamilies
            | Table::AdminRefreshTokens
            | Table::SessionIndex
            | Table::StreamRuns
            | Table::StreamEvents
            | Table::StreamRunApiKeyIndex
    )));
    let archive = encode_backup(entries).expect("encode backup");
    let backup_path = temporary_backup_path(&database, "backup.exoroute");
    fs::write(&backup_path, archive).expect("write backup file");

    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "later-provider",
            name: "Later provider",
            base_url: "https://1.1.1.1/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "later-provider",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed later provider");
    restore_application_data(&database.db, &backup_path)
        .await
        .expect("restore archive");

    let ids = database
        .db
        .read(|transaction| {
            transaction
                .scan_prefix::<Record>(Table::Providers, "", 10)?
                .into_iter()
                .map(|(_, record)| record.text("id").map(str::to_owned))
                .collect::<Result<Vec<_>, _>>()
        })
        .await
        .expect("read restored providers");
    assert_eq!(ids, ["backup-provider"]);
    assert_eq!(
        crate::infra::db::admin_password_change_required(&database.db)
            .await
            .expect("read restored password-change state"),
        Some(false)
    );
    let no_logs = database
        .db
        .snapshot_entries(false, 1024 * 1024)
        .await
        .expect("snapshot without logs");
    assert!(
        !no_logs
            .iter()
            .any(|entry| entry.table == Table::RequestLogs)
    );
    assert!(
        database
            .db
            .read(|transaction| transaction.get::<Record>(Table::StreamRuns, "temporary-run"))
            .await
            .expect("read stream state")
            .is_none()
    );
    assert!(
        database
            .db
            .read(|transaction| transaction
                .get::<Record>(Table::AdminSessionFamilies, "active-family"))
            .await
            .expect("read sessions")
            .is_none()
    );
    let settings = crate::infra::db::load_operational_settings(&database.db)
        .await
        .expect("settings survive backup");
    assert_eq!(settings.revision, 0);
    let output_styles = crate::infra::db::load_output_styles(&database.db)
        .await
        .expect("output styles survive backup");
    assert_eq!(output_styles.revision, 2);
    assert_eq!(
        output_styles.styles,
        vec![crate::support::output_styles::OutputStyleSelection {
            id: crate::support::output_styles::OutputStyleId::LessCode,
            level: crate::support::output_styles::OutputStyleLevel::Full,
        }]
    );
}

#[tokio::test]
async fn restore_worker_reconciles_state_after_its_response_handle_is_dropped() {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    state
        .admin
        .admin_password_change_required
        .store(false, std::sync::atomic::Ordering::Release);
    crate::support::test_support::seed_provider(
        &state.db,
        crate::support::test_support::ProviderSeed {
            id: "restore-provider",
            name: "Restore provider",
            base_url: "https://1.1.1.1/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "restore-provider",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed backup provider");
    let password_hash = crate::admin::hash_admin_password("a-valid-test-password-for-restore")
        .expect("hash test password");
    state
        .db
        .write(move |transaction| {
            transaction.put(
                Table::AdminAuth,
                "singleton",
                &Record::new()
                    .with("password_hash", Field::Text(password_hash.clone()))
                    .with("must_change_password", Field::Bool(true))
                    .with("updated_at", Field::Text("2026-01-01 00:00:00".to_owned())),
            )?;
            transaction.put(
                Table::AdminSessionFamilies,
                "active-family",
                &Record::new().with("id", Field::Text("active-family".to_owned())),
            )?;
            Ok(())
        })
        .await
        .expect("seed auth state");
    state
        .save_output_styles(
            vec![crate::support::output_styles::OutputStyleSelection {
                id: crate::support::output_styles::OutputStyleId::LessCode,
                level: crate::support::output_styles::OutputStyleLevel::Full,
            }],
            0,
            true,
        )
        .await
        .expect("save output styles into backup");
    let entries = state
        .db
        .snapshot_entries(false, 1024 * 1024)
        .await
        .expect("snapshot backup data");
    let archive = encode_backup(entries).expect("encode valid backup");
    let temporary_directory =
        create_private_temp_dir(state.config.app_dir.as_path(), "restore-cancellation-test")
            .expect("create isolated backup directory");
    let backup_path = temporary_directory.join("backup.exoroute");
    fs::write(&backup_path, archive).expect("write backup");
    let cleanup = TempDirectory(temporary_directory);

    state
        .save_output_styles(
            vec![crate::support::output_styles::OutputStyleSelection {
                id: crate::support::output_styles::OutputStyleId::TerseProse,
                level: crate::support::output_styles::OutputStyleLevel::Lite,
            }],
            1,
            true,
        )
        .await
        .expect("change live output styles after backup");

    crate::support::test_support::seed_provider(
        &state.db,
        crate::support::test_support::ProviderSeed {
            id: "later-provider",
            name: "Later provider",
            base_url: "https://1.1.1.1/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "later-provider",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed data that restore should replace");

    state
        .admin
        .admin_password_change_required
        .store(true, std::sync::atomic::Ordering::Release);
    let auth_guard = state.admin.admin_auth_lock.clone().lock_owned().await;
    let import_permit = state
        .admin
        .admin_database_import_in_flight
        .clone()
        .acquire_owned()
        .await
        .expect("acquire import slot");
    let response_waiter = spawn_restore_reconciliation_task(
        state.clone(),
        backup_path,
        false,
        false,
        auth_guard,
        import_permit,
        cleanup,
    );
    drop(response_waiter);

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (restored_provider, later_provider, session_family) = state
                .db
                .read(|transaction| {
                    Ok((
                        transaction.get::<Record>(Table::Providers, "restore-provider")?,
                        transaction.get::<Record>(Table::Providers, "later-provider")?,
                        transaction.get::<Record>(Table::AdminSessionFamilies, "active-family")?,
                    ))
                })
                .await
                .expect("read restored state");
            if restored_provider.is_some()
                && later_provider.is_none()
                && session_family.is_none()
                && state.output_styles().styles
                    == vec![crate::support::output_styles::OutputStyleSelection {
                        id: crate::support::output_styles::OutputStyleId::LessCode,
                        level: crate::support::output_styles::OutputStyleLevel::Full,
                    }]
                && state.output_styles().revision == 3
                && state
                    .admin
                    .admin_password_change_required
                    .load(std::sync::atomic::Ordering::Acquire)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("detached restore worker finishes after response disconnect");
    assert_eq!(
        crate::infra::db::admin_password_change_required(&state.db)
            .await
            .expect("read restored authentication policy"),
        Some(true)
    );
    assert_eq!(
        crate::infra::db::load_output_styles(&state.db)
            .await
            .expect("read restored output styles"),
        state.output_styles()
    );
}
