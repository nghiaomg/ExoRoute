//! Antigravity credential storage split by concern: lookup/refresh in
//! [`load`], persisted updates in [`save`].

use super::*;

mod load;
mod save;

#[path = "tests.rs"]
#[cfg(test)]
mod tests;

pub(in crate::provider_adapters) use load::account_for_use;
pub(in crate::provider_adapters) use load::load_account_for_use;
pub(in crate::provider_adapters) use save::save_account;
