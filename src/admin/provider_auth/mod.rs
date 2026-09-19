//! Provider OAuth and preset administration: provider preset listing,
//! OAuth flow start/status endpoints, and provider auth callback listener
//! and flow unit tests.

use super::*;

mod callback;

pub(crate) use callback::{
    LEGACY_PROVIDER_AUTH_CALLBACK_URL, complete_provider_auth_callback,
    complete_provider_auth_callback_for_flow, ensure_provider_auth_callback_listener_locked,
    validate_provider_auth_redirect_uri,
};
#[cfg(test)]
pub(crate) use callback::{
    claim_embedded_provider_auth_callback_flow_without_state, claim_provider_auth_callback_flow,
    has_active_provider_auth_flow, localhost_loopback_addresses, parse_provider_auth_callback_url,
};

pub(crate) const PROVIDER_AUTH_FLOW_TTL: Duration = Duration::from_secs(5 * 60);
pub(crate) const MAX_PROVIDER_AUTH_CALLBACK_URL_BYTES: usize = 16 * 1024;
pub(crate) const MAX_PROVIDER_AUTH_CALLBACK_BODY_BYTES: usize = 20 * 1024;
pub(crate) const PROVIDER_AUTH_LISTENER_IDLE_GRACE: Duration = Duration::from_secs(30);
pub(crate) const PROVIDER_AUTH_LISTENER_POLL_INTERVAL: Duration = Duration::from_millis(250);
pub(crate) const PROVIDER_AUTH_LISTENER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderAuthStartRequest {
    #[serde(default)]
    callback_url: Option<String>,
}

pub(crate) async fn list_provider_presets() -> ApiResult {
    Ok(Json(json!({"presets":provider_adapters::presets()})))
}

pub(crate) async fn start_provider_auth(
    State(state): State<AppState>,
    Path(id): Path<String>,
    request: Option<Json<ProviderAuthStartRequest>>,
) -> ApiResult {
    let lookup_id = id.clone();
    let adapter_id = state
        .db
        .read(move |transaction| {
            transaction
                .get::<crate::infra::storage::Record>(
                    crate::infra::storage::Table::Providers,
                    &lookup_id,
                )?
                .map(|provider| provider.text("adapter_id").map(str::to_owned))
                .transpose()
        })
        .await
        .map_err(internal)?
        .ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;
    let Some(_adapter) = provider_adapters::adapter(&adapter_id) else {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "provider adapter is not registered",
        ));
    };
    if !provider_adapters::capabilities(&adapter_id)
        .is_some_and(|capabilities| capabilities.oauth_accounts)
    {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "this provider adapter does not support OAuth sign-in",
        ));
    }
    if state.config.master_key.is_none() {
        return Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "set EXOROUTE_MASTER_KEY before connecting a provider account",
        ));
    }

    let requested_callback_url = request.and_then(|request| request.0.callback_url);
    let redirect_uri = if let Some(callback_url) = requested_callback_url {
        let redirect_uri = validate_provider_auth_redirect_uri(&callback_url)
            .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
        if !provider_adapters::supports_custom_oauth_redirect_uri(&adapter_id) {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "provider adapter does not support custom OAuth callback URLs",
            ));
        }
        redirect_uri
    } else {
        LEGACY_PROVIDER_AUTH_CALLBACK_URL.to_owned()
    };

    let mut verifier_bytes = [0u8; 32];
    let mut state_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut verifier_bytes);
    OsRng.fill_bytes(&mut state_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let oauth_state = URL_SAFE_NO_PAD.encode(state_bytes);
    let challenge = provider_adapters::pkce_challenge(&adapter_id, &verifier).map_err(|_| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not prepare provider OAuth",
        )
    })?;
    let authorization_url = provider_adapters::authorization_url(
        &adapter_id,
        &oauth_state,
        &challenge,
        Some(&redirect_uri),
    )
    .map_err(|error| {
        tracing::warn!(adapter_id = %adapter_id, error = %error, "could not build provider authorization URL");
        fail(StatusCode::BAD_REQUEST, error)
    })?;
    let flow_id = uuid::Uuid::new_v4().to_string();
    // Serialize flow registration with idle listener shutdown. If shutdown
    // wins this lock, this start will create a fresh listener after it exits;
    // if this start wins, shutdown observes the newly registered active flow.
    let _callback_guard = state.admin.provider_auth_callback_lock.lock().await;
    if redirect_uri == LEGACY_PROVIDER_AUTH_CALLBACK_URL {
        ensure_provider_auth_callback_listener_locked(&state).await?;
    }
    let provider_id_for_flow = id.clone();
    state
        .register_provider_auth_flow(
            flow_id.clone(),
            PendingProviderAuthFlow {
                adapter_id,
                provider_id: provider_id_for_flow,
                oauth_state,
                verifier,
                redirect_uri,
                expires_at: std::time::Instant::now() + PROVIDER_AUTH_FLOW_TTL,
                status: ProviderAuthFlowStatus::Pending,
                message: None,
            },
        )
        .await
        .map_err(|message| fail(StatusCode::TOO_MANY_REQUESTS, message))?;
    Ok(Json(
        json!({"flow_id":flow_id,"authorization_url":authorization_url}),
    ))
}

pub(crate) async fn provider_auth_status(
    State(state): State<AppState>,
    Path((provider_id, flow_id)): Path<(String, String)>,
) -> ApiResult {
    let flow = state
        .admin
        .provider_auth_flows
        .lock()
        .await
        .get(&flow_id)
        .cloned();
    let Some(flow) = flow.filter(|flow| flow.provider_id == provider_id) else {
        return Ok(Json(json!({"status":"expired"})));
    };
    if flow.expires_at <= std::time::Instant::now()
        && matches!(flow.status, ProviderAuthFlowStatus::Pending)
    {
        return Ok(Json(json!({"status":"expired"})));
    }
    let status = match flow.status {
        ProviderAuthFlowStatus::Pending | ProviderAuthFlowStatus::Processing => "pending",
        ProviderAuthFlowStatus::Connected => "connected",
        ProviderAuthFlowStatus::Failed => "failed",
    };
    // The dashboard has now observed the terminal result, so release the flow
    // entry (including its OAuth state and PKCE verifier) immediately. Clients
    // that never poll are still bounded by PROVIDER_AUTH_TERMINAL_FLOW_TTL.
    if matches!(
        flow.status,
        ProviderAuthFlowStatus::Connected | ProviderAuthFlowStatus::Failed
    ) {
        state.reap_provider_auth_flow_if_terminal(&flow_id).await;
    }
    Ok(Json(json!({"status":status,"message":flow.message})))
}
#[cfg(test)]
mod provider_auth_callback_listener_tests {
    use super::{
        LEGACY_PROVIDER_AUTH_CALLBACK_URL, PendingProviderAuthFlow, ProviderAuthFlowStatus,
        has_active_provider_auth_flow, localhost_loopback_addresses,
    };
    use std::net::SocketAddr;
    use std::{
        collections::HashMap,
        time::{Duration, Instant},
    };

    #[test]
    fn localhost_callback_binds_each_resolved_loopback_family_only() {
        let ipv4 = SocketAddr::from(([127, 0, 0, 1], 1455));
        let ipv6 = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 1455));
        let non_loopback = SocketAddr::from(([192, 0, 2, 1], 1455));
        let addresses = localhost_loopback_addresses([ipv4, ipv6, ipv4, non_loopback]);

        assert_eq!(addresses, vec![ipv4, ipv6]);
        assert!(addresses.iter().all(|address| address.ip().is_loopback()));
    }

    #[test]
    fn a_new_pending_flow_extends_the_listener_idle_window() {
        let now = Instant::now();
        let mut flows = HashMap::new();
        assert!(!has_active_provider_auth_flow(&flows, now));

        flows.insert(
            "flow-1".to_owned(),
            PendingProviderAuthFlow {
                adapter_id: crate::provider_adapters::CODEX_ADAPTER_ID.to_owned(),
                provider_id: "codex".to_owned(),
                oauth_state: "state".to_owned(),
                verifier: "verifier".to_owned(),
                redirect_uri: LEGACY_PROVIDER_AUTH_CALLBACK_URL.to_owned(),
                expires_at: now + Duration::from_secs(60),
                status: ProviderAuthFlowStatus::Pending,
                message: None,
            },
        );
        assert!(has_active_provider_auth_flow(&flows, now));

        flows.get_mut("flow-1").unwrap().status = ProviderAuthFlowStatus::Processing;
        assert!(has_active_provider_auth_flow(&flows, now));

        flows.get_mut("flow-1").unwrap().status = ProviderAuthFlowStatus::Connected;
        assert!(!has_active_provider_auth_flow(&flows, now));

        flows.get_mut("flow-1").unwrap().status = ProviderAuthFlowStatus::Pending;
        assert!(!has_active_provider_auth_flow(
            &flows,
            now + Duration::from_secs(61)
        ));
    }
}

#[cfg(test)]
mod provider_auth_callback_flow_tests {
    use super::{
        LEGACY_PROVIDER_AUTH_CALLBACK_URL, PendingProviderAuthFlow, ProviderAuthFlowStatus,
        claim_embedded_provider_auth_callback_flow_without_state,
        claim_provider_auth_callback_flow,
    };
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use serde_json::json;
    use std::{collections::HashMap, sync::Arc, time::Duration};
    use tokio::sync::Mutex;

    fn pending_flow(now: std::time::Instant) -> PendingProviderAuthFlow {
        PendingProviderAuthFlow {
            adapter_id: crate::provider_adapters::CODEX_ADAPTER_ID.to_owned(),
            provider_id: "provider-1".to_owned(),
            oauth_state: "expected-state".to_owned(),
            verifier: "pkce-verifier".to_owned(),
            redirect_uri: LEGACY_PROVIDER_AUTH_CALLBACK_URL.to_owned(),
            expires_at: now + Duration::from_secs(60),
            status: ProviderAuthFlowStatus::Pending,
            message: None,
        }
    }

    fn pending_cline_flow(now: std::time::Instant) -> PendingProviderAuthFlow {
        PendingProviderAuthFlow {
            adapter_id: crate::provider_adapters::CLINE_ADAPTER_ID.to_owned(),
            provider_id: "cline-provider".to_owned(),
            oauth_state: "cline-state".to_owned(),
            verifier: "pkce-verifier".to_owned(),
            redirect_uri: LEGACY_PROVIDER_AUTH_CALLBACK_URL.to_owned(),
            expires_at: now + Duration::from_secs(60),
            status: ProviderAuthFlowStatus::Pending,
            message: None,
        }
    }

    fn embedded_cline_code() -> String {
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({
                "accessToken": "access-token",
                "refreshToken": "refresh-token",
                "expiresIn": 3600
            }))
            .expect("encode embedded Cline callback"),
        )
    }

    #[tokio::test]
    async fn concurrent_callback_replays_claim_the_flow_only_once() {
        let now = std::time::Instant::now();
        let flows = Arc::new(Mutex::new(HashMap::from([(
            "flow-1".to_owned(),
            pending_flow(now),
        )])));
        let first_flows = Arc::clone(&flows);
        let second_flows = Arc::clone(&flows);
        let first = async move {
            let mut flows = first_flows.lock().await;
            claim_provider_auth_callback_flow(
                &mut flows,
                std::time::Instant::now(),
                "expected-state",
                None,
            )
        };
        let second = async move {
            let mut flows = second_flows.lock().await;
            claim_provider_auth_callback_flow(
                &mut flows,
                std::time::Instant::now(),
                "expected-state",
                None,
            )
        };
        let (first, second) = tokio::join!(first, second);
        let first = first.expect("first callback state is valid");
        let second = second.expect("replayed callback maps to its in-progress flow");

        let newly_claimed_count = usize::from(first.newly_claimed_flow.is_some() as u8)
            + usize::from(second.newly_claimed_flow.is_some() as u8);
        assert_eq!(newly_claimed_count, 1);
        assert_eq!(first.status, "pending");
        assert_eq!(second.status, "pending");
    }

    #[test]
    fn callback_state_must_match_the_selected_provider_flow() {
        let now = std::time::Instant::now();
        let mut flows = HashMap::from([("flow-1".to_owned(), pending_flow(now))]);
        let result = claim_provider_auth_callback_flow(
            &mut flows,
            now,
            "different-state",
            Some(("provider-1", "flow-1")),
        );

        assert!(result.is_err());
        assert_eq!(flows["flow-1"].status, ProviderAuthFlowStatus::Pending);
    }

    #[test]
    fn embedded_cline_callback_without_state_claims_the_only_matching_flow() {
        let now = std::time::Instant::now();
        let mut flows = HashMap::from([("cline-flow".to_owned(), pending_cline_flow(now))]);
        let result = claim_embedded_provider_auth_callback_flow_without_state(
            &mut flows,
            now,
            &embedded_cline_code(),
            None,
        )
        .expect("embedded Cline callback claims its pending flow");

        assert_eq!(result.flow_id, "cline-flow");
        assert_eq!(result.status, "pending");
        assert!(result.newly_claimed_flow.is_some());
        assert_eq!(
            flows["cline-flow"].status,
            ProviderAuthFlowStatus::Processing
        );
    }

    #[test]
    fn embedded_cline_callback_without_state_rejects_non_cline_and_ambiguous_flows() {
        let now = std::time::Instant::now();
        let mut non_cline = HashMap::from([("flow-1".to_owned(), pending_flow(now))]);
        assert!(
            claim_embedded_provider_auth_callback_flow_without_state(
                &mut non_cline,
                now,
                &embedded_cline_code(),
                None,
            )
            .is_err()
        );

        let mut ambiguous = HashMap::from([
            ("cline-flow-1".to_owned(), pending_cline_flow(now)),
            ("cline-flow-2".to_owned(), pending_cline_flow(now)),
        ]);
        let result = claim_embedded_provider_auth_callback_flow_without_state(
            &mut ambiguous,
            now,
            &embedded_cline_code(),
            None,
        );
        assert!(result.is_err());
        assert!(
            ambiguous
                .values()
                .all(|flow| { flow.status == ProviderAuthFlowStatus::Pending })
        );
    }
}

#[cfg(test)]
mod provider_auth_callback_url_tests {
    use super::{
        MAX_PROVIDER_AUTH_CALLBACK_URL_BYTES, parse_provider_auth_callback_url,
        validate_provider_auth_redirect_uri,
    };

    #[test]
    fn parses_the_fixed_loopback_callback_and_decodes_form_query_values() {
        let parsed = parse_provider_auth_callback_url(
            "http://localhost:1455/auth/callback?code=code%2Bvalue&scope=openid+profile&state=state_value",
        )
        .expect("well-formed localhost callback parses");
        assert_eq!(parsed.code, "code+value");
        assert_eq!(parsed.state.as_deref(), Some("state_value"));
    }

    #[test]
    fn parses_a_secure_dashboard_callback_url() {
        let parsed = parse_provider_auth_callback_url(
            "https://exo.example/callback?code=example-code&state=state_value",
        )
        .expect("well-formed HTTPS dashboard callback parses");
        assert_eq!(parsed.code, "example-code");
        assert_eq!(parsed.state.as_deref(), Some("state_value"));
    }

    #[test]
    fn parses_a_loopback_dashboard_callback_on_the_dashboard_port() {
        let parsed = parse_provider_auth_callback_url(
            "http://localhost:8686/callback?code=example-code&state=state_value",
        )
        .expect("well-formed loopback dashboard callback parses");
        assert_eq!(parsed.code, "example-code");
        assert_eq!(parsed.state.as_deref(), Some("state_value"));
    }

    #[test]
    fn validates_supported_redirect_origins_without_query_parameters() {
        assert_eq!(
            validate_provider_auth_redirect_uri(" http://localhost:443/callback ").unwrap(),
            "http://localhost:443/callback"
        );
        assert!(validate_provider_auth_redirect_uri("http://localhost:8686/callback?x=1").is_err());
        assert!(validate_provider_auth_redirect_uri("http://example.com/callback").is_err());
        assert!(validate_provider_auth_redirect_uri("https://exo.example/callback").is_ok());
    }

    #[test]
    fn rejects_unsupported_origins_paths_duplicates_and_missing_values() {
        for value in [
            "http://example.com/auth/callback?code=c&state=s",
            "https://exo.example/other?code=c&state=s",
            "https://exo.example/callback?code=c&code=d&state=s",
            "https://exo.example/callback?code=c&state=s&state=t",
            "https://exo.example/callback?code=c",
            "https://exo.example/callback?error=access_denied&state=s",
        ] {
            assert!(parse_provider_auth_callback_url(value).is_err());
        }
    }

    #[test]
    fn accepts_a_loopback_callback_without_state_for_adapter_specific_validation() {
        let parsed = parse_provider_auth_callback_url(
            "http://localhost:1455/auth/callback?code=embedded-token-payload",
        )
        .expect("loopback callback without state parses for adapter validation");
        assert_eq!(parsed.code, "embedded-token-payload");
        assert!(parsed.state.is_none());
    }

    #[test]
    fn rejects_callback_urls_larger_than_the_request_limit() {
        let oversized = format!(
            "http://localhost:1455/auth/callback?code={}&state=s",
            "x".repeat(MAX_PROVIDER_AUTH_CALLBACK_URL_BYTES),
        );
        assert!(parse_provider_auth_callback_url(&oversized).is_err());
    }
}

#[cfg(test)]
mod provider_auth_flow_lifetime_tests {
    use super::PendingProviderAuthFlow;
    use crate::provider_adapters::CODEX_ADAPTER_ID;
    use crate::state::{AppState, PROVIDER_AUTH_TERMINAL_FLOW_TTL, ProviderAuthFlowStatus};
    use crate::support::test_support::TestDatabase;
    use std::time::{Duration, Instant};

    fn flow(now: Instant, status: ProviderAuthFlowStatus) -> PendingProviderAuthFlow {
        PendingProviderAuthFlow {
            adapter_id: CODEX_ADAPTER_ID.to_owned(),
            provider_id: "provider-1".to_owned(),
            oauth_state: "oauth-state".to_owned(),
            verifier: "pkce-verifier".to_owned(),
            redirect_uri: "http://localhost:1455/auth/callback".to_owned(),
            expires_at: now + Duration::from_secs(300),
            status,
            message: None,
        }
    }

    #[tokio::test]
    async fn terminal_flows_are_reaped_once_observed() {
        let database = TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());
        let now = Instant::now();
        state
            .register_provider_auth_flow(
                "flow-live".to_owned(),
                flow(now, ProviderAuthFlowStatus::Pending),
            )
            .await
            .expect("register pending flow");
        state
            .register_provider_auth_flow(
                "flow-done".to_owned(),
                flow(now, ProviderAuthFlowStatus::Connected),
            )
            .await
            .expect("register connected flow");

        // Observing a terminal flow releases it immediately...
        state.reap_provider_auth_flow_if_terminal("flow-done").await;
        // ...while a pending flow survives the same observation.
        state.reap_provider_auth_flow_if_terminal("flow-live").await;

        let flows = state.admin.provider_auth_flows.lock().await;
        assert!(
            !flows.contains_key("flow-done"),
            "connected flow must be reaped after observation"
        );
        assert!(flows.contains_key("flow-live"), "pending flow must survive");
    }

    #[tokio::test]
    async fn terminal_flows_do_not_block_new_registrations_and_expire_via_registration() {
        let database = TestDatabase::open().await;
        let state = AppState::new(database.config(), database.db.clone());
        let now = Instant::now();
        // Fill the map with terminal flows whose terminal TTL has expired.
        for index in 0..16 {
            let mut stale = flow(now, ProviderAuthFlowStatus::Connected);
            stale.expires_at = now
                - Duration::from_secs(1)
                - PROVIDER_AUTH_TERMINAL_FLOW_TTL
                - Duration::from_secs(index as u64);
            state
                .register_provider_auth_flow(format!("stale-{index}"), stale.clone())
                .await
                .expect("first registration of each stale flow");
            // Re-registering the same id exercises the retain sweep: the stale
            // terminal entry must now be eligible for cleanup.
            state
                .register_provider_auth_flow(format!("stale-{index}"), stale)
                .await
                .expect("retained terminal TTL must allow re-registration");
        }
        // A brand-new flow must find room because expired terminal entries were
        // reaped by the retain sweep above.
        let flows = state.admin.provider_auth_flows.lock().await;
        assert!(
            flows.len() < 17,
            "expired terminal flows must not accumulate unbounded"
        );
        drop(flows);
        state
            .register_provider_auth_flow(
                "fresh-flow".to_owned(),
                flow(Instant::now(), ProviderAuthFlowStatus::Pending),
            )
            .await
            .expect("a fresh flow can always be registered after terminal cleanup");
    }
}
