//! Provider URL syntax, resolved-address, and pinned-client policy.
//!
//! `validate_provider_url` is a synchronous syntax and literal-IP check. For
//! hostnames, resolve with `resolve_provider_url` and use the returned addresses
//! to pin the connection. DNS validation is point-in-time only; a caller that
//! needs SSRF protection against DNS rebinding must enforce the same policy at
//! connect time (or use a trusted egress proxy). Redirects are disabled by
//! `build_pinned_client` and must be revalidated at every hop if re-enabled.

use reqwest::Url;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

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
