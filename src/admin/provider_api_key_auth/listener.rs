use super::{
    CALLBACK_IDLE_GRACE, CALLBACK_POLL_INTERVAL, CALLBACK_PORTS, CALLBACK_SHUTDOWN_TIMEOUT,
    callback, preflight,
};
use crate::state::{AppState, ProviderApiKeyAuthFlowStatus};
use axum::{Router, routing::post};
use std::{io, net::SocketAddr, sync::atomic::Ordering, time::Instant};
use tokio::{net::TcpListener, sync::watch, task::JoinSet};

pub(super) async fn ensure_callback_listener_locked(state: &AppState) -> io::Result<u16> {
    if state
        .admin
        .provider_api_key_auth_callback_started
        .load(Ordering::Acquire)
    {
        let port = state
            .admin
            .provider_api_key_auth_callback_port
            .load(Ordering::Acquire);
        if port != 0 {
            return Ok(port);
        }
    }
    let (port, listeners) = bind_callback_listeners().await?;
    let callback_state = state.clone();
    let callback_router = Router::new()
        .route("/callback", post(callback).options(preflight))
        .with_state(callback_state.clone());
    state
        .admin
        .provider_api_key_auth_callback_port
        .store(port, Ordering::Release);
    state
        .admin
        .provider_api_key_auth_callback_started
        .store(true, Ordering::Release);
    tokio::spawn(async move {
        supervise_callback_listener(callback_state, listeners, callback_router).await;
    });
    Ok(port)
}

async fn bind_callback_listeners() -> io::Result<(u16, Vec<TcpListener>)> {
    let mut last_error = None;
    for port in CALLBACK_PORTS {
        let resolved = match tokio::net::lookup_host(("localhost", port)).await {
            Ok(addresses) => addresses,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        let addresses = loopback_addresses(resolved);
        if addresses.is_empty() {
            last_error = Some(io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "localhost did not resolve to a loopback address",
            ));
            continue;
        }
        let mut listeners = Vec::with_capacity(addresses.len());
        let mut bind_error = None;
        for address in addresses {
            match TcpListener::bind(address).await {
                Ok(listener) => listeners.push(listener),
                Err(error) => {
                    bind_error = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = bind_error {
            last_error = Some(error);
            drop(listeners);
            continue;
        }
        return Ok((port, listeners));
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "no Command Code callback port is available",
        )
    }))
}

pub(super) fn loopback_addresses(
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

async fn supervise_callback_listener(
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
        if has_active_flows(&state).await {
            tokio::select! {
                result = servers.join_next() => {
                    log_listener_exit(result);
                    break;
                }
                _ = app_shutdown.changed() => break 'supervisor,
                _ = tokio::time::sleep(CALLBACK_POLL_INTERVAL) => continue,
            }
        }

        let idle_deadline = tokio::time::Instant::now() + CALLBACK_IDLE_GRACE;
        let idle_timer = tokio::time::sleep_until(idle_deadline);
        tokio::pin!(idle_timer);
        let mut idle_window_extended = false;
        let mut flow_poll = tokio::time::interval(CALLBACK_POLL_INTERVAL);
        flow_poll.tick().await;
        loop {
            tokio::select! {
                result = servers.join_next() => {
                    log_listener_exit(result);
                    break 'supervisor;
                }
                _ = app_shutdown.changed() => break 'supervisor,
                _ = &mut idle_timer => break,
                _ = flow_poll.tick() => {
                    if has_active_flows(&state).await {
                        idle_window_extended = true;
                        break;
                    }
                }
            }
        }
        if idle_window_extended {
            continue;
        }

        let _callback_guard = state.admin.provider_api_key_auth_callback_lock.lock().await;
        if has_active_flows(&state).await {
            continue;
        }
        shutdown_tx.send_replace(true);
        let shutdown_deadline = tokio::time::sleep(CALLBACK_SHUTDOWN_TIMEOUT);
        tokio::pin!(shutdown_deadline);
        loop {
            tokio::select! {
                result = servers.join_next() => match result {
                    Some(Ok(Err(error))) => tracing::warn!(%error, "Command Code callback listener failed during shutdown"),
                    Some(Err(error)) => tracing::warn!(%error, "Command Code callback listener task failed during shutdown"),
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
            .provider_api_key_auth_callback_port
            .store(0, Ordering::Release);
        state
            .admin
            .provider_api_key_auth_callback_started
            .store(false, Ordering::Release);
        return;
    }

    servers.abort_all();
    while servers.join_next().await.is_some() {}
    state
        .admin
        .provider_api_key_auth_callback_port
        .store(0, Ordering::Release);
    state
        .admin
        .provider_api_key_auth_callback_started
        .store(false, Ordering::Release);
}

async fn has_active_flows(state: &AppState) -> bool {
    let flows = state.admin.provider_api_key_auth_flows.lock().await;
    flows.values().any(|flow| {
        flow.expires_at > Instant::now() && flow.status == ProviderApiKeyAuthFlowStatus::Pending
    })
}

fn log_listener_exit(result: Option<Result<io::Result<()>, tokio::task::JoinError>>) {
    match result {
        Some(Ok(Err(error))) => {
            tracing::warn!(%error, "Command Code callback listener stopped unexpectedly")
        }
        Some(Err(error)) => {
            tracing::warn!(%error, "Command Code callback listener task stopped unexpectedly")
        }
        Some(Ok(Ok(()))) | None => {
            tracing::warn!("Command Code callback listener stopped unexpectedly")
        }
    }
}
