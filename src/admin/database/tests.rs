//! Tests for database backup export, validation, and restore.
use super::*;

use crate::{
    infra::storage::{Field, Record},
    state::AppState,
    support::test_support::TestDatabase,
};

fn temporary_backup_path(database: &TestDatabase, name: &str) -> PathBuf {
    database
        .db
        .path()
        .parent()
        .expect("LMDB path parent")
        .join(name)
}

fn test_route_target_key(route_id: &str, priority: u32, ordinal: u32) -> String {
    let mut encoded_route_id = String::with_capacity(route_id.len().saturating_mul(2));
    for byte in route_id.bytes() {
        use std::fmt::Write as _;
        let _ = write!(encoded_route_id, "{byte:02x}");
    }
    format!("r/{encoded_route_id}/{priority:010}/{ordinal:010}")
}

fn write_raw_backup_archive(path: &Path, payload: &[u8]) {
    let checksum = Sha256::digest(payload);
    let mut archive = Vec::new();
    archive.extend_from_slice(BACKUP_MAGIC);
    archive.extend_from_slice(&BACKUP_FORMAT_VERSION.to_le_bytes());
    archive.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    archive.extend_from_slice(&checksum);
    archive.extend_from_slice(payload);
    fs::write(path, archive).expect("write checksummed backup archive");
}

fn raw_backup_payload(table: Table, value: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(&1_u64.to_le_bytes());
    let table_index = Table::ALL
        .iter()
        .position(|candidate| *candidate == table)
        .expect("backup table index");
    payload.extend_from_slice(&(table_index as u32).to_le_bytes());
    payload.extend_from_slice(&1_u64.to_le_bytes());
    payload.push(b'p');
    payload.extend_from_slice(&(value.len() as u64).to_le_bytes());
    payload.extend_from_slice(value);
    payload
}

async fn route_backup_fixture() -> (TestDatabase, BackupPayload) {
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
    .expect("seed backup provider");

    let model_key = crate::infra::db::provider_model_key("backup-provider", "model-a")
        .expect("build provider model key");
    let target_key = test_route_target_key("route-a", 1, 0);
    let password_hash =
        crate::admin::hash_admin_password("a-valid-test-password-for-backup-validation")
            .expect("hash test password");
    database
        .db
        .write(move |transaction| {
            transaction.put(
                Table::ProviderModels,
                &model_key,
                &Record::new()
                    .with("provider_id", Field::Text("backup-provider".to_owned()))
                    .with("model", Field::Text("model-a".to_owned())),
            )?;
            for route_id in ["route-a", "route-b"] {
                transaction.put(
                    Table::Routes,
                    route_id,
                    &Record::new()
                        .with("id", Field::Text(route_id.to_owned()))
                        .with("name", Field::Text(format!("Route {route_id}")))
                        .with("strategy", Field::Text("priority".to_owned()))
                        .with(
                            "accepted_protocols",
                            Field::Text("[\"chat_completions\"]".to_owned()),
                        )
                        .with("enabled", Field::Bool(true))
                        .with("created_at", Field::Text("2026-01-01T00:00:00Z".to_owned()))
                        .with("updated_at", Field::Text("2026-01-01T00:00:00Z".to_owned())),
                )?;
            }
            transaction.put(
                Table::RouteTargets,
                &target_key,
                &Record::new()
                    .with("route_id", Field::Text("route-a".to_owned()))
                    .with("provider_id", Field::Text("backup-provider".to_owned()))
                    .with("model", Field::Text("model-a".to_owned()))
                    .with("protocol", Field::Null)
                    .with("priority", Field::I64(1))
                    .with("weight", Field::Null)
                    .with("enabled", Field::Bool(true)),
            )?;
            transaction.put(
                Table::AdminAuth,
                "singleton",
                &Record::new()
                    .with("password_hash", Field::Text(password_hash.clone()))
                    .with("must_change_password", Field::Bool(false))
                    .with("updated_at", Field::Text("2026-01-01T00:00:00Z".to_owned())),
            )?;
            Ok(())
        })
        .await
        .expect("seed route backup records");

    let entries = database
        .db
        .snapshot_entries(false, 8 * 1024 * 1024)
        .await
        .expect("snapshot valid route backup");
    (
        database,
        BackupPayload {
            storage_format_version: crate::infra::storage::STORAGE_FORMAT_VERSION,
            entries,
        },
    )
}

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
fn stale_temp_cleanup_removes_only_owned_backup_directories() {
    let root = std::env::temp_dir().join(format!(
        "exoroute-temp-cleanup-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).expect("create isolated temp folder");
    let stale_import = root.join("lmdb-import-stale");
    let stale_backup = root.join("lmdb-backup-stale");
    let unrelated = root.join("user-data");
    fs::create_dir(&stale_import).expect("create stale import folder");
    fs::create_dir(&stale_backup).expect("create stale backup folder");
    fs::create_dir(&unrelated).expect("create unrelated folder");

    assert_eq!(
        cleanup_stale_temp_directories(&root).expect("clean stale dirs"),
        2
    );
    assert!(!stale_import.exists());
    assert!(!stale_backup.exists());
    assert!(unrelated.exists());
    fs::remove_dir_all(root).expect("remove isolated test folder");
}

#[tokio::test]
async fn temp_directory_drop_queues_cleanup_off_runtime_workers() {
    let root = std::env::temp_dir().join(format!(
        "exoroute-temp-drop-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir(&root).expect("create isolated temp folder");
    drop(TempDirectory(root.clone()));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !tokio::fs::try_exists(&root)
                .await
                .expect("inspect temp folder")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("bounded cleanup worker removes dropped temp directory");
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

#[test]
fn backup_route_target_ordinals_respect_the_configured_target_limit() {
    let prefix = "r/726f7574652d6964/0000000001/";
    assert_eq!(
        route_target_key_components(&format!("{prefix}0000009999"), "route-id"),
        Ok((1, 9999))
    );
    assert!(route_target_key_components(&format!("{prefix}0000010000"), "route-id").is_err());
}

#[test]
fn backup_rejects_provider_keys_that_reference_missing_providers() {
    let record = Record::new()
        .with("id", Field::Text("orphan-key".to_owned()))
        .with("provider_id", Field::Text("missing-provider".to_owned()))
        .with("name", Field::Text("Orphan key".to_owned()))
        .with("secret", Field::Bytes(vec![1, 2, 3]))
        .with("enabled", Field::Bool(true))
        .with("invalid", Field::Bool(false))
        .with("created_at", Field::Text("2026-01-01 00:00:00".to_owned()));
    let payload = BackupPayload {
        storage_format_version: crate::infra::storage::STORAGE_FORMAT_VERSION,
        entries: vec![SnapshotEntry {
            table: Table::ProviderApiKeys,
            key: "orphan-key".to_owned(),
            value: bincode::serialize(&record).expect("serialize provider key"),
        }],
    };

    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("A provider API key refers to a provider that is missing from the backup.")
    ));
}

#[test]
fn backup_rejects_orphan_provider_usage_meters() {
    let credential_id = "missing-credential";
    let minute = 1_700_000_000_i64;
    let meter_key = format!("{credential_id}/{minute:020}");
    let record = Record::new()
        .with("credential_id", Field::Text(credential_id.to_owned()))
        .with("minute", Field::I64(minute))
        .with("request_count", Field::I64(1))
        .with("cost_micro_usd", Field::I64(10_000))
        .with("cost_events", Field::I64(1))
        .with("missing_cost_events", Field::I64(0))
        .with("input_tokens", Field::I64(1))
        .with("output_tokens", Field::I64(1));
    let payload = BackupPayload {
        storage_format_version: crate::infra::storage::STORAGE_FORMAT_VERSION,
        entries: vec![SnapshotEntry {
            table: Table::ProviderUsageMeters,
            key: meter_key,
            value: bincode::serialize(&record).expect("serialize provider usage meter"),
        }],
    };

    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err(
            "A provider usage meter refers to a provider credential that is missing from the backup."
        )
    ));
}

#[tokio::test]
async fn backup_with_unlimited_provider_concurrency_requires_import_acknowledgement() {
    let (_database, mut payload) = route_backup_fixture().await;
    let limits = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::GatewayResourceLimits && entry.key == "singleton")
        .expect("gateway resource limits entry");
    let mut record: Record = bincode::deserialize(&limits.value).expect("decode limits");
    record.insert(
        "provider_max_concurrency",
        Field::I64(
            i64::try_from(crate::config::GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY)
                .expect("unlimited sentinel fits stored integer"),
        ),
    );
    limits.value = bincode::serialize(&record).expect("encode unlimited limits");

    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err(
            "This backup enables unlimited gateway or per-provider concurrency. Confirm the import warning before applying it."
        )
    ));
    assert!(validate_backup_payload(&payload, false, true).is_ok());
}

#[tokio::test]
async fn backup_rejects_provider_model_primary_key_mismatch() {
    let (_database, mut payload) = route_backup_fixture().await;
    let model = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::ProviderModels)
        .expect("provider model entry");
    model.key = "mis-keyed-provider-model".to_owned();

    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("The backup contains an invalid, duplicate, or mis-keyed provider model.")
    ));
}

#[tokio::test]
async fn backup_accepts_legacy_models_and_roundtrips_upstream_protocol_overrides() {
    let (database, mut payload) = route_backup_fixture().await;
    // The fixture represents a pre-override backup: ProviderModels has no
    // upstream_protocol field and must remain valid.
    assert!(validate_backup_payload(&payload, false, false).is_ok());

    let provider = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::Providers)
        .expect("provider entry");
    let mut provider_record: Record =
        bincode::deserialize(&provider.value).expect("decode provider");
    provider_record.insert("adapter_id", Field::Text("opencode_zen".to_owned()));
    provider_record.insert("auth_type", Field::Text("bearer".to_owned()));
    provider_record.insert("base_url", Field::Text("https://1.1.1.1/zen/v1".to_owned()));
    provider_record.insert(
        "supported_protocols",
        Field::Text(
            serde_json::to_string(&[
                "chat_completions",
                "responses",
                "messages",
                "google_generate_content",
            ])
            .expect("encode supported protocols"),
        ),
    );
    provider.value = bincode::serialize(&provider_record).expect("encode provider");

    let model = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::ProviderModels)
        .expect("provider model entry");
    let mut model_record: Record = bincode::deserialize(&model.value).expect("decode model");
    model_record.insert(
        "upstream_protocol",
        Field::Text("google_generate_content".to_owned()),
    );
    model.value = bincode::serialize(&model_record).expect("encode model override");

    let target = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::RouteTargets)
        .expect("route target entry");
    let mut target_record: Record =
        bincode::deserialize(&target.value).expect("decode route target");
    target_record.insert(
        "protocol",
        Field::Text("google_generate_content".to_owned()),
    );
    target.value = bincode::serialize(&target_record).expect("encode target");

    assert!(validate_backup_payload(&payload, false, false).is_ok());
    rebuild_backup_secondary_indexes(&mut payload.entries).expect("rebuild backup indexes");
    let rebuilt_model: Record = bincode::deserialize(
        &payload
            .entries
            .iter()
            .find(|entry| entry.table == Table::ProviderModels)
            .expect("rebuilt provider model entry")
            .value,
    )
    .expect("decode rebuilt model");
    assert_eq!(
        rebuilt_model
            .text("upstream_protocol")
            .expect("saved upstream override"),
        "google_generate_content"
    );

    let path = temporary_backup_path(&database, "opencode-upstream-protocol.exoroute");
    fs::write(
        &path,
        encode_backup(payload.entries).expect("encode OpenCode backup"),
    )
    .expect("write OpenCode backup");
    restore_application_data(&database.db, &path)
        .await
        .expect("import OpenCode backup");
    let model_key = crate::infra::db::provider_model_key("backup-provider", "model-a")
        .expect("provider model key");
    let restored = database
        .db
        .read(move |transaction| transaction.get::<Record>(Table::ProviderModels, &model_key))
        .await
        .expect("read restored provider model")
        .expect("restored provider model");
    assert_eq!(
        restored
            .text("upstream_protocol")
            .expect("restored upstream override"),
        "google_generate_content"
    );
}

#[tokio::test]
async fn backup_rejects_invalid_or_adapter_unsupported_upstream_protocols() {
    let (_database, mut payload) = route_backup_fixture().await;
    let model = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::ProviderModels)
        .expect("provider model entry");
    let mut model_record: Record = bincode::deserialize(&model.value).expect("decode model");
    model_record.insert(
        "upstream_protocol",
        Field::Text("not_a_protocol".to_owned()),
    );
    model.value = bincode::serialize(&model_record).expect("encode invalid model protocol");
    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("A provider model record in the backup has an invalid upstream protocol.")
    ));

    model_record.insert(
        "upstream_protocol",
        Field::Text("google_generate_content".to_owned()),
    );
    payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::ProviderModels)
        .expect("provider model entry")
        .value = bincode::serialize(&model_record).expect("encode unsupported override");
    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("A provider model uses an upstream protocol unsupported by its adapter.")
    ));

    let mut payload = route_backup_fixture().await.1;
    let target = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::RouteTargets)
        .expect("route target entry");
    let mut target_record: Record =
        bincode::deserialize(&target.value).expect("decode route target");
    target_record.insert(
        "protocol",
        Field::Text("google_generate_content".to_owned()),
    );
    target.value = bincode::serialize(&target_record).expect("encode unsupported target");
    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("A route target uses an upstream protocol unsupported by its provider adapter.")
    ));
}

#[tokio::test]
async fn backup_rejects_cross_route_target_keys() {
    let (_database, mut payload) = route_backup_fixture().await;
    let target = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::RouteTargets)
        .expect("route target entry");
    target.key = test_route_target_key("route-b", 1, 0);

    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("The backup route target key does not match its route ID.")
    ));
}

#[tokio::test]
async fn backup_rejects_route_targets_without_a_saved_provider_model() {
    let (_database, mut payload) = route_backup_fixture().await;
    let target = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::RouteTargets)
        .expect("route target entry");
    let mut record: Record = bincode::deserialize(&target.value).expect("decode target");
    record.insert("model", Field::Text("missing-model".to_owned()));
    target.value = bincode::serialize(&record).expect("encode target");

    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("A route target refers to a model that is missing from its provider.")
    ));
}

#[tokio::test]
async fn backup_accepts_missing_target_model_for_a_valid_pending_provider_deletion() {
    let (_database, mut payload) = route_backup_fixture().await;
    let provider = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::Providers)
        .expect("provider entry");
    let mut provider_record: Record =
        bincode::deserialize(&provider.value).expect("decode provider");
    provider_record.insert("enabled", Field::Bool(false));
    provider_record.insert("deleting", Field::Bool(true));
    provider_record.insert("deletion_phase", Field::Text("route_targets".to_owned()));
    provider.value = bincode::serialize(&provider_record).expect("encode tombstone");
    payload
        .entries
        .retain(|entry| entry.table != Table::ProviderModels);

    assert!(validate_backup_payload(&payload, false, false).is_ok());
}

#[tokio::test]
async fn backup_rejects_invalid_target_protocol_and_priority() {
    let (_database, mut payload) = route_backup_fixture().await;
    let original_value = payload
        .entries
        .iter()
        .find(|entry| entry.table == Table::RouteTargets)
        .expect("route target entry")
        .value
        .clone();
    let mut record: Record = bincode::deserialize(&original_value).expect("decode target");
    record.insert("protocol", Field::Text("unsupported-protocol".to_owned()));
    payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::RouteTargets)
        .expect("route target entry")
        .value = bincode::serialize(&record).expect("encode target");
    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("The backup contains a route target with an invalid protocol.")
    ));

    let mut record: Record = bincode::deserialize(&original_value).expect("decode target");
    record.insert("priority", Field::I64(-1));
    payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::RouteTargets)
        .expect("route target entry")
        .value = bincode::serialize(&record).expect("encode target");
    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("The backup contains a route target with an invalid priority.")
    ));
}

#[tokio::test]
async fn invalid_route_target_backup_does_not_change_live_database() {
    let (database, mut payload) = route_backup_fixture().await;
    crate::support::test_support::seed_provider(
        &database.db,
        crate::support::test_support::ProviderSeed {
            id: "current-only-provider",
            name: "Current only provider",
            base_url: "https://1.0.0.1/v1",
            adapter_id: "generic",
            auth_type: "none",
            model_prefix: "current-only-provider",
            preferred_protocol: "chat_completions",
            supported_protocols: &["chat_completions"],
        },
    )
    .await
    .expect("seed live-only provider");

    let target = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::RouteTargets)
        .expect("route target entry");
    target.key = test_route_target_key("route-b", 1, 0);
    let path = temporary_backup_path(&database, "invalid-route-target.exoroute");
    fs::write(
        &path,
        encode_backup(payload.entries).expect("encode malformed backup"),
    )
    .expect("write malformed backup");

    assert!(restore_application_data(&database.db, &path).await.is_err());
    let (backup_provider, current_provider, route_target) = database
        .db
        .read(|transaction| {
            Ok((
                transaction.get::<Record>(Table::Providers, "backup-provider")?,
                transaction.get::<Record>(Table::Providers, "current-only-provider")?,
                transaction
                    .get::<Record>(Table::RouteTargets, &test_route_target_key("route-a", 1, 0))?,
            ))
        })
        .await
        .expect("read unchanged live records");
    assert!(backup_provider.is_some());
    assert!(current_provider.is_some());
    assert!(route_target.is_some());
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
