use super::token::{bounded, parse_expiry_value, unix_now, validate_token};
use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClineAccount {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug)]
pub enum RefreshError {
    ReauthenticationRequired,
    Temporary(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenPayload {
    #[serde(alias = "access_token")]
    access_token: String,
    #[serde(default, alias = "refresh_token")]
    refresh_token: Option<String>,
    #[serde(default, alias = "expires_in")]
    expires_in: Option<i64>,
    #[serde(default, alias = "expires_at")]
    expires_at: Option<Value>,
    #[serde(default, alias = "account_id")]
    account_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

impl ClineAccount {
    pub(crate) fn parse(value: &Value) -> Result<Self, String> {
        Self::parse_at(value, unix_now())
    }

    pub(crate) fn parse_at(value: &Value, now: i64) -> Result<Self, String> {
        let data = value
            .get("data")
            .filter(|item| item.is_object())
            .unwrap_or(value);
        let payload: TokenPayload = serde_json::from_value(data.clone())
            .map_err(|_| "Cline OAuth response did not contain valid tokens".to_owned())?;
        validate_token(&payload.access_token)?;
        if let Some(token) = payload.refresh_token.as_deref() {
            validate_token(token)?;
        }
        let expiry = payload
            .expires_at
            .as_ref()
            .and_then(parse_expiry_value)
            .or_else(|| {
                payload
                    .expires_in
                    .map(|seconds| now.saturating_add(seconds.clamp(60, 31_536_000)))
            })
            .unwrap_or_else(|| now.saturating_add(3600))
            .clamp(now.saturating_add(60), now.saturating_add(31_536_000));
        Ok(Self {
            access_token: payload.access_token,
            refresh_token: payload.refresh_token,
            expires_at: expiry,
            account_id: bounded(payload.account_id),
            email: bounded(payload.email),
            name: bounded(payload.name),
        })
    }

    pub fn display_name(&self) -> String {
        self.name
            .as_deref()
            .or(self.email.as_deref())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Cline account")
            .chars()
            .take(256)
            .collect()
    }
}
