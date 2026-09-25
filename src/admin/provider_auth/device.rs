//! Device-authorization provider sign-in.
//!
//! Providers such as Kilo Code have no redirect URI: they issue a pending code,
//! the operator approves it in the provider's own page, and the dashboard polls
//! until the account is connected. The start handler registers the flow and the
//! poll handler performs the upstream poll the dashboard requests.

use super::*;
use std::time::Duration;

/// Starts a device authorization for a provider whose adapter supports it.
pub(crate) async fn start_provider_device_auth(
    state: AppState,
    provider_id: String,
    adapter_id: String,
) -> ApiResult {
    let authorization = provider_adapters::start_device_authorization(&adapter_id, &state)
        .await
        .map_err(|error| {
            tracing::warn!(adapter_id = %adapter_id, error = %error, "could not start device authorization");
            fail(StatusCode::BAD_GATEWAY, error)
        })?;
    let flow_id = uuid::Uuid::new_v4().to_string();
    let expires_in = authorization.expires_in.min(PROVIDER_AUTH_FLOW_TTL);
    state
        .register_provider_auth_flow(
            flow_id.clone(),
            PendingProviderAuthFlow {
                adapter_id,
                provider_id,
                method: ProviderAuthFlowMethod::DeviceCode {
                    device_code: authorization.device_code.clone(),
                    user_code: authorization.user_code.clone(),
                    verification_uri: authorization.verification_uri.clone(),
                    interval: authorization.interval.as_secs().max(1),
                    last_poll_at: None,
                },
                expires_at: std::time::Instant::now() + expires_in,
                status: ProviderAuthFlowStatus::Pending,
                message: None,
            },
        )
        .await
        .map_err(|message| fail(StatusCode::TOO_MANY_REQUESTS, message))?;
    Ok(Json(json!({
        "flow_id": flow_id,
        "authorization_url": authorization.verification_uri,
        "user_code": authorization.user_code,
        "expires_in": expires_in.as_secs(),
        "interval": authorization.interval.as_secs(),
        "method": "device_code",
    })))
}

/// Polls one pending device authorization and reports the flow status.
///
/// The poll is guarded per flow so a repeated dashboard poll cannot reach the
/// provider more often than the cadence the device grant asked for. A transient
/// upstream failure leaves the flow pending with a message the dashboard shows
/// while it keeps polling.
pub(crate) async fn provider_auth_poll(
    State(state): State<AppState>,
    Path((provider_id, flow_id)): Path<(String, String)>,
) -> ApiResult {
    let now = std::time::Instant::now();
    let prepared = {
        let mut flows = state.admin.provider_auth_flows.lock().await;
        let Some(flow) = flows.get_mut(&flow_id) else {
            return Ok(Json(json!({"status": "expired"})));
        };
        if flow.provider_id != provider_id {
            return Ok(Json(json!({"status": "expired"})));
        }
        if matches!(
            flow.status,
            ProviderAuthFlowStatus::Connected | ProviderAuthFlowStatus::Failed
        ) {
            return Ok(Json(json!({
                "status": match flow.status {
                    ProviderAuthFlowStatus::Connected => "connected",
                    _ => "failed",
                },
                "message": flow.message,
            })));
        }
        if flow.expires_at <= now {
            return Ok(Json(json!({"status": "expired"})));
        }
        let ProviderAuthFlowMethod::DeviceCode {
            device_code,
            interval,
            last_poll_at,
            ..
        } = &mut flow.method
        else {
            return Err(fail(
                StatusCode::BAD_REQUEST,
                "this sign-in does not use device authorization",
            ));
        };
        if last_poll_at.is_some_and(|last| {
            now.saturating_duration_since(last) < Duration::from_secs((*interval).max(1))
        }) {
            return Ok(Json(json!({"status": "pending", "message": flow.message})));
        }
        *last_poll_at = Some(now);
        (flow.adapter_id.clone(), device_code.clone())
    };

    let (adapter_id, device_code) = prepared;
    let outcome =
        provider_adapters::poll_device_authorization(&adapter_id, &state, &device_code).await;
    match outcome {
        Ok(provider_adapters::AdapterDevicePoll::Approved(account)) => {
            finish_provider_auth_flow(&state, &flow_id, &adapter_id, &provider_id, account).await;
        }
        Ok(provider_adapters::AdapterDevicePoll::Denied) => {
            fail_provider_auth_flow(
                &state,
                &flow_id,
                "The Kilo Code request was denied; start sign-in again",
            )
            .await;
        }
        Ok(provider_adapters::AdapterDevicePoll::Expired) => {
            fail_provider_auth_flow(
                &state,
                &flow_id,
                "The Kilo Code device code expired; start sign-in again",
            )
            .await;
        }
        Ok(provider_adapters::AdapterDevicePoll::SlowDown) => {
            note_provider_auth_flow(
                &state,
                &flow_id,
                "Kilo Code is limiting sign-in checks; the next check happens automatically",
            )
            .await;
        }
        Ok(provider_adapters::AdapterDevicePoll::Pending) => {
            note_provider_auth_flow(&state, &flow_id, "").await;
        }
        Err(message) => {
            // A transient upstream failure must not fail a sign-in the operator
            // may still approve; the dashboard keeps polling until the flow
            // expires.
            note_provider_auth_flow(&state, &flow_id, &message).await;
        }
    }

    let flows = state.admin.provider_auth_flows.lock().await;
    let Some(flow) = flows.get(&flow_id) else {
        return Ok(Json(json!({"status": "expired"})));
    };
    let status = match flow.status {
        ProviderAuthFlowStatus::Pending | ProviderAuthFlowStatus::Processing => "pending",
        ProviderAuthFlowStatus::Connected => "connected",
        ProviderAuthFlowStatus::Failed => "failed",
    };
    Ok(Json(json!({"status": status, "message": flow.message})))
}
