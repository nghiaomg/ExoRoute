//! Cline OAuth account lifecycle split by concern: account records in
//! [`account`], the OAuth flow in [`oauth`], embedded-callback payload
//! decoding in [`payload`], and token/JWT helpers in [`token`].

use crate::{
    infra::storage::{Field, Record, StorageError, Table},
    provider_adapters::{CLINE_ADAPTER_ID, CLINEPASS_ADAPTER_ID},
    security,
    security::egress,
    state::AppState,
};
use base64::{Engine, engine::general_purpose};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

mod account;
mod oauth;
mod payload;
mod token;

#[path = "tests.rs"]
#[cfg(test)]
mod tests;

pub const AUTHORIZE_URL: &str = "https://api.cline.bot/api/v1/auth/authorize";
pub const TOKEN_URL: &str = "https://api.cline.bot/api/v1/auth/token";
pub const REFRESH_URL: &str = "https://api.cline.bot/api/v1/auth/refresh";
pub const CALLBACK_URL: &str = "http://localhost:1455/auth/callback";
const MAX_TOKEN_BYTES: usize = 32 * 1024;
const MAX_ACCOUNT_BYTES: usize = MAX_TOKEN_BYTES * 2 + 4096;
const MAX_OAUTH_BODY_BYTES: usize = 64 * 1024;
const REFRESH_LEAD_SECONDS: i64 = 300;

pub(in crate::provider_adapters) use account::ClineAccount;
pub(in crate::provider_adapters) use oauth::{
    account_for_use, authorization_url, exchange_code, pkce_challenge,
};
pub(crate) use payload::is_embedded_callback_code;
