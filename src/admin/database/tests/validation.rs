use super::support::*;
use super::*;

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
async fn backup_rejects_an_unknown_provider_model_source() {
    let (_database, mut payload) = route_backup_fixture().await;
    let model = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::ProviderModels)
        .expect("provider model entry");
    let mut model_record: Record = bincode::deserialize(&model.value).expect("decode model");
    model_record.insert("source", Field::Text("not_a_source".to_owned()));
    model.value = bincode::serialize(&model_record).expect("encode unknown source");
    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("A provider model record in the backup has an invalid source.")
    ));

    // Both documented sources are accepted, and so is a row written before the
    // field existed, which reads as imported.
    for source in [None, Some("import"), Some("manual")] {
        let mut accepted = route_backup_fixture().await.1;
        if let Some(source) = source {
            let model = accepted
                .entries
                .iter_mut()
                .find(|entry| entry.table == Table::ProviderModels)
                .expect("provider model entry");
            let mut record: Record = bincode::deserialize(&model.value).expect("decode model");
            record.insert("source", Field::Text(source.to_owned()));
            model.value = bincode::serialize(&record).expect("encode model source");
        }
        assert!(
            validate_backup_payload(&accepted, false, false).is_ok(),
            "{source:?} must be an accepted provider model source"
        );
    }
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
async fn request_logs_stored_compressed_validate_and_rebuild_indexes() {
    let database = TestDatabase::open().await;
    let now = crate::infra::db::utc_timestamp_now().expect("timestamp");
    // A repetitive log row large enough to be stored as a compressed frame.
    let record = Record::new()
        .with("id", Field::Text("compressed-log".to_owned()))
        .with("request_id", Field::Text("request".repeat(40)))
        .with("route_alias", Field::Text("route".to_owned()))
        .with("model", Field::Text("model".to_owned()))
        .with(
            "client_protocol",
            Field::Text("chat_completions".to_owned()),
        )
        .with("status", Field::I64(200))
        .with("duration_ms", Field::I64(1))
        .with("error", Field::Text("upstream 500: unavailable".repeat(24)))
        .with("created_at", Field::Text(now.clone()));
    database
        .db
        .write(move |transaction| transaction.put(Table::RequestLogs, "compressed-log", &record))
        .await
        .expect("seed compressed log");

    let entries = database
        .db
        .snapshot_entries(true, 1024 * 1024)
        .await
        .expect("snapshot with request logs");
    let entry = entries
        .iter()
        .find(|entry| entry.table == Table::RequestLogs && entry.key == "compressed-log")
        .expect("log entry in snapshot");
    // The exported bytes are the stored frame; the backup path must decode it.
    assert!(
        !entry.value.is_empty(),
        "compressed log entry has stored bytes"
    );

    let mut rebuilt = entries.clone();
    super::rebuild_backup_secondary_indexes(&mut rebuilt)
        .expect("rebuild indexes over compressed logs");
    let rebuilt_log = rebuilt
        .iter()
        .find(|entry| entry.table == Table::RequestLogs && entry.key == "compressed-log")
        .expect("log entry survives rebuild");
    let decoded: Record = crate::infra::storage::decode_request_log_bytes(&rebuilt_log.value, None)
        .expect("compressed log decodes through the backup path");
    assert_eq!(decoded.text("id").expect("id"), "compressed-log");

    // Merge the compressed log into a complete, otherwise-valid backup and
    // round-trip it through the real export/import archive: the streaming
    // preflight must treat compressed request-log values as opaque, while
    // restore validation decodes both stored layouts.
    let compressed_entry = entry.clone();
    let (_fixture_database, mut payload) = route_backup_fixture().await;
    payload.entries.push(compressed_entry);

    let root = std::env::temp_dir().join(format!(
        "exoroute-compressed-log-backup-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&root).expect("create isolated backup folder");
    let destination = root.join("backup.exoroute");
    write_backup_archive(&destination, payload.entries).expect("write backup archive");
    let decoded_payload =
        decode_backup_file(&destination).expect("decode backup containing compressed request logs");
    validate_backup_payload(&decoded_payload, false, false)
        .expect("validate backup containing compressed request logs");
    let restored = decoded_payload
        .entries
        .iter()
        .find(|entry| entry.table == Table::RequestLogs && entry.key == "compressed-log")
        .expect("log entry survives import decode");
    let restored_record: Record =
        crate::infra::storage::decode_request_log_bytes(&restored.value, None)
            .expect("compressed log decodes after import");
    assert_eq!(restored_record.text("id").expect("id"), "compressed-log");
    crate::config::ensure_private_file(&destination).expect("secure test backup");
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn backup_meta_dictionary_entry_round_trips_the_import_path() {
    let (_database, mut payload) = route_backup_fixture().await;
    // The request-log compression dictionary is an opaque Meta value; it must
    // pass the streaming preflight, snapshot validation, and import decode.
    let dictionary = vec![0x5A_u8; 4096];
    payload.entries.push(SnapshotEntry {
        table: Table::Meta,
        key: crate::infra::storage::REQUEST_LOG_DICTIONARY_KEY.to_owned(),
        value: bincode::serialize(&dictionary).expect("encode dictionary entry"),
    });

    let root = std::env::temp_dir().join(format!(
        "exoroute-dictionary-backup-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir(&root).expect("create isolated backup folder");
    let destination = root.join("backup.exoroute");
    write_backup_archive(&destination, payload.entries).expect("write backup archive");
    let decoded = decode_backup_file(&destination).expect("decode backup with dictionary entry");
    validate_backup_payload(&decoded, false, false).expect("validate backup with dictionary entry");
    let entry = decoded
        .entries
        .iter()
        .find(|entry| {
            entry.table == Table::Meta
                && entry.key == crate::infra::storage::REQUEST_LOG_DICTIONARY_KEY
        })
        .expect("dictionary entry survives the import path");
    let restored: Vec<u8> = bincode::deserialize(&entry.value).expect("decode restored dictionary");
    assert_eq!(restored, dictionary);
    crate::config::ensure_private_file(&destination).expect("secure test backup");
    let _ = std::fs::remove_dir_all(root);
}

fn backup_route_entry(route_id: &str, name: &str) -> SnapshotEntry {
    let record = Record::new()
        .with("id", Field::Text(route_id.to_owned()))
        .with("name", Field::Text(name.to_owned()))
        .with("strategy", Field::Text("priority".to_owned()))
        .with(
            "accepted_protocols",
            Field::Text("[\"chat_completions\"]".to_owned()),
        )
        .with("enabled", Field::Bool(true))
        .with("created_at", Field::Text("2026-01-01T00:00:00Z".to_owned()))
        .with("updated_at", Field::Text("2026-01-01T00:00:00Z".to_owned()));
    SnapshotEntry {
        table: Table::Routes,
        key: route_id.to_owned(),
        value: bincode::serialize(&record).expect("encode backup route"),
    }
}

fn backup_nested_target_entry(route_id: &str, combo_id: &str, ordinal: u32) -> SnapshotEntry {
    let record = Record::new()
        .with("route_id", Field::Text(route_id.to_owned()))
        .with("combo_id", Field::Text(combo_id.to_owned()))
        .with("provider_id", Field::Null)
        .with("model", Field::Null)
        .with("protocol", Field::Null)
        .with("priority", Field::I64(i64::from(ordinal)))
        .with("weight", Field::Null)
        .with("enabled", Field::Bool(true));
    SnapshotEntry {
        table: Table::RouteTargets,
        key: test_route_target_key(route_id, ordinal, ordinal),
        value: bincode::serialize(&record).expect("encode nested target"),
    }
}

#[tokio::test]
async fn backup_rejects_a_nested_combo_cycle() {
    let (_database, mut payload) = route_backup_fixture().await;
    payload
        .entries
        .push(backup_route_entry("combo-a", "Combo A"));
    payload
        .entries
        .push(backup_route_entry("combo-b", "Combo B"));
    payload
        .entries
        .push(backup_nested_target_entry("combo-a", "combo-b", 0));
    payload
        .entries
        .push(backup_nested_target_entry("combo-b", "combo-a", 0));

    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err(
            "The backup contains a circular, too deeply nested, or oversized combo reference graph."
        )
    ));
}

#[tokio::test]
async fn backup_rejects_a_reference_to_a_combo_missing_from_the_backup() {
    let (_database, mut payload) = route_backup_fixture().await;
    payload
        .entries
        .push(backup_route_entry("combo-a", "Combo A"));
    payload
        .entries
        .push(backup_nested_target_entry("combo-a", "ghost", 0));

    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("A route target refers to a combo that is missing from the backup.")
    ));
}

#[tokio::test]
async fn backup_rejects_a_target_that_is_both_a_provider_and_a_combo() {
    let (_database, mut payload) = route_backup_fixture().await;
    let target = payload
        .entries
        .iter_mut()
        .find(|entry| entry.table == Table::RouteTargets)
        .expect("route target entry");
    let mut record: Record = bincode::deserialize(&target.value).expect("decode target");
    // The fixture row keeps its provider and model, so adding a combo ID makes
    // the row ambiguous rather than converted.
    record.insert("combo_id", Field::Text("route-b".to_owned()));
    target.value = bincode::serialize(&record).expect("encode mixed target");

    assert!(matches!(
        validate_backup_payload(&payload, false, false),
        Err("The backup contains an invalid nested combo target.")
    ));
}

#[tokio::test]
async fn backup_round_trips_a_nested_combo_reference_and_rebuilds_its_index() {
    let (database, mut payload) = route_backup_fixture().await;
    payload
        .entries
        .push(backup_route_entry("combo-parent", "Combo parent"));
    payload
        .entries
        .push(backup_route_entry("combo-child", "Combo child"));
    payload
        .entries
        .push(backup_nested_target_entry("combo-parent", "combo-child", 0));
    assert!(validate_backup_payload(&payload, false, false).is_ok());

    let index_key = crate::admin::combos::route_target_combo_index_key(
        "combo-child",
        &test_route_target_key("combo-parent", 0, 0),
    )
    .expect("combo index key");
    rebuild_backup_secondary_indexes(&mut payload.entries).expect("rebuild backup indexes");
    let index = payload
        .entries
        .iter()
        .find(|entry| entry.table == Table::RouteTargetComboIndex && entry.key == index_key)
        .expect("rebuilt combo index entry");
    let owner: String = bincode::deserialize(&index.value).expect("decode combo index owner");
    assert_eq!(owner, "combo-parent");

    let path = temporary_backup_path(&database, "nested-combo.exoroute");
    fs::write(
        &path,
        encode_backup(payload.entries).expect("encode nested combo backup"),
    )
    .expect("write nested combo backup");
    restore_application_data(&database.db, &path)
        .await
        .expect("import nested combo backup");
    let restored = database
        .db
        .read({
            let index_key = index_key.clone();
            move |transaction| transaction.get::<String>(Table::RouteTargetComboIndex, &index_key)
        })
        .await
        .expect("read restored combo index");
    assert_eq!(restored.as_deref(), Some("combo-parent"));
}

#[tokio::test]
async fn backup_without_combo_fields_keeps_the_provider_target_index() {
    // A backup written before nested combos has no combo_id on any target row
    // and must still import through the provider index.
    let (_database, mut payload) = route_backup_fixture().await;
    assert!(validate_backup_payload(&payload, false, false).is_ok());

    rebuild_backup_secondary_indexes(&mut payload.entries).expect("rebuild backup indexes");
    let target_key = test_route_target_key("route-a", 1, 0);
    let provider_index_key =
        crate::admin::combos::route_target_provider_index_key("backup-provider", &target_key)
            .expect("provider index key");
    assert!(
        payload
            .entries
            .iter()
            .any(|entry| entry.table == Table::RouteTargetProviderIndex
                && entry.key == provider_index_key)
    );
    assert!(
        !payload
            .entries
            .iter()
            .any(|entry| entry.table == Table::RouteTargetComboIndex)
    );
}
