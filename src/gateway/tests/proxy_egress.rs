//! End-to-end proof that a provider bound to a proxy egresses through it.
//!
//! The provider's base URL is a local mock upstream and the proxy is a second
//! local mock server (both loopback, which the explicit local-provider
//! permission allows). For an `http://` destination reqwest sends an
//! absolute-form request target to the configured proxy, so the proxy mock can
//! answer directly with a canned completion; the test proves the gateway
//! dialed the proxy mock and never the upstream mock directly, and that a
//! disabled bound proxy fails the request instead of silently egressing
//! directly.

use super::support::*;
use super::*;
use std::sync::Arc;

/// Serves one JSON completion and counts calls, so a test can prove the
/// gateway never dialed the upstream directly.
async fn counted_completion_upstream(
    State(calls): State<Arc<AtomicUsize>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    calls.fetch_add(1, Ordering::Relaxed);
    Json(json!({
        "id":"proxy-e2e-upstream",
        "model":body["model"],
        "choices":[{"message":{"role":"assistant","content":"dialed directly"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":1,"completion_tokens":1}
    }))
    .into_response()
}

/// The mock forward proxy. reqwest addresses `http://` targets with an
/// absolute-form request line, so the route matches on the path while the
/// captured URI proves the request was really addressed through the proxy.
async fn capturing_proxy(
    State(calls): State<Arc<ProxyMockCalls>>,
    uri: axum::http::Uri,
    Json(body): Json<Value>,
) -> axum::response::Response {
    {
        let mut uris = calls.uris.lock().await;
        uris.push(uri.to_string());
    }
    calls.count.fetch_add(1, Ordering::Relaxed);
    Json(json!({
        "id":"proxy-e2e-proxy",
        "model":body["model"],
        "choices":[{"message":{"role":"assistant","content":"served by the proxy"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":1,"completion_tokens":1}
    }))
    .into_response()
}

#[derive(Default)]
struct ProxyMockCalls {
    count: AtomicUsize,
    uris: Mutex<Vec<String>>,
}

impl ProxyMockCalls {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn served(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
}

fn hex_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

/// Writes one proxy row plus one endpoint row in the layout the proxy store
/// maintains, so egress validation reads real stored shapes.
async fn seed_proxy_with_endpoint(
    db: &crate::infra::storage::Database,
    proxy_id: &str,
    name: &str,
    enabled: bool,
    endpoint_url: &str,
) {
    let now = crate::infra::db::utc_timestamp_now().expect("test timestamp");
    let proxy = Record::new()
        .with("id", Field::Text(proxy_id.to_owned()))
        .with("name", Field::Text(name.to_owned()))
        .with("strategy", Field::Text("priority".to_owned()))
        .with("enabled", Field::Bool(enabled))
        .with("created_at", Field::Text(now.clone()))
        .with("updated_at", Field::Text(now));
    let endpoint_key = format!("px/{}/0000000000/0000000000", hex_component(proxy_id));
    let endpoint = Record::new()
        .with("proxy_id", Field::Text(proxy_id.to_owned()))
        .with("endpoint_id", Field::Text(format!("{proxy_id}-ep-0")))
        .with("ref_proxy_id", Field::Null)
        .with("url", Field::Text(endpoint_url.to_owned()))
        .with("username", Field::Null)
        .with("secret", Field::Null)
        .with("label", Field::Null)
        .with("priority", Field::I64(0))
        .with("enabled", Field::Bool(true));
    let proxy_key = proxy_id.to_owned();
    db.write(move |transaction| {
        transaction.put(Table::Proxies, &proxy_key, &proxy)?;
        transaction.put(Table::ProxyEndpoints, &endpoint_key, &endpoint)
    })
    .await
    .expect("seed proxy with endpoint");
}

/// Binds a seeded provider to a proxy id, the way a provider save does.
async fn bind_provider_proxy(
    db: &crate::infra::storage::Database,
    provider_id: &str,
    proxy_id: &str,
) {
    let provider_id = provider_id.to_owned();
    let proxy_id = proxy_id.to_owned();
    db.write(move |transaction| {
        let mut provider = transaction
            .get::<Record>(Table::Providers, &provider_id)?
            .ok_or(crate::infra::storage::StorageError::NotFound)?;
        provider.insert("proxy_id", Field::Text(proxy_id.clone()));
        transaction.put(Table::Providers, &provider_id, &provider)
    })
    .await
    .expect("bind provider proxy");
}

async fn proxied_gateway_state(
    upstream: std::net::SocketAddr,
    proxy: std::net::SocketAddr,
    proxy_enabled: bool,
) -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = Some([53_u8; 32]);
    config.allow_private_provider_urls = true;
    let state = AppState::new(config, database.db.clone());
    seed_http_provider(
        &database.db,
        format!("http://{upstream}/v1"),
        "chat_completions",
        &["chat_completions"],
    )
    .await;
    bind_provider_proxy(&database.db, "provider", "px-e2e").await;
    seed_proxy_with_endpoint(
        &database.db,
        "px-e2e",
        "Mock proxy",
        proxy_enabled,
        &format!("http://{proxy}"),
    )
    .await;
    crate::support::test_support::seed_route(
        &database.db,
        "coding",
        "Coding",
        &["chat_completions"],
        &[("provider", "mock-model", "chat_completions", 0)],
    )
    .await
    .expect("seed route");
    (database, state)
}

#[tokio::test]
async fn gateway_requests_to_a_proxied_provider_egress_through_the_proxy() {
    let upstream_calls = Arc::new(AtomicUsize::new(0));
    let upstream = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_completion_upstream))
            .with_state(upstream_calls.clone()),
    )
    .await;
    let proxy_calls = ProxyMockCalls::new();
    let proxy = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(capturing_proxy))
            .with_state(proxy_calls.clone()),
    )
    .await;

    let (_database, state) = proxied_gateway_state(upstream, proxy, true).await;
    let status = gateway_completion(&state).await;

    assert_eq!(status, StatusCode::OK, "the proxied request succeeded");
    assert_eq!(
        proxy_calls.served(),
        1,
        "the proxy mock served the gateway request"
    );
    assert_eq!(
        upstream_calls.load(Ordering::Relaxed),
        0,
        "the gateway delegated the connection to the proxy and never dialed the upstream directly"
    );
    let uris = proxy_calls.uris.lock().await;
    assert!(
        uris.first().is_some_and(|uri| uri.starts_with("http://")),
        "the proxy received an absolute-form target for the http destination: {:?}",
        uris.first()
    );
}

#[tokio::test]
async fn a_disabled_bound_proxy_fails_the_request_without_direct_egress() {
    let upstream_calls = Arc::new(AtomicUsize::new(0));
    let upstream = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(counted_completion_upstream))
            .with_state(upstream_calls.clone()),
    )
    .await;
    let proxy_calls = ProxyMockCalls::new();
    let proxy = spawn_upstream(
        Router::new()
            .route("/v1/chat/completions", post(capturing_proxy))
            .with_state(proxy_calls.clone()),
    )
    .await;

    let (_database, state) = proxied_gateway_state(upstream, proxy, false).await;
    let status = gateway_completion(&state).await;

    assert!(
        !status.is_success(),
        "a bound but disabled proxy must fail the request, not egress directly"
    );
    assert_eq!(proxy_calls.served(), 0, "the disabled proxy served nothing");
    assert_eq!(
        upstream_calls.load(Ordering::Relaxed),
        0,
        "no direct connection to the upstream happened either"
    );
}
