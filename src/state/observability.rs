use super::auth::{ADMIN_ACCESS_TOKEN_TTL, AdminSession, AdminStepUpProof, AdminStepUpScope};
use super::{
    AppState, MAX_ADMIN_ACCESS_TOKENS, MAX_ADMIN_STEP_UP_PROOFS, RequestLogRecord,
    admin_token_digest, new_admin_token,
};
use crate::state::{RequestLiveEvent, RequestLiveGuard, RequestLiveSnapshot};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch};

impl AppState {
    pub fn enqueue_request_log(&self, record: RequestLogRecord) {
        self.telemetry.enqueue_request_log(record);
    }

    pub(crate) fn begin_request_live(
        &self,
        request_id: &str,
        route_alias: &str,
        model: &str,
        client_protocol: &str,
        api_key_id: Option<&str>,
    ) -> RequestLiveGuard {
        self.request_live
            .start(request_id, route_alias, model, client_protocol, api_key_id)
    }

    pub(crate) fn request_live_snapshot(&self) -> RequestLiveSnapshot {
        self.request_live.snapshot()
    }

    pub(crate) fn subscribe_request_live(&self) -> broadcast::Receiver<RequestLiveEvent> {
        self.request_live.subscribe()
    }

    pub fn record_api_key_request(&self, api_key_id: String) {
        self.telemetry.record_api_key_request(api_key_id);
    }

    pub fn background_workers_ready(&self) -> bool {
        !self.telemetry.is_closed()
    }

    pub fn shutdown_receiver(&self) -> watch::Receiver<bool> {
        self.shutdown.subscribe()
    }

    pub fn request_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    pub async fn register_stream_cancellation(&self, run_id: &str) -> watch::Receiver<bool> {
        let (sender, receiver) = watch::channel(false);
        self.gates
            .stream_cancellations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(run_id.to_owned(), sender);
        receiver
    }

    pub fn request_stream_cancellation(&self, run_id: &str) -> bool {
        let cancellations = self
            .gates
            .stream_cancellations
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cancellations
            .get(run_id)
            .is_some_and(|sender| sender.send(true).is_ok())
    }

    pub fn remove_stream_cancellation(&self, run_id: &str) {
        self.gates
            .stream_cancellations
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(run_id);
    }

    #[cfg(test)]
    pub async fn issue_admin_access_token(
        &self,
        family_id: String,
        must_change_password: bool,
    ) -> Result<String, ()> {
        let token = new_admin_token()?;
        self.cache_admin_access_token(token.clone(), family_id, must_change_password)
            .await;
        Ok(token)
    }

    pub fn generate_admin_access_token(&self) -> Result<String, ()> {
        new_admin_token()
    }

    pub async fn cache_admin_access_token(
        &self,
        token: String,
        family_id: String,
        must_change_password: bool,
    ) {
        let digest = admin_token_digest(&token);
        let now = Instant::now();
        let mut sessions = self.admin.admin_sessions.write().await;
        sessions.retain(|_, session| session.expires_at > now);
        if sessions.len() >= MAX_ADMIN_ACCESS_TOKENS
            && let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, session)| session.expires_at)
                .map(|(digest, _)| *digest)
        {
            sessions.remove(&oldest);
        }
        sessions.insert(
            digest,
            AdminSession {
                expires_at: now + ADMIN_ACCESS_TOKEN_TTL,
                family_id,
                must_change_password,
            },
        );
    }

    pub async fn admin_session(&self, token: &str) -> Option<AdminSession> {
        let now = Instant::now();
        let mut sessions = self.admin.admin_sessions.write().await;
        sessions.retain(|_, session| session.expires_at > now);
        sessions.get(&admin_token_digest(token)).cloned()
    }

    pub async fn revoke_admin_family(&self, family_id: &str) {
        self.admin
            .admin_sessions
            .write()
            .await
            .retain(|_, session| session.family_id != family_id);
        self.admin
            .admin_step_up_proofs
            .write()
            .await
            .retain(|_, proof| proof.family_id != family_id);
    }

    pub async fn clear_admin_sessions(&self) {
        self.admin.admin_sessions.write().await.clear();
        self.admin.admin_step_up_proofs.write().await.clear();
    }

    pub async fn issue_admin_step_up_proof(
        &self,
        family_id: String,
        scope: AdminStepUpScope,
    ) -> Result<String, ()> {
        let token = new_admin_token()?;
        let now = Instant::now();
        let mut proofs = self.admin.admin_step_up_proofs.write().await;
        proofs.retain(|_, proof| proof.expires_at > now);
        if proofs.len() >= MAX_ADMIN_STEP_UP_PROOFS
            && let Some(oldest) = proofs
                .iter()
                .min_by_key(|(_, proof)| proof.expires_at)
                .map(|(digest, _)| *digest)
        {
            proofs.remove(&oldest);
        }
        proofs.insert(
            admin_token_digest(&token),
            AdminStepUpProof {
                family_id,
                scope,
                expires_at: now + Duration::from_secs(5 * 60),
            },
        );
        Ok(token)
    }

    pub async fn consume_admin_step_up_proof(
        &self,
        family_id: &str,
        token: &str,
        scope: AdminStepUpScope,
    ) -> bool {
        let now = Instant::now();
        let mut proofs = self.admin.admin_step_up_proofs.write().await;
        proofs.retain(|_, proof| proof.expires_at > now);
        let Some(proof) = proofs.remove(&admin_token_digest(token)) else {
            return false;
        };
        proof.family_id == family_id && proof.scope == scope && proof.expires_at > now
    }
}
