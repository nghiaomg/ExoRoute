//! LMDB record codec for the global output-style selection.

use crate::{
    infra::storage::{Field, Record, StorageError},
    support::output_styles::{self, OutputStyleSelection, OutputStylesSnapshot},
};

pub(crate) fn output_styles_record(
    snapshot: &OutputStylesSnapshot,
) -> Result<Record, StorageError> {
    if snapshot.revision < 0 || snapshot.revision == i64::MAX {
        return Err(StorageError::Invalid(
            "output styles revision is invalid".to_owned(),
        ));
    }
    let styles = output_styles::validate_styles(&snapshot.styles).map_err(StorageError::Invalid)?;
    if !snapshot.overridden && !styles.is_empty() {
        return Err(StorageError::Invalid(
            "output styles defaults cannot contain enabled styles".to_owned(),
        ));
    }
    let encoded = bincode::serialize(&styles)
        .map_err(|error| StorageError::Codec(format!("output styles encoding failed: {error}")))?;
    if encoded.len() > output_styles::MAX_STYLES_PAYLOAD_BYTES {
        return Err(StorageError::Invalid(
            "output styles configuration is too large".to_owned(),
        ));
    }
    Ok(Record::new()
        .with(
            "format_version",
            Field::I64(output_styles::OUTPUT_STYLE_FORMAT_VERSION),
        )
        .with("styles", Field::Bytes(encoded))
        .with("revision", Field::I64(snapshot.revision))
        .with("overridden", Field::Bool(snapshot.overridden)))
}

pub(crate) fn output_styles_from_record(
    record: &Record,
) -> Result<OutputStylesSnapshot, StorageError> {
    if record.integer("format_version")? != output_styles::OUTPUT_STYLE_FORMAT_VERSION {
        return Err(StorageError::Invalid(
            "output styles record format is unsupported".to_owned(),
        ));
    }
    let encoded = record.bytes("styles")?;
    if encoded.len() > output_styles::MAX_STYLES_PAYLOAD_BYTES {
        return Err(StorageError::Invalid(
            "output styles configuration is too large".to_owned(),
        ));
    }
    let styles: Vec<OutputStyleSelection> = bincode::deserialize(encoded).map_err(|error| {
        StorageError::Codec(format!("output styles record is invalid: {error}"))
    })?;
    let normalized = output_styles::validate_styles(&styles).map_err(StorageError::Invalid)?;
    if normalized != styles {
        return Err(StorageError::Invalid(
            "output styles record is not in canonical order".to_owned(),
        ));
    }
    let revision = record.integer("revision")?;
    if revision < 0 || revision == i64::MAX {
        return Err(StorageError::Invalid(
            "output styles revision is invalid".to_owned(),
        ));
    }
    let overridden = record.boolean("overridden")?;
    if !overridden && !normalized.is_empty() {
        return Err(StorageError::Invalid(
            "output styles defaults cannot contain enabled styles".to_owned(),
        ));
    }
    Ok(OutputStylesSnapshot {
        styles: normalized,
        revision,
        overridden,
    })
}
