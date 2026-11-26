use super::MAX_TOKEN_BYTES;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Account metadata is display-only, so it is bounded far below a token and
/// rejected when it carries control characters.
const MAX_METADATA_BYTES: usize = 1024;

/// The stored Muse account.
///
/// Both secrets are kept because they fail differently: the durable device
/// token is what makes a rejected inference key recoverable, and the inference
/// key is what a chat request actually authenticates with.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MuseAccount {
    /// Durable device-client token returned by the device grant. This is the
    /// only value that can mint a replacement inference key.
    pub dca_token: String,
    /// Minted subscription inference key. `None` means sign-in succeeded but
    /// the key could not be minted yet, so it is minted on first use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_tier: Option<String>,
    /// UTC timestamp of the last inference-key mint. It bounds how often the
    /// gateway may mint a replacement key for one account, so an upstream that
    /// keeps rejecting a freshly minted key cannot turn client traffic into
    /// mint traffic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_mint_at: Option<String>,
    /// How many inference keys the gateway has minted for this account since the
    /// operator last signed in.
    ///
    /// The cooldown above bounds how *often* a mint happens; this bounds how
    /// *many* ever happen. Without it a key minted after the cooldown that is
    /// rejected again simply restarts the cycle, and the installation mints one
    /// key per cooldown window forever. Signing in again rebuilds the record
    /// from the device grant and resets the count.
    #[serde(default)]
    pub remint_attempts: u32,
}

impl MuseAccount {
    /// Reads the device grant response into a record that still has no
    /// inference key. The grant is upstream input, so every field is validated.
    pub(crate) fn from_device_grant(value: &Value) -> Result<Self, String> {
        let dca_token = value
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| "Muse sign-in did not return an access token".to_owned())?;
        validate_token(dca_token)?;
        Ok(Self {
            dca_token: dca_token.to_owned(),
            ..Self::default()
        })
    }

    /// Decodes a payload handed across a module boundary. The payload this
    /// crate builds always round-trips, so a failure here means the record is
    /// corrupt or was written by a build that stored a different shape.
    pub(crate) fn decode(value: &Value) -> Result<Self, String> {
        let account: Self = serde_json::from_value(value.clone())
            .map_err(|_| "stored Muse account data is invalid".to_owned())?;
        account.validate()?;
        Ok(account)
    }

    /// Decodes the encrypted record read back from storage.
    pub(crate) fn decode_str(serialized: &str) -> Result<Self, String> {
        let account: Self = serde_json::from_str(serialized)
            .map_err(|_| "stored Muse account data is invalid".to_owned())?;
        account.validate()?;
        Ok(account)
    }

    fn validate(&self) -> Result<(), String> {
        validate_token(&self.dca_token)?;
        if let Some(api_key) = self.api_key.as_deref() {
            validate_token(api_key)?;
        }
        for metadata in [&self.email, &self.display_name, &self.subscription_tier]
            .into_iter()
            .flatten()
        {
            validate_metadata(metadata)?;
        }
        if let Some(minted_at) = self.last_mint_at.as_deref()
            && crate::infra::db::parse_utc_timestamp(minted_at).is_none()
        {
            return Err("stored Muse account data is invalid".to_owned());
        }
        Ok(())
    }

    /// Records the freshly minted inference key.
    #[cfg(test)]
    pub(crate) fn with_minted_key(&self, api_key: &str) -> Result<Self, String> {
        let mut account = self.clone();
        account.apply_minted_key(api_key)?;
        Ok(account)
    }

    /// Replaces the stored inference key in place.
    pub(crate) fn apply_minted_key(&mut self, api_key: &str) -> Result<(), String> {
        let api_key = api_key.trim();
        validate_token(api_key)?;
        self.api_key = Some(api_key.to_owned());
        Ok(())
    }

    /// Records the account details the mint endpoint reports.
    ///
    /// Every field is optional metadata, so a response that omits or malforms
    /// one keeps the stored value instead of failing a sign-in that already
    /// succeeded; the inference key is what the request needs.
    pub(crate) fn apply_mint_metadata(&mut self, value: &Value) {
        if let Some(email) = minted_text(value, &["user_email", "email"]) {
            self.email = Some(email);
        }
        if let Some(name) = minted_text(value, &["user_full_name", "name"]) {
            self.display_name = Some(name);
        }
        if let Some(tier) = minted_text(value, &["subs_tier_name", "subs_tier_id"]) {
            self.subscription_tier = Some(tier);
        }
    }

    /// The key a chat request authenticates with, if one has been minted.
    pub(crate) fn inference_key(&self) -> Option<&str> {
        self.api_key.as_deref().filter(|key| !key.is_empty())
    }

    /// Whether the durable device token must be exchanged for a key before the
    /// account can serve a request.
    pub(crate) fn needs_remint(&self) -> bool {
        self.inference_key().is_none()
    }

    /// Whether a replacement key was minted too recently to mint another one.
    /// An account with no recorded mint is never cooling down.
    pub(crate) fn remint_is_cooling_down(&self, now: u64, cooldown_seconds: u64) -> bool {
        let Some(minted_at) = self
            .last_mint_at
            .as_deref()
            .and_then(crate::infra::db::parse_utc_timestamp)
        else {
            return false;
        };
        now.saturating_sub(minted_at) < cooldown_seconds
    }

    /// How many mint attempts still count against the budget at `now`.
    ///
    /// The budget is a rolling window, not a lifetime cap. What has to be bounded
    /// is a burst of mints driven by client traffic; an account that legitimately
    /// mints once every few months must never be locked out, so attempts older
    /// than the window stop counting and the next mint starts a fresh window.
    fn counting_attempts(&self, now: u64, window_seconds: u64) -> u32 {
        let Some(last_attempt) = self
            .last_mint_at
            .as_deref()
            .and_then(crate::infra::db::parse_utc_timestamp)
        else {
            return 0;
        };
        if now.saturating_sub(last_attempt) >= window_seconds {
            return 0;
        }
        self.remint_attempts
    }

    /// Why the gateway must not mint an inference key for this account right
    /// now, or `None` when a mint is allowed.
    ///
    /// Every path that can mint — a chat request whose account has no key yet,
    /// catalog discovery, a model probe, and repairing a rejected key — asks
    /// this first, so none of them can turn client traffic into mint traffic.
    pub(crate) fn remint_blocked(
        &self,
        now: u64,
        cooldown_seconds: u64,
        attempt_limit: u32,
        window_seconds: u64,
    ) -> Option<&'static str> {
        if self.counting_attempts(now, window_seconds) >= attempt_limit {
            return Some(
                "Muse rejected every inference key minted recently; reconnect the Muse account, or retry after the budget window",
            );
        }
        if self.remint_is_cooling_down(now, cooldown_seconds) {
            return Some("a Muse inference key was minted too recently to mint another one");
        }
        None
    }

    /// Records that the gateway is about to attempt a mint for this account.
    ///
    /// Called *before* the network call so an attempt that fails still advances
    /// the throttle; otherwise a mint that never succeeds would be retried on
    /// every request. An attempt made after the previous window has aged out
    /// starts a fresh window instead of inheriting a spent budget.
    pub(crate) fn record_mint_attempt(
        &mut self,
        now: u64,
        window_seconds: u64,
    ) -> Result<(), String> {
        if self.counting_attempts(now, window_seconds) == 0 {
            self.remint_attempts = 0;
        }
        self.remint_attempts = self.remint_attempts.saturating_add(1);
        self.last_mint_at = Some(
            crate::infra::db::utc_timestamp_now()
                .map_err(|_| "could not timestamp the Muse inference key".to_owned())?,
        );
        Ok(())
    }

    /// Stable identity used to deduplicate repeated sign-ins of one account.
    /// The email is preferred so reconnecting after a token rotation updates
    /// the existing credential; otherwise the durable token identifies it.
    pub(crate) fn identity(&self) -> String {
        match self.email.as_deref() {
            Some(email) => format!("email:{}", email.to_ascii_lowercase()),
            None => format!("token:{}", token_digest(&self.dca_token)),
        }
    }

    /// Operator-facing credential name. Never contains either secret.
    pub(crate) fn display_name(&self) -> String {
        if let Some(name) = self.display_name.as_deref() {
            return name.to_owned();
        }
        if let Some(email) = self.email.as_deref() {
            return email.to_owned();
        }
        let digest = token_digest(&self.dca_token);
        format!("Muse account {}", &digest[..8])
    }

    /// The stored JSON shape. Kept as one value so callers that only carry a
    /// payload across a module boundary never see the field names.
    pub(crate) fn to_payload(&self) -> Result<Value, String> {
        serde_json::to_value(self)
            .map_err(|_| "could not encode Muse account credentials".to_owned())
    }
}

/// Rejects a token the gateway could not send as a header or that is larger
/// than any Muse credential.
fn validate_token(token: &str) -> Result<(), String> {
    if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
        return Err("Muse returned an invalid credential".to_owned());
    }
    if !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("Muse returned an invalid credential".to_owned());
    }
    Ok(())
}

fn validate_metadata(text: &str) -> Result<(), String> {
    if text.len() > MAX_METADATA_BYTES || text.chars().any(char::is_control) {
        return Err("stored Muse account data is invalid".to_owned());
    }
    Ok(())
}

/// Reads the first present, non-empty string among `keys`, ignoring anything
/// that is not a plausible display value rather than failing the whole mint.
fn minted_text(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let text = value.get(*key)?.as_str()?.trim();
        if text.is_empty() || validate_metadata(text).is_err() {
            return None;
        }
        Some(text.to_owned())
    })
}

/// Short, non-reversible digest of a secret, used only to name a credential
/// that carries no email.
fn token_digest(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
