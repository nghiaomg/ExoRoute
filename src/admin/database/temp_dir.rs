use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
    sync::OnceLock,
    sync::mpsc::{self, SyncSender, TrySendError},
    thread,
};

const TEMP_DIRECTORY_CLEANUP_QUEUE_CAPACITY: usize = 32;
static TEMP_DIRECTORY_CLEANUP: OnceLock<Option<SyncSender<PathBuf>>> = OnceLock::new();

pub(crate) struct TempDirectory(pub(crate) PathBuf);

fn temp_directory_cleanup_sender() -> Option<&'static SyncSender<PathBuf>> {
    TEMP_DIRECTORY_CLEANUP
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel::<PathBuf>(
                TEMP_DIRECTORY_CLEANUP_QUEUE_CAPACITY,
            );
            thread::Builder::new()
                .name("exoroute-temp-cleanup".to_owned())
                .spawn(move || {
                    while let Ok(path) = receiver.recv() {
                        if let Err(error) = fs::remove_dir_all(&path)
                            && error.kind() != ErrorKind::NotFound
                        {
                            tracing::warn!(%error, path = %path.display(), "could not remove temporary database directory");
                        }
                    }
                })
                .ok()?;
            Some(sender)
        })
        .as_ref()
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let Some(sender) = temp_directory_cleanup_sender() else {
            tracing::warn!(path = %self.0.display(), "temporary database directory cleanup worker is unavailable; directory will be removed on next startup");
            return;
        };
        if let Err(error) = sender.try_send(self.0.clone()) {
            let reason = match error {
                TrySendError::Full(_) => "cleanup queue is full",
                TrySendError::Disconnected(_) => "cleanup worker stopped",
            };
            tracing::warn!(path = %self.0.display(), %reason, "temporary database directory will be removed on next startup");
        }
    }
}

pub(crate) fn cleanup_stale_temp_directories(app_dir: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(app_dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let is_backup_temp = name.to_string_lossy().starts_with("lmdb-backup-");
        let is_import_temp = name.to_string_lossy().starts_with("lmdb-import-");
        if file_type.is_dir() && (is_backup_temp || is_import_temp) {
            fs::remove_dir_all(entry.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}

pub(crate) fn create_private_temp_dir(base: &Path, prefix: &str) -> io::Result<PathBuf> {
    for _ in 0..8 {
        let path = base.join(format!("{prefix}-{}", uuid::Uuid::new_v4().simple()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            match builder.mode(0o700).create(&path) {
                Ok(()) => return Ok(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        #[cfg(not(unix))]
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary directory",
    ))
}
