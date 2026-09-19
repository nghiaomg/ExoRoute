//! Authentication, credential encryption, egress policy, and rate limiting.
//!
//! `secrets` holds the credential primitives and is re-exported here so callers
//! keep a single `security` entry point for secret handling.

pub(crate) mod client_ip;
mod credential_secrets;
pub(crate) mod egress;
pub(crate) mod rate_limit;

pub use credential_secrets::{decrypt_secret, encrypt_secret, secure_eq, token_hash};
