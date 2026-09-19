//! Mutable view over an LMDB write transaction.
//!
//! Writes go through the single-writer gate in `environment.rs`; this view
//! only exposes bounded put/delete helpers plus snapshot restore.

use crate::infra::storage::environment::RawDatabase;
use crate::infra::storage::{StorageError, Table, codec, keys};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub struct WriteTxn<'view, 'env> {
    pub(super) txn: &'view mut heed::RwTxn<'env>,
    pub(super) tables: &'view std::collections::HashMap<Table, RawDatabase>,
}

impl WriteTxn<'_, '_> {
    fn database(&self, table: Table) -> Result<&RawDatabase, StorageError> {
        self.tables.get(&table).ok_or_else(|| {
            StorageError::Invalid(format!("LMDB table '{}' is unavailable", table.name()))
        })
    }

    fn database_mut(&mut self, table: Table) -> Result<RawDatabase, StorageError> {
        self.tables.get(&table).cloned().ok_or_else(|| {
            StorageError::Invalid(format!("LMDB table '{}' is unavailable", table.name()))
        })
    }

    pub fn get<T: DeserializeOwned>(
        &self,
        table: Table,
        key: &str,
    ) -> Result<Option<T>, StorageError> {
        keys::validate_key(key)?;
        self.database(table)?
            .get(&*self.txn, key)?
            .map(codec::decode_record)
            .transpose()
    }

    pub fn scan_prefix<T: DeserializeOwned>(
        &self,
        table: Table,
        prefix: &str,
        limit: usize,
    ) -> Result<Vec<(String, T)>, StorageError> {
        self.scan_prefix_after(table, prefix, None, limit)
    }

    pub fn scan_prefix_after<T: DeserializeOwned>(
        &self,
        table: Table,
        prefix: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<(String, T)>, StorageError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        keys::validate_prefix(prefix)?;
        if let Some(cursor) = after {
            keys::validate_key(cursor)?;
        }
        let database = self.database(table)?;
        let mut values = Vec::with_capacity(limit.min(256));
        if let Some(cursor) = after {
            let range = (
                std::ops::Bound::Excluded(cursor),
                std::ops::Bound::Unbounded,
            );
            for item in database.range(&*self.txn, &range)? {
                let (key, value) = item?;
                if !key.starts_with(prefix) {
                    break;
                }
                values.push((key.to_owned(), codec::decode_record(value)?));
                if values.len() == limit {
                    break;
                }
            }
        } else if prefix.is_empty() {
            for item in database.iter(&*self.txn)? {
                let (key, value) = item?;
                values.push((key.to_owned(), codec::decode_record(value)?));
                if values.len() == limit {
                    break;
                }
            }
        } else {
            for item in database.prefix_iter(&*self.txn, prefix)? {
                let (key, value) = item?;
                values.push((key.to_owned(), codec::decode_record(value)?));
                if values.len() == limit {
                    break;
                }
            }
        }
        Ok(values)
    }

    pub fn put<T: Serialize>(
        &mut self,
        table: Table,
        key: &str,
        value: &T,
    ) -> Result<(), StorageError> {
        keys::validate_key(key)?;
        let encoded = codec::encode_record(value)?;
        let database = self.database_mut(table)?;
        database.put(&mut *self.txn, key, &encoded)?;
        Ok(())
    }

    pub fn put_if_absent<T: Serialize>(
        &mut self,
        table: Table,
        key: &str,
        value: &T,
    ) -> Result<(), StorageError> {
        keys::validate_key(key)?;
        if self.database(table)?.get(&*self.txn, key)?.is_some() {
            return Err(StorageError::Conflict);
        }
        self.put(table, key, value)
    }

    pub fn delete(&mut self, table: Table, key: &str) -> Result<bool, StorageError> {
        keys::validate_key(key)?;
        let database = self.database_mut(table)?;
        Ok(database.delete(&mut *self.txn, key)?)
    }

    pub fn delete_prefix(&mut self, table: Table, prefix: &str) -> Result<usize, StorageError> {
        keys::validate_prefix(prefix)?;
        if prefix.is_empty() {
            return Err(StorageError::Invalid(
                "deleting an entire LMDB table must use clear()".to_owned(),
            ));
        }
        let database = self.database_mut(table)?;
        let mut deleted = 0usize;
        loop {
            let keys = database
                .prefix_iter(&*self.txn, prefix)?
                .take(256)
                .map(|item| {
                    item.map(|(key, _)| key.to_owned())
                        .map_err(StorageError::from)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if keys.is_empty() {
                break;
            }
            for key in keys {
                deleted =
                    deleted.saturating_add(usize::from(database.delete(&mut *self.txn, &key)?));
            }
        }
        Ok(deleted)
    }

    pub fn clear(&mut self, table: Table) -> Result<(), StorageError> {
        let database = self.database_mut(table)?;
        database.clear(&mut *self.txn)?;
        Ok(())
    }

    pub(super) fn replace_snapshot_entries(
        &mut self,
        entries: &[super::SnapshotEntry],
    ) -> Result<(), StorageError> {
        let storage_version = self
            .get::<u32>(Table::Meta, "storage_format_version")?
            .ok_or_else(|| {
                StorageError::Invalid("LMDB storage format version is missing".to_owned())
            })?;
        for &table in Table::ALL {
            self.clear(table)?;
        }
        self.put(Table::Meta, "storage_format_version", &storage_version)?;

        for entry in entries {
            let database = self.database_mut(entry.table)?;
            database.put(&mut *self.txn, &entry.key, &entry.value)?;
        }
        Ok(())
    }
}
