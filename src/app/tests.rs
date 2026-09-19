use super::*;
use crate::support::test_support::TestDatabase;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use std::{sync::Arc, time::Duration};
use tokio::{io::AsyncWriteExt, sync::Notify};
use tower::ServiceExt;

fn bind_test_config(bind: std::net::SocketAddr) -> Config {
    let mut config = Config {
        app_dir: std::path::PathBuf::new(),
        bind,
        data_dir: std::path::PathBuf::new(),
        database_path: std::path::PathBuf::new(),
        admin_key: Some("a-strong-admin-password-for-tests".to_owned()),
        master_key: None,
        allow_private_provider_urls: false,
        trust_proxy_headers: false,
        connect_timeout: std::time::Duration::from_secs(1),
        request_timeout: std::time::Duration::from_secs(1),
        stream_idle_timeout: std::time::Duration::from_secs(1),
        circuit_breaker_enabled: true,
        circuit_breaker_threshold: 5,
        circuit_breaker_cooldown: std::time::Duration::from_secs(1),
    };
    config.database_path = std::path::PathBuf::from("test.lmdb");
    config
}

#[test]
fn service_listener_accepts_only_ipv4_and_ipv6_loopback() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    assert!(
        ensure_supported_bind(&bind_test_config(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            8686
        )))
        .is_ok()
    );
    assert!(
        ensure_supported_bind(&bind_test_config(SocketAddr::new(
            IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
            8686
        )))
        .is_ok()
    );
    assert!(
        ensure_supported_bind(&bind_test_config(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            8686
        )))
        .is_err()
    );
    assert!(
        ensure_supported_bind(&bind_test_config(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            8686
        )))
        .is_err()
    );
    assert!(
        ensure_supported_bind(&bind_test_config(SocketAddr::new(
            IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED),
            8686
        )))
        .is_err()
    );
    assert!(
        ensure_supported_bind(&bind_test_config(SocketAddr::new(
            IpAddr::V6(std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1)),
            8686
        )))
        .is_err()
    );
}

#[tokio::test]
async fn health_and_readiness_work_with_initialized_lmdb() {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let app = router(state);

    for path in ["/health", "/ready", "/health/ready"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .expect("health request"),
            )
            .await
            .expect("health response");
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn server_shutdown_is_bounded_when_an_active_request_never_finishes() {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("test listener");
    let address = listener.local_addr().expect("test listener address");
    let request_started = Arc::new(Notify::new());
    let handler_started = request_started.clone();
    let app = Router::new().route(
        "/hang",
        get(move || {
            let handler_started = handler_started.clone();
            async move {
                handler_started.notify_one();
                std::future::pending::<Response>().await
            }
        }),
    );
    let (signal_sender, signal_receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(serve_with_shutdown_signal(
        listener,
        app,
        state.clone(),
        async move {
            let _ = signal_receiver.await;
        },
    ));
    let mut client = tokio::net::TcpStream::connect(address)
        .await
        .expect("connect to test server");
    client
        .write_all(b"GET /hang HTTP/1.1\r\nHost: localhost\r\nConnection: keep-alive\r\n\r\n")
        .await
        .expect("send hanging request");
    tokio::time::timeout(Duration::from_secs(1), request_started.notified())
        .await
        .expect("request reached the test handler");

    signal_sender.send(()).expect("send shutdown signal");
    let result = tokio::time::timeout(
        SERVER_SHUTDOWN_GRACE_PERIOD + Duration::from_secs(1),
        server,
    )
    .await
    .expect("server shutdown deadline")
    .expect("server task result");
    assert!(result.is_ok());
}

#[tokio::test]
async fn anonymous_session_bootstrap_does_not_exhaust_the_login_rate_limit() {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    let app = router(state.clone());

    for _ in 0..8 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/admin/auth/refresh")
                    .header("host", "localhost:8686")
                    .header("origin", "http://localhost:8686")
                    .header("sec-fetch-site", "same-origin")
                    .body(Body::empty())
                    .expect("anonymous refresh request"),
            )
            .await
            .expect("anonymous refresh response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    assert_eq!(state.admin.admin_login_limiter.entry_count(), 0);
    assert_eq!(state.admin.admin_refresh_limiter.entry_count(), 0);
}

#[tokio::test]
async fn gateway_accepts_json_above_axum_default_body_limit() {
    let database = TestDatabase::open().await;
    let token = "body-limit-test-client-key";
    crate::support::test_support::seed_api_key(
        &database.db,
        "body-limit-test-key",
        "Body limit test",
        token,
    )
    .await
    .expect("seed gateway key");
    let state = AppState::new(
        bind_test_config(database.config().bind),
        database.db.clone(),
    );
    let app = router(state);
    let mut body = String::from(r#"{"messages":[],"padding":""#);
    body.push_str(&"x".repeat(3 * 1024 * 1024));
    body.push_str(r#""}"#);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::from(body))
                .expect("large gateway request"),
        )
        .await
        .expect("gateway response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn gateway_rate_limit_returns_retry_after_and_updates_usage_in_lmdb() {
    let database = TestDatabase::open().await;
    let token = "rate-limit-test-client-key";
    crate::support::test_support::seed_api_key(
        &database.db,
        "rate-limit-test-key",
        "Rate limit test",
        token,
    )
    .await
    .expect("seed gateway key");
    let state = AppState::new(
        bind_test_config(database.config().bind),
        database.db.clone(),
    );
    let mut settings = state.operational_settings().settings;
    settings.gateway_key_capacity = 1;
    settings.gateway_key_refill_tokens = 1;
    settings.gateway_key_refill_interval = std::time::Duration::from_secs(60);
    state.apply_operational_settings(OperationalSettingsRecord {
        settings,
        revision: 1,
        overridden: true,
    });
    let app = router(state.clone());

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .expect("first gateway request"),
        )
        .await
        .expect("first gateway response");
    assert_eq!(first.status(), StatusCode::OK);
    let limited = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .expect("second gateway request"),
        )
        .await
        .expect("second gateway response");
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited.headers()["retry-after"], "60");

    state.telemetry.flush().await.expect("flush usage");
    let request_count = database
        .db
        .read(|transaction| {
            transaction
                .get::<crate::infra::storage::Record>(
                    crate::infra::storage::Table::ApiKeys,
                    "rate-limit-test-key",
                )?
                .ok_or(crate::infra::storage::StorageError::NotFound)?
                .integer("request_count")
        })
        .await
        .expect("read key usage");
    assert_eq!(request_count, 1);
}
