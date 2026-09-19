use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};

pub struct DatabaseLock {
    _file: File,
}

/// Lock the LMDB environment for this process. LMDB itself permits multiple
/// processes, but ExoRoute's in-memory auth and rate-limit state is process-local.
/// Keep the established sibling lock path for upgrades from earlier versions;
/// secure the lock file itself without changing a custom path's parent ACL.
pub fn lock_database(path: &Path) -> io::Result<DatabaseLock> {
    crate::config::ensure_no_reparse_path_components(path)?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        crate::config::ensure_no_reparse_path_components(parent)?;
        fs::create_dir_all(parent)?;
        crate::config::ensure_no_reparse_path_components(parent)?;
    }
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    let lock_path = Path::new(&lock_path);
    crate::config::ensure_no_reparse_path_components(lock_path)?;
    ensure_lock_file_is_safe(lock_path)?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(lock_path)?;
    crate::config::ensure_private_file(lock_path)?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|error| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("LMDB environment is already open by another ExoRoute process: {error}"),
        )
    })?;
    Ok(DatabaseLock { _file: file })
}

fn ensure_lock_file_is_safe(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "LMDB lock path must be a regular file, not a symlink",
            ))
        }
        Ok(_) => crate::config::ensure_private_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
