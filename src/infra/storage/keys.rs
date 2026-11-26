//! LMDB key and prefix validation.
//!
//! The 500-byte limit mirrors LMDB's key-size constraint; rejecting NUL
//! bytes up front keeps B-tree ordering stable for snapshots and scans.

use crate::infra::storage::{MAX_KEY_BYTES, StorageError};

pub fn validate_key(key: &str) -> Result<(), StorageError> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES || key.as_bytes().contains(&0) {
        return Err(StorageError::Invalid(
            "LMDB key is empty or exceeds the 500-byte key limit".to_owned(),
        ));
    }
    Ok(())
}

pub fn validate_prefix(prefix: &str) -> Result<(), StorageError> {
    if prefix.len() > MAX_KEY_BYTES || prefix.as_bytes().contains(&0) {
        return Err(StorageError::Invalid(
            "LMDB key prefix exceeds the 500-byte key limit".to_owned(),
        ));
    }
    Ok(())
}
