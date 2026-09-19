use super::super::{ApiResult, fail, internal};
use super::{MAX_PROVIDER_SCAN_ROWS, provider_is_deleting, provider_model_prefix_value};
use crate::{
    infra::storage::{Record, StorageError, Table},
    provider_adapters,
    state::AppState,
};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event, KeepAlive, Sse},
    },
};
use serde_json::{Value, json};
use std::{convert::Infallible, sync::atomic::Ordering};

pub(crate) async fn list_providers(State(state): State<AppState>) -> ApiResult {
    let master_key = state.config.master_key;
    let providers = state
        .db
        .read(move |transaction| {
            let rows = transaction.scan_prefix::<String>(
                Table::ProviderNameIndex,
                "",
                MAX_PROVIDER_SCAN_ROWS.saturating_add(1),
            )?;
            if rows.len() > MAX_PROVIDER_SCAN_ROWS {
                return Err(StorageError::Busy);
            }
            let mut providers = Vec::with_capacity(rows.len());
            for (_, id) in rows {
                let record = transaction
                    .get::<Record>(Table::Providers, &id)?
                    .ok_or_else(|| {
                        StorageError::Invalid(
                            "provider name index points to a missing provider".to_owned(),
                        )
                    })?;
                let adapter_id = record.text("adapter_id")?.to_owned();
                let local_rpm_target =
                    super::validation::stored_local_rpm_target(&record, &adapter_id)?;
                let (thinking_mode, thinking_override) =
                    super::validation::stored_thinking_settings(&record)?;
                let supported = record.text("supported_protocols")?;
                let api_key_count = record.integer("api_key_count")?;
                let custom_headers =
                    super::custom_headers::stored_custom_headers(&record, master_key.as_ref())
                        .map_err(StorageError::Invalid)?
                        .into_keys()
                        .collect();
                providers.push(super::provider_view::ProviderView {
                    id: record.text("id")?.to_owned(),
                    capabilities: provider_adapters::capabilities(&adapter_id),
                    adapter_id,
                    name: record.text("name")?.to_owned(),
                    base_url: record.text("base_url")?.to_owned(),
                    logo_url: record.optional_text("logo_url")?.map(str::to_owned),
                    model_prefix: provider_model_prefix_value(&record, &id)?,
                    enabled: record.boolean("enabled")?,
                    auth_type: record.text("auth_type")?.to_owned(),
                    auth_header: record.optional_text("auth_header")?.map(str::to_owned),
                    custom_headers,
                    has_api_key: api_key_count > 0 || record.optional_bytes("secret")?.is_some(),
                    api_key_count,
                    invalid_api_key_count: record.integer("invalid_api_key_count")?,
                    model_count: record.integer("model_count")?,
                    local_rpm_target,
                    preferred_protocol: record.text("preferred_protocol")?.to_owned(),
                    supported_protocols: serde_json::from_str(supported).map_err(|error| {
                        StorageError::Invalid(format!("provider protocol list is invalid: {error}"))
                    })?,
                    thinking_mode,
                    thinking_override,
                    key_strategy: super::validation::stored_key_strategy(&record)?,
                    created_at: record.text("created_at")?.to_owned(),
                    updated_at: record.text("updated_at")?.to_owned(),
                });
            }
            Ok(providers)
        })
        .await
        .map_err(internal)?;
    Ok(Json(json!({"providers":providers})))
}

/// Recomputes the enabled-provider projection after configuration changes.
/// The first SSE connection initializes the cache lazily. The lock keeps
/// concurrent refreshes from publishing an older scan; gateway requests never
/// take it.
pub(crate) async fn refresh_enabled_provider_count(
    state: &AppState,
) -> Result<usize, StorageError> {
    refresh_enabled_provider_count_inner(state, true).await
}

async fn enabled_provider_count(state: &AppState) -> Result<usize, StorageError> {
    refresh_enabled_provider_count_inner(state, false).await
}

async fn refresh_enabled_provider_count_inner(
    state: &AppState,
    force: bool,
) -> Result<usize, StorageError> {
    if !force
        && state
            .upstream_live_count_initialized
            .load(Ordering::Acquire)
    {
        return Ok(*state.upstream_live_count.borrow());
    }
    let _refresh_guard = state.upstream_live_count_refresh_lock.lock().await;
    if !force
        && state
            .upstream_live_count_initialized
            .load(Ordering::Acquire)
    {
        return Ok(*state.upstream_live_count.borrow());
    }
    let count = state
        .db
        .read(|transaction| {
            let rows = transaction.scan_prefix::<String>(
                Table::ProviderNameIndex,
                "",
                MAX_PROVIDER_SCAN_ROWS.saturating_add(1),
            )?;
            if rows.len() > MAX_PROVIDER_SCAN_ROWS {
                return Err(StorageError::Busy);
            }
            let mut count = 0_usize;
            for (_, id) in rows {
                let provider = transaction
                    .get::<Record>(Table::Providers, &id)?
                    .ok_or_else(|| {
                        StorageError::Invalid(
                            "provider name index points to a missing provider".to_owned(),
                        )
                    })?;
                if provider.boolean("enabled")? && !provider_is_deleting(&provider)? {
                    count = count.checked_add(1).ok_or_else(|| {
                        StorageError::Invalid("enabled provider count overflowed".to_owned())
                    })?;
                }
            }
            Ok(count)
        })
        .await?;
    if *state.upstream_live_count.borrow() != count {
        state.upstream_live_count.send_replace(count);
    }
    state
        .upstream_live_count_initialized
        .store(true, Ordering::Release);
    Ok(count)
}

pub(crate) async fn notify_enabled_provider_count_changed(state: &AppState) {
    if let Err(error) = refresh_enabled_provider_count(state).await {
        tracing::warn!(%error, "could not refresh the overview upstream count");
    }
}

pub(crate) async fn upstream_events(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, Json<Value>)> {
    let upstream = state.operational_settings().settings.upstream;

    let connection_permit = state
        .admin
        .admin_upstream_event_streams
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            fail(
                StatusCode::TOO_MANY_REQUESTS,
                "The ExoRoute API is busy. Try again shortly.",
            )
        })?;
    let mut updates = state.upstream_live_count.subscribe();
    enabled_provider_count(&state).await.map_err(internal)?;
    let initial_count = *updates.borrow_and_update();
    let events = async_stream::stream! {
        let _connection_permit = connection_permit;
        let mut last_count = initial_count;
        yield Ok::<Event, Infallible>(upstream_count_event(initial_count));

        let max_duration = tokio::time::sleep(upstream.provider_live_events_max_duration);
        tokio::pin!(max_duration);
        loop {
            tokio::select! {
                _ = &mut max_duration => break,
                changed = updates.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let count = *updates.borrow_and_update();
                    if count == last_count {
                        continue;
                    }
                    last_count = count;
                    yield Ok(upstream_count_event(count));
                }
            }
        }
    };

    Ok(Sse::new(events).keep_alive(
        KeepAlive::new()
            .interval(upstream.provider_live_events_keepalive)
            .text("keep-alive"),
    ))
}

fn upstream_count_event(enabled_provider_count: usize) -> Event {
    Event::default().event("upstream-count").data(format!(
        "{{\"enabled_provider_count\":{enabled_provider_count}}}"
    ))
}
