//! Muse account records.
//!
//! A Muse sign-in produces two different secrets and the difference matters.
//! The RFC 8628 device grant returns a durable `dca:` device-client token, and
//! that token is exchanged for the subscription inference key chat requests
//! authenticate with. Only the durable token can mint a replacement key, so
//! both are stored together and an account whose inference key was rejected can
//! be repaired without asking the operator to sign in again.
//!
//! Account records and token repair live here; the matching `ProviderAdapter`
//! implementation lives in `provider_adapters::muse` and calls into these
//! account services.

mod account;
mod store;
#[cfg(test)]
mod tests;

pub(crate) use account::MuseAccount;
pub(crate) use store::{
    load_muse_account, persist_account, persist_remint_attempt, provider_base_url,
    save_muse_account,
};

/// One Muse secret — either the durable device token or the minted inference
/// key — is far shorter than this. The bound exists so a corrupt or hostile
/// record cannot make the gateway allocate without limit.
const MAX_TOKEN_BYTES: usize = 8 * 1024;
/// The durable token, the inference key, and the account metadata.
const MAX_ACCOUNT_BYTES: usize = MAX_TOKEN_BYTES * 3 + 4096;
