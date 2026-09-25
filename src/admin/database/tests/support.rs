use super::*;

pub(super) fn temporary_backup_path(database: &TestDatabase, name: &str) -> PathBuf {
    database
        .db
        .path()
        .parent()
        .expect("LMDB path parent")
        .join(name)
}

pub(super) fn test_route_target_key(route_id: &str, priority: u32, ordinal: u32) -> String {
    let mut encoded_route_id = String::with_capacity(route_id.len().saturating_mul(2));
    for byte in route_id.bytes() {
        use std::fmt::Write as _;
        let _ = write!(encoded_route_id, "{byte:02x}");
    }
    format!("r/{encoded_route_id}/{priority:010}/{ordinal:010}")
}

pub(super) fn write_raw_backup_archive(path: &Path, payload: &[u8]) {
    let checksum = Sha256::digest(payload);
    let mut archive = Vec::new();
    archive.extend_from_slice(BACKUP_MAGIC);
    archive.extend_from_slice(&BACKUP_FORMAT_VERSION.to_le_bytes());
    archive.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    archive.extend_from_slice(&checksum);
    archive.extend_from_slice(payload);
    fs::write(path, archive).expect("write checksummed backup archive");
}

pub(super) fn raw_backup_payload(table: Table, value: &[u8]) -> Vec<u8> {
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

pub(super) async fn route_backup_fixture() -> (TestDatabase, BackupPayload) {
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
