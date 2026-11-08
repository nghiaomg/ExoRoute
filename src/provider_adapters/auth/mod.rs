//! Provider OAuth account handling and dashboard sign-in panels.
//!
//! Account records, token refresh, and PKCE live here; the matching
//! `ProviderAdapter` implementations live in the parent module and call into
//! these account services.

pub(crate) mod antigravity;
pub(crate) mod cline;
pub(crate) mod codex;
pub(crate) mod kilocode;
pub(crate) mod kiro;
pub(crate) mod muse;
pub(crate) mod panels;
pub(crate) mod qoder;

/// Refresh this far ahead of the recorded expiry so a token never expires
/// mid-request.
pub(crate) const REFRESH_LEAD_SECONDS: i64 = 5 * 60;

/// Decodes the payload segment of a JWT without verifying it — callers must
/// treat the claims as display metadata only, never as proof of identity.
pub(crate) fn jwt_claims(token: &str) -> Option<serde_json::Value> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// RFC 7636 S256 code challenge for a PKCE verifier.
pub(crate) fn pkce_challenge(verifier: &str) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use sha2::{Digest, Sha256};
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Trims optional account metadata, rejecting empty, over-512-byte, or
/// control-character values.
pub(crate) fn bounded_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned)
}

/// Current Unix time in seconds, saturating at `i64::MAX` and falling back to
/// 0 when the system clock is before the epoch.
pub(crate) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}
