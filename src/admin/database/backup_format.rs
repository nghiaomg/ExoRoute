#[cfg(test)]
use super::preflight_backup_payload;
use super::{
    BACKUP_CHECKSUM_BYTES, BACKUP_FORMAT_VERSION, BACKUP_MAGIC, BackupPayload,
    MAX_DATABASE_IMPORT_BYTES, preflight_backup_reader,
};
use crate::infra::storage::SnapshotEntry;
use bincode::Options as _;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

#[cfg(test)]
pub(crate) fn encode_backup(entries: Vec<SnapshotEntry>) -> Result<Vec<u8>, &'static str> {
    let payload = BackupPayload {
        storage_format_version: crate::infra::storage::STORAGE_FORMAT_VERSION,
        entries,
    };
    let payload_bytes =
        bincode::serialize(&payload).map_err(|_| "Could not encode database backup.")?;
    let header_size = BACKUP_MAGIC
        .len()
        .saturating_add(4 + 8 + BACKUP_CHECKSUM_BYTES);
    let total_size = header_size
        .checked_add(payload_bytes.len())
        .ok_or("The database backup exceeds the 512 MB limit.")?;
    if total_size as u64 > MAX_DATABASE_IMPORT_BYTES {
        return Err("The database backup exceeds the 512 MB limit.");
    }
    let checksum = Sha256::digest(&payload_bytes);
    let mut archive = Vec::with_capacity(total_size);
    archive.extend_from_slice(BACKUP_MAGIC);
    archive.extend_from_slice(&BACKUP_FORMAT_VERSION.to_le_bytes());
    archive.extend_from_slice(&(payload_bytes.len() as u64).to_le_bytes());
    archive.extend_from_slice(&checksum);
    archive.extend_from_slice(&payload_bytes);
    Ok(archive)
}

pub(crate) fn write_backup_archive(
    destination: &Path,
    entries: Vec<SnapshotEntry>,
) -> Result<u64, &'static str> {
    let parent = destination
        .parent()
        .ok_or("Could not create database backup.")?;
    let payload_path = parent.join("backup-payload.tmp");
    let payload = BackupPayload {
        storage_format_version: crate::infra::storage::STORAGE_FORMAT_VERSION,
        entries,
    };
    let payload_len = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .serialized_size(&payload)
        .map_err(|_| "Could not encode database backup.")?;
    let header_len = BACKUP_MAGIC
        .len()
        .saturating_add(4 + 8 + BACKUP_CHECKSUM_BYTES);
    let total_len = (header_len as u64)
        .checked_add(payload_len)
        .ok_or("The database backup exceeds the 512 MB limit.")?;
    if total_len > MAX_DATABASE_IMPORT_BYTES {
        return Err("The database backup exceeds the 512 MB limit.");
    }

    let mut payload_options = fs::OpenOptions::new();
    payload_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        payload_options.mode(0o600);
    }
    let mut payload_file = payload_options
        .open(&payload_path)
        .map_err(|_| "Could not create database backup.")?;
    bincode::serialize_into(&mut payload_file, &payload)
        .map_err(|_| "Could not encode database backup.")?;
    payload_file
        .flush()
        .and_then(|()| payload_file.sync_all())
        .map_err(|_| "Could not write database backup.")?;
    drop(payload_file);
    crate::config::ensure_private_file(&payload_path)
        .map_err(|_| "Could not secure database backup.")?;

    let mut hasher = Sha256::new();
    let mut payload_reader =
        fs::File::open(&payload_path).map_err(|_| "Could not read database backup.")?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = payload_reader
            .read(&mut buffer)
            .map_err(|_| "Could not read database backup.")?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let checksum = hasher.finalize();
    payload_reader
        .seek(SeekFrom::Start(0))
        .map_err(|_| "Could not read database backup.")?;

    let mut archive_options = fs::OpenOptions::new();
    archive_options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        archive_options.mode(0o600);
    }
    let mut archive = archive_options
        .open(destination)
        .map_err(|_| "Could not create database backup.")?;
    archive
        .write_all(BACKUP_MAGIC)
        .and_then(|()| archive.write_all(&BACKUP_FORMAT_VERSION.to_le_bytes()))
        .and_then(|()| archive.write_all(&payload_len.to_le_bytes()))
        .and_then(|()| archive.write_all(&checksum))
        .map_err(|_| "Could not write database backup.")?;
    let copied = io::copy(&mut payload_reader, &mut archive)
        .map_err(|_| "Could not write database backup.")?;
    if copied != payload_len {
        return Err("The database backup could not be written completely.");
    }
    archive
        .flush()
        .and_then(|()| archive.sync_all())
        .map_err(|_| "Could not write database backup.")?;
    drop(archive);
    crate::config::ensure_private_file(destination)
        .map_err(|_| "Could not secure database backup.")?;
    fs::remove_file(&payload_path).map_err(|_| "Could not remove temporary backup data.")?;
    Ok(total_len)
}

#[cfg(test)]
pub(crate) fn decode_backup(bytes: &[u8]) -> Result<BackupPayload, &'static str> {
    let header_size = BACKUP_MAGIC
        .len()
        .saturating_add(4 + 8 + BACKUP_CHECKSUM_BYTES);
    if bytes.len() as u64 > MAX_DATABASE_IMPORT_BYTES {
        return Err("The database file exceeds the 512 MB upload limit.");
    }
    if bytes.len() < header_size || !bytes.starts_with(BACKUP_MAGIC) {
        return Err("This is not a valid ExoRoute LMDB backup. SQLite backups are not supported.");
    }
    let version_start = BACKUP_MAGIC.len();
    let version_end = version_start + 4;
    let version = u32::from_le_bytes(
        bytes[version_start..version_end]
            .try_into()
            .map_err(|_| "The backup header is invalid.")?,
    );
    if version != BACKUP_FORMAT_VERSION {
        return Err("This LMDB backup format version is not supported by this ExoRoute build.");
    }
    let size_end = version_end + 8;
    let payload_size = u64::from_le_bytes(
        bytes[version_end..size_end]
            .try_into()
            .map_err(|_| "The backup header is invalid.")?,
    );
    let payload_size =
        usize::try_from(payload_size).map_err(|_| "The backup file is too large.")?;
    let checksum_end = size_end + BACKUP_CHECKSUM_BYTES;
    let expected_size = header_size
        .checked_add(payload_size)
        .ok_or("The backup size is invalid.")?;
    if expected_size != bytes.len() {
        return Err("The backup file size does not match its header.");
    }
    let payload_bytes = &bytes[checksum_end..];
    let actual_checksum = Sha256::digest(payload_bytes);
    if actual_checksum.as_slice() != &bytes[size_end..checksum_end] {
        return Err("The database backup checksum is invalid.");
    }
    preflight_backup_payload(payload_bytes)?;
    let payload: BackupPayload = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_DATABASE_IMPORT_BYTES)
        .deserialize::<BackupPayload>(payload_bytes)
        .map_err(|_| "The database backup payload is invalid or truncated.")?;
    if payload.storage_format_version == 0
        || payload.storage_format_version > crate::infra::storage::STORAGE_FORMAT_VERSION
    {
        return Err("This LMDB storage format version is not supported by this ExoRoute build.");
    }
    Ok(payload)
}

pub(crate) fn decode_backup_file(path: &Path) -> Result<BackupPayload, &'static str> {
    let mut file = fs::File::open(path).map_err(|_| "The uploaded file could not be read.")?;
    let file_len = file
        .metadata()
        .map_err(|_| "The uploaded file could not be inspected.")?
        .len();
    if file_len > MAX_DATABASE_IMPORT_BYTES {
        return Err("The database file exceeds the 512 MB upload limit.");
    }
    let header_len = BACKUP_MAGIC
        .len()
        .saturating_add(4 + 8 + BACKUP_CHECKSUM_BYTES);
    if file_len < header_len as u64 {
        return Err("This is not a valid ExoRoute LMDB backup. SQLite backups are not supported.");
    }
    let mut header = vec![0_u8; header_len];
    file.read_exact(&mut header)
        .map_err(|_| "The backup header is invalid.")?;
    if !header.starts_with(BACKUP_MAGIC) {
        return Err("This is not a valid ExoRoute LMDB backup. SQLite backups are not supported.");
    }
    let version_start = BACKUP_MAGIC.len();
    let version_end = version_start + 4;
    let version = u32::from_le_bytes(
        header[version_start..version_end]
            .try_into()
            .map_err(|_| "The backup header is invalid.")?,
    );
    if version != BACKUP_FORMAT_VERSION {
        return Err("This LMDB backup format version is not supported by this ExoRoute build.");
    }
    let size_end = version_end + 8;
    let payload_len = u64::from_le_bytes(
        header[version_end..size_end]
            .try_into()
            .map_err(|_| "The backup header is invalid.")?,
    );
    let expected_file_len = (header_len as u64)
        .checked_add(payload_len)
        .ok_or("The backup size is invalid.")?;
    if expected_file_len != file_len {
        return Err("The backup file size does not match its header.");
    }
    let checksum_end = size_end + BACKUP_CHECKSUM_BYTES;
    let expected_checksum = &header[size_end..checksum_end];

    let mut hasher = Sha256::new();
    let mut remaining = payload_len;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let amount = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| "The backup file is too large.")?;
        file.read_exact(&mut buffer[..amount])
            .map_err(|_| "The backup payload is truncated.")?;
        hasher.update(&buffer[..amount]);
        remaining -= amount as u64;
    }
    let checksum = hasher.finalize();
    if checksum.as_slice() != expected_checksum {
        return Err("The database backup checksum is invalid.");
    }

    let payload_start = header_len as u64;
    file.seek(SeekFrom::Start(payload_start))
        .map_err(|_| "The backup file could not be read.")?;
    preflight_backup_reader(&mut file, payload_len)?;
    file.seek(SeekFrom::Start(payload_start))
        .map_err(|_| "The backup file could not be read.")?;
    let payload: BackupPayload = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_limit(MAX_DATABASE_IMPORT_BYTES)
        .deserialize_from(file.take(payload_len))
        .map_err(|_| "The database backup payload is invalid or truncated.")?;
    if payload.storage_format_version == 0
        || payload.storage_format_version > crate::infra::storage::STORAGE_FORMAT_VERSION
    {
        return Err("This LMDB storage format version is not supported by this ExoRoute build.");
    }
    Ok(payload)
}
