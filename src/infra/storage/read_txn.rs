//! Read-only view over an LMDB read transaction.
//!
//! All scans are prefix-bounded and limit-bounded so request handlers cannot
//! materialize user-growable tables with a single call.

use crate::infra::storage::environment::RawDatabase;
use crate::infra::storage::{StorageError, Table, codec, keys};
use serde::de::DeserializeOwned;

pub struct ReadTxn<'view, 'env> {
    pub(super) txn: &'view heed::RoTxn<'env, heed::WithoutTls>,
    pub(super) tables: &'view std::collections::HashMap<Table, RawDatabase>,
}

impl ReadTxn<'_, '_> {
    fn database(&self, table: Table) -> Result<&RawDatabase, StorageError> {
        self.tables.get(&table).ok_or_else(|| {
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
            .get(self.txn, key)?
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
            for item in database.range(self.txn, &range)? {
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
            for item in database.iter(self.txn)? {
                let (key, value) = item?;
                values.push((key.to_owned(), codec::decode_record(value)?));
                if values.len() == limit {
                    break;
                }
            }
        } else {
            for item in database.prefix_iter(self.txn, prefix)? {
                let (key, value) = item?;
                values.push((key.to_owned(), codec::decode_record(value)?));
                if values.len() == limit {
                    break;
                }
            }
        }
        Ok(values)
    }

    pub fn scan_prefix_reverse_after<T: DeserializeOwned>(
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
        for item in database.rev_prefix_iter(self.txn, prefix)? {
            let (key, value) = item?;
            if after.is_some_and(|cursor| key >= cursor) {
                continue;
            }
            values.push((key.to_owned(), codec::decode_record(value)?));
            if values.len() == limit {
                break;
            }
        }
        Ok(values)
    }

    pub(super) fn snapshot_entries(
        &self,
        include_request_logs: bool,
        max_bytes: usize,
    ) -> Result<Vec<super::SnapshotEntry>, StorageError> {
        use super::{Field, MAX_SNAPSHOT_ENTRIES, Record};
        let mut entries = Vec::new();
        let mut bytes_seen = 0usize;
        for &table in Table::ALL {
            if super::snapshot::excluded_from_snapshot(table, include_request_logs) {
                continue;
            }
            let database = self.database(table)?;
            for item in database.iter(self.txn)? {
                let (raw_key, raw_value) = item?;
                let key = raw_key.to_owned();
                if table == Table::Meta && key == "storage_format_version" {
                    continue;
                }
                let value = if !include_request_logs
                    && table == Table::Meta
                    && key == "request_log_count"
                {
                    codec::encode_record(&0_i64)?
                } else if !include_request_logs && table == Table::ApiKeys {
                    let mut record: Record = codec::decode_record(raw_value)?;
                    record.insert("request_count", Field::I64(0));
                    codec::encode_record(&record)?
                } else {
                    raw_value.to_vec()
                };
                bytes_seen = bytes_seen
                    .checked_add(key.len())
                    .and_then(|size| size.checked_add(value.len()))
                    .ok_or_else(|| {
                        StorageError::Invalid("LMDB snapshot size overflowed".to_owned())
                    })?;
                if bytes_seen > max_bytes || entries.len() >= MAX_SNAPSHOT_ENTRIES {
                    return Err(StorageError::Invalid(
                        "LMDB snapshot exceeds its configured size or record limit".to_owned(),
                    ));
                }
                entries.push(super::SnapshotEntry { table, key, value });
            }
        }
        Ok(entries)
    }
}
