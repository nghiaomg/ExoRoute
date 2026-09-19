use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

pub(crate) const ADMIN_ACCESS_TOKEN_TTL: Duration = Duration::from_secs(10 * 60);
pub(crate) const MAX_ADMIN_STEP_UP_PROOFS: usize = 512;

pub(crate) fn new_admin_token() -> Result<String, ()> {
    let mut bytes = [0_u8; 32];
    let mut rng = OsRng;
    rng.try_fill_bytes(&mut bytes).map_err(|_| ())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn admin_token_digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderAuthFlowStatus {
    Pending,
    Processing,
    Connected,
    Failed,
}

#[derive(Clone)]
pub struct PendingProviderAuthFlow {
    pub adapter_id: String,
    pub provider_id: String,
    pub oauth_state: String,
    pub verifier: String,
    pub redirect_uri: String,
    pub expires_at: Instant,
    pub status: ProviderAuthFlowStatus,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderApiKeyAuthFlowStatus {
    Pending,
    Received,
    Applying,
    Applied,
    Failed,
}

#[derive(Clone)]
pub struct PendingProviderApiKeyAuthFlow {
    pub adapter_id: String,
    pub provider_id: String,
    pub state_hash: Vec<u8>,
    pub encrypted_api_key: Option<Vec<u8>>,
    pub provider_key_id: Option<String>,
    pub user_id: Option<String>,
    pub key_name: Option<String>,
    pub user_name: Option<String>,
    pub expires_at: Instant,
    pub applying_since: Option<Instant>,
    pub status: ProviderApiKeyAuthFlowStatus,
    pub message: Option<String>,
}

#[derive(Clone)]
pub struct AdminSession {
    pub expires_at: Instant,
    pub family_id: String,
    pub must_change_password: bool,
}

#[derive(Clone)]
pub struct AdminRequestContext {
    pub family_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdminStepUpScope {
    DatabaseExport,
    DatabaseImport,
}

#[derive(Clone)]
pub struct AdminStepUpProof {
    pub(crate) family_id: String,
    pub(crate) scope: AdminStepUpScope,
    pub(crate) expires_at: Instant,
}
