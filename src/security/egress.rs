//! Provider URL egress validation.
//!
//! `validate_provider_url` is intentionally a synchronous, syntax and literal-IP
//! check. For hostnames, resolve with `resolve_provider_url` and use the returned
//! addresses to pin the connection. DNS validation is only a point-in-time check:
//! it does not pin a later connection by itself. A caller that needs SSRF
//! protection against DNS rebinding must enforce the same address policy at
//! connect time (or use a trusted egress proxy/firewall). Redirects also need to
//! be disabled or revalidated at every hop.

use crate::config::UpstreamSettings;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::{
    collections::HashMap,
    sync::LazyLock,
    time::{Duration, Instant},
};

use reqwest::Url;
use tokio::sync::Mutex;

const PROVIDER_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Parse a provider's custom authentication header without allowing it to
/// replace HTTP framing or hop-by-hop headers managed by the client.
pub fn provider_auth_header_name(name: &str) -> Result<http::header::HeaderName, String> {
    let name = http::header::HeaderName::from_bytes(name.as_bytes())
        .map_err(|_| "Provider authentication header name is invalid".to_owned())?;
    if matches!(
        name.as_str(),
        "connection"
            | "content-length"
            | "content-type"
            | "expect"
            | "host"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    ) {
        return Err(
            "Provider authentication header cannot override an HTTP transport header".to_owned(),
        );
    }
    Ok(name)
}

/// Parse an operator-supplied provider header without allowing it to replace
/// HTTP framing, hop-by-hop, or credential headers owned by ExoRoute.
pub fn provider_custom_header_name(name: &str) -> Result<http::header::HeaderName, String> {
    let name = http::header::HeaderName::from_bytes(name.trim().as_bytes())
        .map_err(|_| "Provider custom header name is invalid".to_owned())?;
    if matches!(
        name.as_str(),
        "authorization"
            | "api-key"
            | "cookie"
            | "content-length"
            | "content-type"
            | "connection"
            | "expect"
            | "host"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "x-api-key"
            | "x-goog-api-key"
    ) {
        return Err(
            "Provider custom header cannot override authentication or HTTP transport headers"
                .to_owned(),
        );
    }
    Ok(name)
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct ProviderClientKey {
    origin: String,
    allow_local: bool,
    connect_timeout: Duration,
    request_timeout: Duration,
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
    let client = build_pinned_client(
        reqwest::Client::builder()
            .connect_timeout(connect_timeout)
            .timeout(request_timeout)
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

/// Validates an HTTP(S) provider URL.
///
/// `allow_local` explicitly enables loopback, RFC1918/ULA, and local-only
/// hostnames for deployments that intentionally use a local provider. Link-
/// local addresses, cloud metadata endpoints, multicast, unspecified, and
/// reserved/non-public special-use addresses remain rejected even when local
/// providers are allowed.
pub fn validate_provider_url(input: &str, allow_local: bool) -> Result<Url, String> {
    let url = Url::parse(input).map_err(|_| "Invalid provider URL".to_owned())?;

    if !matches!(url.scheme(), "http" | "https") {
        return Err("Provider URL must use http or https".to_owned());
    }
    if url.scheme() == "http" && !allow_local {
        return Err("Public provider URLs must use https; http is allowed only with explicit local-provider permission".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Provider URL must not contain username or password".to_owned());
    }

    let host = url
        .host_str()
        .ok_or_else(|| "Provider URL must include a host".to_owned())?;
    let host = strip_ipv6_brackets(host);

    if let Ok(ip) = host.parse::<IpAddr>() {
        validate_ip_address(ip, allow_local)?;
    } else if is_local_hostname(host) {
        if !allow_local {
            return Err("Local provider hosts require explicit local-provider permission".into());
        }
    } else if is_special_hostname(host) && !allow_local {
        return Err("Non-public provider host requires explicit local-provider permission".into());
    }

    Ok(url)
}

/// A URL plus the addresses approved during DNS resolution.
#[derive(Clone, Debug)]
pub struct ResolvedProviderUrl {
    pub url: Url,
    pub addresses: Vec<SocketAddr>,
}

/// Builds a reqwest client pinned to the address set in `resolved` and with
/// redirects disabled. Build/cache one client per provider origin so unrelated
/// providers do not share DNS overrides. Apply normal timeout and pool settings
/// to the supplied builder before calling this function.
pub fn build_pinned_client(
    builder: reqwest::ClientBuilder,
    resolved: &ResolvedProviderUrl,
) -> Result<reqwest::Client, String> {
    let host = resolved
        .url
        .host_str()
        .ok_or_else(|| "Provider URL must include a host".to_owned())?;
    if resolved.addresses.is_empty() {
        return Err("Provider host resolved to no addresses".to_owned());
    }

    builder
        // Environment-configured proxies bypass reqwest's per-host DNS pin;
        // do not inherit them implicitly for credential-bearing provider calls.
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(strip_ipv6_brackets(host), &resolved.addresses)
        .build()
        .map_err(|_| "Could not build pinned provider HTTP client".to_owned())
}

/// Validates a URL and checks the addresses its hostname resolves to now.
///
/// All returned addresses must satisfy the same policy. This rejects a public
/// hostname with even one private answer, avoiding the common mixed-answer
/// bypass. It cannot prevent DNS rebinding between this check and the HTTP
/// client's own resolution; callers must enforce policy at connection time for
/// that guarantee.
pub async fn resolve_provider_url(
    input: &str,
    allow_local: bool,
    max_addresses: usize,
) -> Result<ResolvedProviderUrl, String> {
    let url = validate_provider_url(input, allow_local)?;
    let host = url
        .host_str()
        .ok_or_else(|| "Provider URL must include a host".to_owned())?;
    let host = strip_ipv6_brackets(host).to_owned();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Provider URL must include a valid port".to_owned())?;

    if let Ok(ip) = host.parse::<IpAddr>() {
        validate_ip_address(ip, allow_local)?;
        if url.scheme() == "http" && classify_ip(ip) != IpClass::Local {
            return Err("HTTP is allowed only for local provider addresses".into());
        }
        return Ok(ResolvedProviderUrl {
            url,
            addresses: vec![SocketAddr::new(ip, port)],
        });
    }

    let mut addresses: Vec<SocketAddr> = Vec::with_capacity(8);
    let mut resolved = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| "Provider host resolution timed out".to_owned())?
    .map_err(|_| "Could not resolve provider host".to_owned())?;
    for address in &mut resolved {
        if addresses.len() >= max_addresses {
            return Err("Provider host resolved to too many addresses".to_owned());
        }
        addresses.push(address);
    }
    drop(resolved);
    if addresses.is_empty() {
        return Err("Provider host resolved to no addresses".to_owned());
    }
    for address in &addresses {
        validate_ip_address(address.ip(), allow_local)?;
    }
    if url.scheme() == "http"
        && addresses
            .iter()
            .any(|address| classify_ip(address.ip()) != IpClass::Local)
    {
        return Err("HTTP is allowed only for local provider addresses".into());
    }

    Ok(ResolvedProviderUrl { url, addresses })
}

/// Checks a resolved destination address with the URL policy. A custom HTTP
/// connector can call this immediately before connecting to each resolved IP.
pub fn validate_ip_address(ip: IpAddr, allow_local: bool) -> Result<(), String> {
    match classify_ip(ip) {
        IpClass::Public => Ok(()),
        IpClass::Local if allow_local => Ok(()),
        IpClass::Local => {
            Err("Local provider addresses require explicit local-provider permission".into())
        }
        IpClass::AlwaysBlocked => Err("Provider address is reserved or unsafe".into()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IpClass {
    Public,
    Local,
    AlwaysBlocked,
}

fn classify_ip(ip: IpAddr) -> IpClass {
    match ip {
        IpAddr::V4(ip) => classify_ipv4(ip),
        IpAddr::V6(ip) => classify_ipv6(ip),
    }
}

fn classify_ipv4(ip: Ipv4Addr) -> IpClass {
    let octets = ip.octets();
    let a = octets[0];
    let b = octets[1];
    let c = octets[2];

    if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() {
        return IpClass::AlwaysBlocked;
    }

    // Explicitly local address ranges.
    if ip.is_link_local() || ip == Ipv4Addr::new(168, 63, 129, 16) {
        // Link-local networks include the common cloud metadata services. The
        // Azure platform virtual IP is special despite appearing globally
        // routable. Never make either reachable through the local-provider opt-in.
        return IpClass::AlwaysBlocked;
    }
    if ip.is_loopback() || ip.is_private() {
        return IpClass::Local;
    }

    // Special-purpose, documentation, benchmarking, and protocol ranges that
    // must not be treated as public provider destinations.
    if a == 0
        || (a == 100 && (64..=127).contains(&b)) // shared address space
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224
    {
        return IpClass::AlwaysBlocked;
    }

    IpClass::Public
}

fn classify_ipv6(ip: Ipv6Addr) -> IpClass {
    if ip.is_unspecified() || ip.is_multicast() {
        return IpClass::AlwaysBlocked;
    }
    if ip.is_unicast_link_local() || ip == Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254) {
        // Link-local addresses and the AWS IPv6 IMDS endpoint are never valid
        // provider targets, even when local upstreams are explicitly enabled.
        return IpClass::AlwaysBlocked;
    }
    if ip.is_loopback() || ip.is_unique_local() {
        return IpClass::Local;
    }

    // IPv4-mapped IPv6 addresses inherit the embedded IPv4 classification.
    if let Some(ipv4) = ip.to_ipv4_mapped() {
        return classify_ipv4(ipv4);
    }

    let segments = ip.segments();
    // Documentation prefixes, 6to4, NAT64 well-known translation, and IETF
    // protocol assignment space can bridge to or describe non-public targets.
    if (segments[0] & 0xffc0) == 0x2000 && segments[1] == 0x0db8 // 2001:db8::/32
        || (segments[0] & 0xff00) == 0x2000 && segments[0] == 0x2002 // 2002::/16
        || (segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2..].iter().all(|v| *v == 0))
        || (segments[0] & 0xffe0) == 0x2000 && (segments[0] & 0xfff0) == 0x2000 && segments[1] <= 0x01ff
    {
        return IpClass::AlwaysBlocked;
    }

    // Only global-unicast 2000::/3 addresses are accepted as public IPv6.
    if (segments[0] & 0xe000) != 0x2000 {
        return IpClass::AlwaysBlocked;
    }

    IpClass::Public
}

fn is_local_hostname(host: &str) -> bool {
    let host = normalized_host(host);
    host == "localhost"
        || host.ends_with(".localhost")
        || host == "local"
        || host.ends_with(".local")
        || host == "localdomain"
        || host.ends_with(".localdomain")
        || host == "lan"
        || host.ends_with(".lan")
        || host == "internal"
        || host.ends_with(".internal")
        || host == "home"
        || host.ends_with(".home")
        || host == "home.arpa"
        || host.ends_with(".home.arpa")
}

fn is_special_hostname(host: &str) -> bool {
    let host = normalized_host(host);
    // Reject unqualified names because resolver search domains can map them to
    // internal services. Also reject reserved-use TLDs by default.
    !host.contains('.')
        || ["test", "invalid", "example", "onion"]
            .iter()
            .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

fn normalized_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

fn strip_ipv6_brackets(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_auth_headers_cannot_override_http_framing() {
        assert!(provider_auth_header_name("x-api-key").is_ok());
        assert!(provider_auth_header_name("Authorization").is_ok());
        for name in [
            "Host",
            "Content-Length",
            "Content-Type",
            "Transfer-Encoding",
            "Connection",
            "Proxy-Authorization",
        ] {
            assert!(provider_auth_header_name(name).is_err(), "{name}");
        }
    }

    #[test]
    fn requires_https_for_public_provider_urls() {
        assert!(validate_provider_url("https://api.example.com/v1", false).is_ok());
        assert!(validate_provider_url("http://203.0.113.8", false).is_err());
        assert!(validate_provider_url("http://8.8.8.8/v1", false).is_err());
        assert!(validate_provider_url("http://127.0.0.1/v1", true).is_ok());
    }

    #[test]
    fn rejects_credentials_and_unsupported_schemes() {
        assert!(validate_provider_url("https://user:secret@example.com", false).is_err());
        assert!(validate_provider_url("ftp://example.com", false).is_err());
    }

    #[test]
    fn rejects_local_hosts_by_default_and_allows_them_explicitly() {
        for url in [
            "http://localhost:8080",
            "http://service.local",
            "http://192.168.1.2",
            "http://[fd00::1]",
        ] {
            assert!(validate_provider_url(url, false).is_err(), "{url}");
            assert!(validate_provider_url(url, true).is_ok(), "{url}");
        }
    }

    #[test]
    fn rejects_unsafe_special_addresses_even_with_local_policy() {
        for url in [
            "http://0.0.0.0",
            "http://255.255.255.255",
            "http://224.0.0.1",
            "http://[::]",
            "http://[fe80::1]",
            "http://[ff02::1]",
            "http://[2001:db8::1]",
        ] {
            assert!(validate_provider_url(url, false).is_err(), "{url}");
            assert!(validate_provider_url(url, true).is_err(), "{url}");
        }
        for url in [
            "http://169.254.169.254/latest/meta-data/",
            "http://169.254.170.2/",
            "http://168.63.129.16/",
            "http://[fd00:ec2::254]/latest/meta-data/",
        ] {
            assert!(validate_provider_url(url, false).is_err(), "{url}");
            assert!(validate_provider_url(url, true).is_err(), "{url}");
        }
    }

    #[tokio::test]
    async fn provider_client_enforces_policy_and_builds_for_explicit_local_upstreams() {
        let denied = provider_client(
            "http://127.0.0.1:9876/v1",
            false,
            Duration::from_secs(1),
            Duration::from_secs(1),
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
            "ExoRoute/test",
            UpstreamSettings::default(),
        )
        .await
        .expect("explicit local provider is pinned");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        drop(client);
    }

    #[test]
    fn rejects_local_and_reserved_hostnames_by_default() {
        for host in [
            "localhost.",
            "metadata.google.internal",
            "db.home.arpa",
            "printer.local",
            "service",
            "example.test",
        ] {
            let url = format!("http://{host}");
            assert!(validate_provider_url(&url, false).is_err(), "{host}");
            assert!(validate_provider_url(&url, true).is_ok(), "{host}");
        }
    }

    #[test]
    fn classifies_mapped_ipv4_and_public_ipv6() {
        assert_eq!(
            classify_ip("::ffff:192.168.1.1".parse().unwrap()),
            IpClass::Local
        );
        assert_eq!(
            classify_ip("2001:4860:4860::8888".parse().unwrap()),
            IpClass::Public
        );
        assert_eq!(
            classify_ip("100.64.0.1".parse().unwrap()),
            IpClass::AlwaysBlocked
        );
    }
}
