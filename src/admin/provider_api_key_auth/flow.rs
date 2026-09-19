//! State transitions for the provider API-key sign-in flow.
//!
//! This module keeps flow claims and callback-state acceptance separate from
//! the HTTP handlers and listener lifecycle.

use super::fail;
use crate::{
    security::secure_eq,
    state::{AppState, PendingProviderApiKeyAuthFlow, ProviderApiKeyAuthFlowStatus},
};
use axum::{Json, http::StatusCode};
use serde_json::{Value, json};
use std::{collections::HashMap, time::Instant};

pub(crate) enum ApiKeyAuthApplyClaim {
    AlreadyApplied(Option<String>),
    Claimed {
        adapter_id: String,
        encrypted_api_key: Vec<u8>,
        key_name: Option<String>,
    },
}

pub(crate) async fn claim_api_key_auth_apply(
    state: &AppState,
    provider_id: &str,
    flow_id: &str,
) -> Result<ApiKeyAuthApplyClaim, (StatusCode, Json<Value>)> {
    let mut flows = state.admin.provider_api_key_auth_flows.lock().await;
    let Some(flow) = flows
        .get_mut(flow_id)
        .filter(|flow| flow.provider_id == provider_id)
    else {
        return Err(fail(
            StatusCode::NOT_FOUND,
            "provider sign-in flow not found",
        ));
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
    if flow.status == ProviderApiKeyAuthFlowStatus::Applied {
        return Ok(ApiKeyAuthApplyClaim::AlreadyApplied(
            flow.provider_key_id.clone(),
        ));
    }
    if flow.status == ProviderApiKeyAuthFlowStatus::Applying {
        return Err(fail(
            StatusCode::CONFLICT,
            "provider API key is already being saved",
        ));
    }
    if flow.expires_at <= now {
        flow.encrypted_api_key = None;
        flow.status = ProviderApiKeyAuthFlowStatus::Failed;
        flow.applying_since = None;
        return Err(fail(StatusCode::GONE, "provider sign-in flow expired"));
    }
    if flow.status != ProviderApiKeyAuthFlowStatus::Received {
        return Err(fail(
            StatusCode::CONFLICT,
            "provider has not returned an API key yet",
        ));
    }
    let Some(encrypted_api_key) = flow.encrypted_api_key.clone() else {
        return Err(fail(
            StatusCode::CONFLICT,
            "provider API key is no longer available",
        ));
    };
    flow.status = ProviderApiKeyAuthFlowStatus::Applying;
    flow.applying_since = Some(now);
    Ok(ApiKeyAuthApplyClaim::Claimed {
        adapter_id: flow.adapter_id.clone(),
        encrypted_api_key,
        key_name: flow.key_name.clone(),
    })
}

pub(crate) struct ApiKeyCallbackAcceptance<'a> {
    pub(crate) adapter_id: &'a str,
    pub(crate) submitted_state_hash: &'a [u8],
    pub(crate) encrypted_api_key: Vec<u8>,
    pub(crate) user_id: Option<String>,
    pub(crate) key_name: Option<String>,
    pub(crate) user_name: Option<String>,
    pub(crate) now: Instant,
}

pub(crate) fn accept_callback_state(
    flows: &mut HashMap<String, PendingProviderApiKeyAuthFlow>,
    acceptance: ApiKeyCallbackAcceptance<'_>,
) -> Option<Value> {
    let ApiKeyCallbackAcceptance {
        adapter_id,
        submitted_state_hash,
        encrypted_api_key,
        user_id,
        key_name,
        user_name,
        now,
    } = acceptance;
    flows.retain(|_, flow| {
        flow.expires_at > now || flow.status == ProviderApiKeyAuthFlowStatus::Applying
    });
    let (_, flow) = flows.iter_mut().find(|(_, flow)| {
        flow.status == ProviderApiKeyAuthFlowStatus::Pending
            && flow.expires_at > now
            && flow.adapter_id == adapter_id
            && secure_eq(&flow.state_hash, submitted_state_hash)
    })?;
    flow.encrypted_api_key = Some(encrypted_api_key);
    flow.user_id = normalized_metadata(user_id);
    flow.key_name = normalized_metadata(key_name);
    flow.user_name = normalized_metadata(user_name);
    flow.status = ProviderApiKeyAuthFlowStatus::Received;
    Some(json!({
        "user_id": flow.user_id,
        "key_name": flow.key_name,
        "user_name": flow.user_name
    }))
}

pub(crate) fn pending_callback_adapter_id(
    flows: &mut HashMap<String, PendingProviderApiKeyAuthFlow>,
    submitted_state_hash: &[u8],
    now: Instant,
) -> Option<String> {
    flows.retain(|_, flow| {
        flow.expires_at > now || flow.status == ProviderApiKeyAuthFlowStatus::Applying
    });
    flows
        .values()
        .find(|flow| {
            flow.status == ProviderApiKeyAuthFlowStatus::Pending
                && flow.expires_at > now
                && secure_eq(&flow.state_hash, submitted_state_hash)
        })
        .map(|flow| flow.adapter_id.clone())
}

fn normalized_metadata(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(256).collect())
}
