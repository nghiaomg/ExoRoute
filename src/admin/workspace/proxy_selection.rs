//! Shared selection of a provider's bound proxy for admin-initiated provider
//! requests.
//!
//! The dashboard's workspace chat relay and the provider model probes must
//! egress through the same proxy binding the public gateway model API uses.
//! This module is the single place that turns a stored provider row into
//! decrypted egress endpoints, so every admin-initiated upstream call applies
//! the gateway's own semantics: a disabled proxy or a proxy with no enabled
//! endpoints is a hard failure and never falls back to a direct connection,
//! the `priority` strategy keeps the stored endpoint order, and `round_robin`
//! advances the same process-local endpoint cursor the gateway advances.
//!
//! Storage access stays behind the existing proxy APIs; this module only
//! reads through `load_proxy_endpoint_configs` and decrypts the selected
//! endpoint records the way `target_preparation` does.

use super::*;
use crate::{
    infra::storage::{Record, StorageError, Table},
    security::egress::ProxyEgress,
};

/// The decrypted egress endpoints one provider request must use.
///
/// `endpoints[0]` is the selected endpoint; the remaining entries are
/// fallbacks a caller may try only when the upstream connection itself fails
/// before any request could have been delivered.
pub(crate) struct ResolvedProxyEgress {
    pub(crate) endpoints: Vec<ProxyEgress>,
}

/// Why a provider's bound proxy could not be turned into usable egress.
#[derive(Debug)]
pub(crate) enum ProxyEgressError {
    /// Reading the provider or proxy rows failed.
    Database(StorageError),
    /// The provider names a proxy that no longer exists.
    BindingMissing,
    /// The bound proxy is disabled.
    ProxyDisabled,
    /// The bound proxy has no enabled endpoint.
    NoEnabledEndpoints,
    /// A selected endpoint record could not be validated or decrypted.
    EgressRejected(String),
}

impl ProxyEgressError {
    /// The sanitized, provider-attributed diagnostic callers surface to the
    /// dashboard, shaped like the gateway's own proxy skip messages.
    pub(crate) fn message(&self, provider_id: &str) -> String {
        match self {
            Self::Database(_) => {
                format!("provider '{provider_id}' proxy binding could not be read")
            }
            Self::BindingMissing => {
                format!("provider '{provider_id}' is bound to a proxy that no longer exists")
            }
            Self::ProxyDisabled => format!("provider '{provider_id}' is bound to a disabled proxy"),
            Self::NoEnabledEndpoints => {
                format!("provider '{provider_id}' is bound to a proxy with no enabled endpoints")
            }
            Self::EgressRejected(error) => {
                format!("provider '{provider_id}' proxy endpoint could not be prepared: {error}")
            }
        }
    }
}

/// The outcome of loading one provider's proxy binding.
enum LoadedProxyBinding {
    /// The provider names a proxy that no longer exists.
    Missing,
    Bound(crate::admin::ProxyEndpointGroup),
}

/// Resolves the proxy egress a provider's requests must use, mirroring the
/// gateway's attempt preparation.
///
/// `Ok(None)` means the provider has no proxy binding, or its transport never
/// reaches egress (the provider spawns its own local CLI process), so the
/// caller builds a direct client exactly as before.
pub(crate) async fn resolve_provider_proxy_egress(
    state: &AppState,
    provider_id: &str,
) -> Result<Option<ResolvedProxyEgress>, ProxyEgressError> {
    let lookup_id = provider_id.to_owned();
    let loaded = state
        .db
        .read(move |transaction| {
            let Some(provider) = transaction.get::<Record>(Table::Providers, &lookup_id)? else {
                return Ok(None);
            };
            let Some(proxy_id) = provider.optional_text("proxy_id")?.map(str::to_owned) else {
                return Ok(None);
            };
            // Local transports (a provider's own CLI process) never reach
            // egress, matching the gateway's attempt preparation.
            if crate::provider_adapters::uses_local_transport(provider.text("adapter_id")?) {
                return Ok(None);
            }
            match crate::admin::load_proxy_endpoint_configs(transaction, &proxy_id) {
                Ok(group) => Ok(Some(LoadedProxyBinding::Bound(group))),
                Err(StorageError::NotFound) => Ok(Some(LoadedProxyBinding::Missing)),
                Err(error) => Err(error),
            }
        })
        .await
        .map_err(ProxyEgressError::Database)?;
    let group = match loaded {
        None => return Ok(None),
        Some(LoadedProxyBinding::Missing) => return Err(ProxyEgressError::BindingMissing),
        Some(LoadedProxyBinding::Bound(group)) => group,
    };
    if !group.enabled {
        return Err(ProxyEgressError::ProxyDisabled);
    }
    if group.endpoints.is_empty() {
        return Err(ProxyEgressError::NoEnabledEndpoints);
    }
    let mut endpoint_configs = group.endpoints;
    if group.strategy == "round_robin" && endpoint_configs.len() > 1 {
        let start = state
            .advance_proxy_endpoint_cursor(&group.proxy_id, endpoint_configs.len())
            .await;
        endpoint_configs.rotate_left(start);
    }
    let mut endpoints = Vec::with_capacity(endpoint_configs.len());
    for config in endpoint_configs {
        endpoints.push(
            crate::security::egress::proxy_egress_from_record(
                &config.endpoint_id,
                &config.record,
                state.config.master_key.as_ref(),
                state.config.allow_private_provider_urls,
            )
            .map_err(ProxyEgressError::EgressRejected)?,
        );
    }
    Ok(Some(ResolvedProxyEgress { endpoints }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        infra::storage::{Field, Table},
        support::test_support::TestDatabase,
    };

    fn hex_component(value: &str) -> String {
        value
            .bytes()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    }

    /// Writes one proxy row plus its endpoint rows in the same layout the
    /// proxy store maintains, so the resolver reads real stored shapes.
    async fn seed_proxy(
        db: &crate::infra::storage::Database,
        proxy_id: &str,
        name: &str,
        strategy: &str,
        enabled: bool,
        endpoints: &[(&str, &str, bool)],
    ) {
        let now = crate::infra::db::utc_timestamp_now().expect("test timestamp");
        let proxy = Record::new()
            .with("id", Field::Text(proxy_id.to_owned()))
            .with("name", Field::Text(name.to_owned()))
            .with("strategy", Field::Text(strategy.to_owned()))
            .with("enabled", Field::Bool(enabled))
            .with("created_at", Field::Text(now.clone()))
            .with("updated_at", Field::Text(now));
        let proxy_id_hex = hex_component(proxy_id);
        let mut endpoint_rows = Vec::with_capacity(endpoints.len());
        for (ordinal, (endpoint_id, url, endpoint_enabled)) in endpoints.iter().enumerate() {
            let key = format!("px/{proxy_id_hex}/{ordinal:010}/{ordinal:010}");
            let record = Record::new()
                .with("proxy_id", Field::Text(proxy_id.to_owned()))
                .with("endpoint_id", Field::Text((*endpoint_id).to_owned()))
                .with("ref_proxy_id", Field::Null)
                .with("url", Field::Text((*url).to_owned()))
                .with("username", Field::Null)
                .with("secret", Field::Null)
                .with("label", Field::Null)
                .with("priority", Field::I64(ordinal as i64))
                .with("enabled", Field::Bool(*endpoint_enabled));
            endpoint_rows.push((key, record));
        }
        let proxy_key = proxy_id.to_owned();
        db.write(move |transaction| {
            transaction.put(Table::Proxies, &proxy_key, &proxy)?;
            for (key, record) in &endpoint_rows {
                transaction.put(Table::ProxyEndpoints, key, record)?;
            }
            Ok(())
        })
        .await
        .expect("seed proxy");
    }

    /// Seeds a generic provider and binds it to `proxy_id` when given. The
    /// model prefix is derived from the provider id, which is unique per test,
    /// so the provider-model-prefix index never conflicts between fixtures.
    async fn seed_bound_provider(
        db: &crate::infra::storage::Database,
        provider_id: &str,
        adapter_id: &str,
        proxy_id: Option<&str>,
    ) {
        crate::support::test_support::seed_provider(
            db,
            crate::support::test_support::ProviderSeed {
                id: provider_id,
                name: "Proxy provider",
                base_url: "https://api.example.com/v1",
                adapter_id,
                auth_type: "none",
                model_prefix: provider_id,
                preferred_protocol: "chat_completions",
                supported_protocols: &["chat_completions"],
            },
        )
        .await
        .expect("seed provider");
        if let Some(proxy_id) = proxy_id {
            let provider_id = provider_id.to_owned();
            let proxy_id = proxy_id.to_owned();
            db.write(move |transaction| {
                let mut provider = transaction
                    .get::<Record>(Table::Providers, &provider_id)?
                    .ok_or(StorageError::NotFound)?;
                provider.insert("proxy_id", Field::Text(proxy_id.clone()));
                transaction.put(Table::Providers, &provider_id, &provider)
            })
            .await
            .expect("bind provider proxy");
        }
    }

    fn proxied_state(database: &TestDatabase) -> AppState {
        let mut config = database.config();
        config.allow_private_provider_urls = true;
        AppState::new(config, database.db.clone())
    }

    fn endpoint_ids(resolved: &ResolvedProxyEgress) -> Vec<&str> {
        resolved.endpoints.iter().map(|e| e.id.as_str()).collect()
    }

    #[tokio::test]
    async fn priority_keeps_the_stored_endpoint_order_on_every_call() {
        let database = TestDatabase::open().await;
        let state = proxied_state(&database);
        seed_proxy(
            &database.db,
            "px-priority",
            "HQ",
            "priority",
            true,
            &[
                ("ep-first", "http://127.0.0.1:9", true),
                ("ep-second", "http://127.0.0.1:10", true),
            ],
        )
        .await;
        seed_bound_provider(&database.db, "provider", "generic", Some("px-priority")).await;

        for _ in 0..2 {
            let resolved = resolve_provider_proxy_egress(&state, "provider")
                .await
                .expect("priority resolution")
                .expect("provider is bound to a proxy");
            assert_eq!(endpoint_ids(&resolved), ["ep-first", "ep-second"]);
        }
    }

    #[tokio::test]
    async fn round_robin_advances_the_shared_gateway_cursor() {
        let database = TestDatabase::open().await;
        let state = proxied_state(&database);
        seed_proxy(
            &database.db,
            "px-rr",
            "Rotating",
            "round_robin",
            true,
            &[
                ("ep-a", "http://127.0.0.1:9", true),
                ("ep-b", "http://127.0.0.1:10", true),
            ],
        )
        .await;
        seed_bound_provider(&database.db, "provider", "generic", Some("px-rr")).await;

        let first = resolve_provider_proxy_egress(&state, "provider")
            .await
            .expect("round-robin resolution")
            .expect("provider is bound to a proxy");
        assert_eq!(endpoint_ids(&first), ["ep-a", "ep-b"]);

        let second = resolve_provider_proxy_egress(&state, "provider")
            .await
            .expect("round-robin resolution")
            .expect("provider is bound to a proxy");
        assert_eq!(
            endpoint_ids(&second),
            ["ep-b", "ep-a"],
            "the second selection starts at the advanced cursor"
        );
        // The gateway advances the same cursor, so a gateway request after two
        // workspace selections starts at the third position.
        assert_eq!(
            state.proxy_endpoint_cursor("px-rr").await,
            2,
            "the selection shares the gateway's endpoint cursor"
        );
    }

    #[tokio::test]
    async fn a_disabled_proxy_is_a_typed_error() {
        let database = TestDatabase::open().await;
        let state = proxied_state(&database);
        seed_proxy(
            &database.db,
            "px-off",
            "Disabled",
            "priority",
            false,
            &[("ep-a", "http://127.0.0.1:9", true)],
        )
        .await;
        seed_bound_provider(&database.db, "provider", "generic", Some("px-off")).await;

        let Err(error) = resolve_provider_proxy_egress(&state, "provider").await else {
            panic!("a disabled proxy must not resolve to a direct connection");
        };
        assert!(matches!(error, ProxyEgressError::ProxyDisabled));
        assert_eq!(
            error.message("provider"),
            "provider 'provider' is bound to a disabled proxy"
        );
    }

    #[tokio::test]
    async fn a_proxy_without_enabled_endpoints_is_a_typed_error() {
        let database = TestDatabase::open().await;
        let state = proxied_state(&database);
        seed_proxy(
            &database.db,
            "px-empty",
            "Empty",
            "priority",
            true,
            &[("ep-a", "http://127.0.0.1:9", false)],
        )
        .await;
        seed_bound_provider(&database.db, "provider", "generic", Some("px-empty")).await;

        let Err(error) = resolve_provider_proxy_egress(&state, "provider").await else {
            panic!("an endpoint-less proxy must not resolve to a direct connection");
        };
        assert!(matches!(error, ProxyEgressError::NoEnabledEndpoints));
    }

    #[tokio::test]
    async fn a_deleted_proxy_binding_is_reported_as_missing() {
        let database = TestDatabase::open().await;
        let state = proxied_state(&database);
        seed_bound_provider(&database.db, "provider", "generic", Some("px-gone")).await;

        let Err(error) = resolve_provider_proxy_egress(&state, "provider").await else {
            panic!("a missing proxy row must fail instead of egressing directly");
        };
        assert!(matches!(error, ProxyEgressError::BindingMissing));
        assert_eq!(
            error.message("provider"),
            "provider 'provider' is bound to a proxy that no longer exists"
        );
    }

    #[tokio::test]
    async fn unbound_providers_and_local_transports_resolve_to_none() {
        let database = TestDatabase::open().await;
        let state = proxied_state(&database);
        seed_proxy(
            &database.db,
            "px-unused",
            "Unused",
            "priority",
            true,
            &[("ep-a", "http://127.0.0.1:9", true)],
        )
        .await;
        seed_bound_provider(&database.db, "provider", "generic", None).await;
        seed_bound_provider(
            &database.db,
            "local-provider",
            "devin_cli",
            Some("px-unused"),
        )
        .await;

        assert!(
            resolve_provider_proxy_egress(&state, "provider")
                .await
                .expect("unbound provider")
                .is_none(),
            "a provider without a proxy binding resolves no proxy"
        );
        assert!(
            resolve_provider_proxy_egress(&state, "local-provider")
                .await
                .expect("local transport provider")
                .is_none(),
            "a local-transport provider never resolves proxy egress"
        );
    }
}
