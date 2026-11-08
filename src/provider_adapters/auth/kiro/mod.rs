mod account;
mod refresh;
mod store;
#[cfg(test)]
mod tests;

pub use account::KiroAccount;
pub(crate) use account::email_from_jwt;
pub(crate) use refresh::{
    import_refresh_token, poll_device_token, register_oidc_client, start_device_authorization,
};
pub(crate) use store::{account_for_use, save_kiro_account};

pub(crate) const MAX_TOKEN_BYTES: usize = 16 * 1024;
pub(crate) const MAX_ACCOUNT_BYTES: usize = 32 * 1024;
const REFRESH_LEAD_SECONDS: i64 = 5 * 60;
