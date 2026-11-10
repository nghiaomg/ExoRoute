use super::*;
use crate::state::ProxyCheckPermitOutcome;
use crate::support::mock_upstream::spawn_upstream;
use crate::support::test_support::TestDatabase;
use check::{CHECK_CONNECT_TIMEOUT, CHECK_TOTAL_TIMEOUT, CheckableEndpoint, check_endpoints};
use tokio::net::TcpListener;

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
    assert_eq!(
        validate_proxy(&mut empty),
        Err("proxy must have at least one endpoint")
    );

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
    assert_eq!(
        valid.endpoints[0].url.as_deref(),
        Some("http://proxy.example:8080/")
    );

    let mut invalid = input(vec![endpoint(
        Some("proxy.example:8080"),
        None,
        MAX_PROXY_ENDPOINTS as u32 + 1,
    )]);
    assert_eq!(
        validate_proxy(&mut invalid),
        Err("proxy endpoint priority is out of range")
    );
}

/// A loopback port that accepts no connections: bound, measured, then dropped
/// so a probe of it fails fast with connection refused.
async fn dead_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("dead listener");
    let port = listener.local_addr().expect("dead listener address").port();
    drop(listener);
    port
}

async fn check_state() -> (AppState, u16) {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let port = dead_port().await;
    (state, port)
}

fn check_proxy_input(dead_port: u16, disabled_url: Option<&str>) -> ProxyInput {
    let mut endpoints = vec![json!({
        "url": format!("http://127.0.0.1:{dead_port}"),
        "priority": 0,
        "enabled": true
    })];
    if let Some(url) = disabled_url {
        endpoints.push(json!({ "url": url, "priority": 1, "enabled": false }));
    }
    serde_json::from_value(json!({
        "name": "Check proxy",
        "strategy": "priority",
        "enabled": true,
        "endpoints": endpoints
    }))
    .expect("check proxy input")
}

async fn created_proxy_id(state: &AppState, proxy: ProxyInput) -> String {
    let Ok(Json(created)) = create_proxy(State(state.clone()), Json(proxy)).await else {
        panic!("proxy saves");
    };
    created["id"].as_str().expect("created proxy id").to_owned()
}

#[tokio::test]
async fn proxy_check_returns_404_for_unknown_proxy() {
    let (state, _port) = check_state().await;
    match check_proxy(State(state), Path("missing-proxy".to_owned())).await {
        Ok(_) => panic!("an unknown proxy must not be checked"),
        Err((status, Json(body))) => {
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert_eq!(body["error"]["message"], "proxy not found");
        }
    }
}

#[tokio::test]
async fn proxy_check_classifies_reachable_and_dead_endpoints() {
    let mock = spawn_upstream(Router::new().fallback(|| async { StatusCode::NO_CONTENT })).await;
    let port = dead_port().await;
    let target = format!("http://127.0.0.1:{}/probe", mock.port());
    let endpoints = vec![
        CheckableEndpoint {
            endpoint_id: "live".to_owned(),
            label: Some("mock".to_owned()),
            url: format!("http://127.0.0.1:{}", mock.port()),
            username: None,
            secret: None,
        },
        CheckableEndpoint {
            endpoint_id: "authed".to_owned(),
            label: None,
            url: format!("http://127.0.0.1:{}", mock.port()),
            username: Some("user".to_owned()),
            secret: Some("super-secret-password".to_owned()),
        },
        CheckableEndpoint {
            endpoint_id: "dead".to_owned(),
            label: None,
            url: format!("http://127.0.0.1:{port}"),
            username: None,
            secret: None,
        },
    ];

    let results = check_endpoints(
        endpoints,
        &target,
        CHECK_CONNECT_TIMEOUT,
        CHECK_TOTAL_TIMEOUT,
    )
    .await;

    assert_eq!(results.len(), 3);
    assert!(
        results[0].live,
        "the mock proxy answers the probe: {:?}",
        results[0]
    );
    assert!(
        results[1].live,
        "an authenticated mock proxy answers too: {:?}",
        results[1]
    );
    assert!(results[0].latency_millis <= CHECK_TOTAL_TIMEOUT.as_millis() as u64);
    assert!(
        !results[2].live,
        "a closed port cannot answer: {:?}",
        results[2]
    );
    let error = results[2]
        .error
        .as_deref()
        .expect("dead endpoint reports an error");
    assert!(!error.contains("super-secret-password"));
    assert!(!error.contains("user"));
    assert!(!error.contains("127.0.0.1"));
    assert!(error.len() < 120, "errors stay short: {error}");
}

#[tokio::test]
async fn proxy_check_excludes_disabled_endpoints_and_publishes_the_summary() {
    let (state, port) = check_state().await;
    let id = created_proxy_id(&state, check_proxy_input(port, Some("http://10.9.8.7:9"))).await;

    let Ok(Json(response)) = check_proxy(State(state.clone()), Path(id.clone())).await else {
        panic!("the check completes");
    };
    let results = response["results"].as_array().expect("check results");
    assert_eq!(results.len(), 1, "disabled endpoints are excluded");
    assert_eq!(results[0]["status"], "die");
    assert_eq!(results[0]["endpoint_id"].as_str().map(str::len), Some(36));
    assert_eq!(response["proxy_id"], id);
    assert_eq!(response["live_count"], 0);
    assert_eq!(response["die_count"], 1);
    assert!(response["checked_at"].as_u64().is_some());

    let Ok(Json(listed)) = list_proxies(State(state.clone())).await else {
        panic!("the proxy list renders");
    };
    let proxies = listed["proxies"].as_array().expect("proxy list");
    assert_eq!(proxies.len(), 1);
    let last_check = &proxies[0]["last_check"];
    assert_eq!(last_check["live_count"], 0);
    assert_eq!(last_check["die_count"], 1);
    assert!(last_check["checked_at"].as_u64().is_some());
}

#[tokio::test]
async fn proxy_update_clears_the_stored_check_result() {
    let (state, port) = check_state().await;
    let id = created_proxy_id(&state, check_proxy_input(port, None)).await;
    let Ok(Json(_)) = check_proxy(State(state.clone()), Path(id.clone())).await else {
        panic!("the check completes");
    };

    let update: ProxyInput = serde_json::from_value(json!({
        "id": id,
        "name": "Check proxy renamed",
        "strategy": "priority",
        "enabled": true,
        "endpoints": [{ "url": format!("http://127.0.0.1:{port}"), "priority": 0, "enabled": true }]
    }))
    .expect("proxy update input");
    let Ok(Json(_)) = update_proxy(State(state.clone()), Path(id.clone()), Json(update)).await
    else {
        panic!("the proxy updates");
    };

    let Ok(Json(listed)) = list_proxies(State(state.clone())).await else {
        panic!("the proxy list renders");
    };
    assert!(listed["proxies"][0]["last_check"].is_null());
}

#[tokio::test]
async fn proxy_delete_clears_the_stored_check_result() {
    let (state, port) = check_state().await;
    let id = created_proxy_id(&state, check_proxy_input(port, None)).await;
    let Ok(Json(_)) = check_proxy(State(state.clone()), Path(id.clone())).await else {
        panic!("the check completes");
    };

    let Ok(Json(_)) = delete_proxy(State(state.clone()), Path(id.clone())).await else {
        panic!("the proxy deletes");
    };
    let Ok(Json(listed)) = list_proxies(State(state)).await else {
        panic!("the proxy list renders");
    };
    assert_eq!(listed["proxies"].as_array().expect("proxy list").len(), 0);
}

#[tokio::test]
async fn concurrent_proxy_check_returns_a_sanitized_conflict() {
    let (state, port) = check_state().await;
    let id = created_proxy_id(&state, check_proxy_input(port, None)).await;

    let permit = match state.try_proxy_check_permit(&id).await {
        ProxyCheckPermitOutcome::Acquired(permit) => permit,
        _ => panic!("the first check permit is available"),
    };
    match check_proxy(State(state.clone()), Path(id.clone())).await {
        Ok(_) => panic!("a second concurrent check must conflict"),
        Err((status, Json(body))) => {
            assert_eq!(status, StatusCode::CONFLICT);
            let message = body["error"]["message"].as_str().expect("error message");
            assert!(message.contains("already running"), "{message}");
        }
    }
    drop(permit);

    let Ok(Json(response)) = check_proxy(State(state), Path(id)).await else {
        panic!("the check runs once the earlier permit is released");
    };
    assert_eq!(response["die_count"], 1);
}
