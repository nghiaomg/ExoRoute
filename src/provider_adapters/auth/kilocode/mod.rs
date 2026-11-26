//! Kilo Code account lifecycle: the stored account record in [`account`] and
//! the encrypted store/load path in [`store`].
//!
//! Kilo Code issues a long-lived device-authorization token without a refresh
//! token or an expiry, so there is nothing to refresh here. A token the gateway
//! later rejects is marked invalid by the normal credential-rotation path and
//! the operator reconnects.

mod account;
mod store;
#[cfg(test)]
mod tests;

pub(crate) use account::KilocodeAccount;
pub(crate) use store::{account_for_use, provider_base_url, save_kilocode_account};

const MAX_TOKEN_BYTES: usize = 8 * 1024;
const MAX_ACCOUNT_BYTES: usize = MAX_TOKEN_BYTES * 2 + 4096;
