//! Pinned provider HTTP client construction and bounded caching.
//!
//! Direct clients pin provider DNS. Proxy clients deliberately delegate provider
//! DNS to the validated egress proxy; both shapes are isolated in the client
//! cache so unrelated providers never share proxy credentials or DNS overrides.

use super::url::{
    ResolvedProviderUrl, build_pinned_client, build_proxy_client, resolve_provider_url,
    resolve_proxy_url, validate_provider_url, validate_provider_url_via_proxy, validate_proxy_url,
};
use crate::{config::UpstreamSettings, security::decrypt_secret};
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::LazyLock,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const PROVIDER_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Upper bound for a single streaming provider response that has no total
/// request timeout. Liveness is owned by the gateway's own idle deadlines.
pub const PROVIDER_STREAM_TOTAL_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

/// Decrypted proxy settings for exactly one endpoint. The secret is kept only in
/// this request-scoped value and is never formatted or logged.
#[derive(Clone)]
pub struct ProxyEgress {
    pub id: String,
    pub url: Url,
    pub username: Option<String>,
    pub secret: Option<String>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct ProviderClientKey {
    origin: String,
    proxy_fingerprint: Option<[u8; 32]>,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    streaming: bool,
    user_agent: String,
}

struct CachedProviderClient {
    client: reqwest::Client,
    expires_at: Instant,
    last_used_at: Instant,
}

static PROVIDER_CLIENTS: LazyLock<Mutex<HashMap<ProviderClientKey, CachedProviderClient>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Build a client for a provider origin. With no proxy, provider DNS is resolved
/// and pinned as before. With a proxy, the proxy is validated as the actual
/// destination and provider DNS is delegated to that proxy; reqwest's
/// `resolve_to_addrs` cannot constrain a remote CONNECT target.
pub async fn provider_client(
    input: &str,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    streaming: bool,
    user_agent: &str,
    upstream: UpstreamSettings,
) -> Result<(Url, reqwest::Client), String> {
    provider_client_with_proxy(
        input,
        allow_local,
        connect_timeout,
        request_timeout,
        streaming,
        user_agent,
        upstream,
        None,
    )
    .await
}

pub async fn provider_client_with_proxy(
    input: &str,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    streaming: bool,
    user_agent: &str,
    upstream: UpstreamSettings,
    proxy: Option<&ProxyEgress>,
) -> Result<(Url, reqwest::Client), String> {
    let url = if proxy.is_some() {
        validate_provider_url_via_proxy(input, allow_local)?
    } else {
        validate_provider_url(input, allow_local)?
    };
    let host = url
        .host_str()
        .ok_or_else(|| "Provider URL must include a host".to_owned())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Provider URL must include a valid port".to_owned())?;
    let proxy_fingerprint = proxy.map(proxy_fingerprint);
    let key = ProviderClientKey {
        origin: format!("{}://{}:{port}", url.scheme(), host.to_ascii_lowercase()),
        proxy_fingerprint,
        allow_local,
        connect_timeout,
        request_timeout,
        streaming,
        user_agent: user_agent.to_owned(),
    };
    let now = Instant::now();
    {
        let mut cache = PROVIDER_CLIENTS.lock().await;
        cache.retain(|_, entry| {
            entry.expires_at > now
                && entry
                    .last_used_at
                    .checked_add(upstream.provider_client_cache_ttl)
                    .is_some_and(|expires_at| expires_at > now)
        });
        if let Some(entry) = cache.get_mut(&key) {
            entry.last_used_at = now;
            return Ok((url, entry.client.clone()));
        }
    }

    let total_timeout = if streaming {
        PROVIDER_STREAM_TOTAL_TIMEOUT
    } else {
        request_timeout
    };
    let builder = reqwest::Client::builder()
        .connect_timeout(connect_timeout)
        .timeout(total_timeout)
        .pool_idle_timeout(PROVIDER_POOL_IDLE_TIMEOUT)
        .user_agent(user_agent);
    let client = if let Some(proxy) = proxy {
        let proxy_url = validate_proxy_url(proxy.url.as_str(), allow_local)?;
        let resolved_proxy = resolve_proxy_url(
            proxy_url.as_str(),
            allow_local,
            upstream.provider_client_max_resolved_addresses,
        )
        .await?;
        let mut configured_proxy = reqwest::Proxy::all(proxy_url)
            .map_err(|_| "Configured proxy URL could not be validated".to_owned())?;
        if let (Some(username), Some(secret)) = (proxy.username.as_deref(), proxy.secret.as_deref())
        {
            configured_proxy = configured_proxy.basic_auth(username, secret);
        }
        build_proxy_client(builder, configured_proxy, &resolved_proxy)?
    } else {
        let resolved = resolve_provider_url(
            input,
            allow_local,
            upstream.provider_client_max_resolved_addresses,
        )
        .await?;
        build_pinned_client(builder, &resolved)?
    };

    let mut cache = PROVIDER_CLIENTS.lock().await;
    let inserted_at = Instant::now();
    cache.retain(|_, entry| {
        entry.expires_at > inserted_at
            && entry
                .last_used_at
                .checked_add(upstream.provider_client_cache_ttl)
                .is_some_and(|expires_at| expires_at > inserted_at)
    });
    if let Some(entry) = cache.get_mut(&key) {
        entry.last_used_at = inserted_at;
        return Ok((url, entry.client.clone()));
    }
    if cache.len() >= upstream.provider_client_cache_max_entries
        && let Some(least_recently_used) = cache
            .iter()
            .min_by_key(|(_, entry)| entry.last_used_at)
            .map(|(key, _)| key.clone())
    {
        cache.remove(&least_recently_used);
    }
    cache.insert(
        key,
        CachedProviderClient {
            client: client.clone(),
            expires_at: inserted_at
                .checked_add(upstream.provider_client_cache_ttl)
                .unwrap_or(inserted_at),
            last_used_at: inserted_at,
        },
    );
    Ok((url, client))
}

fn proxy_fingerprint(proxy: &ProxyEgress) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(proxy.id.as_bytes());
    hash.update([0]);
    hash.update(proxy.url.as_str().as_bytes());
    hash.update([0]);
    hash.update(proxy.username.as_deref().unwrap_or("").as_bytes());
    hash.update([0]);
    hash.update(proxy.secret.as_deref().unwrap_or("").as_bytes());
    hash.finalize().into()
}

/// Decode encrypted proxy endpoint records for request preparation. The
/// endpoint secret is decrypted only after it has been selected for a request.
pub(crate) fn proxy_egress_from_record(
    endpoint_id: &str,
    record: &crate::infra::storage::Record,
    master_key: Option<&[u8; 32]>,
    allow_local: bool,
) -> Result<ProxyEgress, String> {
    let url = record
        .text("url")
        .map_err(|_| "Stored proxy endpoint has no URL".to_owned())?;
    let url = validate_proxy_url(url, allow_local)?;
    let username = record
        .optional_text("username")
        .map_err(|_| "Stored proxy endpoint username is invalid".to_owned())?
        .map(str::to_owned);
    let secret = decrypt_secret(
        master_key,
        record
            .optional_bytes("secret")
            .map_err(|_| "Stored proxy endpoint credential is invalid".to_owned())?,
    )?;
    Ok(ProxyEgress {
        id: endpoint_id.to_owned(),
        url,
        username,
        secret,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn provider_client_enforces_policy_and_builds_for_explicit_local_upstreams() {
        let denied = provider_client(
            "http://127.0.0.1:9876/v1",
            false,
            Duration::from_secs(1),
            Duration::from_secs(1),
            false,
            "ExoRoute/test",
            UpstreamSettings::default(),
        )
        .await;
        assert!(denied.is_err());

        let (url, client) = provider_client(
            "http://127.0.0.1:9876/v1",
            true,
            Duration::from_secs(1),
            Duration::from_secs(1),
            false,
            "ExoRoute/test",
            UpstreamSettings::default(),
        )
        .await
        .expect("explicit local provider is pinned");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        drop(client);
    }
}
