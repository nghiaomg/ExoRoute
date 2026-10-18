//! Request-log compression dictionary: trained once from stored rows,
//! persisted in the `Meta` table so backups carry it, and loaded into
//! process memory at startup. A backup import reloads the runtime copy, so
//! it always matches the persisted one.
//!
//! Per-row zstd frames compress far better when they share a trained
//! dictionary: field names, protocol names, model identifiers, and error
//! templates repeat across rows. Frames written without a dictionary stay
//! decodable once one is loaded, because zstd consults the dictionary only
//! when a frame references it; frames written with the dictionary require
//! that exact dictionary, which is why the dictionary is persisted in the
//! database itself instead of a sidecar file that backups would lose.

use std::sync::RwLock;

use super::{Database, Record, SnapshotEntry, StorageError, Table};

/// Meta key holding the trained dictionary bytes.
pub(crate) const REQUEST_LOG_DICTIONARY_KEY: &str = "request_log_dictionary";

/// Hard size bound for the stored dictionary. Training requests at most this
/// many bytes, and restore validation enforces the same bound, so a hostile
/// backup cannot plant an oversized dictionary blob.
pub(crate) const REQUEST_LOG_DICTIONARY_MAX_BYTES: usize = 128 * 1024;

/// Training starts once this many request logs are stored, so the sample
/// reflects real traffic instead of a handful of startup requests.
const REQUEST_LOG_DICTIONARY_MIN_ROWS: i64 = 256;

/// Bounded training sample. Scan order is enough: the dictionary captures
/// recurring structure, not row-exact content.
const REQUEST_LOG_DICTIONARY_SAMPLE_ROWS: usize = 100;

/// Upper bound on the bytes handed to the trainer in one shot.
const REQUEST_LOG_DICTIONARY_SAMPLE_BYTES: usize = 512 * 1024;

/// The loaded dictionary as a leaked `&'static` slice. Restores are rare,
/// admin-initiated events, so the per-reload leak (at most
/// [`REQUEST_LOG_DICTIONARY_MAX_BYTES`]) stays bounded in practice.
static REQUEST_LOG_DICTIONARY: RwLock<Option<&'static [u8]>> = RwLock::new(None);

/// The currently loaded dictionary, if any. The lock only guards a copy of
/// the `&'static` slice; it is never held across compression work, awaits,
/// or database calls.
pub(super) fn current_request_log_dictionary() -> Option<&'static [u8]> {
    *REQUEST_LOG_DICTIONARY
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn set_request_log_dictionary(dictionary: &[u8]) {
    let leaked: &'static [u8] = Box::leak(dictionary.to_vec().into_boxed_slice());
    *REQUEST_LOG_DICTIONARY
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(leaked);
}

fn clear_request_log_dictionary() {
    *REQUEST_LOG_DICTIONARY
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

/// Load the persisted dictionary (if any) into process memory, or clear the
/// runtime copy when the database genuinely has none. Called at startup and
/// after a backup import, so compression always matches the database contents.
///
/// Must run before the first read of `Table::RequestLogs` in the process: a
/// frame written with the dictionary cannot be decoded without it, so any scan
/// that happens first fails as a codec error instead of returning rows.
///
/// A read failure or an oversized stored value keeps the currently loaded
/// dictionary instead of clearing it: dropping the dictionary would make every
/// frame already written with it undecodable, which is far worse than
/// continuing with a possibly stale dictionary.
pub(crate) async fn load_request_log_dictionary(db: &Database) {
    let stored = db
        .read(|transaction| transaction.get::<Vec<u8>>(Table::Meta, REQUEST_LOG_DICTIONARY_KEY))
        .await;
    match stored {
        Ok(Some(dictionary)) if dictionary.len() <= REQUEST_LOG_DICTIONARY_MAX_BYTES => {
            set_request_log_dictionary(&dictionary);
        }
        Ok(None) => clear_request_log_dictionary(),
        Ok(Some(_)) => {
            tracing::warn!(
                "the stored request-log compression dictionary exceeds its size bound; keeping the currently loaded dictionary"
            );
        }
        Err(error) => {
            tracing::warn!(
                %error,
                "could not read the request-log compression dictionary; keeping the currently loaded dictionary"
            );
        }
    }
}

/// Extract the request-log compression dictionary carried by a snapshot or
/// backup payload. The snapshot path decodes request-log frames with this
/// value rather than the runtime copy, so a backup produced by another
/// installation remains decodable. Returns `None` when the snapshot has no
/// dictionary; an oversized or undecodable entry also yields `None`, which
/// makes frame decoding fail loudly instead of silently mis-reading.
pub(crate) fn request_log_dictionary_from_entries(entries: &[SnapshotEntry]) -> Option<Vec<u8>> {
    let entry = entries
        .iter()
        .find(|entry| entry.table == Table::Meta && entry.key == REQUEST_LOG_DICTIONARY_KEY)?;
    match bincode::deserialize::<Vec<u8>>(&entry.value) {
        Ok(dictionary) if dictionary.len() <= REQUEST_LOG_DICTIONARY_MAX_BYTES => Some(dictionary),
        Ok(_) => {
            tracing::warn!(
                "ignoring an oversized request-log compression dictionary carried by a snapshot"
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                %error,
                "ignoring an undecodable request-log compression dictionary carried by a snapshot"
            );
            None
        }
    }
}

/// Make sure a dictionary is loaded or, when the database has enough rows and
/// none is stored yet, train one and load it. Cheap no-op once loaded; the
/// telemetry maintenance tick calls this periodically.
pub(crate) async fn ensure_request_log_dictionary_trained(db: &Database) {
    if current_request_log_dictionary().is_some() {
        return;
    }
    if let Err(error) = train_and_store(db).await {
        tracing::warn!(%error, "could not prepare the request-log compression dictionary");
        return;
    }
    load_request_log_dictionary(db).await;
}

/// Train a request-log dictionary from stored rows and persist it. Returns
/// whether a new dictionary was written. Training failures are logged and
/// reported as `Ok(false)`: compression is an optimization, and a failed
/// training run must never break telemetry or change read compatibility.
/// Test callers use this directly because it never touches the runtime copy.
pub(crate) async fn train_and_store(db: &Database) -> Result<bool, StorageError> {
    let stored = db
        .read(|transaction| transaction.get::<Vec<u8>>(Table::Meta, REQUEST_LOG_DICTIONARY_KEY))
        .await?;
    if stored.is_some() {
        return Ok(false);
    }
    let row_count = db
        .read(|transaction| transaction.get::<i64>(Table::Meta, "request_log_count"))
        .await?
        .unwrap_or(0);
    if row_count < REQUEST_LOG_DICTIONARY_MIN_ROWS {
        return Ok(false);
    }
    let samples: Vec<Vec<u8>> = db
        .read(|transaction| {
            let rows = transaction.scan_prefix::<Record>(
                Table::RequestLogs,
                "",
                REQUEST_LOG_DICTIONARY_SAMPLE_ROWS,
            )?;
            rows.into_iter()
                .map(|(_, record)| {
                    bincode::serialize(&record).map_err(|error| {
                        StorageError::Codec(format!("record encoding failed: {error}"))
                    })
                })
                .collect()
        })
        .await?;
    let mut total = 0_usize;
    let samples: Vec<Vec<u8>> = samples
        .into_iter()
        .take_while(|sample| {
            let take = total + sample.len() <= REQUEST_LOG_DICTIONARY_SAMPLE_BYTES;
            if take {
                total += sample.len();
            }
            take
        })
        .collect();

    let trained = tokio::task::spawn_blocking(move || -> Result<Option<Vec<u8>>, StorageError> {
        // ZDICT needs the sample corpus to dwarf the requested dictionary;
        // cap the request by the sample size and skip tiny corpora.
        if total < 4 * 1024 {
            return Ok(None);
        }
        let max_size = REQUEST_LOG_DICTIONARY_MAX_BYTES.min(total / 8);
        let sample_refs: Vec<&[u8]> = samples.iter().map(|sample| sample.as_slice()).collect();
        match zstd::dict::from_samples(&sample_refs, max_size) {
            Ok(dictionary) if !dictionary.is_empty() => Ok(Some(dictionary)),
            Ok(_) => Ok(None),
            Err(error) => {
                tracing::warn!(
                    %error,
                    "request-log dictionary training failed; keeping per-row compression without a dictionary"
                );
                Ok(None)
            }
        }
    })
    .await
    .map_err(|error| {
        StorageError::Task(format!("request-log dictionary training task failed: {error}"))
    })??;

    if let Some(dictionary) = trained {
        db.write(move |transaction| {
            transaction.put(Table::Meta, REQUEST_LOG_DICTIONARY_KEY, &dictionary)
        })
        .await?;
        return Ok(true);
    }
    Ok(false)
}
