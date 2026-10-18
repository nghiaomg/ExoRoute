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
/// Decode for stored request-log bytes: accepts both the compressed frame
/// format and the legacy raw bincode layout, using an explicit dictionary.
/// Exposed for the backup path, which must decode a snapshot's frames with the
/// dictionary carried in that same snapshot.
pub(crate) use codec::decode_request_log_bytes;
mod dictionary;
/// Request-log compression dictionary lifecycle: startup load, backup-restore
/// reload, and one-shot training from the telemetry maintenance tick. Training
/// itself stays module-internal; tests reach it through `dictionary::`.
pub(crate) use dictionary::{
    REQUEST_LOG_DICTIONARY_KEY, REQUEST_LOG_DICTIONARY_MAX_BYTES,
    ensure_request_log_dictionary_trained, load_request_log_dictionary,
    request_log_dictionary_from_entries,
};
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

/// Stored-format revision stamped into every environment and carried by every
/// backup.
///
/// Raise it only when an older build would misread records that already exist.
/// An additive extension — a new named table, or an optional field that older
/// readers ignore — must not raise it: the gate below refuses to open a newer
/// environment and to import a newer backup, so a bump locks an installed older
/// build out of the user's own data until that user upgrades.
pub(crate) const STORAGE_FORMAT_VERSION: u32 = 4;

/// Pre-release revisions that were withdrawn before release. Each one is
/// additive over [`STORAGE_FORMAT_VERSION`] — a new named table or an optional
/// record field — so an environment or backup stamped with it is still
/// readable. Opening an environment carrying one re-stamps the marker to the
/// released revision, which keeps an installed older build able to open it.
/// Remove a revision from this list if it is ever reused for a real change.
pub(crate) const WITHDRAWN_STORAGE_FORMAT_VERSIONS: &[u32] = &[5];

/// Whether this build can read stored data stamped `version`: every revision up
/// to the released one, plus a withdrawn pre-release revision. Only an unknown
/// newer revision (or the never-written 0) is unreadable.
pub(crate) fn is_readable_storage_version(version: u32) -> bool {
    version != 0
        && (version <= STORAGE_FORMAT_VERSION
            || WITHDRAWN_STORAGE_FORMAT_VERSIONS.contains(&version))
}

pub const MAX_SNAPSHOT_ENTRIES: usize = 2_000_000;
pub const MAX_SNAPSHOT_RECORD_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_RECORD_FIELDS: usize = 128;
pub(crate) const MAX_RECORD_FIELD_NAME_BYTES: usize = 128;
pub(crate) const MAX_RECORD_FIELD_BYTES: usize = 16 * 1024 * 1024;

pub(super) const MAX_KEY_BYTES: usize = 500;
