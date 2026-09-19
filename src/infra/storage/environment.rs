//! Environment lifecycle: open, version bootstrap, bounded read/write gates.
//!
//! Owns the `Database` handle, the single-writer mutex, the operation budget,
//! and the crash-safe map-growth protocol. Record access itself lives in
//! `read_txn.rs` / `write_txn.rs`; filesystem validation lives in
//! `fs_guard.rs`.

use crate::infra::storage::{
    ReadTxn, StorageError, Table, WriteTxn,
    fs_guard::{
        decode_record, encode_record, ensure_environment_directory, preflight_environment_files,
        read_map_size_marker, resize_with_marker_cleanup, secure_environment_files,
        write_map_size_marker,
    },
    snapshot,
};
use heed::{
    Database as HeedDatabase, Env, EnvOpenOptions, WithoutTls,
    types::{Bytes, Str},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::{
    sync::{Mutex, OwnedRwLockWriteGuard, RwLock, Semaphore},
    task,
};

pub(super) const INITIAL_MAP_SIZE: usize = 1024 * 1024 * 1024;
pub(super) const MAX_MAP_SIZE: usize = {
    let target_size = (1_u64 << 40) as usize;
    if target_size == 0 {
        usize::MAX & !4095
    } else {
        target_size
    }
};
pub(super) const MAX_DATABASE_OPERATIONS: usize = 64;
const MAX_READERS: u32 = 256;
const MAX_NAMED_DATABASES: u32 = 64;

#[cfg(test)]
pub(super) use crate::infra::storage::fs_guard::read_map_size_marker as read_map_size_marker_for_test_support;

pub(super) type RawDatabase = HeedDatabase<Str, Bytes>;

pub(super) struct DatabaseInner {
    env: Env<WithoutTls>,
    tables: HashMap<Table, RawDatabase>,
    path: PathBuf,
    pub(super) map_size: AtomicUsize,
    map_size_limit: usize,
    resize_gate: Arc<RwLock<()>>,
    operation_slots: Arc<Semaphore>,
    writer: Arc<Mutex<()>>,
}

/// A shared handle to the open LMDB environment. Cloning is cheap and the
/// intended way to pass database access across handlers and workers.
#[derive(Clone)]
pub struct Database {
    inner: Arc<DatabaseInner>,
}

impl Database {
    #[cfg(test)]
    pub(super) fn inner_for_test(&self) -> &DatabaseInner {
        &self.inner
    }

    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        Self::open_with_map_bounds(path, INITIAL_MAP_SIZE, MAX_MAP_SIZE).await
    }

    pub(crate) async fn open_with_map_bounds(
        path: &Path,
        initial_map_size: usize,
        map_size_limit: usize,
    ) -> Result<Self, StorageError> {
        if initial_map_size == 0
            || initial_map_size > map_size_limit
            || !initial_map_size.is_multiple_of(4096)
        {
            return Err(StorageError::Invalid(
                "invalid LMDB map size configuration".to_owned(),
            ));
        }
        let path = path.to_path_buf();
        let open_path = path.clone();
        let opened = task::spawn_blocking(move || {
            ensure_environment_directory(&open_path)?;
            preflight_environment_files(&open_path)?;
            let map_size = read_map_size_marker(&open_path, initial_map_size, map_size_limit)?;
            let mut options = EnvOpenOptions::new().read_txn_without_tls();
            options
                .map_size(map_size)
                .max_readers(MAX_READERS)
                .max_dbs(MAX_NAMED_DATABASES);
            // SAFETY: the environment path is validated as a private real
            // directory. Production startup holds the exclusive process lock
            // for this environment's lifetime; isolated tests use unique paths.
            // The app never replaces mapped files, and transaction/resize
            // access is routed through this module's gates.
            let env = unsafe { options.open(&open_path) }?;
            let mut txn = env.write_txn()?;
            let mut tables = HashMap::with_capacity(Table::ALL.len());
            for &table in Table::ALL {
                let database = env.create_database::<Str, Bytes>(&mut txn, Some(table.name()))?;
                tables.insert(table, database);
            }
            bootstrap_storage_version(&mut txn, &tables)?;
            txn.commit()?;
            secure_environment_files(&open_path)?;
            let actual_map_size = env.info().map_size;
            if actual_map_size < map_size || actual_map_size > map_size_limit {
                return Err(StorageError::Invalid(
                    "LMDB opened with a map size outside the configured bounds".to_owned(),
                ));
            }
            Ok::<_, StorageError>((env, tables, actual_map_size))
        })
        .await
        .map_err(|error| {
            StorageError::Task(format!("could not open LMDB environment: {error}"))
        })??;

        Ok(Self {
            inner: Arc::new(DatabaseInner {
                env: opened.0,
                tables: opened.1,
                path,
                map_size: AtomicUsize::new(opened.2),
                map_size_limit,
                resize_gate: Arc::new(RwLock::new(())),
                operation_slots: Arc::new(Semaphore::new(MAX_DATABASE_OPERATIONS)),
                writer: Arc::new(Mutex::new(())),
            }),
        })
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub async fn snapshot_entries(
        &self,
        include_request_logs: bool,
        max_bytes: usize,
    ) -> Result<Vec<super::SnapshotEntry>, StorageError> {
        self.read(move |transaction| transaction.snapshot_entries(include_request_logs, max_bytes))
            .await
    }

    pub async fn replace_snapshot_entries(
        &self,
        entries: Vec<super::SnapshotEntry>,
    ) -> Result<(), StorageError> {
        let slot = self
            .inner
            .operation_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| StorageError::Busy)?;
        let entries = task::spawn_blocking(move || {
            let _slot = slot;
            snapshot::validate_snapshot_entries(&entries)?;
            Ok::<_, StorageError>(entries)
        })
        .await
        .map_err(|error| {
            StorageError::Task(format!("LMDB snapshot validation task failed: {error}"))
        })??;
        self.write(move |transaction| transaction.replace_snapshot_entries(&entries))
            .await
    }

    pub async fn read<R, F>(&self, operation: F) -> Result<R, StorageError>
    where
        R: Send + 'static,
        F: for<'view, 'env> FnOnce(&ReadTxn<'view, 'env>) -> Result<R, StorageError>
            + Send
            + 'static,
    {
        let slot = self
            .inner
            .operation_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| StorageError::Busy)?;
        let resize_guard = self.inner.resize_gate.clone().read_owned().await;
        let inner = self.inner.clone();
        task::spawn_blocking(move || {
            let _slot = slot;
            let _resize_guard = resize_guard;
            let txn = inner.env.read_txn()?;
            let view = ReadTxn {
                txn: &txn,
                tables: &inner.tables,
            };
            operation(&view)
        })
        .await
        .map_err(|error| StorageError::Task(format!("LMDB reader task failed: {error}")))?
    }

    pub async fn write<R, F>(&self, operation: F) -> Result<R, StorageError>
    where
        R: Send + 'static,
        F: for<'view, 'env> Fn(&mut WriteTxn<'view, 'env>) -> Result<R, StorageError>
            + Send
            + Sync
            + 'static,
    {
        let operation = Arc::new(operation);

        loop {
            let slot = self
                .inner
                .operation_slots
                .clone()
                .try_acquire_owned()
                .map_err(|_| StorageError::Busy)?;
            let writer = self.inner.writer.clone().lock_owned().await;
            let resize_guard = self.inner.resize_gate.clone().read_owned().await;
            let inner = self.inner.clone();
            // The read guard prevents this size from changing until the LMDB
            // transaction has ended. A queued MapFull result can then be
            // coalesced if another writer grows the map before this task does.
            let attempted_map_size = inner.map_size.load(Ordering::Acquire);
            let operation = operation.clone();
            let result = task::spawn_blocking(move || {
                let result = (|| -> Result<R, StorageError> {
                    let _resize_guard = resize_guard;
                    let mut txn = inner.env.write_txn()?;
                    let result = {
                        let mut view = WriteTxn {
                            txn: &mut txn,
                            tables: &inner.tables,
                        };
                        operation(&mut view)
                    };
                    match result {
                        Ok(value) => txn.commit().map(|()| value).map_err(StorageError::from),
                        Err(error) => Err(error),
                    }
                })();
                (result, slot, writer, attempted_map_size)
            })
            .await
            .map_err(|error| StorageError::Task(format!("LMDB writer task failed: {error}")))?;

            let (outcome, _slot, _writer, attempted_map_size) = result;
            match outcome {
                Err(StorageError::MapFull) => {
                    self.grow_map(attempted_map_size).await?;
                }
                result => return result,
            }
        }
    }

    pub(crate) async fn grow_map(&self, attempted_map_size: usize) -> Result<(), StorageError> {
        let inner = self.inner.clone();
        let resize_guard: OwnedRwLockWriteGuard<()> = inner.resize_gate.clone().write_owned().await;
        let current = inner.map_size.load(Ordering::Acquire);
        if current > attempted_map_size {
            // This waiter failed against an older map size; retry its write
            // without another resize because a peer already handled growth.
            return Ok(());
        }
        if current < attempted_map_size {
            return Err(StorageError::Invalid(
                "LMDB map size moved backwards while handling a full map".to_owned(),
            ));
        }
        let next = current
            .checked_mul(2)
            .map(|value| value.min(inner.map_size_limit))
            .filter(|value| *value > current)
            .ok_or(StorageError::MapFull)?;
        task::spawn_blocking(move || {
            let _resize_guard = resize_guard;
            write_map_size_marker(&inner.path, next)?;
            // SAFETY: all read and write transactions retain a read guard until
            // their LMDB transaction has dropped. Holding the exclusive gate
            // therefore guarantees no transaction is active during resize.
            resize_with_marker_cleanup(&inner.path, next, || {
                unsafe { inner.env.resize(next) }.map_err(StorageError::from)
            })?;
            inner.map_size.store(next, Ordering::Release);
            Ok::<(), StorageError>(())
        })
        .await
        .map_err(|error| StorageError::Task(format!("LMDB resize task failed: {error}")))??;
        Ok(())
    }

    #[cfg(test)]
    pub async fn open_for_test(path: &Path, map_size: usize) -> Result<Self, StorageError> {
        Self::open_with_map_bounds(path, map_size, map_size.saturating_mul(8)).await
    }
}

fn bootstrap_storage_version(
    txn: &mut heed::RwTxn<'_>,
    tables: &HashMap<Table, RawDatabase>,
) -> Result<(), StorageError> {
    use super::STORAGE_FORMAT_VERSION;
    let meta = tables
        .get(&Table::Meta)
        .ok_or_else(|| StorageError::Invalid("LMDB metadata store is missing".to_owned()))?;
    let current_version: Option<u32> = meta
        .get(txn, "storage_format_version")?
        .map(decode_record)
        .transpose()?;
    match current_version {
        None => {
            stamp_storage_version(txn, meta)?;
        }
        Some(version) if version == STORAGE_FORMAT_VERSION => {}
        Some(1) if STORAGE_FORMAT_VERSION >= 2 => {
            // v1 provider records have no local_rpm_target field; readers
            // supply the provider-specific default without rewriting rows.
            // The same version bump also creates any newer named tables.
            stamp_storage_version(txn, meta)?;
        }
        Some(2) if STORAGE_FORMAT_VERSION >= 3 => {
            // v2 has no provider usage meter table. The named table
            // is created above, so an empty meter starts safely and
            // existing provider credentials remain compatible.
            stamp_storage_version(txn, meta)?;
        }
        Some(3) if STORAGE_FORMAT_VERSION == 4 => {
            // v3 has no output-style settings table. The named table
            // is created above and initialized by the domain layer.
            stamp_storage_version(txn, meta)?;
        }
        Some(version) if version < STORAGE_FORMAT_VERSION => {
            return Err(StorageError::Invalid(format!(
                "LMDB storage format {version} has no registered upgrade path"
            )));
        }
        Some(version) => {
            return Err(StorageError::Invalid(format!(
                "LMDB storage format {version} is newer than this ExoRoute build"
            )));
        }
    }
    Ok(())
}

fn stamp_storage_version(
    txn: &mut heed::RwTxn<'_>,
    meta: &RawDatabase,
) -> Result<(), StorageError> {
    use super::STORAGE_FORMAT_VERSION;
    let encoded = encode_record(&STORAGE_FORMAT_VERSION)?;
    meta.put(txn, "storage_format_version", &encoded)?;
    Ok(())
}
