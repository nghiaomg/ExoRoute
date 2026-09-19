//! Bounded asynchronous access to the LMDB environment.
//!
//! All transactions are created and consumed in `spawn_blocking` closures.
//! The resize gate is moved into each closure so `Env::resize` can only run
//! after every transaction has ended. The process-level database lock must be
//! held for the lifetime of this environment; no code outside this module may
//! mutate or replace its mapped files.
//!
//! The engine is split by concern: `environment` owns open/resize/gating,
//! `read_txn`/`write_txn` own transaction views, `snapshot` owns backup
//! validation, `fs_guard` owns filesystem checks, and `records` owns the
//! typed record model.

mod codec;
mod environment;
pub(crate) mod fs_guard;
mod keys;
mod read_txn;
mod records;
mod snapshot;
#[cfg(test)]
mod tests;
mod write_txn;

pub use environment::Database;
pub use keys::{validate_key, validate_prefix};
pub use read_txn::ReadTxn;
pub use records::{Field, Record, SnapshotEntry, StorageError, Table};
pub use snapshot::validate_snapshot_entries;
pub use write_txn::WriteTxn;

pub(crate) const STORAGE_FORMAT_VERSION: u32 = 4;
pub const MAX_SNAPSHOT_ENTRIES: usize = 2_000_000;
pub const MAX_SNAPSHOT_RECORD_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_RECORD_FIELDS: usize = 128;
pub(crate) const MAX_RECORD_FIELD_NAME_BYTES: usize = 128;
pub(crate) const MAX_RECORD_FIELD_BYTES: usize = 16 * 1024 * 1024;

pub(super) const MAX_KEY_BYTES: usize = 500;
