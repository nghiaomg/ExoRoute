use super::MAX_TOKEN_BYTES;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The stored Qoder account.
///
/// Qoder's device authorization returns a `dt-...` device token that is valid
/// for ~30 days, along with user details such as user ID, machine ID, and email.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QoderAccount {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub machine_id: Option<String>,
    #[serde(default)]
    pub organization_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
}

impl QoderAccount {
    /// Reads the approval payload of one device-authorization poll.
    pub(crate) fn from_device_approval(value: &Value) -> Result<Self, String> {
        let token = value
            .get("token")
            .or_else(|| value.get("access_token"))
            .and_then(Value::as_str)
            .ok_or_else(|| "Qoder did not return an access token".to_owned())?;
        validate_token(token)?;

        let expires_at = value
            .get("expires_at")
            .and_then(Value::as_u64)
            .or_else(|| {
                value.get("expires_in").and_then(Value::as_u64).map(|secs| {
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    now_ms.saturating_add(secs.saturating_mul(1000))
                })
            });

        let display_name = bounded(
            value
                .get("display_name")
                .or_else(|| value.get("name"))
                .or_else(|| value.get("username"))
                .and_then(Value::as_str),
        );

        Ok(Self {
            access_token: token.to_owned(),
            refresh_token: bounded(value.get("refresh_token").and_then(Value::as_str)),
            user_id: bounded(value.get("user_id").and_then(Value::as_str)),
            machine_id: bounded(value.get("machine_id").and_then(Value::as_str)),
            organization_id: bounded(value.get("organization_id").and_then(Value::as_str)),
            email: bounded(value.get("email").and_then(Value::as_str)),
            display_name,
            region: bounded(value.get("region").and_then(Value::as_str)),
            expires_at,
        })
    }

    /// Decodes an encrypted account record written by this module.
    pub(super) fn decode(value: &Value) -> Result<Self, String> {
        let account: Self = serde_json::from_value(value.clone())
            .map_err(|_| "stored Qoder account data is invalid".to_owned())?;
        validate_token(&account.access_token)?;
        Ok(Self {
            access_token: account.access_token,
            refresh_token: bounded(account.refresh_token.as_deref()),
            user_id: bounded(account.user_id.as_deref()),
            machine_id: bounded(account.machine_id.as_deref()),
            organization_id: bounded(account.organization_id.as_deref()),
            email: bounded(account.email.as_deref()),
            display_name: bounded(account.display_name.as_deref()),
            region: bounded(account.region.as_deref()),
            expires_at: account.expires_at,
        })
    }

    pub(crate) fn display_name(&self) -> String {
        if let Some(name) = self.display_name.as_deref().filter(|s| !s.trim().is_empty()) {
            return name.chars().take(256).collect();
        }
        if let Some(email) = self.email.as_deref().filter(|s| !s.trim().is_empty()) {
            return email.chars().take(256).collect();
        }
        if let Some(user_id) = self.user_id.as_deref().filter(|s| !s.trim().is_empty()) {
            return format!("Qoder ({user_id})").chars().take(256).collect();
        }
        "Qoder account".to_owned()
    }

    /// Stable non-secret identity used to match a reconnection to the account
    /// already stored for this provider.
    ///
    /// Priority:
    /// 1. Email (e.g. `email:user@example.com`)
    /// 2. User ID (e.g. `qoder:user-12345`)
    /// 3. Token SHA256 digest (e.g. `token:abcdef...`)
    pub(super) fn identity(&self) -> String {
        if let Some(email) = self.email.as_deref().map(normalize_identity) {
            return format!("email:{email}");
        }
        if let Some(user_id) = self.user_id.as_deref().map(normalize_identity) {
            return format!("qoder:{user_id}");
        }
        let digest = Sha256::digest(self.access_token.as_bytes());
        let mut encoded = String::with_capacity(32);
        for byte in digest.iter().take(16) {
            encoded.push_str(&format!("{byte:02x}"));
        }
        format!("token:{encoded}")
    }
}

pub(super) fn normalize_identity(value: &str) -> String {
    value.trim().to_lowercase()
}

fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Err("Qoder access token is empty".to_owned());
    }
    if token.len() > MAX_TOKEN_BYTES {
        return Err("Qoder access token exceeded its size limit".to_owned());
    }
    if !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("Qoder access token contains invalid characters".to_owned());
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
