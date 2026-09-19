//! Operational-settings persistence with revision compare-and-swap.
//!
//! Concurrent dashboard writers pass the revision they read; a mismatch
//! returns `None` so the caller can reload instead of silently overwriting.

use super::SINGLETON_KEY;
use crate::{
    config::OperationalSettings,
    infra::{
        db::OperationalSettingsRecord,
        storage::{Database, Field, Record, StorageError, Table},
    },
};

pub async fn load_operational_settings(
    database: &Database,
) -> Result<OperationalSettingsRecord, StorageError> {
    let record = database
        .read(|transaction| transaction.get::<Record>(Table::OperationalSettings, SINGLETON_KEY))
        .await?
        .ok_or_else(|| {
            StorageError::Invalid("operational settings record is missing".to_owned())
        })?;
    super::settings_records::operational_settings_from_record(&record)
}

pub async fn save_operational_settings(
    database: &Database,
    settings: OperationalSettings,
    expected_revision: i64,
    overridden: bool,
) -> Result<Option<i64>, StorageError> {
    settings
        .validate()
        .map_err(|message| StorageError::Invalid(message.to_owned()))?;
    let settings_record =
        super::settings_records::operational_settings_record(settings, 0, overridden)?;
    database
        .write(move |transaction| {
            let current = transaction
                .get::<Record>(Table::OperationalSettings, SINGLETON_KEY)?
                .ok_or_else(|| {
                    StorageError::Invalid("operational settings record is missing".to_owned())
                })?;
            let current_revision = current.integer("revision")?;
            if current_revision != expected_revision {
                return Ok(None);
            }
            let next_revision = current_revision.checked_add(1).ok_or_else(|| {
                StorageError::Invalid("operational settings revision is exhausted".to_owned())
            })?;
            let mut updated = settings_record.clone();
            updated.insert("revision", Field::I64(next_revision));
            transaction.put(Table::OperationalSettings, SINGLETON_KEY, &updated)?;
            Ok(Some(next_revision))
        })
        .await
}
