//! Admin session regression tests: replay revocation, expiry deadlines,
//! family-cap eviction, step-up proofs, and cookie policy.

use super::super::cookie_policy::{
    cookie_is_secure, cookie_name, refresh_cookie, same_origin_rejection,
};
use super::*;
use crate::state::AdminStepUpScope;
use crate::{
    infra::storage::{Record, StorageError, Table},
    support::test_support::TestDatabase,
};
use axum::body::Body;
use axum::extract::ConnectInfo;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    ops::Deref,
    time::{Duration, Instant},
};

struct TestState {
    state: AppState,
    _database: TestDatabase,
}

impl Deref for TestState {
    type Target = AppState;

    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

async fn test_state(trust_proxy_headers: bool) -> TestState {
    let database = TestDatabase::open().await;
    let mut config = database.config();
    config.trust_proxy_headers = trust_proxy_headers;
    let state = AppState::new(config, database.db.clone());
    TestState {
        state,
        _database: database,
    }
}

#[tokio::test]
async fn consumed_refresh_replay_revokes_only_its_family() {
    let state = test_state(false).await;
    let first_family = issue_admin_session_at(&state, false, 1_000)
        .await
        .expect("create first family");
    let second_family = issue_admin_session_at(&state, false, 1_000)
        .await
        .expect("create second family");
    let first_hash_key = digest_key(&token_digest(&first_family.refresh_token));
    let first_family_id = state
        .db
        .read(move |transaction| {
            transaction
                .get::<Record>(Table::AdminRefreshTokens, &first_hash_key)?
                .ok_or(StorageError::NotFound)?
                .text("family_id")
                .map(str::to_owned)
        })
        .await
        .expect("first family id");
    let raw_refresh_token = first_family.refresh_token.clone();

    let rotated = rotate_refresh_token_at(&state, &raw_refresh_token, 1_001)
        .await
        .expect("rotate first family");
    let RefreshResult::Rotated(rotated) = rotated else {
        panic!("fresh refresh token must rotate");
    };
    assert_ne!(rotated.refresh_token, raw_refresh_token);
    let stored_hash_key = digest_key(&token_digest(&raw_refresh_token));
    let consumed_generation = state
        .db
        .read(move |transaction| {
            transaction
                .get::<Record>(Table::AdminRefreshTokens, &stored_hash_key)?
                .ok_or(StorageError::NotFound)?
                .optional_integer("consumed_at")
        })
        .await
        .expect("stored refresh hash");
    assert!(consumed_generation.is_some());

    assert!(matches!(
        rotate_refresh_token_at(&state, &raw_refresh_token, 1_002)
            .await
            .expect("replay old refresh token"),
        RefreshResult::Invalid
    ));
    assert!(state.admin_session(&rotated.access_token).await.is_none());
    let revoked_family_id = first_family_id.clone();
    let revoked_at = state
        .db
        .read(move |transaction| {
            transaction
                .get::<Record>(Table::AdminSessionFamilies, &revoked_family_id)?
                .ok_or(StorageError::NotFound)?
                .optional_integer("revoked_at")
        })
        .await
        .expect("first family revocation");
    assert_eq!(revoked_at, Some(1_002));
    assert!(matches!(
        rotate_refresh_token_at(&state, &second_family.refresh_token, 1_002)
            .await
            .expect("unrelated family refresh"),
        RefreshResult::Rotated(_)
    ));
}

#[tokio::test]
async fn refresh_enforces_idle_and_absolute_deadlines() {
    let state = test_state(false).await;
    let start = 1_000_i64;
    let idle_expired = issue_admin_session_at(&state, false, start)
        .await
        .expect("create idle family");
    assert!(matches!(
        rotate_refresh_token_at(
            &state,
            &idle_expired.refresh_token,
            start + IDLE_TTL_SECONDS,
        )
        .await
        .expect("idle-expired refresh"),
        RefreshResult::Invalid
    ));

    let mut session = issue_admin_session_at(&state, false, start)
        .await
        .expect("create absolute-expiry family");
    for elapsed in [23, 46, 69, 92, 115, 138, 161] {
        let rotated =
            rotate_refresh_token_at(&state, &session.refresh_token, start + elapsed * 60 * 60)
                .await
                .expect("refresh before idle expiry");
        let RefreshResult::Rotated(next) = rotated else {
            panic!("active family must rotate before absolute expiry");
        };
        session = next;
    }
    let final_rotation = rotate_refresh_token_at(
        &state,
        &session.refresh_token,
        start + ABSOLUTE_TTL_SECONDS - 1,
    )
    .await
    .expect("refresh just before absolute expiry");
    let RefreshResult::Rotated(last) = final_rotation else {
        panic!("family must remain valid until absolute expiry");
    };
    assert_eq!(last.refresh_max_age, 1);
    assert!(matches!(
        rotate_refresh_token_at(&state, &last.refresh_token, start + ABSOLUTE_TTL_SECONDS,)
            .await
            .expect("absolute-expired refresh"),
        RefreshResult::Invalid
    ));
}

#[tokio::test]
async fn persisted_refresh_session_survives_runtime_reconstruction() {
    let first_runtime = test_state(false).await;
    let original = issue_admin_session(&first_runtime, false)
        .await
        .expect("create persistent family");

    let restarted_runtime = AppState::new(
        first_runtime.config.as_ref().clone(),
        first_runtime.db.clone(),
    );
    assert!(
        restarted_runtime
            .admin_session(&original.access_token)
            .await
            .is_none()
    );

    let result = rotate_refresh_token(&restarted_runtime, &original.refresh_token)
        .await
        .expect("refresh after restart");
    let RefreshResult::Rotated(refreshed) = result else {
        panic!("persisted refresh token must restore a session after restart");
    };
    assert_ne!(refreshed.access_token, original.access_token);
    assert_ne!(refreshed.refresh_token, original.refresh_token);
    assert!(
        restarted_runtime
            .admin_session(&refreshed.access_token)
            .await
            .is_some()
    );
}

#[tokio::test]
async fn session_family_cap_evicts_the_oldest_active_family() {
    let state = test_state(false).await;
    let mut oldest_access = String::new();
    for index in 0..=MAX_SESSION_FAMILIES {
        let session = issue_admin_session_at(&state, false, 10_000 + index)
            .await
            .expect("create bounded session family");
        if index == 0 {
            oldest_access = session.access_token;
        }
    }

    let active_count = state
        .db
        .read(|transaction| {
            let records = transaction.scan_prefix::<Record>(
                Table::AdminSessionFamilies,
                "",
                usize::try_from(MAX_SESSION_FAMILIES).unwrap_or(128) + 1,
            )?;
            records.iter().try_fold(0_i64, |count, (_, record)| {
                Ok::<_, StorageError>(
                    count + i64::from(record.optional_integer("revoked_at")?.is_none()),
                )
            })
        })
        .await
        .expect("count active session families");
    assert_eq!(active_count, MAX_SESSION_FAMILIES);
    assert!(state.admin_session(&oldest_access).await.is_none());
}

#[tokio::test]
async fn step_up_proof_is_one_use_and_bound_to_family_and_scope() {
    let state = test_state(false).await;
    let proof = state
        .issue_admin_step_up_proof("family-a".to_owned(), AdminStepUpScope::DatabaseExport)
        .await
        .expect("proof entropy");
    assert!(
        !state
            .consume_admin_step_up_proof("family-a", &proof, AdminStepUpScope::DatabaseImport,)
            .await
    );
    assert!(
        !state
            .consume_admin_step_up_proof("family-a", &proof, AdminStepUpScope::DatabaseExport,)
            .await
    );

    let proof = state
        .issue_admin_step_up_proof("family-a".to_owned(), AdminStepUpScope::DatabaseImport)
        .await
        .expect("proof entropy");
    assert!(
        !state
            .consume_admin_step_up_proof("family-b", &proof, AdminStepUpScope::DatabaseImport,)
            .await
    );
    assert!(
        !state
            .consume_admin_step_up_proof("family-a", &proof, AdminStepUpScope::DatabaseImport,)
            .await
    );

    let expired = state
        .issue_admin_step_up_proof("family-a".to_owned(), AdminStepUpScope::DatabaseExport)
        .await
        .expect("proof entropy");
    if let Some(proof) = state
        .admin
        .admin_step_up_proofs
        .write()
        .await
        .get_mut(&token_digest(&expired))
    {
        proof.expires_at = Instant::now() - Duration::from_secs(1);
    }
    assert!(
        !state
            .consume_admin_step_up_proof("family-a", &expired, AdminStepUpScope::DatabaseExport,)
            .await
    );
}

#[tokio::test]
async fn cookie_scope_origin_and_trusted_proxy_scheme_are_enforced() {
    let state = test_state(true).await;
    let cookie_value = URL_SAFE_NO_PAD.encode([7_u8; 32]);
    let name = cookie_name(&state);
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/auth/refresh")
        .header(header::HOST, "localhost:8686")
        .header(header::ORIGIN, "http://localhost:8686")
        .header("sec-fetch-site", "same-origin")
        .header(
            header::COOKIE,
            format!("{name}={cookie_value}; other=unrelated"),
        )
        .body(Body::empty())
        .expect("same-origin request");
    assert!(same_origin_rejection(&state, &request).is_none());
    assert_eq!(
        refresh_cookie(request.headers(), &state),
        Some(cookie_value.clone())
    );
    assert!(!cookie_is_secure(&state, &request));

    let duplicate_cookie = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/auth/refresh")
        .header(
            header::COOKIE,
            format!("{name}={cookie_value}; {name}={cookie_value}"),
        )
        .body(Body::empty())
        .expect("duplicate cookie request");
    assert!(refresh_cookie(duplicate_cookie.headers(), &state).is_none());

    let mut proxied = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/auth/refresh")
        .header(header::HOST, "exoroute.example:443")
        .header(header::ORIGIN, "https://exoroute.example")
        .header("sec-fetch-site", "same-origin")
        .header("x-forwarded-proto", "https")
        .body(Body::empty())
        .expect("proxied HTTPS request");
    proxied.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        50_000,
    )));
    assert!(same_origin_rejection(&state, &proxied).is_none());
    assert!(cookie_is_secure(&state, &proxied));

    proxied.extensions_mut().insert(ConnectInfo(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)),
        50_000,
    )));
    assert!(same_origin_rejection(&state, &proxied).is_some());
    assert!(!cookie_is_secure(&state, &proxied));
}

#[tokio::test]
async fn same_origin_posts_from_browsers_without_fetch_metadata_are_accepted() {
    let state = test_state(false).await;
    let post = |host: &'static str, origin: &'static str| {
        Request::builder()
            .method("POST")
            .uri("/api/v1/admin/auth/refresh")
            .header(header::HOST, host)
            .header(header::ORIGIN, origin)
            .body(Body::empty())
            .expect("same-origin request")
    };

    // A plain-HTTP origin that is not `localhost` receives no `Sec-Fetch-*`
    // headers from the browser, and engines older than Safari 16.4 send none at
    // all. The Origin/Host comparison still proves the request is same-origin.
    assert!(
        same_origin_rejection(
            &state,
            &post("exoroute.lan:8686", "http://exoroute.lan:8686")
        )
        .is_none()
    );
    assert!(
        same_origin_rejection(&state, &post("localhost:8686", "http://localhost:8686")).is_none()
    );

    // When the browser does send Fetch Metadata it stays authoritative, because
    // page script cannot forge it.
    let cross_site = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/auth/refresh")
        .header(header::HOST, "localhost:8686")
        .header(header::ORIGIN, "http://localhost:8686")
        .header("sec-fetch-site", "cross-site")
        .body(Body::empty())
        .expect("cross-site request");
    assert_eq!(
        same_origin_rejection(&state, &cross_site),
        Some("cross-site-fetch-metadata")
    );
    let duplicated = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/auth/refresh")
        .header(header::HOST, "localhost:8686")
        .header(header::ORIGIN, "http://localhost:8686")
        .header("sec-fetch-site", "same-origin")
        .header("sec-fetch-site", "same-origin")
        .body(Body::empty())
        .expect("duplicated fetch metadata");
    assert_eq!(
        same_origin_rejection(&state, &duplicated),
        Some("duplicate-sec-fetch-site")
    );

    // Without an Origin the initiator is unknown: a non-browser client gets the
    // same rejection as before.
    let no_origin = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/auth/refresh")
        .header(header::HOST, "localhost:8686")
        .body(Body::empty())
        .expect("header-less request");
    assert!(same_origin_rejection(&state, &no_origin).is_some());

    // A dashboard reached over HTTPS behind a proxy still has to declare the
    // proxied scheme, otherwise the origin scheme cannot be verified.
    assert_eq!(
        same_origin_rejection(
            &state,
            &post("exoroute.example:443", "https://exoroute.example")
        ),
        Some("origin-scheme-mismatch")
    );

    // The development dashboard is served from the Vite port and proxied to the
    // API, so Origin and Host describe the dev server rather than the API. That
    // passes while the proxy forwards the browser's Host header; a proxy that
    // rewrites Host into the API authority makes every refresh and logout look
    // cross-origin, which is what the Vite configuration now avoids.
    assert!(
        same_origin_rejection(&state, &post("localhost:5173", "http://localhost:5173")).is_none()
    );
    assert_eq!(
        same_origin_rejection(&state, &post("localhost:8686", "http://localhost:5173")),
        Some("origin-host-mismatch")
    );
}

#[tokio::test]
async fn cookie_less_refresh_does_not_consume_refresh_or_login_limits() {
    let state = test_state(false).await;
    let request = || {
        Request::builder()
            .method("POST")
            .uri("/api/v1/admin/auth/refresh")
            .header(header::HOST, "localhost:8686")
            .header(header::ORIGIN, "http://localhost:8686")
            .header("sec-fetch-site", "same-origin")
            .body(Body::empty())
            .expect("same-origin anonymous refresh request")
    };

    let response = refresh(State(state.state.clone()), request())
        .await
        .expect("anonymous refresh response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(state.admin.admin_login_limiter.entry_count(), 0);
    assert_eq!(state.admin.admin_refresh_limiter.entry_count(), 0);
}

#[tokio::test]
async fn credentialed_refresh_uses_its_own_rate_limit_bucket() {
    let state = test_state(false).await;
    let cookie_value = URL_SAFE_NO_PAD.encode([8_u8; 32]);
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/auth/refresh")
        .header(header::HOST, "localhost:8686")
        .header(header::ORIGIN, "http://localhost:8686")
        .header("sec-fetch-site", "same-origin")
        .header(
            header::COOKIE,
            format!("{}={cookie_value}", cookie_name(&state)),
        )
        .body(Body::empty())
        .expect("same-origin credentialed refresh request");
    let response = refresh(State(state.state.clone()), request)
        .await
        .expect("credentialed refresh response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(state.admin.admin_login_limiter.entry_count(), 0);
    assert_eq!(state.admin.admin_refresh_limiter.entry_count(), 1);
}
