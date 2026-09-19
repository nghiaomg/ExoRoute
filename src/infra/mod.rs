//! Infrastructure layer: the LMDB environment, typed domain operations, the
//! single-process database lock, and telemetry.
//!
//! `storage` owns the single LMDB environment and transaction primitives.
//! `db` owns ExoRoute domain operations above that environment; it must never
//! open or replace a second environment.

pub(crate) mod db;
pub(crate) mod lock;
pub(crate) mod settings_record;
pub(crate) mod storage;
pub(crate) mod telemetry;
