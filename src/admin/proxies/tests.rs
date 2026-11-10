use super::*;

fn endpoint(url: Option<&str>, reference: Option<&str>, priority: u32) -> ProxyEndpointInput {
    ProxyEndpointInput {
        endpoint_id: uuid::Uuid::new_v4().to_string(),
        url: url.map(str::to_owned),
        ref_proxy_id: reference.map(str::to_owned),
        username: None,
        secret: None,
        secret_ciphertext: None,
        has_secret: false,
        label: None,
        priority,
        enabled: true,
    }
}

fn input(endpoints: Vec<ProxyEndpointInput>) -> ProxyInput {
    ProxyInput {
        id: String::new(),
        name: "Test proxy".to_owned(),
        strategy: "priority".to_owned(),
        enabled: true,
        endpoints,
    }
}

#[test]
fn normalizes_bare_host_port_and_rejects_unsupported_schemes() {
    assert_eq!(
        normalize_proxy_url(" 10.0.0.1:8080 ").expect("bare endpoint parses"),
        "http://10.0.0.1:8080/"
    );
    assert_eq!(
        normalize_proxy_url("https://proxy.example:8443").expect("https endpoint parses"),
        "https://proxy.example:8443/"
    );
    assert!(normalize_proxy_url("socks5h://proxy.example:1080").is_err());
    assert!(normalize_proxy_url("http://user:secret@proxy.example:8080").is_err());
}

#[test]
fn proxy_requires_endpoints_and_exclusive_endpoint_kind() {
    let mut empty = input(Vec::new());
    assert_eq!(validate_proxy(&mut empty), Err("proxy must have at least one endpoint"));

    let mut mixed = input(vec![endpoint(Some("proxy.example:8080"), Some("child"), 0)]);
    assert_eq!(
        validate_proxy(&mut mixed),
        Err("an endpoint is either a proxy server URL or an embedded proxy, not both")
    );
}

#[test]
fn endpoint_priority_is_bounded_and_url_is_canonicalized() {
    let mut valid = input(vec![endpoint(Some("proxy.example:8080"), None, 1)]);
    validate_proxy(&mut valid).expect("valid proxy");
    assert_eq!(valid.endpoints[0].url.as_deref(), Some("http://proxy.example:8080/"));

    let mut invalid = input(vec![endpoint(Some("proxy.example:8080"), None, MAX_PROXY_ENDPOINTS as u32 + 1)]);
    assert_eq!(validate_proxy(&mut invalid), Err("proxy endpoint priority is out of range"));
}
