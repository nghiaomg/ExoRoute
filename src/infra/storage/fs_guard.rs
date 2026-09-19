//! Filesystem guard for the LMDB environment directory.
//!
//! Owns directory validation, environment-file preflight checks, post-open
//! permission hardening, and the map-size marker protocol used to make
//! `Env::resize` crash-safe across restarts.

use crate::infra::storage::{StorageError, codec};
use std::{
    fs::{self, OpenOptions},
    io,
    path::Path,
};

pub(super) fn ensure_environment_directory(path: &Path) -> Result<(), StorageError> {
    // The configured environment itself is ExoRoute-owned. Its parent may be
    // a shared directory such as /tmp and must not have its permissions changed.
    crate::config::ensure_private_dir(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(StorageError::Invalid(
                    "LMDB path must be a real directory, not a symlink or file".to_owned(),
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(StorageError::Io(error));
        }
        Err(error) => return Err(StorageError::Io(error)),
    }
    Ok(())
}

pub(super) fn preflight_environment_files(path: &Path) -> Result<(), StorageError> {
    crate::config::ensure_no_reparse_path_components(path)?;
    for name in ["data.mdb", "lock.mdb"] {
        let file = path.join(name);
        match fs::symlink_metadata(&file) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(StorageError::Invalid(format!(
                        "LMDB environment file '{name}' is not a regular file"
                    )));
                }
                crate::config::ensure_private_file(&file)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(StorageError::Io(error)),
        }
    }
    Ok(())
}

pub(super) fn secure_environment_files(path: &Path) -> Result<(), StorageError> {
    for name in ["data.mdb", "lock.mdb"] {
        let file = path.join(name);
        match fs::symlink_metadata(&file) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
                return Err(StorageError::Invalid(format!(
                    "LMDB environment file '{name}' is not a regular file"
                )));
            }
            Ok(_) => crate::config::ensure_private_file(&file)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound && name == "lock.mdb" => {}
            Err(error) => return Err(StorageError::Io(error)),
        }
    }
    Ok(())
}

pub(super) fn read_map_size_marker(
    path: &Path,
    initial_map_size: usize,
    map_size_limit: usize,
) -> Result<usize, StorageError> {
    const PREFIX: &str = ".exoroute-map-size-";
    let mut map_size = initial_map_size;
    let mut candidate = initial_map_size;
    loop {
        let marker = path.join(format!("{PREFIX}{candidate}"));
        match fs::symlink_metadata(&marker) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != 0 {
                    return Err(StorageError::Invalid(
                        "LMDB map-size marker must be an empty regular file".to_owned(),
                    ));
                }
                crate::config::ensure_private_file(&marker)?;
                map_size = map_size.max(candidate);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(StorageError::Io(error)),
        }
        if candidate == map_size_limit {
            break;
        }
        let next = candidate
            .checked_mul(2)
            .unwrap_or(map_size_limit)
            .min(map_size_limit);
        if next <= candidate {
            break;
        }
        candidate = next;
    }
    Ok(map_size)
}

pub(crate) fn write_map_size_marker(path: &Path, map_size: usize) -> Result<(), StorageError> {
    let marker = path.join(format!(".exoroute-map-size-{map_size}"));
    match fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != 0 {
                return Err(StorageError::Invalid(
                    "LMDB map-size marker must be an empty regular file".to_owned(),
                ));
            }
            crate::config::ensure_private_file(&marker)?;
            return Ok(());
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(StorageError::Io(error)),
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&marker)?;
    crate::config::ensure_private_file(&marker)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn remove_map_size_marker(path: &Path, map_size: usize) -> Result<(), StorageError> {
    let marker = path.join(format!(".exoroute-map-size-{map_size}"));
    match fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != 0 {
                return Err(StorageError::Invalid(
                    "LMDB map-size marker must be an empty regular file".to_owned(),
                ));
            }
            crate::config::ensure_private_file(&marker)?;
            fs::remove_file(marker)?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StorageError::Io(error)),
    }
}

pub(crate) fn resize_with_marker_cleanup(
    path: &Path,
    map_size: usize,
    resize: impl FnOnce() -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    match resize() {
        Ok(()) => Ok(()),
        Err(resize_error) => match remove_map_size_marker(path, map_size) {
            Ok(()) => Err(resize_error),
            Err(cleanup_error) => Err(StorageError::Io(io::Error::other(format!(
                "LMDB resize failed ({resize_error}); its map-size marker could not be removed ({cleanup_error})"
            )))),
        },
    }
}

pub(super) fn encode_record<T: serde::Serialize>(record: &T) -> Result<Vec<u8>, StorageError> {
    codec::encode_record(record)
}

pub(super) fn decode_record<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, StorageError> {
    codec::decode_record(bytes)
}
