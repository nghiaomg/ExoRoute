use super::support::*;
use super::*;

#[test]
fn backup_header_checks_format_size_and_checksum() {
    let entries = vec![SnapshotEntry {
        table: Table::OperationalSettings,
        key: "singleton".to_owned(),
        value: bincode::serialize(&Record::new()).expect("encode test record"),
    }];
    let archive = encode_backup(entries).expect("encode backup");
    let payload = decode_backup(&archive).expect("decode backup");
    assert_eq!(payload.entries.len(), 1);
    assert!(decode_backup(b"SQLite format 3\0").is_err());
    let mut corrupted = archive;
    if let Some(last) = corrupted.last_mut() {
        *last ^= 0x40;
    }
    assert!(decode_backup(&corrupted).is_err());
}

#[test]
fn streamed_backup_writer_matches_the_in_memory_wire_format() {
    let root = std::env::temp_dir().join(format!(
        "exoroute-backup-writer-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).expect("create isolated backup folder");
    let destination = root.join("backup.exoroute");
    let entries = vec![SnapshotEntry {
        table: Table::OperationalSettings,
        key: "singleton".to_owned(),
        value: bincode::serialize(&Record::new()).expect("encode test record"),
    }];
    let expected = encode_backup(entries.clone()).expect("encode reference backup");
    let length = write_backup_archive(&destination, entries).expect("write streamed backup");
    let actual = fs::read(&destination).expect("read streamed backup");
    assert_eq!(length, actual.len() as u64);
    assert_eq!(actual, expected);
    assert_eq!(
        decode_backup_file(&destination)
            .expect("decode streamed backup")
            .entries
            .len(),
        1
    );
    crate::config::ensure_private_file(&destination).expect("secure test backup");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn backup_preflight_rejects_forged_lengths_before_deserialization() {
    let mut too_many_entries = Vec::new();
    too_many_entries.extend_from_slice(&1_u32.to_le_bytes());
    too_many_entries.extend_from_slice(
        &((crate::infra::storage::MAX_SNAPSHOT_ENTRIES as u64) + 1).to_le_bytes(),
    );
    assert_eq!(
        preflight_backup_payload(&too_many_entries),
        Err("The backup contains too many records.")
    );

    let mut oversized_value = Vec::new();
    oversized_value.extend_from_slice(&1_u32.to_le_bytes());
    oversized_value.extend_from_slice(&1_u64.to_le_bytes());
    oversized_value.extend_from_slice(&0_u32.to_le_bytes());
    oversized_value.extend_from_slice(&1_u64.to_le_bytes());
    oversized_value.push(b'k');
    oversized_value.extend_from_slice(
        &((crate::infra::storage::MAX_SNAPSHOT_RECORD_BYTES as u64) + 1).to_le_bytes(),
    );
    assert_eq!(
        preflight_backup_payload(&oversized_value),
        Err("The backup contains a record larger than the supported limit.")
    );

    let mut truncated = Vec::new();
    truncated.extend_from_slice(&1_u32.to_le_bytes());
    truncated.extend_from_slice(&1_u64.to_le_bytes());
    assert_eq!(
        preflight_backup_payload(&truncated),
        Err("The backup payload is truncated.")
    );
}

#[test]
fn production_backup_decoder_preflights_forged_record_field_counts_and_lengths() {
    let root = std::env::temp_dir().join(format!(
        "exoroute-backup-record-preflight-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).expect("create isolated backup folder");

    let mut oversized_count = Vec::new();
    oversized_count
        .extend_from_slice(&((crate::infra::storage::MAX_RECORD_FIELDS as u64) + 1).to_le_bytes());

    let mut oversized_name = Vec::new();
    oversized_name.extend_from_slice(&1_u64.to_le_bytes());
    oversized_name.extend_from_slice(
        &((crate::infra::storage::MAX_RECORD_FIELD_NAME_BYTES as u64) + 1).to_le_bytes(),
    );

    let mut oversized_value = Vec::new();
    oversized_value.extend_from_slice(&1_u64.to_le_bytes());
    oversized_value.extend_from_slice(&1_u64.to_le_bytes());
    oversized_value.push(b'x');
    oversized_value.extend_from_slice(&4_u32.to_le_bytes());
    oversized_value.extend_from_slice(
        &((crate::infra::storage::MAX_RECORD_FIELD_BYTES as u64) + 1).to_le_bytes(),
    );

    let mut oversized_index_string = Vec::new();
    oversized_index_string.extend_from_slice(&501_u64.to_le_bytes());

    let mut malformed_meta_record = Vec::new();
    malformed_meta_record
        .extend_from_slice(&((crate::infra::storage::MAX_RECORD_FIELDS as u64) + 1).to_le_bytes());
    malformed_meta_record.push(0);

    for (name, table, record_bytes, expected_error) in [
        (
            "field-count",
            Table::Providers,
            oversized_count,
            "The backup contains a record with too many fields.",
        ),
        (
            "field-name-length",
            Table::Providers,
            oversized_name,
            "The backup contains a record with an invalid field name.",
        ),
        (
            "field-value-length",
            Table::Providers,
            oversized_value,
            "The backup contains an oversized record field.",
        ),
        (
            "index-string-length",
            Table::ApiKeyIndex,
            oversized_index_string,
            "The backup contains an oversized index value.",
        ),
        (
            "meta-fallback-record-field-count",
            Table::Meta,
            malformed_meta_record,
            "The backup contains a record with too many fields.",
        ),
    ] {
        let path = root.join(format!("{name}.exoroute"));
        let payload = raw_backup_payload(table, &record_bytes);
        write_raw_backup_archive(&path, &payload);
        assert!(matches!(
            decode_backup_file(&path),
            Err(error) if error == expected_error
        ));
    }
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn malformed_backup_is_rejected_before_database_changes() {
    let database = TestDatabase::open().await;
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "current-provider",
            name: "Current provider",
            base_url: "https://1.1.1.1/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "current-provider",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed current provider");
    let path = temporary_backup_path(&database, "malformed.exoroute");
    fs::write(&path, b"not a backup").expect("write malformed backup");
    assert!(restore_application_data(&database.db, &path).await.is_err());
    let remains = database
        .db
        .read(|transaction| transaction.get::<Record>(Table::Providers, "current-provider"))
        .await
        .expect("read current provider");
    assert!(remains.is_some());
}
