use crate::{
    config::Config,
    infra::db,
    infra::lock::DatabaseLock,
    infra::storage::{Database, Field, Record, StorageError, Table},
};
use sha2::{Digest, Sha256};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

/// An isolated LMDB environment held with its process lock for one test.
/// Fields are declared in drop order: close the environment, release the lock,
/// then remove the temporary directory.
pub struct TestDatabase {
    pub db: Database,
    _lock: DatabaseLock,
    _cleanup: TestDirectory,
}

struct TestDirectory {
    root: PathBuf,
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl TestDatabase {
    pub async fn open() -> Self {
        let root = std::env::temp_dir().join(format!(
            "exoroute-lmdb-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let path = root.join("exoroute.lmdb");
        let cleanup = TestDirectory { root };
        let lock = db::lock_database(&path).expect("acquire isolated LMDB lock");
        let database = db::connect_for_test(&path)
            .await
            .expect("open isolated LMDB");
        Self {
            db: database,
            _lock: lock,
            _cleanup: cleanup,
        }
    }

    pub fn config(&self) -> Config {
        let root = self
            .db
            .path()
            .parent()
            .expect("test LMDB has a parent directory")
            .to_path_buf();
        Config {
            app_dir: root.clone(),
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8686),
            data_dir: root,
            database_path: self.db.path().to_path_buf(),
            admin_key: Some("a-strong-admin-password-for-tests".to_owned()),
            master_key: None,
            allow_private_provider_urls: false,
            trust_proxy_headers: false,
            connect_timeout: Duration::from_secs(1),
            request_timeout: Duration::from_secs(2),
            stream_idle_timeout: Duration::from_secs(1),
            circuit_breaker_enabled: true,
            circuit_breaker_threshold: 5,
            circuit_breaker_cooldown: Duration::from_secs(1),
        }
    }
}

pub async fn seed_provider(db: &Database, seed: ProviderSeed<'_>) -> Result<(), StorageError> {
    let ProviderSeed {
        id,
        name,
        base_url,
        adapter_id,
        auth_type,
        model_prefix,
        preferred_protocol,
        supported_protocols,
    } = seed;
    let id = id.to_owned();
    let name = name.to_owned();
    let base_url = base_url.to_owned();
    let adapter_id = adapter_id.to_owned();
    let auth_type = auth_type.to_owned();
    let model_prefix = model_prefix.to_owned();
    let preferred_protocol = preferred_protocol.to_owned();
    let supported_protocols = serde_json::to_string(supported_protocols)
        .map_err(|error| StorageError::Codec(error.to_string()))?;
    let timestamp = db::utc_timestamp_now()?;
    let name_digest = Sha256::digest(id.as_bytes());
    let mut name_hash = String::with_capacity(name_digest.len().saturating_mul(2));
    for byte in name_digest {
        use std::fmt::Write as _;
        let _ = write!(name_hash, "{byte:02x}");
    }
    let name_index = format!("{}/{name_hash}", name.to_lowercase());
    let prefix_index = format!("provider_model_prefix/{model_prefix}");
    db.write(move |transaction| {
        let provider = Record::new()
            .with("id", Field::Text(id.clone()))
            .with("name", Field::Text(name.clone()))
            .with("base_url", Field::Text(base_url.clone()))
            .with("logo_url", Field::Null)
            .with("model_prefix", Field::Text(model_prefix.clone()))
            .with("adapter_id", Field::Text(adapter_id.clone()))
            .with("enabled", Field::Bool(true))
            .with("auth_type", Field::Text(auth_type.clone()))
            .with("auth_header", Field::Null)
            .with(
                "preferred_protocol",
                Field::Text(preferred_protocol.clone()),
            )
            .with(
                "supported_protocols",
                Field::Text(supported_protocols.clone()),
            )
            .with("created_at", Field::Text(timestamp.clone()))
            .with("updated_at", Field::Text(timestamp.clone()))
            .with("api_key_count", Field::I64(0))
            .with("invalid_api_key_count", Field::I64(0))
            .with("model_count", Field::I64(0));
        transaction.put_if_absent(Table::Providers, &id, &provider)?;
        transaction.put_if_absent(Table::ProviderNameIndex, &name_index, &id)?;
        transaction.put_if_absent(Table::Indexes, &prefix_index, &id)?;
        let count = transaction
            .get::<i64>(Table::Meta, "provider_count")?
            .unwrap_or(0);
        transaction.put(Table::Meta, "provider_count", &count.saturating_add(1))
    })
    .await
}

#[derive(Clone, Copy)]
pub(crate) struct ProviderSeed<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub base_url: &'a str,
    pub adapter_id: &'a str,
    pub auth_type: &'a str,
    pub model_prefix: &'a str,
    pub preferred_protocol: &'a str,
    pub supported_protocols: &'a [&'a str],
}

pub async fn seed_provider_model(
    db: &Database,
    provider_id: &str,
    model: &str,
) -> Result<(), StorageError> {
    let provider_id = provider_id.to_owned();
    let model = model.to_owned();
    let primary_key = db::provider_model_key(&provider_id, &model)?;
    let index_key = db::provider_model_index_key(&provider_id, &model)?;
    db.write(move |transaction| {
        let Some(mut provider) = transaction.get::<Record>(Table::Providers, &provider_id)? else {
            return Err(StorageError::NotFound);
        };
        if transaction
            .get::<String>(Table::ProviderModelIndex, &index_key)?
            .is_some()
        {
            return Ok(());
        }
        transaction.put_if_absent(
            Table::ProviderModels,
            &primary_key,
            &Record::new()
                .with("provider_id", Field::Text(provider_id.clone()))
                .with("model", Field::Text(model.clone())),
        )?;
        transaction.put_if_absent(Table::ProviderModelIndex, &index_key, &model)?;
        provider.insert(
            "model_count",
            Field::I64(provider.integer("model_count")?.saturating_add(1)),
        );
        transaction.put(Table::Providers, &provider_id, &provider)
    })
    .await
}

pub async fn seed_provider_key(
    db: &Database,
    provider_id: &str,
    key_id: &str,
    secret: &[u8],
    invalid: bool,
) -> Result<(), StorageError> {
    let provider_id_owned = provider_id.to_owned();
    let key_id_owned = key_id.to_owned();
    let secret = std::str::from_utf8(secret)
        .map_err(|_| StorageError::Invalid("test provider key is not UTF-8".to_owned()))?;
    let timestamp = db::utc_timestamp_now()?;
    let secret = crate::security::encrypt_secret(Some(&[53_u8; 32]), secret)
        .map_err(StorageError::Invalid)?
        .ok_or_else(|| StorageError::Invalid("test provider key is empty".to_owned()))?;
    let record = Record::new()
        .with("id", Field::Text(key_id_owned.clone()))
        .with("provider_id", Field::Text(provider_id_owned.clone()))
        .with("name", Field::Text(key_id_owned.clone()))
        .with("secret", Field::Bytes(secret))
        .with("enabled", Field::Bool(true))
        .with("invalid", Field::Bool(invalid))
        .with("last_error", Field::Null)
        .with("last_test_passed", Field::Bool(true))
        .with("last_test_status", Field::Null)
        .with("last_tested_at", Field::Text(timestamp.clone()))
        .with("last_used_at", Field::Null)
        .with("created_at", Field::Text(timestamp));
    let provider_id_for_write = provider_id_owned.clone();
    db.write(move |transaction| {
        crate::admin::providers::store_provider_api_key(transaction, &record)?;
        if let Some(mut provider) =
            transaction.get::<Record>(Table::Providers, &provider_id_for_write)?
        {
            provider.insert(
                "api_key_count",
                Field::I64(provider.integer("api_key_count")?.saturating_add(1)),
            );
            if invalid {
                provider.insert(
                    "invalid_api_key_count",
                    Field::I64(provider.integer("invalid_api_key_count")?.saturating_add(1)),
                );
            }
            transaction.put(Table::Providers, &provider_id_for_write, &provider)?;
        }
        Ok(())
    })
    .await
}

pub async fn seed_route(
    db: &Database,
    route_id: &str,
    name: &str,
    accepted_protocols: &[&str],
    targets: &[(&str, &str, &str, u32)],
) -> Result<(), StorageError> {
    let route_id = route_id.to_owned();
    let name = name.to_owned();
    let accepted_protocols = serde_json::to_string(accepted_protocols)
        .map_err(|error| StorageError::Codec(error.to_string()))?;
    let timestamp = db::utc_timestamp_now()?;
    let route = Record::new()
        .with("id", Field::Text(route_id.clone()))
        .with("name", Field::Text(name.clone()))
        .with("strategy", Field::Text("priority".to_owned()))
        .with("accepted_protocols", Field::Text(accepted_protocols))
        .with("enabled", Field::Bool(true))
        .with("created_at", Field::Text(timestamp.clone()))
        .with("updated_at", Field::Text(timestamp));
    let name_index = format!("name/{name}\u{1}{route_id}");
    let targets = targets
        .iter()
        .enumerate()
        .map(|(ordinal, (provider_id, model, protocol, priority))| {
            let encoded_route = hex_component(&route_id);
            let target_key = format!("r/{encoded_route}/{priority:010}/{ordinal:010}");
            let target = Record::new()
                .with("route_id", Field::Text(route_id.clone()))
                .with("provider_id", Field::Text((*provider_id).to_owned()))
                .with("model", Field::Text((*model).to_owned()))
                .with("protocol", Field::Text((*protocol).to_owned()))
                .with("priority", Field::I64(i64::from(*priority)))
                .with("weight", Field::Null)
                .with("enabled", Field::Bool(true));
            let provider_index = format!("p/{}/{target_key}", hex_component(provider_id));
            (target_key, provider_index, target)
        })
        .collect::<Vec<_>>();
    db.write(move |transaction| {
        transaction.put_if_absent(Table::Routes, &route_id, &route)?;
        transaction.put_if_absent(Table::RouteNameIndex, &name_index, &route_id)?;
        for (target_key, provider_index, target) in &targets {
            transaction.put_if_absent(Table::RouteTargets, target_key, target)?;
            transaction.put_if_absent(
                Table::RouteTargetProviderIndex,
                provider_index,
                target_key,
            )?;
        }
        Ok(())
    })
    .await
}

pub async fn seed_api_key(
    db: &Database,
    id: &str,
    name: &str,
    token: &str,
) -> Result<(), StorageError> {
    let id = id.to_owned();
    let name = name.to_owned();
    let token_hash = crate::security::token_hash(token);
    let created_at = db::utc_timestamp_now()?;
    let index_key = format!(
        "created/{}/{}/{}",
        reverse_lexical_component(&created_at),
        reverse_lexical_component(&id),
        id
    );
    let mut token_hex = String::with_capacity(token_hash.len().saturating_mul(2));
    for byte in &token_hash {
        use std::fmt::Write as _;
        let _ = write!(token_hex, "{byte:02x}");
    }
    let token_index = format!("token/{token_hex}");
    let record = Record::new()
        .with("id", Field::Text(id.clone()))
        .with("name", Field::Text(name))
        .with("token_hash", Field::Bytes(token_hash))
        .with("enabled", Field::Bool(true))
        .with("created_at", Field::Text(created_at))
        .with("last_used_at", Field::Null)
        .with("request_count", Field::I64(0))
        .with("request_count_generation", Field::I64(0));
    db.write(move |transaction| {
        transaction.put_if_absent(Table::ApiKeys, &id, &record)?;
        transaction.put_if_absent(Table::ApiKeyIndex, &index_key, &id)?;
        transaction.put_if_absent(Table::ApiKeyTokenIndex, &token_index, &id)
    })
    .await
}

fn reverse_lexical_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len().saturating_mul(2).saturating_add(2));
    for byte in value.bytes() {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{:02x}", 255_u8.saturating_sub(byte));
    }
    encoded.push_str("ff");
    encoded
}

fn hex_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len().saturating_mul(2));
    for byte in value.bytes() {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}
