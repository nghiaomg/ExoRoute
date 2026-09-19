//! Record codec shared by the storage engine and its filesystem guard.
//!
//! Centralizes bincode serialization so transaction views, snapshot code,
//! and environment bootstrap all fail with the same codec diagnostics.

use crate::infra::storage::StorageError;
use serde::{Serialize, de::DeserializeOwned};

pub(super) fn encode_record<T: Serialize>(record: &T) -> Result<Vec<u8>, StorageError> {
    bincode::serialize(record).map_err(|error| StorageError::Codec(error.to_string()))
}

pub(super) fn decode_record<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StorageError> {
    bincode::deserialize(bytes).map_err(|error| StorageError::Codec(error.to_string()))
}
