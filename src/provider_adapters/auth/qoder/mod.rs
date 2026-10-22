mod account;
mod store;
#[cfg(test)]
mod tests;

pub use account::QoderAccount;
#[allow(unused_imports)]
pub(crate) use store::{account_for_use, provider_base_url, save_qoder_account};

pub(crate) const MAX_TOKEN_BYTES: usize = 4096;
pub(crate) const MAX_ACCOUNT_BYTES: usize = 16 * 1024;
