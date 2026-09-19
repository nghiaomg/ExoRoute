//! Admin session barrel: shared types plus lifecycle/record/handler modules.
//!
//! Token issuance and rotation live in `lifecycle`, record encoding in
//! `records`, and Axum handlers in `handlers`. This file only holds the
//! cross-module constants and small crypto/clock helpers.

use super::cookie_policy::{
    clear_refresh_cookie, refresh_cookie, same_origin_browser_post, set_refresh_cookie,
};
use super::*;
use crate::infra::storage::StorageError;
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) mod handlers;
pub(crate) mod lifecycle;
pub(crate) mod records;
#[cfg(test)]
mod tests;

pub(crate) use handlers::{
    logout, reauthenticate, refresh, require_step_up, session_database_error,
};
pub(crate) use lifecycle::{
    family_is_active, issue_admin_session, revoke_all_sessions_in_transaction,
};
#[cfg(test)]
pub(crate) use lifecycle::{issue_admin_session_at, rotate_refresh_token, rotate_refresh_token_at};
#[cfg(test)]
pub(crate) use records::digest_key;

pub(super) const ACCESS_TOKEN_TTL_SECONDS: i64 = 10 * 60;
pub(super) const IDLE_TTL_SECONDS: i64 = 24 * 60 * 60;
pub(super) const ABSOLUTE_TTL_SECONDS: i64 = 7 * 24 * 60 * 60;
pub(super) const MAX_SESSION_FAMILIES: i64 = 128;
pub(crate) const MAX_REFRESH_GENERATION: i64 = 10_080;

pub(super) struct IssuedAdminSession {
    pub access_token: String,
    pub refresh_token: String,
    pub must_change_password: bool,
    pub refresh_max_age: i64,
}

pub(super) enum RefreshResult {
    Rotated(IssuedAdminSession),
    Invalid,
}

pub(super) enum RotationCommit {
    Invalid(Option<String>),
    Rotated {
        family_id: String,
        must_change_password: bool,
        next_idle_expiry: i64,
    },
}

pub(super) fn token_digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

pub(super) fn new_token() -> Result<String, StorageError> {
    let mut bytes = [0_u8; 32];
    let mut rng = OsRng;
    rng.try_fill_bytes(&mut bytes)
        .map_err(|_| StorageError::Invalid("secure random source is unavailable".into()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(super) fn db_now_unix_seconds() -> Result<i64, StorageError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::Invalid("system clock is invalid".into()))?
        .as_secs();
    i64::try_from(seconds).map_err(|_| StorageError::Invalid("system clock is out of range".into()))
}
