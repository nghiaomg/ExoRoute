use super::*;

pub(crate) async fn serve_with_shutdown_signal<F>(
    listener: tokio::net::TcpListener,
    app: Router,
    state: AppState,
    signal: F,
) -> std::io::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    // Axum's graceful shutdown waits for every active connection. Keep that
    // useful drain bounded because an idle SSE/upload/request can otherwise
    // keep the process alive for its normal request deadline.
    let shutdown = state.shutdown_receiver();
    let server = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let mut shutdown = shutdown;
        while shutdown.changed().await.is_ok() {
            if *shutdown.borrow() {
                break;
            }
        }
    })
    .into_future();
    let mut server = std::pin::pin!(server);
    let mut signal = std::pin::pin!(signal);
    tokio::select! {
        result = server.as_mut() => {
            state.request_shutdown();
            result
        }
        _ = signal.as_mut() => {
            state.request_shutdown();
            match tokio::time::timeout(SERVER_SHUTDOWN_GRACE_PERIOD, server).await {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!(
                        timeout_ms = SERVER_SHUTDOWN_GRACE_PERIOD.as_millis(),
                        "server graceful shutdown timed out; active connections will be dropped"
                    );
                    Ok(())
                }
            }
        }
    }
}

pub(crate) fn router(state: AppState) -> Router {
    let gateway = Router::new()
        .route("/v1", get(crate::gateway::api_root))
        .route(
            "/v1/chat/completions",
            post(crate::gateway::chat_completions),
        )
        .route("/v1/responses", post(crate::gateway::responses))
        .route("/v1/messages", post(crate::gateway::messages))
        .route(
            "/v1/streams/{stream_id}",
            get(crate::gateway::resume_stream).delete(crate::gateway::cancel_stream),
        )
        .route("/v1/models", get(crate::gateway::models))
        .route(
            "/api/v1/chat/completions",
            post(crate::gateway::chat_completions),
        )
        .route("/api/v1/responses", post(crate::gateway::responses))
        .route("/api/v1/messages", post(crate::gateway::messages))
        .route(
            "/api/v1/streams/{stream_id}",
            get(crate::gateway::resume_stream).delete(crate::gateway::cancel_stream),
        )
        .route("/api/v1/models", get(crate::gateway::models))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            crate::gateway::require_api_key,
        ));
    let admin = crate::admin::router(state.clone());
    Router::new()
        .route("/health", get(crate::gateway::health))
        .route("/api/v1/health", get(crate::gateway::health))
        .route("/health/live", get(crate::gateway::health))
        .route("/health/ready", get(crate::gateway::readiness))
        .route("/ready", get(crate::gateway::readiness))
        .merge(gateway)
        .merge(admin)
        .fallback(axum::routing::get(crate::static_assets::serve))
        .layer(TraceLayer::new_for_http().make_span_with(
            |request: &axum::http::Request<axum::body::Body>| {
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    path = %request.uri().path(),
                )
            },
        ))
        .with_state(state)
}

pub(crate) async fn shutdown_signal(state: AppState) {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::warn!(%error, "could not listen for Ctrl+C");
        }
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::warn!(%error, "could not listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
    state.request_shutdown();
}
