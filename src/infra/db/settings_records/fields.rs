//! Shared record field helpers used by more than one settings codec.

use crate::infra::storage::{Field, Record, StorageError};

pub(crate) fn usize_field(value: usize, name: &'static str) -> Result<Field, StorageError> {
    i64::try_from(value)
        .map(Field::I64)
        .map_err(|_| StorageError::Invalid(format!("{name} is outside the supported range")))
}

pub(crate) fn field_usize(record: &Record, name: &str) -> Result<usize, StorageError> {
    usize::try_from(record.integer(name)?)
        .map_err(|_| StorageError::Invalid(format!("LMDB record field '{name}' is invalid")))
}

pub(crate) fn optional_field_usize(
    record: &Record,
    name: &str,
) -> Result<Option<usize>, StorageError> {
    record
        .optional_integer(name)?
        .map(|value| {
            usize::try_from(value).map_err(|_| {
                StorageError::Invalid(format!("LMDB record field '{name}' is invalid"))
            })
        })
        .transpose()
}
