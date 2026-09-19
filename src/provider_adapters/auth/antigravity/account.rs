use super::{PROFILE_IDE, bounded_text, bounded_token, unix_now};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AntigravityAccount {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    pub expires_at: i64,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default = "default_client_profile")]
    pub client_profile: String,
}

fn default_client_profile() -> String {
    PROFILE_IDE.to_owned()
}

#[derive(Debug)]
pub enum RefreshError {
    ReauthenticationRequired,
    Temporary(String),
}

impl AntigravityAccount {
    pub(super) fn from_token_response(value: &Value) -> Result<Self, String> {
        let access_token = bounded_token(
            value
                .get("access_token")
                .and_then(Value::as_str)
                .ok_or("Antigravity OAuth did not return an access token")?,
        )?;
        let refresh_token = value
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(bounded_token)
            .transpose()?;
        let expires_in = value
            .get("expires_in")
            .and_then(Value::as_i64)
            .unwrap_or(3600)
            .clamp(60, 31_536_000);
        Ok(Self {
            access_token,
            refresh_token,
            expires_at: unix_now().saturating_add(expires_in),
            scope: value.get("scope").and_then(Value::as_str).map(bounded_text),
            email: None,
            project_id: None,
            tier: None,
            client_profile: PROFILE_IDE.to_owned(),
        })
    }

    pub fn display_name(&self) -> String {
        self.email
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("Antigravity account")
            .chars()
            .take(256)
            .collect()
    }
}
