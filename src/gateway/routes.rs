use super::*;

pub async fn health() -> Json<Value> {
    Json(json!({"status":"ok","service":"exoroute"}))
}

pub async fn readiness(State(state): State<AppState>) -> Response {
    let database_ready = tokio::time::timeout(
        Duration::from_secs(1),
        state.db.read(|transaction| {
            transaction
                .get::<u32>(Table::Meta, "storage_format_version")?
                .map(|_| ())
                .ok_or_else(|| StorageError::Invalid("LMDB storage version is missing".to_owned()))
        }),
    )
    .await
    .is_ok_and(|result| result.is_ok());
    let workers_ready = state.background_workers_ready();
    let ready = database_ready && workers_ready;
    let status = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(json!({"status": if ready {"ready"} else {"not_ready"},"service":"exoroute"})),
    )
        .into_response()
}

pub async fn api_root() -> Json<Value> {
    Json(json!({
        "service": "exoroute",
        "version": "v1",
        "endpoints": ["/v1/chat/completions", "/v1/responses", "/v1/messages", "/v1/models", "/v1/streams/{stream_id}"],
        "stream_continuity": {"resume":"GET /v1/streams/{stream_id} with Last-Event-ID", "cancel":"DELETE /v1/streams/{stream_id}"}
    }))
}

pub async fn require_api_key(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    if state
        .admin
        .admin_password_change_required
        .load(std::sync::atomic::Ordering::Acquire)
    {
        return gateway_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "change the admin password before enabling gateway traffic",
        );
    }

    match authenticate_client(&state, request.headers()).await {
        Ok(api_key_id) => {
            let (operational_settings, permit) = {
                let runtime = state
                    .runtime
                    .operational_settings
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let policy = RateLimitPolicy::TokenBucket {
                    capacity: runtime.settings.gateway_key_capacity,
                    refill_tokens: runtime.settings.gateway_key_refill_tokens,
                    refill_interval: runtime.settings.gateway_key_refill_interval,
                };
                let decision = state
                    .gates
                    .gateway_key_limiter
                    .check_with_policy(&api_key_id, policy);
                if !decision.allowed {
                    return rate_limited_response(decision.retry_after);
                }
                let permit = match state.gates.gateway_in_flight.clone().try_acquire_owned() {
                    Some(permit) => permit,
                    None => {
                        return gateway_error(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "gateway is at its in-flight request limit; retry shortly",
                        );
                    }
                };
                (runtime.settings, permit)
            };
            let body_processing_permit = if request.method() == axum::http::Method::POST {
                match tokio::time::timeout(
                    operational_settings
                        .upstream
                        .gateway_body_processing_queue_timeout,
                    state.gates.gateway_body_processing.clone().acquire_owned(),
                )
                .await
                {
                    Ok(permit) => Some(permit),
                    Err(_) => {
                        return gateway_error(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "gateway request processing is busy; retry shortly",
                        );
                    }
                }
            } else {
                None
            };
            let analytics = (request.method() == axum::http::Method::POST
                && matches!(
                    request.uri().path(),
                    "/v1/chat/completions" | "/v1/responses" | "/v1/messages"
                ))
            .then(|| state.telemetry.start_request(api_key_id.clone()));
            request.extensions_mut().insert(GatewayRequestContext {
                resource_limits: state.gateway_resource_limits(),
                operational_settings,
                api_key_id: api_key_id.clone(),
                analytics: analytics.clone(),
            });
            state.request_count.fetch_add(1, Ordering::Relaxed);
            state.record_api_key_request(api_key_id);
            let response = next.run(request).await;
            let response = match analytics {
                Some(analytics) if !analytics.completion_is_deferred() => {
                    track_analytics_response(response, analytics)
                }
                _ => response,
            };
            drop(body_processing_permit);
            let (mut parts, body) = response.into_parts();
            let live = parts.extensions.remove::<RequestLiveGuard>();
            let body_stream = async_stream::stream! {
                let _permit = permit;
                let _live = live;
                let mut chunks = body.into_data_stream();
                while let Some(chunk) = chunks.next().await {
                    yield chunk;
                }
            };
            Response::from_parts(parts, Body::from_stream(body_stream))
        }
        Err(response) if response.status() == StatusCode::UNAUTHORIZED => {
            let peer_ip =
                crate::security::client_ip::client_ip(&request, state.config.trust_proxy_headers);
            let decision = state.gates.gateway_auth_failure_limiter.check(&peer_ip);
            if !decision.allowed {
                rate_limited_response(decision.retry_after)
            } else {
                response
            }
        }
        Err(response) => response,
    }
}

pub async fn models(State(state): State<AppState>) -> Response {
    let models = state
        .db
        .read(|transaction| {
            let routes = transaction.scan_prefix::<Record>(
                Table::Routes,
                "",
                MAX_PUBLIC_MODEL_RECORDS + 1,
            )?;
            let providers = transaction.scan_prefix::<Record>(
                Table::Providers,
                "",
                MAX_PUBLIC_MODEL_RECORDS + 1,
            )?;
            let provider_models = transaction.scan_prefix::<Record>(
                Table::ProviderModels,
                "",
                MAX_PUBLIC_MODEL_RECORDS + 1,
            )?;
            if routes.len() > MAX_PUBLIC_MODEL_RECORDS
                || providers.len() > MAX_PUBLIC_MODEL_RECORDS
                || provider_models.len() > MAX_PUBLIC_MODEL_RECORDS
            {
                return Err(StorageError::Busy);
            }

            let mut models = Vec::with_capacity(routes.len().saturating_add(provider_models.len()));
            let mut seen = std::collections::HashSet::with_capacity(
                routes.len().saturating_add(provider_models.len()),
            );
            for (_, route) in routes {
                if route.boolean("enabled")? {
                    let id = route.text("id")?.to_owned();
                    if seen.insert(id.clone()) {
                        models.push(
                            json!({"id":id,"object":"model","created":0,"owned_by":"exoroute"}),
                        );
                    }
                }
            }

            let mut enabled_providers = std::collections::HashMap::with_capacity(providers.len());
            for (_, provider) in providers {
                if provider.boolean("enabled")? {
                    let provider_id = provider.text("id")?.to_owned();
                    let prefix = provider
                        .optional_text("model_prefix")?
                        .filter(|value| !value.trim().is_empty())
                        .map(str::trim)
                        .map(str::to_ascii_lowercase)
                        .unwrap_or_else(|| provider_id.to_ascii_lowercase());
                    enabled_providers.insert(provider_id, prefix);
                }
            }
            let mut aliases = Vec::with_capacity(provider_models.len());
            for (_, provider_model) in provider_models {
                let provider_id = provider_model.text("provider_id")?;
                let Some(prefix) = enabled_providers.get(provider_id) else {
                    continue;
                };
                let model = provider_model.text("model")?;
                aliases.push((prefix.clone(), model.to_owned()));
            }
            aliases.sort_by(|left, right| {
                (&left.0, left.1.to_lowercase(), &left.1).cmp(&(
                    &right.0,
                    right.1.to_lowercase(),
                    &right.1,
                ))
            });
            for (prefix, model) in aliases {
                let id = format!("{prefix}/{model}");
                if seen.insert(id.clone()) {
                    if models.len() == MAX_PUBLIC_MODEL_RECORDS {
                        return Err(StorageError::Busy);
                    }
                    models.push(json!({"id":id,"object":"model","created":0,"owned_by":prefix}));
                }
            }
            Ok(models)
        })
        .await;
    match models {
        Ok(models) => Json(json!({"object":"list","data":models})).into_response(),
        Err(error) => gateway_database_error(error),
    }
}

pub async fn chat_completions(State(state): State<AppState>, request: Request<Body>) -> Response {
    let (headers, body, execution_settings) = match gateway_json_request(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    handle_request_with_limits_and_owner(
        state,
        headers,
        body,
        Protocol::ChatCompletions,
        execution_settings,
    )
    .await
}
pub async fn responses(State(state): State<AppState>, request: Request<Body>) -> Response {
    let (headers, body, execution_settings) = match gateway_json_request(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    handle_request_with_limits_and_owner(
        state,
        headers,
        body,
        Protocol::Responses,
        execution_settings,
    )
    .await
}
pub async fn messages(State(state): State<AppState>, request: Request<Body>) -> Response {
    let (headers, body, execution_settings) = match gateway_json_request(request).await {
        Ok(request) => request,
        Err(response) => return response,
    };
    handle_request_with_limits_and_owner(
        state,
        headers,
        body,
        Protocol::Messages,
        execution_settings,
    )
    .await
}

pub async fn resume_stream(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Extension(context): Extension<GatewayRequestContext>,
    headers: HeaderMap,
) -> Response {
    let after = match headers.get("last-event-id") {
        None => 0,
        Some(value) => match value
            .to_str()
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
        {
            Some(sequence) if sequence >= 0 => sequence,
            _ => {
                return gateway_error(
                    StatusCode::BAD_REQUEST,
                    "Last-Event-ID must be a non-negative integer",
                );
            }
        },
    };
    let snapshot = match continuity::load_run_with_settings(
        &state.db,
        &run_id,
        &context.api_key_id,
        state.operational_settings().settings.upstream,
    )
    .await
    {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return gateway_error(StatusCode::NOT_FOUND, "stream run not found"),
        Err(error) => return gateway_database_error(error),
    };
    if after > snapshot.latest_sequence {
        return gateway_error(
            StatusCode::BAD_REQUEST,
            "Last-Event-ID is ahead of the stream",
        );
    }
    continuity::resume_response(state, run_id, context.api_key_id, after)
}

pub async fn cancel_stream(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Extension(context): Extension<GatewayRequestContext>,
) -> Response {
    let snapshot = match continuity::load_run_with_settings(
        &state.db,
        &run_id,
        &context.api_key_id,
        state.operational_settings().settings.upstream,
    )
    .await
    {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return gateway_error(StatusCode::NOT_FOUND, "stream run not found"),
        Err(error) => return gateway_database_error(error),
    };
    if snapshot.status != "running" {
        return gateway_error(StatusCode::CONFLICT, "stream is already finished");
    }
    if !state.request_stream_cancellation(&run_id) {
        return gateway_error(StatusCode::CONFLICT, "stream task is no longer active");
    }
    (
        StatusCode::ACCEPTED,
        Json(json!({"ok":true,"status":"cancellation_requested","stream_id":run_id})),
    )
        .into_response()
}
