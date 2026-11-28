use super::*;
use crate::{
    admin::router as admin_router,
    state::RequestLogRecord,
    support::test_support::{TestDatabase, seed_api_key},
};
use axum::{
    body::to_bytes,
    http::{Request as HttpRequest, header},
};
use serde_json::Value;
use tower::ServiceExt;

const TOKEN_A: &str = "exo_11111111-1111-4111-8111-111111111111";
const TOKEN_B: &str = "exo_22222222-2222-4222-8222-222222222222";

async fn fixture() -> (TestDatabase, AppState) {
    let database = TestDatabase::open().await;
    let state = AppState::new(database.config(), database.db.clone());
    seed_api_key(&database.db, "key-a", "Key A", TOKEN_A)
        .await
        .expect("seed key a");
    seed_api_key(&database.db, "key-b", "Key B", TOKEN_B)
        .await
        .expect("seed key b");
    (database, state)
}

async fn call(app: axum::Router, request: HttpRequest<Body>) -> axum::response::Response {
    app.oneshot(request).await.expect("router handles request")
}

async fn login(app: &axum::Router, token: &str) -> axum::response::Response {
    call(
        app.clone(),
        HttpRequest::builder()
            .method("POST")
            .uri("/api/v1/key-portal/login")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({"api_key": token}).to_string()))
            .expect("login request"),
    )
    .await
}

fn session_cookie(response: &axum::response::Response) -> String {
    let header = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("session cookie")
        .to_str()
        .expect("cookie text");
    assert!(header.starts_with("exoroute_key_session="));
    assert!(header.contains("HttpOnly"));
    assert!(header.contains("SameSite=Lax"));
    header
        .split(';')
        .next()
        .expect("cookie pair")
        .split_once('=')
        .expect("cookie value")
        .1
        .to_owned()
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("JSON response")
}

async fn get(app: &axum::Router, path: &str, cookie: &str) -> axum::response::Response {
    call(
        app.clone(),
        HttpRequest::builder()
            .method("GET")
            .uri(path)
            .header(header::COOKIE, format!("exoroute_key_session={cookie}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

fn log(id: &str, key_id: &str, model: &str, status: i64) -> RequestLogRecord {
    RequestLogRecord {
        id: id.to_owned(),
        request_id: id.to_owned(),
        route_alias: "coding".to_owned(),
        provider_id: Some("provider".to_owned()),
        provider_credential_id: None,
        api_key_id: Some(key_id.to_owned()),
        model: model.to_owned(),
        client_protocol: "chat_completions".to_owned(),
        upstream_protocol: None,
        proxy_id: None,
        status,
        duration_ms: 12,
        input_tokens: Some(100),
        output_tokens: Some(20),
        cached_tokens: None,
        cache_input_tokens: None,
        reasoning_effort: None,
        cost_micro_usd: Some(500),
        error: None,
    }
}

fn emit_usage(state: &AppState, key_id: &str, model: &str, succeeded: bool) {
    let analytics = state.telemetry.start_request(key_id.to_owned());
    analytics.set_requested_model(Some(model));
    analytics.set_token_usage(Some(100), Some(20));
    analytics.finish(succeeded);
    state.telemetry.record_api_key_request(key_id.to_owned());
}

#[tokio::test]
async fn valid_key_login_session_and_stats_are_scoped() {
    let (database, state) = fixture().await;
    emit_usage(&state, "key-a", "model-a", true);
    state.enqueue_request_log(log("log-a", "key-a", "model-a", 200));
    state
        .telemetry
        .flush()
        .await
        .expect("flush telemetry before stats");
    let app = super::router().with_state(state.clone());

    let response = login(&app, TOKEN_A).await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = session_cookie(&response);
    let body = json_body(response).await;
    assert_eq!(body["authenticated"], true);
    assert_eq!(body["key"]["id"], "key-a");
    assert!(
        !serde_json::to_string(&body)
            .expect("body")
            .contains(TOKEN_A)
    );

    let session = json_body(get(&app, "/api/v1/key-portal/session", &cookie).await).await;
    assert_eq!(session["key"]["id"], "key-a");
    let stats = json_body(get(&app, "/api/v1/key-portal/stats?range=7d", &cookie).await).await;
    assert_eq!(stats["key"]["id"], "key-a");
    assert_eq!(stats["summary"]["total_requests"], 1);
    assert_eq!(stats["recent_requests"].as_array().map(Vec::len), Some(1));
    assert_eq!(stats["recent_requests"][0]["model"], "model-a");
    drop(database);
}

#[tokio::test]
async fn invalid_revoked_and_disabled_keys_are_rejected() {
    let (database, state) = fixture().await;
    let app = super::router().with_state(state.clone());
    assert_eq!(
        login(&app, "exo_bad").await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(login(&app, "").await.status(), StatusCode::UNAUTHORIZED);

    let key_id = "key-a".to_owned();
    state
        .db
        .write(move |transaction| {
            let mut record = transaction
                .get::<crate::infra::storage::Record>(
                    crate::infra::storage::Table::ApiKeys,
                    &key_id,
                )?
                .expect("key exists");
            record.insert("enabled", crate::infra::storage::Field::Bool(false));
            transaction.put(crate::infra::storage::Table::ApiKeys, &key_id, &record)
        })
        .await
        .expect("disable key");
    assert_eq!(
        login(&app, TOKEN_A).await.status(),
        StatusCode::UNAUTHORIZED
    );
    drop(database);
}

#[tokio::test]
async fn key_portal_cookie_cannot_reach_admin_routes() {
    let (_database, state) = fixture().await;
    let key_portal = super::router().with_state(state.clone());
    let cookie = session_cookie(&login(&key_portal, TOKEN_A).await);
    let admin = admin_router(state.clone()).with_state(state.clone());
    for path in [
        "/api/v1/admin/api-keys",
        "/api/v1/admin/providers",
        "/api/v1/admin/overview",
    ] {
        let response = call(
            admin.clone(),
            HttpRequest::builder()
                .uri(path)
                .header(header::COOKIE, format!("exoroute_key_session={cookie}"))
                .body(Body::empty())
                .expect("admin request"),
        )
        .await;
        assert!(
            matches!(
                response.status(),
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ),
            "key cookie reached {path}: {}",
            response.status()
        );
    }
}

#[tokio::test]
async fn stats_never_leak_another_keys_requests() {
    let (database, state) = fixture().await;
    emit_usage(&state, "key-a", "model-a", true);
    emit_usage(&state, "key-a", "model-a", true);
    state.enqueue_request_log(log("log-a1", "key-a", "model-a", 200));
    state.enqueue_request_log(log("log-a2", "key-a", "model-a", 200));
    emit_usage(&state, "key-b", "secret-model-b", false);
    state.enqueue_request_log(log("log-b1", "key-b", "secret-model-b", 500));
    state
        .telemetry
        .flush()
        .await
        .expect("flush telemetry before stats");
    let app = super::router().with_state(state.clone());
    let cookie = session_cookie(&login(&app, TOKEN_A).await);
    let stats = json_body(get(&app, "/api/v1/key-portal/stats", &cookie).await).await;
    assert_eq!(stats["summary"]["total_requests"], 2);
    let recent = stats["recent_requests"].as_array().expect("recent array");
    assert_eq!(recent.len(), 2);
    assert!(recent.iter().all(|row| row["model"] != "secret-model-b"));
    let rendered = serde_json::to_string(&stats).expect("render stats");
    assert!(!rendered.contains("secret-model-b"));
    drop(database);
}

#[tokio::test]
async fn disabling_after_login_immediately_invalidates_session() {
    let (database, state) = fixture().await;
    let app = super::router().with_state(state.clone());
    let cookie = session_cookie(&login(&app, TOKEN_A).await);
    assert_eq!(
        get(&app, "/api/v1/key-portal/session", &cookie)
            .await
            .status(),
        StatusCode::OK
    );
    let key_id = "key-a".to_owned();
    state
        .db
        .write(move |transaction| {
            let mut record = transaction
                .get::<crate::infra::storage::Record>(
                    crate::infra::storage::Table::ApiKeys,
                    &key_id,
                )?
                .expect("key exists");
            record.insert("enabled", crate::infra::storage::Field::Bool(false));
            transaction.put(crate::infra::storage::Table::ApiKeys, &key_id, &record)
        })
        .await
        .expect("disable key");
    assert_eq!(
        get(&app, "/api/v1/key-portal/session", &cookie)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&app, "/api/v1/key-portal/stats", &cookie)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    drop(database);
}

#[tokio::test]
async fn logout_clears_the_portal_cookie() {
    let (_database, state) = fixture().await;
    let app = super::router().with_state(state.clone());
    let response = call(
        app,
        HttpRequest::builder()
            .method("POST")
            .uri("/api/v1/key-portal/logout")
            .body(Body::empty())
            .expect("logout request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let header = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("clear cookie")
        .to_str()
        .expect("cookie text");
    assert!(header.starts_with("exoroute_key_session=;"));
    assert_eq!(json_body(response).await["authenticated"], false);
}

#[tokio::test]
async fn missing_signing_secret_fails_closed() {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.master_key = None;
    config.admin_key = None;
    let state = AppState::new(config, database.db.clone());
    seed_api_key(&database.db, "key-a", "Key A", TOKEN_A)
        .await
        .expect("seed key");
    let app = super::router().with_state(state.clone());
    let response = login(&app, TOKEN_A).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response.headers().get(header::SET_COOKIE).is_none());

    let forged = get(
        &app,
        "/api/v1/key-portal/session",
        "eyJ2IjoxLCJrIjoia2V5LWEiLCJlIjo5OTk5OTk5OTk5LCJuIjoibm9uY2UiLCJ0IjoiMDEyMzQ1Njc4In0.bad",
    )
    .await;
    assert_eq!(forged.status(), StatusCode::UNAUTHORIZED);
    drop(database);
}

#[tokio::test]
async fn logout_revokes_a_copied_cookie() {
    let (_database, state) = fixture().await;
    let app = super::router().with_state(state.clone());
    let cookie = session_cookie(&login(&app, TOKEN_A).await);
    assert_eq!(
        get(&app, "/api/v1/key-portal/session", &cookie)
            .await
            .status(),
        StatusCode::OK
    );
    let logout = call(
        app.clone(),
        HttpRequest::builder()
            .method("POST")
            .uri("/api/v1/key-portal/logout")
            .header(header::COOKIE, format!("exoroute_key_session={cookie}"))
            .body(Body::empty())
            .expect("logout request"),
    )
    .await;
    assert_eq!(logout.status(), StatusCode::OK);
    assert_eq!(
        get(&app, "/api/v1/key-portal/session", &cookie)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&app, "/api/v1/key-portal/stats", &cookie)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn key_portal_rate_limit_is_independent_from_admin_login() {
    let (_database, state) = fixture().await;
    let portal = super::router().with_state(state.clone());
    let mut saw_limit = false;
    for _ in 0..8 {
        let status = login(&portal, "exo_bad").await.status();
        if status == StatusCode::TOO_MANY_REQUESTS {
            saw_limit = true;
            break;
        }
    }
    assert!(saw_limit, "key portal limiter was not exhausted");
    let peer = "127.0.0.1";
    assert!(
        state.admin.admin_login_limiter.check(peer).allowed,
        "admin login limiter must be untouched by key portal attempts"
    );
}

#[tokio::test]
async fn authenticated_responses_have_no_store_and_scoped_cookie_path() {
    let (_database, state) = fixture().await;
    let app = super::router().with_state(state.clone());
    let login_response = login(&app, TOKEN_A).await;
    assert_eq!(
        login_response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    let set_cookie = login_response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("session cookie");
    assert!(set_cookie.contains("Path=/api/v1/key-portal"));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    let cookie = session_cookie(&login_response);

    for path in ["/api/v1/key-portal/session", "/api/v1/key-portal/stats"] {
        let response = get(&app, path, &cookie).await;
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-store"),
            "missing no-store on {path}"
        );
    }
    let logout = call(
        app.clone(),
        HttpRequest::builder()
            .method("POST")
            .uri("/api/v1/key-portal/logout")
            .header(header::COOKIE, format!("exoroute_key_session={cookie}"))
            .body(Body::empty())
            .expect("logout request"),
    )
    .await;
    assert_eq!(
        logout
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    let clear = logout
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("clear cookie");
    assert!(clear.contains("Path=/api/v1/key-portal"));
}
