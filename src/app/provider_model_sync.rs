//! Background OpenCode model catalog sync.
//!
//! OpenCode adds upstream models without a client release. The gateway can
//! already passthrough an unknown id through the provider's stored adapter, but
//! the public `/v1/models` catalog and dashboard lists are database-backed, so
//! newly published ids must be registered at runtime. This module does that on
//! a bounded interval, using the same validated discovery path as the admin
//! import endpoint, and never overwrites manual rows or `providers.yaml` data.

use crate::{
    infra::{
        db::{
            provider_api_key_created_prefix, provider_model_index_key, provider_model_index_prefix,
            provider_model_key,
        },
        storage::{Field, Record, StorageError, Table},
    },
    provider_adapters::{
        self, AdapterApiKeyRequest, ModelDiscoveryResult, OPENCODE_GO_ADAPTER_ID,
        OPENCODE_ZEN_ADAPTER_ID,
    },
    security::{SecretContext, decrypt_secret},
    state::AppState,
};
use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};
use tokio::sync::watch;

/// Default refresh cadence. The first tick runs shortly after startup on a
/// spawned task, so discovery never blocks server boot.
pub(crate) const DEFAULT_MODEL_SYNC_INTERVAL: Duration = Duration::from_secs(300);
const MAX_SYNC_PROVIDERS: usize = 256;
const MAX_SYNC_KEYS: usize = 8;
const MAX_SYNC_MODELS: usize = 2048;
const MAX_MODEL_ID_BYTES: usize = 256;
const IMPORT_SOURCE: &str = "import";

#[derive(Debug)]
struct SyncProvider {
    id: String,
    adapter_id: String,
    base_url: String,
    auth_type: String,
    auth_header: Option<String>,
    preferred_protocol: String,
    key_ids: Vec<String>,
}

fn is_deleting(provider: &Record) -> Result<bool, StorageError> {
    match provider.field("deleting") {
        None | Some(Field::Null) => Ok(false),
        Some(Field::Bool(value)) => Ok(*value),
        _ => Err(StorageError::Invalid(
            "provider deletion state is malformed".to_owned(),
        )),
    }
}

fn opencode_provider(adapter_id: &str) -> bool {
    adapter_id == OPENCODE_GO_ADAPTER_ID || adapter_id == OPENCODE_ZEN_ADAPTER_ID
}

/// Bounded scan of enabled OpenCode providers and their usable API keys.
async fn sync_targets(state: &AppState) -> Result<Vec<SyncProvider>, StorageError> {
    let master_key = state.config.master_key;
    state
        .db
        .read(move |transaction| {
            let providers = transaction.scan_prefix::<Record>(
                Table::Providers,
                "",
                MAX_SYNC_PROVIDERS.saturating_add(1),
            )?;
            if providers.len() > MAX_SYNC_PROVIDERS {
                return Err(StorageError::Busy);
            }
            let mut targets = Vec::new();
            for (_, provider) in providers {
                let adapter_id = provider.text("adapter_id")?.to_owned();
                if !opencode_provider(&adapter_id)
                    || !provider.boolean("enabled")?
                    || is_deleting(&provider)?
                {
                    continue;
                }
                let provider_id = provider.text("id")?.to_owned();
                let prefix = provider_api_key_created_prefix(&provider_id)?;
                let rows = transaction.scan_prefix::<String>(
                    Table::ProviderApiKeyCreatedIndex,
                    &prefix,
                    MAX_SYNC_KEYS.saturating_add(1),
                )?;
                let mut key_ids = Vec::with_capacity(rows.len().min(MAX_SYNC_KEYS));
                for (_, key_id) in rows {
                    if key_ids.len() >= MAX_SYNC_KEYS {
                        break;
                    }
                    let Some(key) = transaction.get::<Record>(Table::ProviderApiKeys, &key_id)?
                    else {
                        continue;
                    };
                    if key.boolean("enabled")? && !key.boolean("invalid")? {
                        key_ids.push(key_id);
                    }
                }
                targets.push(SyncProvider {
                    id: provider_id,
                    adapter_id,
                    base_url: provider.text("base_url")?.to_owned(),
                    auth_type: provider.text("auth_type")?.to_owned(),
                    auth_header: provider.optional_text("auth_header")?.map(str::to_owned),
                    preferred_protocol: provider.text("preferred_protocol")?.to_owned(),
                    key_ids,
                });
            }
            let _ = master_key;
            Ok(targets)
        })
        .await
}

fn valid_discovered_model(model: &str) -> bool {
    !model.trim().is_empty()
        && model.len() <= MAX_MODEL_ID_BYTES
        && !model
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
}

/// Inserts only models that are neither already saved nor explicitly removed
/// in `providers.yaml`. Existing rows, their source, and all manual/override
/// data are left untouched; the update is one bounded LMDB transaction.
async fn merge_discovered_models(
    state: &AppState,
    provider_id: &str,
    discovered: Vec<String>,
) -> Result<usize, StorageError> {
    let config = state.providers_yaml_snapshot();
    let removed = config
        .providers
        .get(provider_id)
        .map(|overlay| {
            overlay
                .remove_models
                .iter()
                .map(String::as_str)
                .collect::<HashSet<&str>>()
        })
        .unwrap_or_default();
    let mut seen = HashSet::with_capacity(discovered.len().min(MAX_SYNC_MODELS));
    let models = discovered
        .into_iter()
        .filter(|model| {
            valid_discovered_model(model)
                && !removed.contains(model.as_str())
                && seen.insert(model.clone())
        })
        .take(MAX_SYNC_MODELS)
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Ok(0);
    }

    let provider_id = provider_id.to_owned();
    state
        .db
        .write(move |transaction| {
            let mut provider = transaction
                .get::<Record>(Table::Providers, &provider_id)?
                .ok_or(StorageError::NotFound)?;
            if is_deleting(&provider)? {
                return Ok(0);
            }
            let prefix = provider_model_index_prefix(&provider_id)?;
            let rows = transaction.scan_prefix::<String>(
                Table::ProviderModelIndex,
                &prefix,
                MAX_SYNC_MODELS.saturating_add(1),
            )?;
            if rows.len() > MAX_SYNC_MODELS {
                return Err(StorageError::Busy);
            }
            let existing = rows
                .into_iter()
                .map(|(_, model)| model)
                .collect::<HashSet<_>>();
            let mut added = 0_i64;
            for model in &models {
                if existing.contains(model) {
                    continue;
                }
                let primary_key = provider_model_key(&provider_id, model)?;
                let index_key = provider_model_index_key(&provider_id, model)?;
                let record = Record::new()
                    .with("provider_id", Field::Text(provider_id.clone()))
                    .with("model", Field::Text(model.clone()))
                    .with("upstream_protocol", Field::Null)
                    .with("source", Field::Text(IMPORT_SOURCE.to_owned()));
                transaction.put(Table::ProviderModels, &primary_key, &record)?;
                transaction.put(Table::ProviderModelIndex, &index_key, model)?;
                added = added.checked_add(1).ok_or_else(|| {
                    StorageError::Invalid("provider model count overflowed".to_owned())
                })?;
            }
            if added > 0 {
                let count = provider.integer("model_count").unwrap_or(0);
                provider.insert("model_count", Field::I64(count.saturating_add(added)));
                transaction.put(Table::Providers, &provider_id, &provider)?;
            }
            Ok(added as usize)
        })
        .await
}

/// Discovers and registers new OpenCode models. Returns the number of newly
/// saved rows. Every upstream/key failure is logged and skipped, so a broken
/// provider never blocks the others or server startup.
pub(crate) async fn sync_opencode_models(state: &AppState) -> Result<usize, String> {
    let targets = sync_targets(state)
        .await
        .map_err(|error| format!("could not scan OpenCode providers: {error}"))?;
    let mut total = 0_usize;
    for provider in targets {
        let Some(master_key) = state.config.master_key else {
            tracing::warn!(
                provider_id = %provider.id,
                "skipping OpenCode model sync: no master key is configured"
            );
            continue;
        };
        let mut discovered = None;
        for key_id in &provider.key_ids {
            let key = state
                .db
                .read({
                    let key_id = key_id.clone();
                    move |transaction| {
                        transaction
                            .get::<Record>(Table::ProviderApiKeys, &key_id)?
                            .ok_or(StorageError::NotFound)
                    }
                })
                .await;
            let key = match key {
                Ok(key) => key,
                Err(error) => {
                    tracing::warn!(provider_id = %provider.id, %error, "skipping unreadable provider key");
                    continue;
                }
            };
            let stored = match key.optional_bytes("secret") {
                Ok(stored) => stored,
                Err(error) => {
                    tracing::warn!(provider_id = %provider.id, %error, "skipping malformed provider key");
                    continue;
                }
            };
            let secret = match decrypt_secret(
                Some(&master_key),
                &SecretContext::provider_key(key_id),
                stored,
            ) {
                Ok(secret) => secret,
                Err(error) => {
                    tracing::warn!(provider_id = %provider.id, %error, "skipping unreadable provider key");
                    continue;
                }
            };
            let Some(secret) = secret.filter(|secret| !secret.is_empty()) else {
                continue;
            };
            let headers = BTreeMap::new();
            match provider_adapters::discover_api_key_models(
                &provider.adapter_id,
                AdapterApiKeyRequest {
                    state,
                    base_url: &provider.base_url,
                    auth_type: &provider.auth_type,
                    auth_header: provider.auth_header.as_deref(),
                    custom_headers: &headers,
                    preferred_protocol: &provider.preferred_protocol,
                    credential: &secret,
                },
            )
            .await
            {
                Ok(ModelDiscoveryResult::Available { models, .. }) => {
                    discovered = Some(models);
                    break;
                }
                Ok(ModelDiscoveryResult::Unsupported) => {
                    tracing::warn!(
                        provider_id = %provider.id,
                        "OpenCode model discovery is unsupported; keeping cached catalog"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        provider_id = %provider.id,
                        %error,
                        "OpenCode /models discovery failed; keeping cached catalog"
                    );
                }
            }
        }
        let Some(models) = discovered else {
            if provider.key_ids.is_empty() {
                tracing::warn!(
                    provider_id = %provider.id,
                    "no active API key for OpenCode model discovery; keeping cached catalog"
                );
            }
            continue;
        };
        match merge_discovered_models(state, &provider.id, models).await {
            Ok(added) => {
                total = total.saturating_add(added);
                if added > 0 {
                    tracing::info!(
                        provider_id = %provider.id,
                        added,
                        "registered newly discovered OpenCode models"
                    );
                }
            }
            Err(error) => tracing::warn!(
                provider_id = %provider.id,
                %error,
                "could not save discovered OpenCode models; keeping cached catalog"
            ),
        }
    }
    Ok(total)
}

/// Periodic, shutdown-aware runner. Spawned by bootstrap so discovery never
/// delays HTTP readiness.
pub(crate) async fn provider_model_sync_loop(
    state: AppState,
    interval: Duration,
    mut shutdown: watch::Receiver<bool>,
) {
    let interval = interval.max(Duration::from_millis(1));
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        if *shutdown.borrow() {
            return;
        }
        tokio::select! {
            biased;
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return; }
            }
            _ = ticker.tick() => {
                if let Err(error) = sync_opencode_models(&state).await {
                    tracing::warn!(%error, "OpenCode model sync skipped; keeping cached catalog");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::{
        mock_upstream::spawn_upstream,
        test_support::{
            ProviderSeed, TestDatabase, seed_provider, seed_provider_key, seed_provider_model,
        },
    };
    use axum::{Json, Router, routing::get};
    use serde_json::json;

    const MASTER_KEY: [u8; 32] = [53_u8; 32];

    async fn fixture(models: Router<()>) -> (TestDatabase, AppState) {
        let address = spawn_upstream(models).await;
        let database = TestDatabase::open().await;
        let mut config = database.config();
        config.master_key = Some(MASTER_KEY);
        config.allow_private_provider_urls = true;
        let state = AppState::new(config, database.db.clone());
        seed_provider(
            &database.db,
            ProviderSeed {
                id: "opencode",
                name: "OpenCode Go",
                base_url: &format!("http://{address}/v1"),
                adapter_id: OPENCODE_GO_ADAPTER_ID,
                auth_type: "bearer",
                model_prefix: "ocg",
                preferred_protocol: "chat_completions",
                supported_protocols: &["chat_completions"],
            },
        )
        .await
        .expect("seed OpenCode provider");
        seed_provider_key(&database.db, "opencode", "key-1", b"test-key", false)
            .await
            .expect("seed provider key");
        seed_provider_model(&database.db, "opencode", "existing-model")
            .await
            .expect("seed existing model");
        (database, state)
    }

    fn models_router() -> Router<()> {
        Router::new().route(
            "/v1/models",
            get(|| async {
                Json(json!({"data":[
                    {"id":"existing-model"},
                    {"id":"brand-new-model"},
                    {"id":"removed-by-config"},
                    {"id":""},
                ]}))
            }),
        )
    }

    fn models_error_router() -> Router<()> {
        Router::new().route(
            "/v1/models",
            get(|| async {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "upstream down",
                )
            }),
        )
    }

    async fn saved_models(database: &TestDatabase) -> Vec<String> {
        database
            .db
            .read(|transaction| {
                transaction.scan_prefix::<String>(
                    Table::ProviderModelIndex,
                    &provider_model_index_prefix("opencode")?,
                    64,
                )
            })
            .await
            .expect("scan saved models")
            .into_iter()
            .map(|(_, model)| model)
            .collect()
    }

    #[tokio::test]
    async fn discovery_merges_new_models_without_overwriting_config_or_db_entries() {
        let (database, state) = fixture(models_router()).await;
        std::fs::write(
            state.config.app_dir.join("providers.yaml"),
            "providers:\n  opencode:\n    remove_models: [removed-by-config]\n    model_overrides:\n      existing-model:\n        pricing: { input_per_million: 2.5 }\n",
        )
        .expect("write provider catalog");
        state.reload_provider_presets().expect("reload catalog");

        let added = sync_opencode_models(&state).await.expect("sync succeeds");
        assert_eq!(added, 1);
        let models = saved_models(&database).await;
        assert!(models.contains(&"existing-model".to_owned()));
        assert!(models.contains(&"brand-new-model".to_owned()));
        assert!(!models.contains(&"removed-by-config".to_owned()));
        assert!(!models.iter().any(|model| model.is_empty()));
        assert_eq!(
            state
                .resolve_model_pricing("opencode", OPENCODE_GO_ADAPTER_ID, "existing-model")
                .and_then(|pricing| pricing.input_per_million),
            Some(2.5)
        );
        assert_eq!(
            sync_opencode_models(&state).await.expect("resync succeeds"),
            0
        );
    }

    #[tokio::test]
    async fn upstream_failure_keeps_the_existing_catalog() {
        let (database, state) = fixture(models_error_router()).await;
        let before = saved_models(&database).await;
        assert_eq!(
            sync_opencode_models(&state)
                .await
                .expect("failure is non-fatal"),
            0
        );
        assert_eq!(saved_models(&database).await, before);
    }

    #[tokio::test]
    async fn sync_stops_on_shutdown() {
        let (database, state) = fixture(models_router()).await;
        state.request_shutdown();
        let shutdown = state.shutdown_receiver();
        let task = tokio::spawn(provider_model_sync_loop(
            state.clone(),
            Duration::from_millis(50),
            shutdown,
        ));
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("sync loop stops on shutdown")
            .expect("sync task does not panic");
        drop(database);
    }
}
