use super::*;

use super::token::{decode_jwt_claims, validate_token};
use super::usage::usage_core::unix_now;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodexAccount {
    pub access_token: String,
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
    pub expires_at: i64,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub chatgpt_user_id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
}

impl CodexAccount {
    pub(super) fn from_tokens(tokens: TokenResponse) -> Result<Self, String> {
        validate_token(&tokens.access_token)?;
        if let Some(refresh_token) = tokens.refresh_token.as_deref() {
            validate_token(refresh_token)?;
        }
        if let Some(id_token) = tokens.id_token.as_deref() {
            validate_token(id_token)?;
        }
        let expires_in = tokens.expires_in.unwrap_or(3600).clamp(60, 31_536_000);
        let mut account = Self {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            id_token: tokens.id_token,
            expires_at: unix_now().saturating_add(expires_in),
            account_id: None,
            chatgpt_user_id: None,
            email: None,
            plan: None,
        };
        account.refresh_metadata();
        Ok(account)
    }

    pub fn refresh_metadata(&mut self) {
        let claim_sources = [
            self.id_token.as_deref().and_then(decode_jwt_claims),
            decode_jwt_claims(&self.access_token),
        ];
        let mut jwt_subject = None;
        for claims in claim_sources.into_iter().flatten() {
            if self.email.is_none() {
                self.email = claims
                    .get("email")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            let auth = claims
                .get("https://api.openai.com/auth")
                .or_else(|| claims.get("https://chatgpt.com/auth"));
            if self.account_id.is_none() {
                self.account_id = auth.and_then(|value| {
                    value
                        .get("chatgpt_account_id")
                        .or_else(|| value.get("account_id"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            }
            if self.chatgpt_user_id.is_none() {
                self.chatgpt_user_id = auth.and_then(|value| {
                    value
                        .get("chatgpt_user_id")
                        .or_else(|| value.get("user_id"))
                        .and_then(Value::as_str)
                        .filter(|value| !value.trim().is_empty())
                        .map(str::to_owned)
                });
            }
            if self.plan.is_none() {
                self.plan = auth.and_then(|value| {
                    value
                        .get("chatgpt_plan_type")
                        .or_else(|| value.get("plan_type"))
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            }
            if jwt_subject.is_none() {
                jwt_subject = claims
                    .get("sub")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(str::to_owned);
            }
        }
        if self.chatgpt_user_id.is_none() {
            self.chatgpt_user_id = jwt_subject;
        }
        for value in [
            &mut self.email,
            &mut self.account_id,
            &mut self.chatgpt_user_id,
            &mut self.plan,
        ] {
            if value.as_ref().is_some_and(|item| item.len() > 512) {
                *value = None;
            }
        }
    }

    pub fn display_name(&self) -> String {
        self.email
            .as_deref()
            .filter(|email| !email.trim().is_empty())
            .unwrap_or("OpenAI Codex account")
            .chars()
            .take(256)
            .collect()
    }
}
