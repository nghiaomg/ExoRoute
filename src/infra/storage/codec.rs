//! Record codec shared by the storage engine and its filesystem guard.
//!
//! Centralizes bincode serialization so transaction views, snapshot code,
//! and environment bootstrap all fail with the same codec diagnostics.
//!
//! Large, highly repetitive tables (request logs) are stored as zstd frames
//! behind an explicit marker byte, so the disk footprint shrinks without any
//! loss of fields. Reads accept both the marker format and the legacy raw
//! bincode layout, so older environments keep working without a rewrite.
//!
//! The marker is only meaningful for request-log values: every other table is
//! both written and read as raw bincode, so a raw value that happens to start
//! with the marker byte (for example an `i64` counter whose low byte is `0x5A`)
//! is never mistaken for a frame.

use std::cell::RefCell;

use super::Table;
use super::dictionary::current_request_log_dictionary;
use crate::infra::storage::StorageError;
use serde::{Serialize, de::DeserializeOwned};

thread_local! {
    /// Per-thread zstd contexts preloaded with the current request-log
    /// dictionary, keyed by dictionary identity so a reload swaps them. This
    /// pays the dictionary load once per reload instead of once per record.
    static DICT_COMPRESSOR: RefCell<Option<(&'static [u8], zstd::bulk::Compressor<'static>)>> =
        const { RefCell::new(None) };
    static DICT_DECOMPRESSOR: RefCell<Option<(&'static [u8], zstd::bulk::Decompressor<'static>)>> =
        const { RefCell::new(None) };
}

/// Format marker for zstd-compressed request-log payloads. A stored
/// request-log value starting with this byte is a compressed frame; every
/// other request-log value is a raw bincode payload. Only `Table::RequestLogs`
/// uses this marker, and its values are always `Record` bincode whose first
/// byte is the map length, so the marker is unambiguous within that table
/// except for the single length [`COMPRESSED_RECORD_PREFIX`] itself — which
/// [`encode_record_maybe_compressed`] forces through compression.
pub(super) const COMPRESSED_RECORD_PREFIX: u8 = 0x5A; // b'Z' for zstd

/// Compression level for request-log frames: zstd's middle range, where the
/// ratio is close to maximum while encode cost stays far below the write's
/// own durability cost.
pub(super) const COMPRESSED_RECORD_LEVEL: i32 = 7;

/// Compression is only profitable above this size; smaller records stay raw
/// so tiny rows do not pay the frame header and dictionary warm-up.
pub(super) const COMPRESSED_RECORD_MIN_BYTES: usize = 64;

/// A compressed record may legitimately expand to a multiple of its frame
/// size, but never to hundreds of megabytes. Anything larger is treated as
/// corrupt input rather than an allocation request. The bound is generous
/// next to `MAX_RECORD_FIELD_BYTES` (16 MiB) plus framing overhead.
pub(super) const MAX_COMPRESSED_EXPANSION: usize = 17 * 1024 * 1024;

pub(super) fn encode_record<T: Serialize>(record: &T) -> Result<Vec<u8>, StorageError> {
    bincode::serialize(record).map_err(|error| StorageError::Codec(error.to_string()))
}

/// Serialize a request-log record and store it as a compressed frame when
/// compression applies. Small payloads stay raw, but a payload whose first
/// byte equals [`COMPRESSED_RECORD_PREFIX`] must never be stored raw: the read
/// path would then decompress raw bincode. That combination is forced through
/// compression so the stored value is always an unambiguous frame.
pub(super) fn encode_record_maybe_compressed<T: Serialize>(
    record: &T,
) -> Result<Vec<u8>, StorageError> {
    let payload = encode_record(record)?;
    if payload.len() < COMPRESSED_RECORD_MIN_BYTES
        && payload.first() != Some(&COMPRESSED_RECORD_PREFIX)
    {
        return Ok(payload);
    }
    compress_record_payload(&payload)
}

/// Decode a raw bincode value without any decompression. Only request-log
/// values may be compressed, so every other caller (metadata counters, index
/// strings, environment bootstrap) must use this function; applying the
/// request-log marker test to those types would misread valid raw values.
pub(super) fn decode_record<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StorageError> {
    bincode::deserialize(bytes).map_err(|error| StorageError::Codec(error.to_string()))
}

/// Decode one table value, decompressing only when the table is
/// `Table::RequestLogs` and the stored bytes carry the frame marker. Uses the
/// currently loaded runtime dictionary, which is kept in sync with the live
/// database metadata.
pub(super) fn decode_stored_record<T: DeserializeOwned>(
    table: Table,
    bytes: &[u8],
) -> Result<T, StorageError> {
    let payload = if table == Table::RequestLogs {
        decompressed_request_log_payload(bytes, current_request_log_dictionary())?
    } else {
        bytes.to_vec()
    };
    bincode::deserialize(&payload).map_err(|error| StorageError::Codec(error.to_string()))
}

/// Decode request-log bytes with an explicit dictionary. Used by the snapshot
/// and backup paths, which must decode a snapshot's frames with the dictionary
/// carried in that same snapshot rather than the runtime copy.
pub(crate) fn decode_request_log_bytes<T: DeserializeOwned>(
    bytes: &[u8],
    dictionary: Option<&[u8]>,
) -> Result<T, StorageError> {
    let payload = if is_compressed_record(bytes) {
        decompress_frame_with(&bytes[1..], dictionary)?
    } else {
        bytes.to_vec()
    };
    bincode::deserialize(&payload).map_err(|error| StorageError::Codec(error.to_string()))
}

/// Compress one already-serialized payload into the tagged frame format
/// accepted by the request-log read path. Frames use the loaded request-log
/// dictionary when one is available, which shrinks repetitive rows further;
/// dictionary-free frames remain fully self-contained.
pub(super) fn compress_record_payload(payload: &[u8]) -> Result<Vec<u8>, StorageError> {
    let compressed = match current_request_log_dictionary() {
        Some(dictionary) => compress_with_dictionary(payload, dictionary)?,
        None => zstd::bulk::compress(payload, COMPRESSED_RECORD_LEVEL)
            .map_err(|error| StorageError::Codec(format!("record compression failed: {error}")))?,
    };
    let mut frame = compressed;
    frame.insert(0, COMPRESSED_RECORD_PREFIX);
    Ok(frame)
}

/// Compress into a frame that requires `dictionary` to decode. Contexts are
/// cached per thread and rebuilt only when the dictionary changes.
pub(super) fn compress_with_dictionary(
    payload: &[u8],
    dictionary: &'static [u8],
) -> Result<Vec<u8>, StorageError> {
    DICT_COMPRESSOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|(cached, _)| *cached != dictionary)
        {
            let compressor =
                zstd::bulk::Compressor::with_dictionary(COMPRESSED_RECORD_LEVEL, dictionary)
                    .map_err(|error| {
                        StorageError::Codec(format!("record compression failed: {error}"))
                    })?;
            *slot = Some((dictionary, compressor));
        }
        match slot.as_mut() {
            Some((_, compressor)) => compressor.compress(payload).map_err(|error| {
                StorageError::Codec(format!("record compression failed: {error}"))
            }),
            // The cache was populated directly above; this is unreachable.
            None => Err(StorageError::Codec(
                "compression context was not initialized".to_owned(),
            )),
        }
    })
}

fn is_compressed_record(bytes: &[u8]) -> bool {
    bytes.first() == Some(&COMPRESSED_RECORD_PREFIX)
}

/// Decompress a stored request-log value using the process-wide runtime
/// dictionary, falling back to raw bytes when the value is not a frame.
fn decompressed_request_log_payload(
    bytes: &[u8],
    dictionary: Option<&'static [u8]>,
) -> Result<Vec<u8>, StorageError> {
    if !is_compressed_record(bytes) {
        return Ok(bytes.to_vec());
    }
    decompress_frame(&bytes[1..], dictionary)
}

/// Decode one frame payload using a cached `'static` dictionary (the runtime
/// request-log dictionary). Frames written without a dictionary stay
/// decodable when a dictionary is loaded (zstd only consults the dictionary
/// when a frame references it); frames written with a dictionary require that
/// exact dictionary.
pub(super) fn decompress_frame(
    frame: &[u8],
    dictionary: Option<&'static [u8]>,
) -> Result<Vec<u8>, StorageError> {
    match dictionary {
        Some(dictionary) => decompress_with_dictionary(frame, dictionary),
        None => zstd::bulk::decompress(frame, MAX_COMPRESSED_EXPANSION)
            .map_err(|error| StorageError::Codec(format!("record decompression failed: {error}"))),
    }
}

/// Decode one frame payload with a borrowed dictionary that may not be
/// `'static` (a dictionary read out of a snapshot being validated). Contexts
/// are built per call; the snapshot path is bounded and admin-initiated.
fn decompress_frame_with(frame: &[u8], dictionary: Option<&[u8]>) -> Result<Vec<u8>, StorageError> {
    match dictionary {
        Some(dictionary) => zstd::bulk::Decompressor::with_dictionary(dictionary)
            .map_err(|error| StorageError::Codec(format!("record decompression failed: {error}")))?
            .decompress(frame, MAX_COMPRESSED_EXPANSION)
            .map_err(|error| StorageError::Codec(format!("record decompression failed: {error}"))),
        None => zstd::bulk::decompress(frame, MAX_COMPRESSED_EXPANSION)
            .map_err(|error| StorageError::Codec(format!("record decompression failed: {error}"))),
    }
}

fn decompress_with_dictionary(
    frame: &[u8],
    dictionary: &'static [u8],
) -> Result<Vec<u8>, StorageError> {
    DICT_DECOMPRESSOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot
            .as_ref()
            .is_none_or(|(cached, _)| *cached != dictionary)
        {
            let decompressor =
                zstd::bulk::Decompressor::with_dictionary(dictionary).map_err(|error| {
                    StorageError::Codec(format!("record decompression failed: {error}"))
                })?;
            *slot = Some((dictionary, decompressor));
        }
        match slot.as_mut() {
            Some((_, decompressor)) => decompressor
                .decompress(frame, MAX_COMPRESSED_EXPANSION)
                .map_err(|error| {
                    StorageError::Codec(format!("record decompression failed: {error}"))
                }),
            // The cache was populated directly above; this is unreachable.
            None => Err(StorageError::Codec(
                "decompression context was not initialized".to_owned(),
            )),
        }
    })
}
