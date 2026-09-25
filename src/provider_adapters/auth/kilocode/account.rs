use super::MAX_TOKEN_BYTES;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The stored Kilo Code account. Kilo Code's device authorization returns a
/// long-lived token and no expiry, so the record carries no refresh state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KilocodeAccount {
    pub access_token: String,
    #[serde(default)]
    pub email: Option<String>,
}

impl KilocodeAccount {
    /// Reads the approval payload of one device-authorization poll that
    /// returned `{"status":"approved","token":...,"userEmail":...}`.
    pub(crate) fn from_device_approval(value: &Value) -> Result<Self, String> {
        let token = value
            .get("token")
            .and_then(Value::as_str)
            .ok_or_else(|| "Kilo Code did not return an access token".to_owned())?;
        validate_token(token)?;
        Ok(Self {
            access_token: token.to_owned(),
            email: bounded(value.get("userEmail").and_then(Value::as_str)),
        })
    }

    /// Decodes an encrypted account record that was written by this module.
    pub(super) fn decode(value: &Value) -> Result<Self, String> {
        let account: Self = serde_json::from_value(value.clone())
            .map_err(|_| "stored Kilo Code account data is invalid".to_owned())?;
        validate_token(&account.access_token)?;
        Ok(Self {
            access_token: account.access_token,
            email: bounded(account.email.as_deref()),
        })
    }

    pub(crate) fn display_name(&self) -> String {
        self.email
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Kilo Code account")
            .chars()
            .take(256)
            .collect()
    }

    /// Stable non-secret identity used to match a reconnection to the account
    /// already stored for this provider. The email is the natural identity; a
    /// digest of the token is used when Kilo Code returns no email, so
    /// reconnecting the same account updates its credential instead of adding
    /// a duplicate. The digest never reveals the token.
    pub(super) fn identity(&self) -> String {
        match self.email.as_deref().map(normalize_identity) {
            Some(email) => format!("email:{email}"),
            None => {
                let digest = Sha256::digest(self.access_token.as_bytes());
                let mut encoded = String::with_capacity(32);
                for byte in digest.iter().take(16) {
                    encoded.push_str(&format!("{byte:02x}"));
                }
                format!("token:{encoded}")
            }
        }
    }
}

pub(super) fn normalize_identity(value: &str) -> String {
    value.trim().to_lowercase()
}

/// A Kilo Code token becomes a Bearer header value, so it must be non-empty and
/// printable ASCII. Rejecting anything else keeps a corrupt record from being
/// stored and later breaking a request build.
fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Err("Kilo Code access token is empty".to_owned());
    }
    if token.len() > MAX_TOKEN_BYTES {
        return Err("Kilo Code access token exceeded its size limit".to_owned());
    }
    if !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("Kilo Code access token contains invalid characters".to_owned());
    }
    Ok(())
}

pub(super) fn bounded(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned)
}
