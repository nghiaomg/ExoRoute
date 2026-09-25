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
