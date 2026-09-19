use super::auth::{
    PendingProviderApiKeyAuthFlow, PendingProviderAuthFlow, ProviderApiKeyAuthFlowStatus,
    ProviderAuthFlowStatus,
};
use super::{
    AppState, MAX_PENDING_API_KEY_AUTH_FLOWS, MAX_PENDING_PROVIDER_AUTH_FLOWS,
    PROVIDER_API_KEY_AUTH_APPLY_RECOVERY_TIMEOUT, PROVIDER_AUTH_TERMINAL_FLOW_TTL,
};
use std::time::Instant;

impl AppState {
    pub async fn register_provider_auth_flow(
        &self,
        flow_id: String,
        flow: PendingProviderAuthFlow,
    ) -> Result<(), &'static str> {
        let now = Instant::now();
        let mut flows = self.admin.provider_auth_flows.lock().await;
        flows.retain(|_, item| {
            item.expires_at > now
                || matches!(
                    item.status,
                    ProviderAuthFlowStatus::Processing | ProviderAuthFlowStatus::Connected
                ) && now.saturating_duration_since(item.expires_at)
                    < PROVIDER_AUTH_TERMINAL_FLOW_TTL
        });
        if flows.len() >= MAX_PENDING_PROVIDER_AUTH_FLOWS {
            return Err("too many provider login attempts are pending");
        }
        flows.insert(flow_id, flow);
        Ok(())
    }

    /// Removes a terminal (`Connected`/`Failed`) OAuth flow once the dashboard
    /// has observed its result. Flows that are never polled are still bounded
    /// by the terminal TTL applied during registration, so neither the entry
    /// nor its OAuth state and PKCE verifier stay in RAM forever.
    pub async fn reap_provider_auth_flow_if_terminal(&self, flow_id: &str) {
        let mut flows = self.admin.provider_auth_flows.lock().await;
        if flows.get(flow_id).is_some_and(|flow| {
            matches!(
                flow.status,
                ProviderAuthFlowStatus::Connected | ProviderAuthFlowStatus::Failed
            )
        }) {
            flows.remove(flow_id);
        }
    }

    pub async fn register_provider_api_key_auth_flow(
        &self,
        flow_id: String,
        flow: PendingProviderApiKeyAuthFlow,
    ) -> Result<(), &'static str> {
        let now = Instant::now();
        let mut flows = self.admin.provider_api_key_auth_flows.lock().await;
        for item in flows.values_mut() {
            if item.status == ProviderApiKeyAuthFlowStatus::Applying
                && item.applying_since.is_some_and(|started| {
                    now.saturating_duration_since(started)
                        >= PROVIDER_API_KEY_AUTH_APPLY_RECOVERY_TIMEOUT
                })
            {
                item.status = ProviderApiKeyAuthFlowStatus::Received;
                item.applying_since = None;
            }
        }
        flows.retain(|_, item| {
            item.expires_at > now || item.status == ProviderApiKeyAuthFlowStatus::Applying
        });
        if flows.len() >= MAX_PENDING_API_KEY_AUTH_FLOWS {
            return Err("too many provider key sign-in attempts are pending");
        }
        flows.insert(flow_id, flow);
        Ok(())
    }
}
