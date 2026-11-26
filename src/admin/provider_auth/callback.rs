use super::*;
use crate::provider_adapters::AdapterOAuthAccount;
use std::net::IpAddr;

pub(crate) const LEGACY_PROVIDER_AUTH_CALLBACK_URL: &str = "http://localhost:1455/auth/callback";

pub(crate) async fn ensure_provider_auth_callback_listener_locked(
    state: &AppState,
) -> Result<(), (StatusCode, Json<Value>)> {
    if state
        .admin
        .provider_auth_callback_started
        .load(Ordering::Acquire)
    {
        return Ok(());
    }
    let listeners = match bind_provider_auth_callback_listeners().await {
        Ok(listeners) => listeners,
        Err(error) => {
            tracing::warn!(%error, "could not bind the provider OAuth callback listener");
            return Err(fail(
                StatusCode::SERVICE_UNAVAILABLE,
                "could not listen on the localhost:1455 loopback OAuth callback; close the application using that port and retry",
            ));
        }
    };
    let callback_state = state.clone();
    let callback_router = Router::new()
        .route("/auth/callback", get(provider_auth_callback))
        .with_state(callback_state.clone());
    state
        .admin
        .provider_auth_callback_started
        .store(true, Ordering::Release);
    tokio::spawn(async move {
        supervise_provider_auth_callback_listener(callback_state, listeners, callback_router).await;
    });
    Ok(())
}

async fn supervise_provider_auth_callback_listener(
    state: AppState,
    listeners: Vec<TcpListener>,
    callback_router: Router,
) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut app_shutdown = state.shutdown_receiver();
    let mut servers = JoinSet::new();
    for listener in listeners {
        let router = callback_router.clone();
        let mut shutdown = shutdown_rx.clone();
        servers.spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    while shutdown.changed().await.is_ok() {
                        if *shutdown.borrow() {
                            break;
                        }
                    }
                })
                .await
        });
    }

    'supervisor: loop {
        if *app_shutdown.borrow() {
            break;
        }
        if has_active_provider_auth_flows(&state).await {
            tokio::select! {
                result = servers.join_next() => {
                    log_unexpected_provider_auth_listener_exit(result);
                    break;
                }
                _ = app_shutdown.changed() => break 'supervisor,
                _ = tokio::time::sleep(PROVIDER_AUTH_LISTENER_POLL_INTERVAL) => continue,
            }
        }

        let idle_deadline = tokio::time::Instant::now() + PROVIDER_AUTH_LISTENER_IDLE_GRACE;
        let idle_timer = tokio::time::sleep_until(idle_deadline);
        tokio::pin!(idle_timer);
        let mut idle_window_extended = false;
        let mut flow_poll = tokio::time::interval(PROVIDER_AUTH_LISTENER_POLL_INTERVAL);
        flow_poll.tick().await;

        loop {
            tokio::select! {
                result = servers.join_next() => {
                    log_unexpected_provider_auth_listener_exit(result);
                    break 'supervisor;
                }
                _ = app_shutdown.changed() => break 'supervisor,
                _ = &mut idle_timer => break,
                _ = flow_poll.tick() => {
                    if has_active_provider_auth_flows(&state).await {
                        idle_window_extended = true;
                        break;
                    }
                }
            }
        }
        if idle_window_extended {
            continue;
        }

        let _callback_guard = state.admin.provider_auth_callback_lock.lock().await;
        if has_active_provider_auth_flows(&state).await {
            continue;
        }

        shutdown_tx.send_replace(true);
        let shutdown_deadline = tokio::time::sleep(PROVIDER_AUTH_LISTENER_SHUTDOWN_TIMEOUT);
        tokio::pin!(shutdown_deadline);
        loop {
            tokio::select! {
                result = servers.join_next() => match result {
                    Some(Ok(Err(error))) => {
                        tracing::warn!(%error, "provider OAuth callback listener failed during shutdown");
                    }
                    Some(Err(error)) => {
                        tracing::warn!(%error, "provider OAuth callback listener task failed during shutdown");
                    }
                    Some(Ok(Ok(()))) => {}
                    None => break,
                },
                _ = &mut shutdown_deadline => {
                    servers.abort_all();
                    while servers.join_next().await.is_some() {}
                    break;
                }
            }
        }
        state
            .admin
            .provider_auth_callback_started
            .store(false, Ordering::Release);
        return;
    }

    servers.abort_all();
    while servers.join_next().await.is_some() {}
    state
        .admin
        .provider_auth_callback_started
        .store(false, Ordering::Release);
}

fn log_unexpected_provider_auth_listener_exit(
    result: Option<Result<io::Result<()>, tokio::task::JoinError>>,
) {
    match result {
        Some(Ok(Err(error))) => {
            tracing::warn!(%error, "provider OAuth callback listener stopped unexpectedly");
        }
        Some(Err(error)) => {
            tracing::warn!(%error, "provider OAuth callback listener task stopped unexpectedly");
        }
        Some(Ok(Ok(()))) | None => {
            tracing::warn!("provider OAuth callback listener stopped unexpectedly");
        }
    }
}

async fn has_active_provider_auth_flows(state: &AppState) -> bool {
    let flows = state.admin.provider_auth_flows.lock().await;
    has_active_provider_auth_flow(&flows, std::time::Instant::now())
}

pub(crate) fn has_active_provider_auth_flow(
    flows: &std::collections::HashMap<String, PendingProviderAuthFlow>,
    now: std::time::Instant,
) -> bool {
    flows.values().any(|flow| {
        // Only authorization-code flows need the loopback listener; a pending
        // device authorization must not keep the port bound.
        flow.method.authorization_code().is_some()
            && flow.expires_at > now
            && matches!(
                flow.status,
                ProviderAuthFlowStatus::Pending | ProviderAuthFlowStatus::Processing
            )
    })
}

async fn bind_provider_auth_callback_listeners() -> io::Result<Vec<TcpListener>> {
    let resolved = tokio::net::lookup_host(("localhost", 1455)).await?;
    let addresses = localhost_loopback_addresses(resolved);
    if addresses.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "localhost did not resolve to a loopback address",
        ));
    }

    let mut listeners = Vec::with_capacity(addresses.len());
    for address in addresses {
        listeners.push(TcpListener::bind(address).await?);
    }
    Ok(listeners)
}

pub(crate) fn localhost_loopback_addresses(
    resolved: impl IntoIterator<Item = SocketAddr>,
) -> Vec<SocketAddr> {
    let mut addresses = resolved
        .into_iter()
        .filter(|address| address.ip().is_loopback())
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();
    addresses
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderAuthCallbackRequest {
    callback_url: String,
}

pub(crate) struct ParsedProviderAuthCallback {
    pub(crate) code: String,
    pub(crate) state: Option<String>,
}

pub(crate) struct ProviderAuthCallbackCompletion {
    pub(crate) flow_id: String,
    pub(crate) provider_id: String,
    pub(crate) status: &'static str,
    pub(crate) newly_claimed_flow: Option<PendingProviderAuthFlow>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProviderAuthCallbackKind {
    LegacyLoopback,
    LocalDashboard,
    SecureDashboard,
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn is_secure_dashboard_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    !is_loopback_host(&host) && !host.ends_with(".localhost")
}

fn classify_provider_auth_callback_url(
    url: &reqwest::Url,
) -> Result<ProviderAuthCallbackKind, &'static str> {
    let is_legacy_loopback = url.scheme() == "http"
        && url.host_str() == Some("localhost")
        && url.port_or_known_default() == Some(1455)
        && url.path() == "/auth/callback";
    if is_legacy_loopback {
        return Ok(ProviderAuthCallbackKind::LegacyLoopback);
    }

    let is_local_dashboard = url.scheme() == "http"
        && url.host_str().is_some_and(is_loopback_host)
        && url.port_or_known_default().is_some()
        && url.path() == "/callback";
    if is_local_dashboard {
        return Ok(ProviderAuthCallbackKind::LocalDashboard);
    }

    let is_secure_dashboard = url.scheme() == "https"
        && url.host_str().is_some_and(is_secure_dashboard_host)
        && url.path() == "/callback";
    if is_secure_dashboard {
        return Ok(ProviderAuthCallbackKind::SecureDashboard);
    }

    Err(
        "OAuth callback URL must use localhost:1455/auth/callback, a loopback /callback page, or an HTTPS /callback page",
    )
}

pub(crate) fn validate_provider_auth_redirect_uri(
    redirect_uri: &str,
) -> Result<String, &'static str> {
    let redirect_uri = redirect_uri.trim();
    if redirect_uri.is_empty() || redirect_uri.len() > MAX_PROVIDER_AUTH_CALLBACK_URL_BYTES {
        return Err("OAuth redirect URI is empty or too long");
    }
    let url = reqwest::Url::parse(redirect_uri).map_err(|_| "OAuth redirect URI is invalid")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("OAuth redirect URI is invalid");
    }
    classify_provider_auth_callback_url(&url)?;
    Ok(url.to_string())
}

pub(crate) fn parse_provider_auth_callback_url(
    callback_url: &str,
) -> Result<ParsedProviderAuthCallback, &'static str> {
    let callback_url = callback_url.trim();
    if callback_url.is_empty() || callback_url.len() > MAX_PROVIDER_AUTH_CALLBACK_URL_BYTES {
        return Err("OAuth callback URL is empty or too long");
    }
    let url = reqwest::Url::parse(callback_url).map_err(|_| "OAuth callback URL is invalid")?;
    if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
        return Err("OAuth callback URL is invalid");
    }

    let callback_kind = classify_provider_auth_callback_url(&url)?;

    let mut code = None;
    let mut state = None;
    let mut denied = false;
    for (name, value) in url.query_pairs() {
        match name.as_ref() {
            "code" if code.is_some() => {
                return Err("OAuth callback URL contains duplicate parameters");
            }
            "code" => code = Some(value.into_owned()),
            "state" if state.is_some() => {
                return Err("OAuth callback URL contains duplicate parameters");
            }
            "state" => state = Some(value.into_owned()),
            "error" => denied = true,
            _ => {}
        }
    }
    if denied {
        return Err("provider sign-in was cancelled or denied");
    }
    let code = code
        .filter(|value| !value.trim().is_empty() && value.len() <= 8192)
        .ok_or("provider did not return a valid authorization code")?;
    let state = match state {
        Some(value) if !value.is_empty() && value.len() <= 256 => Some(value),
        Some(_) => return Err("provider did not return a valid OAuth state"),
        None if callback_kind == ProviderAuthCallbackKind::LegacyLoopback => None,
        None => return Err("provider did not return a valid OAuth state"),
    };
    Ok(ParsedProviderAuthCallback { code, state })
}

pub(crate) async fn complete_provider_auth_callback(
    State(state): State<AppState>,
    Json(request): Json<ProviderAuthCallbackRequest>,
) -> ApiResult {
    let callback = parse_provider_auth_callback_url(&request.callback_url)
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    let completion = accept_provider_auth_callback(&state, callback, None).await?;
    Ok(Json(json!({
        "flow_id": completion.flow_id,
        "provider_id": completion.provider_id,
        "status": completion.status,
    })))
}

pub(crate) async fn complete_provider_auth_callback_for_flow(
    State(state): State<AppState>,
    Path((provider_id, flow_id)): Path<(String, String)>,
    Json(request): Json<ProviderAuthCallbackRequest>,
) -> ApiResult {
    let callback = parse_provider_auth_callback_url(&request.callback_url)
        .map_err(|message| fail(StatusCode::BAD_REQUEST, message))?;
    let completion =
        accept_provider_auth_callback(&state, callback, Some((&provider_id, &flow_id))).await?;
    Ok(Json(json!({
        "flow_id": completion.flow_id,
        "provider_id": completion.provider_id,
        "status": completion.status,
    })))
}

async fn accept_provider_auth_callback(
    state: &AppState,
    callback: ParsedProviderAuthCallback,
    expected_flow: Option<(&str, &str)>,
) -> Result<ProviderAuthCallbackCompletion, (StatusCode, Json<Value>)> {
    let claimed = {
        let mut flows = state.admin.provider_auth_flows.lock().await;
        match callback.state.as_deref() {
            Some(callback_state) => claim_provider_auth_callback_flow(
                &mut flows,
                std::time::Instant::now(),
                callback_state,
                expected_flow,
            )?,
            None => claim_embedded_provider_auth_callback_flow_without_state(
                &mut flows,
                std::time::Instant::now(),
                &callback.code,
                expected_flow,
            )?,
        }
    };

    let ProviderAuthCallbackCompletion {
        flow_id,
        provider_id,
        status,
        newly_claimed_flow,
    } = claimed;
    if let Some(flow) = newly_claimed_flow {
        spawn_provider_auth_completion(state.clone(), flow_id.clone(), flow, callback.code);
    }
    Ok(ProviderAuthCallbackCompletion {
        flow_id,
        provider_id,
        status,
        newly_claimed_flow: None,
    })
}

pub(crate) fn claim_provider_auth_callback_flow(
    flows: &mut std::collections::HashMap<String, PendingProviderAuthFlow>,
    now: std::time::Instant,
    callback_state: &str,
    expected_flow: Option<(&str, &str)>,
) -> Result<ProviderAuthCallbackCompletion, (StatusCode, Json<Value>)> {
    flows.retain(|_, flow| flow.expires_at > now);
    let flow_id = if let Some((_, expected_flow_id)) = expected_flow {
        Some(expected_flow_id.to_owned())
    } else {
        flows.iter().find_map(|(flow_id, flow)| {
            flow.method
                .authorization_code()
                .is_some_and(|(oauth_state, _, _)| {
                    secure_eq(oauth_state.as_bytes(), callback_state.as_bytes())
                })
                .then(|| flow_id.clone())
        })
    };
    let Some(flow_id) = flow_id else {
        return Err(fail(
            StatusCode::GONE,
            "OAuth callback state is invalid or expired; start sign-in again",
        ));
    };
    let Some(flow) = flows.get_mut(&flow_id) else {
        return Err(fail(
            StatusCode::GONE,
            "OAuth callback state is invalid or expired; start sign-in again",
        ));
    };
    if let Some((expected_provider_id, _)) = expected_flow
        && flow.provider_id != expected_provider_id
    {
        return Err(fail(
            StatusCode::GONE,
            "OAuth callback state is invalid or expired; start sign-in again",
        ));
    }
    let Some((oauth_state, _, _)) = flow.method.authorization_code() else {
        return Err(fail(
            StatusCode::GONE,
            "OAuth callback state is invalid or expired; start sign-in again",
        ));
    };
    if !secure_eq(oauth_state.as_bytes(), callback_state.as_bytes()) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "OAuth callback state does not match this sign-in attempt",
        ));
    }
    if flow.expires_at <= now {
        return Err(fail(
            StatusCode::GONE,
            "OAuth callback state is invalid or expired; start sign-in again",
        ));
    }

    let (status, newly_claimed) = match &flow.status {
        ProviderAuthFlowStatus::Pending => {
            flow.status = ProviderAuthFlowStatus::Processing;
            ("pending", true)
        }
        ProviderAuthFlowStatus::Processing => ("pending", false),
        ProviderAuthFlowStatus::Connected => ("connected", false),
        ProviderAuthFlowStatus::Failed => ("failed", false),
    };
    Ok(ProviderAuthCallbackCompletion {
        flow_id,
        provider_id: flow.provider_id.clone(),
        status,
        newly_claimed_flow: newly_claimed.then(|| flow.clone()),
    })
}

pub(crate) fn claim_embedded_provider_auth_callback_flow_without_state(
    flows: &mut std::collections::HashMap<String, PendingProviderAuthFlow>,
    now: std::time::Instant,
    code: &str,
    expected_flow: Option<(&str, &str)>,
) -> Result<ProviderAuthCallbackCompletion, (StatusCode, Json<Value>)> {
    flows.retain(|_, flow| flow.expires_at > now);
    let mut candidates = flows
        .iter()
        .filter(|(candidate_flow_id, flow)| {
            matches!(
                flow.status,
                ProviderAuthFlowStatus::Pending | ProviderAuthFlowStatus::Processing
            ) && flow.method.authorization_code().is_some()
                && provider_adapters::accepts_embedded_oauth_callback_without_state(
                    &flow.adapter_id,
                    code,
                )
                && expected_flow.is_none_or(|(expected_provider_id, expected_flow_id)| {
                    flow.provider_id == expected_provider_id
                        && candidate_flow_id.as_str() == expected_flow_id
                })
        })
        .map(|(flow_id, _)| flow_id.clone());

    let Some(flow_id) = candidates.next() else {
        return Err(fail(
            StatusCode::GONE,
            "OAuth callback state is missing or invalid; start sign-in again",
        ));
    };
    if candidates.next().is_some() {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "OAuth callback matches more than one sign-in attempt; close the other attempt and retry",
        ));
    }

    let Some(flow) = flows.get_mut(&flow_id) else {
        return Err(fail(
            StatusCode::GONE,
            "OAuth callback state is missing or invalid; start sign-in again",
        ));
    };
    if let Some((expected_provider_id, _)) = expected_flow
        && flow.provider_id != expected_provider_id
    {
        return Err(fail(
            StatusCode::GONE,
            "OAuth callback state is missing or invalid; start sign-in again",
        ));
    }
    let (status, newly_claimed) = match &flow.status {
        ProviderAuthFlowStatus::Pending => {
            flow.status = ProviderAuthFlowStatus::Processing;
            ("pending", true)
        }
        ProviderAuthFlowStatus::Processing => ("pending", false),
        ProviderAuthFlowStatus::Connected | ProviderAuthFlowStatus::Failed => {
            return Err(fail(
                StatusCode::GONE,
                "OAuth callback state is missing or invalid; start sign-in again",
            ));
        }
    };
    Ok(ProviderAuthCallbackCompletion {
        flow_id,
        provider_id: flow.provider_id.clone(),
        status,
        newly_claimed_flow: newly_claimed.then(|| flow.clone()),
    })
}

fn spawn_provider_auth_completion(
    state: AppState,
    flow_id: String,
    flow: PendingProviderAuthFlow,
    code: String,
) {
    let mut app_shutdown = state.shutdown_receiver();
    tokio::spawn(async move {
        // A shutdown cancels the exchange before the account is stored; every
        // other outcome is published on the flow by `finish_provider_auth_flow`.
        tokio::select! {
            _ = app_shutdown.changed() => {}
            _ = async {
                let exchanged = async {
                    let (_, verifier, redirect_uri) = flow
                        .method
                        .authorization_code()
                        .ok_or_else(|| "provider sign-in did not use a redirect flow".to_owned())?;
                    provider_adapters::exchange_oauth_code(
                        &flow.adapter_id,
                        &code,
                        verifier,
                        redirect_uri,
                        Duration::from_secs(3),
                        Duration::from_secs(10),
                        state.operational_settings().settings.upstream,
                    )
                    .await
                }
                .await;
                match exchanged {
                    Ok(account) => {
                        finish_provider_auth_flow(
                            &state,
                            &flow_id,
                            &flow.adapter_id,
                            &flow.provider_id,
                            account,
                        )
                        .await
                    }
                    Err(message) => fail_provider_auth_flow(&state, &flow_id, &message).await,
                }
            } => {}
        }
    });
}

/// Stores the account a sign-in just returned and publishes the terminal flow
/// status the dashboard polls. Used by both the redirect callback and the
/// device-authorization poll so the two paths report identically.
pub(crate) async fn finish_provider_auth_flow(
    state: &AppState,
    flow_id: &str,
    adapter_id: &str,
    provider_id: &str,
    account: AdapterOAuthAccount,
) {
    let result =
        provider_adapters::save_oauth_account(adapter_id, state, provider_id, account).await;
    let succeeded = result.is_ok();
    let status_message = match result {
        Ok(()) => provider_adapters::preset_for_adapter(adapter_id)
            .map(|preset| format!("{} account connected", preset.name))
            .unwrap_or_else(|| "provider account connected".to_owned()),
        Err(message) => message,
    };
    if let Some(current) = state
        .admin
        .provider_auth_flows
        .lock()
        .await
        .get_mut(flow_id)
        && matches!(
            current.status,
            ProviderAuthFlowStatus::Pending | ProviderAuthFlowStatus::Processing
        )
    {
        current.status = if succeeded {
            ProviderAuthFlowStatus::Connected
        } else {
            ProviderAuthFlowStatus::Failed
        };
        current.message = Some(status_message);
        current.expires_at = std::time::Instant::now() + PROVIDER_AUTH_FLOW_TTL;
    }
}

pub(crate) async fn provider_auth_callback(State(state): State<AppState>, uri: Uri) -> Response {
    let Some(query) = uri.query().filter(|query| !query.is_empty()) else {
        return provider_auth_result_response("failed");
    };
    if query.len()
        > MAX_PROVIDER_AUTH_CALLBACK_URL_BYTES - LEGACY_PROVIDER_AUTH_CALLBACK_URL.len() - 1
    {
        return provider_auth_result_response("failed");
    }
    let mut callback_url =
        String::with_capacity(LEGACY_PROVIDER_AUTH_CALLBACK_URL.len() + 1 + query.len());
    callback_url.push_str(LEGACY_PROVIDER_AUTH_CALLBACK_URL);
    callback_url.push('?');
    callback_url.push_str(query);
    let status = match parse_provider_auth_callback_url(&callback_url) {
        Ok(callback) => match accept_provider_auth_callback(&state, callback, None).await {
            Ok(completion) => completion.status,
            Err(_) => "failed",
        },
        Err(_) => "failed",
    };
    provider_auth_result_response(status)
}

fn provider_auth_result_response(status: &'static str) -> Response {
    let mut response = Html(provider_auth_result_page(status)).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    response
}

fn provider_auth_result_page(status: &str) -> &'static str {
    match status {
        "connected" => {
            "<!doctype html><html><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><title>ExoRoute</title><body><h1>Provider connected</h1><p>Return to ExoRoute to finish setup.</p></body></html>"
        }
        "pending" => {
            "<!doctype html><html><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><title>ExoRoute</title><body><h1>Sign-in received</h1><p>Provider sign-in is being completed. Return to ExoRoute.</p></body></html>"
        }
        _ => {
            "<!doctype html><html><meta charset=\"utf-8\"><meta name=\"referrer\" content=\"no-referrer\"><title>ExoRoute</title><body><h1>Provider sign-in could not be completed</h1><p>Return to ExoRoute and start sign-in again.</p></body></html>"
        }
    }
}
