//! Provider OAuth account handling and dashboard sign-in panels.
//!
//! Account records, token refresh, and PKCE live here; the matching
//! `ProviderAdapter` implementations live in the parent module and call into
//! these account services.

pub(crate) mod antigravity;
pub(crate) mod cline;
pub(crate) mod codex;
pub(crate) mod panels;
