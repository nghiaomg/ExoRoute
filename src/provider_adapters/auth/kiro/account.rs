use super::MAX_TOKEN_BYTES;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// The stored Kiro account.
///
/// Kiro issues OAuth tokens through three paths that all converge on the same
/// CodeWhisperer runtime: AWS Builder ID/IDC device authorization (which also
/// yields the OIDC `client_id`/`client_secret` needed to refresh), Google or
/// GitHub social login through the Kiro auth service (refresh goes through
/// `prod.us-east-1.auth.desktop.kiro.dev/refreshToken`), and pasted refresh
/// tokens imported from the Kiro IDE.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KiroAccount {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// Unix seconds when `access_token` expires; `0` forces a refresh.
    pub expires_at: i64,
    #[serde(default)]
    pub email: Option<String>,
    /// `builder-id`, `social`, or `imported` — selects the refresh endpoint.
    #[serde(default)]
    pub auth_method: String,
    /// OIDC client credentials captured during device authorization; required
    /// to refresh Builder ID tokens.
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    /// AWS OIDC region for Builder ID/IDC refresh; defaults to `us-east-1`.
    #[serde(default)]
    pub region: Option<String>,
    /// AWS CodeWhisperer profile ARN returned by social login.
    #[serde(default)]
    pub profile_arn: Option<String>,
}

impl KiroAccount {
    /// Reads one token payload produced by device approval, social exchange,
    /// or token import. Field names accept both the AWS OIDC camelCase shape
    /// and the snake_case shape used inside stored records.
    pub(crate) fn from_token_payload(value: &Value) -> Result<Self, String> {
        let access_token = value
            .get("access_token")
            .or_else(|| value.get("accessToken"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| "Kiro did not return an access token".to_owned())?;
        validate_token(access_token)?;
        let expires_in = value
            .get("expires_in")
            .or_else(|| value.get("expiresIn"))
            .and_then(Value::as_i64)
            .unwrap_or(3600)
            .clamp(60, 31_536_000);
        Ok(Self {
            access_token: access_token.to_owned(),
            refresh_token: bounded_opt(
                value
                    .get("refresh_token")
                    .or_else(|| value.get("refreshToken"))
                    .and_then(Value::as_str),
            ),
            expires_at: unix_now().saturating_add(expires_in),
            email: bounded_opt(value.get("email").and_then(Value::as_str)),
            auth_method: bounded_opt(value.get("auth_method").and_then(Value::as_str))
                .unwrap_or_else(|| "builder-id".to_owned()),
            client_id: bounded_opt(
                value
                    .get("client_id")
                    .or_else(|| value.get("clientId"))
                    .and_then(Value::as_str),
            ),
            client_secret: bounded_opt(
                value
                    .get("client_secret")
                    .or_else(|| value.get("clientSecret"))
                    .and_then(Value::as_str),
            ),
            region: bounded_opt(value.get("region").and_then(Value::as_str)),
            profile_arn: bounded_opt(
                value
                    .get("profile_arn")
                    .or_else(|| value.get("profileArn"))
                    .and_then(Value::as_str),
            ),
        })
    }

    /// Decodes a stored account record, re-validating token bounds.
    pub(super) fn decode(value: &Value) -> Result<Self, String> {
        let account: Self = serde_json::from_value(value.clone())
            .map_err(|_| "stored Kiro account data is invalid".to_owned())?;
        validate_token(&account.access_token)?;
        if let Some(refresh) = account.refresh_token.as_deref() {
            validate_token(refresh)?;
        }
        Ok(account)
    }

    pub(crate) fn display_name(&self) -> String {
        self.email
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Kiro account")
            .chars()
            .take(256)
            .collect()
    }

    /// Stable non-secret identity used to match a reconnection to the account
    /// already stored for this provider.
    pub(super) fn identity(&self) -> String {
        if let Some(email) = self.email.as_deref().map(normalize_identity) {
            return format!("email:{email}");
        }
        if let Some(arn) = self
            .profile_arn
            .as_deref()
            .map(normalize_identity)
            .filter(|arn| !arn.is_empty())
        {
            return format!("arn:{arn}");
        }
        let digest = Sha256::digest(self.access_token.as_bytes());
        let mut encoded = String::with_capacity(32);
        for byte in digest.iter().take(16) {
            encoded.push_str(&format!("{byte:02x}"));
        }
        format!("token:{encoded}")
    }

    /// Extra per-account fields the runtime request needs, packed into
    /// `OAuthRequestAuth::account_id` as JSON (the same convention Qoder uses).
    pub(crate) fn request_extras(&self) -> Value {
        serde_json::json!({
            "profile_arn": self.profile_arn,
            "auth_method": self.auth_method,
        })
    }
}

pub(super) fn normalize_identity(value: &str) -> String {
    value.trim().to_lowercase()
}

fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty() {
        return Err("Kiro access token is empty".to_owned());
    }
    if token.len() > MAX_TOKEN_BYTES {
        return Err("Kiro access token exceeded its size limit".to_owned());
    }
    if token.bytes().any(|byte| byte.is_ascii_control()) {
        return Err("Kiro access token contains invalid characters".to_owned());
    }
    Ok(())
}

pub(super) fn bounded_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned)
}

pub(super) fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

/// Decodes the payload section of a JWT without verifying it — used only to
/// surface the account email for display, never for authorization.
pub(crate) fn email_from_jwt(access_token: &str) -> Option<String> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let payload = access_token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    bounded_opt(
        value
            .get("email")
            .or_else(|| value.get("preferred_username"))
            .and_then(Value::as_str),
    )
}
