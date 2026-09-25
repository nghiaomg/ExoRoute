//! Provider header-name policy.
//!
//! An operator-supplied header must never replace HTTP framing, hop-by-hop
//! transport headers, or credentials owned by ExoRoute.

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
}
