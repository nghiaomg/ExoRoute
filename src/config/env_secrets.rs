use super::*;
use std::time::SystemTime;

/// Prefix of the temporary file a secret is written to before it is renamed
/// over the settings file.
const ENV_TEMP_PREFIX: &str = ".env.tmp-";

/// Temporary settings files older than this are treated as orphans left by a
/// process killed between writing a secret and renaming it into place. A live
/// write holds its temporary file for the duration of one small write and one
/// rename, so the threshold cannot delete a concurrent process's file.
const ENV_TEMP_ORPHAN_AGE: Duration = Duration::from_secs(60 * 60);

fn env_temp_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{ENV_TEMP_PREFIX}{}",
        uuid::Uuid::new_v4().simple()
    ))
}

/// Remove temporary settings files left behind by a process that was killed
/// before its rename completed. Returns how many were removed. The caller
/// treats this as best-effort: an orphaned temporary file never holds a
/// committed secret, so failing to remove it must not block startup.
pub(crate) fn cleanup_stale_env_temp_files(app_dir: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for entry in fs::read_dir(app_dir)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(ENV_TEMP_PREFIX)
        {
            continue;
        }
        // `read_dir` reports the link itself, so a symlink placed here is
        // skipped rather than followed.
        if !entry.file_type()?.is_file() {
            continue;
        }
        // A file with an unreadable or future timestamp is left alone: it may
        // belong to a process that is still writing it.
        let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
            continue;
        };
        let Ok(age) = SystemTime::now().duration_since(modified) else {
            continue;
        };
        if age < ENV_TEMP_ORPHAN_AGE {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(removed)
}

pub fn ensure_admin_key(env_file: &Path, configured: Option<String>) -> io::Result<(String, bool)> {
    if let Some(key) = configured.filter(|value| !value.is_empty()) {
        return Ok((key, false));
    }

    let mut secret = [0u8; 32];
    OsRng.fill_bytes(&mut secret);
    let key = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, secret);
    persist_env_value(env_file, "EXOROUTE_ADMIN_KEY", &key)?;
    Ok((key, true))
}

/// Returns the configured master key, creating and persisting a random key on first startup.
pub fn ensure_master_key(
    env_file: &Path,
    configured: Option<[u8; 32]>,
) -> io::Result<([u8; 32], bool)> {
    if let Some(key) = configured {
        return Ok((key, false));
    }

    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, key);
    persist_env_value(env_file, "EXOROUTE_MASTER_KEY", &encoded)?;
    Ok((key, true))
}

pub(crate) fn persist_env_value(path: &Path, name: &str, value: &str) -> io::Result<()> {
    let content = fs::read_to_string(path)?;
    let assignment = format!(
        "{name}=\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let mut found = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        if line.trim_start().starts_with(&format!("{name}=")) {
            if !found {
                lines.push(assignment.clone());
                found = true;
            }
        } else {
            lines.push(line.to_owned());
        }
    }
    if !found {
        lines.push(assignment);
    }
    let mut updated = lines.join("\n");
    updated.push('\n');

    let temp_path = env_temp_path(path);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temp_file = options.open(&temp_path)?;
    if let Err(error) = temp_file
        .write_all(updated.as_bytes())
        .and_then(|()| temp_file.sync_all())
    {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    drop(temp_file);

    if let Err(error) = atomic_replace_file(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    ensure_private_file(path)
}

pub fn remove_env_value(path: &Path, name: &str) -> io::Result<()> {
    let content = fs::read_to_string(path)?;
    let mut updated = content
        .lines()
        .filter(|line| !line.trim_start().starts_with(&format!("{name}=")))
        .collect::<Vec<_>>()
        .join("\n");
    if !updated.is_empty() {
        updated.push('\n');
    }

    let temp_path = env_temp_path(path);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temp_file = options.open(&temp_path)?;
    if let Err(error) = temp_file
        .write_all(updated.as_bytes())
        .and_then(|()| temp_file.sync_all())
    {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    drop(temp_file);

    if let Err(error) = atomic_replace_file(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }
    Ok(())
}
