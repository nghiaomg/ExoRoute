//! Pinned provider HTTP client construction and bounded caching.
//!
//! One client is cached per provider origin plus timeout and streaming shape so
//! unrelated providers never share DNS overrides. DNS is revalidated on the
//! cache TTL and every client has redirects disabled.

use super::url::{build_pinned_client, resolve_provider_url, validate_provider_url};
use crate::config::UpstreamSettings;
use reqwest::Url;
use std::{
    collections::HashMap,
    sync::LazyLock,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

const PROVIDER_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Upper bound for a single streaming provider response that has no total
/// request timeout. Liveness is owned by the gateway's own idle deadlines;
/// this bound exists so a provider that keeps the connection open without
/// sending frames cannot hold a socket forever.
pub const PROVIDER_STREAM_TOTAL_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Hash, PartialEq, Eq)]
struct ProviderClientKey {
    origin: String,
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

/// Return a reusable client pinned to recently validated public (or explicitly
/// permitted local) DNS addresses. DNS is revalidated every 30 seconds and all
/// redirects are disabled by the pinned-client builder. Cache size is bounded;
/// cloned clients remain valid for requests already in flight after eviction.
pub async fn provider_client(
    input: &str,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
    streaming: bool,
    user_agent: &str,
    upstream: UpstreamSettings,
) -> Result<(Url, reqwest::Client), String> {
    let url = validate_provider_url(input, allow_local)?;
    let host = url
        .host_str()
        .ok_or_else(|| "Provider URL must include a host".to_owned())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Provider URL must include a valid port".to_owned())?;
    let key = ProviderClientKey {
        origin: format!("{}://{}:{port}", url.scheme(), host.to_ascii_lowercase()),
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

    let resolved = resolve_provider_url(
        input,
        allow_local,
        upstream.provider_client_max_resolved_addresses,
    )
    .await?;
    // reqwest's total timeout spans connect through the end of the response
    // body and does not reset on activity. Applying it to streaming requests
    // kills any stream that outlives `request_timeout` even while frames keep
    // arriving, so streaming clients bound the body with the idle-oriented
    // total limit instead; gateway idle deadlines own liveness.
    let total_timeout = if streaming {
        PROVIDER_STREAM_TOTAL_TIMEOUT
    } else {
        request_timeout
    };
    let client = build_pinned_client(
        reqwest::Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(total_timeout)
            // Evict idle sockets before common provider/proxy keep-alive
            // windows expire. Reusing a server-closed socket makes POSTs
            // fail with a reset instead of opening a fresh connection.
            .pool_idle_timeout(PROVIDER_POOL_IDLE_TIMEOUT)
            .user_agent(user_agent),
        &resolved,
    )?;
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
        return Ok((resolved.url, entry.client.clone()));
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
    Ok((resolved.url, client))
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

    /// Streams a response whose body outlives `request_timeout` in chunks.
    /// The non-streaming client must fail with its total timeout; the
    /// streaming client must deliver every chunk because its total bound is
    /// the idle-oriented 24h limit, not the per-request timeout.

    #[tokio::test]
    async fn streaming_client_survives_bodies_longer_than_request_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener binds");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            // Both flavors connect to the same listener; serve each one with
            // the same headers followed by chunks past the request timeout.
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.expect("test connection");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 512];
                while !request.ends_with(b"\r\n\r\n") {
                    let read = tokio::io::AsyncReadExt::read(&mut socket, &mut buffer)
                        .await
                        .expect("request headers");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                }
                // The non-streaming flavor deliberately abandons the body
                // mid-stream when its total timeout fires, so chunk writes
                // must tolerate a peer that has closed the socket.
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
                    )
                    .await;
                for delay in [0, 150, 500] {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    if socket.write_all(b"3\r\nabc\r\n").await.is_err() {
                        break;
                    }
                }
                let _ = socket.write_all(b"0\r\n\r\n").await;
            }
        });

        let url = format!("http://{address}/v1/stream");
        let (_, streaming_client) = provider_client(
            &url,
            true,
            Duration::from_secs(1),
            Duration::from_millis(200),
            true,
            "ExoRoute/test",
            UpstreamSettings::default(),
        )
        .await
        .expect("streaming client builds");
        let response = streaming_client
            .get(&url)
            .send()
            .await
            .expect("streaming request succeeds");
        use futures_util::StreamExt;
        let mut chunks = response.bytes_stream();
        let mut received = 0_usize;
        while let Some(chunk) = chunks.next().await {
            chunk.expect("streaming body survives past the request timeout");
            received += 1;
        }
        assert_eq!(received, 3, "every streamed chunk is delivered");

        let (_, plain_client) = provider_client(
            &url,
            true,
            Duration::from_secs(1),
            Duration::from_millis(200),
            false,
            "ExoRoute/test",
            UpstreamSettings::default(),
        )
        .await
        .expect("non-streaming client builds");
        let response = plain_client
            .get(&url)
            .send()
            .await
            .expect("headers arrive before the total timeout");
        let mut chunks = response.bytes_stream();
        let mut timed_out = false;
        while let Some(chunk) = chunks.next().await {
            match chunk {
                Ok(_) => {}
                Err(error) if error.is_timeout() => {
                    timed_out = true;
                    break;
                }
                Err(error) => panic!("unexpected non-streaming body error: {error}"),
            }
        }
        assert!(
            timed_out,
            "non-streaming client still enforces its total timeout"
        );
        server.await.expect("test server task");
    }
}
