use axum::{
    extract::connect_info::ConnectInfo,
    http::{HeaderMap, Request},
};
use std::net::{IpAddr, SocketAddr};

/// Resolve the client address for per-client quotas.
///
/// Forwarded addresses are accepted only when explicitly enabled and the
/// direct peer is loopback. The TLS proxy must overwrite X-Forwarded-For with
/// exactly one client IP; comma-separated chains are intentionally rejected.
pub fn client_ip<B>(request: &Request<B>, trust_proxy_headers: bool) -> String {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    client_ip_from_headers(peer, request.headers(), trust_proxy_headers)
}

fn client_ip_from_headers(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trust_proxy_headers: bool,
) -> String {
    if trust_proxy_headers && peer.is_some_and(|address| address.is_loopback()) {
        let forwarded_values = headers.get_all("x-forwarded-for");
        if forwarded_values.iter().count() == 1
            && let Some(forwarded) = forwarded_values
                .iter()
                .next()
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<IpAddr>().ok())
        {
            return forwarded.to_string();
        }
    }

    peer.map(|address| address.to_string())
        .unwrap_or_else(|| "unknown-peer".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn forwarded_address_is_used_only_for_explicitly_trusted_loopback_proxy() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.9"));
        let proxy = "127.0.0.1".parse().expect("loopback IP");
        let remote = "198.51.100.7".parse().expect("remote IP");

        assert_eq!(
            client_ip_from_headers(Some(proxy), &headers, true),
            "203.0.113.9"
        );
        assert_eq!(
            client_ip_from_headers(Some(proxy), &headers, false),
            "127.0.0.1"
        );
        assert_eq!(
            client_ip_from_headers(Some(remote), &headers, true),
            "198.51.100.7"
        );
    }

    #[test]
    fn forwarded_chains_and_invalid_values_fall_back_to_peer() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.9, 127.0.0.1"),
        );
        let proxy = "127.0.0.1".parse().expect("loopback IP");
        assert_eq!(
            client_ip_from_headers(Some(proxy), &headers, true),
            "127.0.0.1"
        );
        headers.append("x-forwarded-for", HeaderValue::from_static("198.51.100.12"));
        assert_eq!(
            client_ip_from_headers(Some(proxy), &headers, true),
            "127.0.0.1"
        );
        assert_eq!(
            client_ip_from_headers(None, &HeaderMap::new(), true),
            "unknown-peer"
        );
    }
}
