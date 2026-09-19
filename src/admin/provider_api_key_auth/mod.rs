use super::{ApiResult, fail};
use crate::{
    infra::storage::{Record, Table},
    provider_adapters,
    security::{decrypt_secret, token_hash},
    state::{AppState, PendingProviderApiKeyAuthFlow, ProviderApiKeyAuthFlowStatus},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde_json::{Value, json};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

mod listener;
use listener::ensure_callback_listener_locked;
mod flow;
pub(super) use flow::{
    ApiKeyAuthApplyClaim, ApiKeyCallbackAcceptance, accept_callback_state,
    claim_api_key_auth_apply, pending_callback_adapter_id,
};
#[cfg(test)]
use listener::loopback_addresses;
mod callback;
#[cfg(test)]
pub(super) use callback::callback_origin;
pub(super) use callback::{callback, preflight};

const COMMAND_CODE_AUTH_TTL: Duration = Duration::from_secs(15 * 60);
const CALLBACK_IDLE_GRACE: Duration = Duration::from_secs(30);
const CALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(250);
const CALLBACK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const CALLBACK_BODY_LIMIT: usize = 10 * 1024;
const CALLBACK_KEY_LIMIT: usize = 4096;
const CALLBACK_PORTS: [u16; 10] = [5959, 5960, 5961, 5962, 5963, 5964, 5965, 5966, 5967, 5968];

pub(super) async fn start(
    State(state): State<AppState>,
    Path(provider_id): Path<String>,
) -> ApiResult {
    let provider_key = provider_id.clone();
    let adapter_id = state
        .db
        .read(move |transaction| {
            transaction
                .get::<Record>(Table::Providers, &provider_key)?
                .map(|record| record.text("adapter_id").map(str::to_owned))
                .transpose()
        })
        .await
        .map_err(super::internal)?
        .ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;
    if !provider_adapters::supports_api_key_auth_assist(&adapter_id) {
        return Err(fail(
            StatusCode::BAD_REQUEST,
            "this provider adapter does not support API-key sign-in assistance",
        ));
    }
    if state.config.master_key.is_none() {
        return Err(fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "set EXOROUTE_MASTER_KEY before connecting a provider key",
        ));
    }

    let mut state_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut state_bytes);
    let callback_state = URL_SAFE_NO_PAD.encode(state_bytes);
    let flow_id = uuid::Uuid::new_v4().to_string();
    let _callback_guard = state.admin.provider_api_key_auth_callback_lock.lock().await;
    let port = ensure_callback_listener_locked(&state).await.map_err(|error| {
        tracing::warn!(%error, "could not bind the Command Code loopback callback listener");
        fail(
            StatusCode::SERVICE_UNAVAILABLE,
            "could not start the Command Code localhost callback listener; close applications using ports 5959–5968 or add the API key manually",
        )
    })?;

    let callback_url = format!("http://localhost:{port}/callback");
    let auth_url = provider_adapters::api_key_auth_assist_start_url(
        &adapter_id,
        &callback_url,
        &callback_state,
    )
    .map_err(|_| {
        fail(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not build the provider sign-in URL",
        )
    })?;
    let expires_at = Instant::now() + COMMAND_CODE_AUTH_TTL;
    state
        .register_provider_api_key_auth_flow(
            flow_id.clone(),
            PendingProviderApiKeyAuthFlow {
                adapter_id,
                provider_id,
                state_hash: token_hash(&callback_state),
                encrypted_api_key: None,
                provider_key_id: None,
                user_id: None,
                key_name: None,
                user_name: None,
                expires_at,
                applying_since: None,
                status: ProviderApiKeyAuthFlowStatus::Pending,
                message: None,
            },
        )
        .await
        .map_err(|message| fail(StatusCode::TOO_MANY_REQUESTS, message))?;

    Ok(Json(json!({
        "flow_id": flow_id,
        "auth_url": auth_url,
        "callback_url": callback_url,
        "expires_at_ms": system_time_millis().saturating_add(COMMAND_CODE_AUTH_TTL.as_millis() as i64),
    })))
}

pub(super) async fn status(
    State(state): State<AppState>,
    Path((provider_id, flow_id)): Path<(String, String)>,
) -> ApiResult {
    let mut flows = state.admin.provider_api_key_auth_flows.lock().await;
    let Some(flow) = flows
        .get_mut(&flow_id)
        .filter(|flow| flow.provider_id == provider_id)
    else {
        return Ok(Json(json!({"status":"expired"})));
    };
    let now = Instant::now();
    if flow.status == ProviderApiKeyAuthFlowStatus::Applying
        && flow.applying_since.is_some_and(|started| {
            now.saturating_duration_since(started)
                >= crate::state::PROVIDER_API_KEY_AUTH_APPLY_RECOVERY_TIMEOUT
        })
    {
        flow.status = ProviderApiKeyAuthFlowStatus::Received;
        flow.applying_since = None;
    }
    if flow.expires_at <= now
        && !matches!(
            flow.status,
            ProviderApiKeyAuthFlowStatus::Applying | ProviderApiKeyAuthFlowStatus::Applied
        )
    {
        flow.encrypted_api_key = None;
        flow.status = ProviderApiKeyAuthFlowStatus::Failed;
        flow.message = Some("Command Code sign-in expired. Add the API key manually.".to_owned());
        return Ok(Json(json!({"status":"expired","message":flow.message})));
    }
    let status = match flow.status {
        ProviderApiKeyAuthFlowStatus::Pending => "pending",
        ProviderApiKeyAuthFlowStatus::Received => "received",
        ProviderApiKeyAuthFlowStatus::Applying => "applying",
        ProviderApiKeyAuthFlowStatus::Applied => "applied",
        ProviderApiKeyAuthFlowStatus::Failed => "failed",
    };
    Ok(Json(json!({
        "status": status,
        "message": flow.message,
        "user_id": flow.user_id,
        "key_name": flow.key_name,
        "user_name": flow.user_name,
        "provider_key_id": flow.provider_key_id,
        "expires_at_ms": system_time_millis().saturating_add(
            flow.expires_at.saturating_duration_since(Instant::now()).as_millis() as i64,
        ),
    })))
}

pub(super) async fn apply(
    State(state): State<AppState>,
    Path((provider_id, flow_id)): Path<(String, String)>,
) -> ApiResult {
    let (adapter_id, encrypted_api_key, key_name) =
        match claim_api_key_auth_apply(&state, &provider_id, &flow_id).await? {
            ApiKeyAuthApplyClaim::AlreadyApplied(provider_key_id) => {
                return Ok(Json(json!({
                    "status": "applied",
                    "provider_key_id": provider_key_id,
                })));
            }
            ApiKeyAuthApplyClaim::Claimed {
                adapter_id,
                encrypted_api_key,
                key_name,
            } => (adapter_id, encrypted_api_key, key_name),
        };
    let key_id = format!("{adapter_id}-auth-{flow_id}");
    let result: Result<Value, (StatusCode, Json<Value>)> = async {
        if !provider_adapters::supports_api_key_auth_assist(&adapter_id) {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "provider sign-in flow is invalid",
            ));
        }
        let provider_key = provider_id.clone();
        let provider_adapter = state
            .db
            .read(move |transaction| {
                transaction
                    .get::<Record>(Table::Providers, &provider_key)?
                    .map(|record| record.text("adapter_id").map(str::to_owned))
                    .transpose()
            })
            .await
            .map_err(super::internal)?
            .ok_or_else(|| fail(StatusCode::NOT_FOUND, "provider not found"))?;
        if provider_adapter != adapter_id {
            return Err(fail(
                StatusCode::CONFLICT,
                "provider adapter changed during sign-in",
            ));
        }
        let api_key = decrypt_secret(state.config.master_key.as_ref(), Some(&encrypted_api_key))
            .map_err(|_| {
                fail(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "could not decrypt the pending provider key",
                )
            })?
            .ok_or_else(|| {
                fail(
                    StatusCode::CONFLICT,
                    "provider API key is no longer available",
                )
            })?;
        super::providers::add_provider_key_with_id(
            &state,
            &provider_id,
            key_name.as_deref(),
            &api_key,
            Some(&key_id),
        )
        .await
    }
    .await;

    match result {
        Ok(saved) => {
            let mut flows = state.admin.provider_api_key_auth_flows.lock().await;
            if let Some(flow) = flows.get_mut(&flow_id)
                && flow.status == ProviderApiKeyAuthFlowStatus::Applying
            {
                flow.encrypted_api_key = None;
                flow.provider_key_id = Some(key_id);
                flow.status = ProviderApiKeyAuthFlowStatus::Applied;
                flow.applying_since = None;
                flow.message = Some("Provider API key saved".to_owned());
            }
            Ok(Json(json!({"status":"applied","key":saved})))
        }
        Err(error) => {
            let mut flows = state.admin.provider_api_key_auth_flows.lock().await;
            if let Some(flow) = flows.get_mut(&flow_id)
                && flow.status == ProviderApiKeyAuthFlowStatus::Applying
            {
                flow.status = ProviderApiKeyAuthFlowStatus::Received;
                flow.applying_since = None;
            }
            Err(error)
        }
    }
}

fn system_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
#[cfg(test)]
mod tests;
