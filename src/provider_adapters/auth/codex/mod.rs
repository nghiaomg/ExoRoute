//! Codex OAuth account lifecycle split by concern: account records in
//! [`account`], the OAuth/PKCE flow in [`oauth`], token/JWT helpers in
//! [`token`], usage HTTP fetching in [`usage`], and provider response
//! parsing in [`usage_core`].

use crate::infra::storage::{Field, Record, StorageError, Table};
use crate::security::egress;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

mod account;
mod oauth;
mod token;
mod usage;

#[path = "tests.rs"]
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use usage::usage_core::MAX_USAGE_QUOTAS;
#[cfg(test)]
pub(crate) use usage::usage_core::parse_usage_snapshot;

pub const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
pub const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
pub const CALLBACK_URL: &str = "http://localhost:1455/auth/callback";
pub const BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
pub const USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
pub const CODEX_CLI_VERSION: &str = "0.154.0";
pub const CODEX_USER_AGENT: &str = "codex_cli_rs/0.154.0";
pub const MAX_TOKEN_BYTES: usize = 32 * 1024;
const MAX_CREDENTIAL_BYTES: usize = MAX_TOKEN_BYTES * 3 + 4096;
pub const REFRESH_LEAD_SECONDS: i64 = 5 * 60;

pub(in crate::provider_adapters) use account::CodexAccount;
pub(in crate::provider_adapters) use oauth::exchange_code;
pub(in crate::provider_adapters) use oauth::{
    account_for_use, authorization_url, force_refresh_account_for_use, pkce_challenge,
};
pub(in crate::provider_adapters) use usage::fetch_usage;
