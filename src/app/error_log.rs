//! Persistent error log: a size-rotated `error-logs.txt` next to the app data.
//!
//! The file receives ERROR-level tracing events only, mirrored from the same
//! events the console shows (console output disappears when the process is
//! closed, so operators lose the diagnostics for a crash they just saw). The
//! sink is best-effort by design: an I/O failure on this file must never
//! affect request handling, so failed writes are dropped after the console
//! layer has already emitted them.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing_subscriber::layer::Layer as _;
use tracing_subscriber::prelude::*;

/// Maximum bytes in the active log before it rotates into `error-logs.old.txt`.
pub(crate) const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

pub(crate) const LOG_FILE_NAME: &str = "error-logs.txt";
const BACKUP_FILE_NAME: &str = "error-logs.old.txt";

/// Size-rotating append-only writer for the error log.
///
/// All file work happens under one mutex on the `tracing-appender` writer
/// thread — never on an async worker — so the synchronous `File` use here is
/// safe and uncontended. Writes after a rotation failure keep appending until
/// the rename succeeds on a later attempt instead of losing logs.
pub(crate) struct RotatingErrorLog {
    path: PathBuf,
    backup_path: PathBuf,
    max_bytes: u64,
    state: Mutex<LogState>,
}

struct LogState {
    file: Option<File>,
    written: u64,
}

impl RotatingErrorLog {
    pub(crate) fn new(path: PathBuf, max_bytes: u64) -> Self {
        let backup_path = path.with_file_name(BACKUP_FILE_NAME);
        Self {
            path,
            backup_path,
            max_bytes,
            state: Mutex::new(LogState {
                file: None,
                written: 0,
            }),
        }
    }

    /// Appends one write, rotating first when the active log is full. Returns
    /// the input length whether or not the bytes landed: callers log to the
    /// console independently, and a failing error sink must not panic the
    /// writer thread.
    fn append(&self, buf: &[u8]) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let file = match state.file.take() {
            Some(file) if state.written <= self.max_bytes => Some((file, state.written)),
            Some(file) => self.rotate(file),
            None => self.open(),
        };
        let Some((mut file, written)) = file else {
            return;
        };
        if file.write_all(buf).is_ok() {
            state.written = written + buf.len() as u64;
            state.file = Some(file);
        }
    }

    /// Swaps a full active log into the backup slot and reopens a fresh one.
    fn rotate(&self, file: File) -> Option<(File, u64)> {
        drop(file);
        let _ = std::fs::rename(&self.path, &self.backup_path);
        self.open()
    }

    fn open(&self) -> Option<(File, u64)> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok()?;
        let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Some((file, written))
    }
}

impl Write for RotatingErrorLog {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.append(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl RotatingErrorLog {
    /// Test/inspection helper: the live bytes the next write will append to.
    #[cfg(test)]
    fn written(&self) -> u64 {
        self.state.lock().map(|state| state.written).unwrap_or(0)
    }
}

/// Installs the process subscriber: the usual console layer with the given
/// filter, plus an ERROR-only mirror into `error-logs.txt`. Returns the
/// worker guard that must stay alive for the process lifetime. When the log
/// file cannot be created the subscriber still initializes — startup keeps
/// console-only logging instead of failing.
pub(crate) fn init_tracing(
    app_dir: &Path,
    console_filter: tracing_subscriber::EnvFilter,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let console = tracing_subscriber::fmt::layer().with_filter(console_filter);
    let registry = tracing_subscriber::registry().with(console);
    let log = RotatingErrorLog::new(app_dir.join(LOG_FILE_NAME), MAX_LOG_BYTES);
    let (writer, guard) = tracing_appender::non_blocking(log);
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(writer)
        .with_filter(tracing_subscriber::filter::LevelFilter::ERROR);
    registry.with(file_layer).try_init().ok().map(|()| guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("exoroute-error-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_append_and_rotate_at_the_size_cap() {
        let dir = temp_dir("rotate");
        let path = dir.join(LOG_FILE_NAME);
        let mut log = RotatingErrorLog::new(path.clone(), 16);

        log.write_all(b"first-line\n").unwrap();
        log.write_all(b"second-line\n").unwrap();
        assert!(path.exists());
        assert!(log.written() >= 22);

        // The next write crosses the cap and must rotate the first two lines
        // into the backup, starting a fresh active log.
        log.write_all(b"third-line\n").unwrap();
        let backup = dir.join(BACKUP_FILE_NAME);
        let backed_up = std::fs::read_to_string(&backup).unwrap();
        assert!(backed_up.contains("first-line"));
        assert!(backed_up.contains("second-line"));
        assert!(!backed_up.contains("third-line"));
        let active = std::fs::read_to_string(&path).unwrap();
        assert_eq!(active, "third-line\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_reopened_writer_resumes_from_the_existing_file_size() {
        let dir = temp_dir("resume");
        let path = dir.join(LOG_FILE_NAME);
        std::fs::write(&path, b"pre-existing\n").unwrap();
        let mut log = RotatingErrorLog::new(path.clone(), 24);
        log.write_all(b"appended\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "pre-existing\nappended\n",
            "the existing log keeps growing until the cap"
        );

        // The cap is checked before each write: 22 bytes fit under 24, so the
        // third line still appends; the next write starts past the cap and
        // moves the whole file into the backup.
        log.write_all(b"trigger\n").unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "pre-existing\nappended\ntrigger\n",
            "a write that starts under the cap still lands in the active log"
        );
        log.write_all(b"rotated\n").unwrap();
        let backup = std::fs::read_to_string(dir.join(BACKUP_FILE_NAME)).unwrap();
        assert!(backup.contains("pre-existing"));
        assert!(backup.contains("appended"));
        assert!(backup.contains("trigger"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "rotated\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failing_sink_reports_success_so_the_writer_thread_never_panics() {
        let dir = temp_dir("sink");
        // A directory at the log path makes every open fail.
        let path = dir.join(LOG_FILE_NAME);
        std::fs::create_dir_all(&path).unwrap();
        let mut log = RotatingErrorLog::new(path, 1024);
        assert_eq!(log.write(b"ignored\n").unwrap(), 8, "errors are swallowed");
        log.flush().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
