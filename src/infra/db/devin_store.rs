//! Devin cloud-agent integration storage: one encrypted API key singleton.
//!
//! The key is stored as the same `Field::Bytes` blob kind provider
//! credentials use; encryption happens in the admin layer (it owns the
//! master key), so a missing master key fails closed before any write.

use super::SINGLETON_KEY;
use crate::infra::storage::{Database, Field, Record, StorageError, Table};

pub async fn load_devin_api_key_blob(database: &Database) -> Result<Option<Vec<u8>>, StorageError> {
    let Some(record) = database
        .read(|transaction| transaction.get::<Record>(Table::Integrations, SINGLETON_KEY))
        .await?
    else {
        return Ok(None);
    };
    Ok(record.optional_bytes("devin_api_key")?.map(<[u8]>::to_vec))
}

pub async fn save_devin_api_key_blob(
    database: &Database,
    encrypted: Option<Vec<u8>>,
) -> Result<(), StorageError> {
    let mut record = Record::new().with(
        "devin_api_key",
        match encrypted {
            Some(bytes) => Field::Bytes(bytes),
            None => Field::Null,
        },
    );
    // Remember when the key was last written so the dashboard can show a
    // stable detail line without ever exposing the key itself.
    let written_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0);
    record = record
        .with("devin_api_key_updated_at", Field::I64(written_at))
        .with("updated_at", Field::Text(written_at.to_string()));
    database
        .write(move |transaction| transaction.put(Table::Integrations, SINGLETON_KEY, &record))
        .await
}
