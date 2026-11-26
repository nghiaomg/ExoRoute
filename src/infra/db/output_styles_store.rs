//! Output-style settings persistence with revision compare-and-swap.
//!
//! A corrupt record fails closed on load so a bad write can never silently
//! enable or disable prompt injection.

use super::SINGLETON_KEY;
use crate::{
    infra::storage::{Database, Record, StorageError, Table},
    support::output_styles::{self, OutputStyleSelection, OutputStylesSnapshot},
};

pub async fn load_output_styles(database: &Database) -> Result<OutputStylesSnapshot, StorageError> {
    let record = database
        .read(|transaction| transaction.get::<Record>(Table::OutputStyles, SINGLETON_KEY))
        .await?
        .ok_or_else(|| StorageError::Invalid("output styles record is missing".to_owned()))?;
    super::settings_records::output_styles_from_record(&record)
}

pub async fn save_output_styles(
    database: &Database,
    styles: Vec<OutputStyleSelection>,
    expected_revision: i64,
    overridden: bool,
) -> Result<Option<OutputStylesSnapshot>, StorageError> {
    if expected_revision < 0 || expected_revision == i64::MAX {
        return Err(StorageError::Invalid(
            "output styles revision is invalid".to_owned(),
        ));
    }
    let styles = output_styles::validate_styles(&styles).map_err(StorageError::Invalid)?;
    let template = OutputStylesSnapshot {
        styles,
        revision: 0,
        overridden,
    };
    database
        .write(move |transaction| {
            let current = transaction
                .get::<Record>(Table::OutputStyles, SINGLETON_KEY)?
                .ok_or_else(|| {
                    StorageError::Invalid("output styles record is missing".to_owned())
                })?;
            let current_snapshot = super::settings_records::output_styles_from_record(&current)?;
            if current_snapshot.revision != expected_revision {
                return Ok(None);
            }
            let next_revision = current_snapshot
                .revision
                .checked_add(1)
                .filter(|revision| *revision < i64::MAX)
                .ok_or_else(|| {
                    StorageError::Invalid("output styles revision is exhausted".to_owned())
                })?;
            let snapshot = OutputStylesSnapshot {
                styles: template.styles.clone(),
                revision: next_revision,
                overridden,
            };
            let updated = super::settings_records::output_styles_record(&snapshot)?;
            transaction.put(Table::OutputStyles, SINGLETON_KEY, &updated)?;
            Ok(Some(snapshot))
        })
        .await
}
