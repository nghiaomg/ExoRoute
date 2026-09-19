use super::*;

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

    let temp_path = path.with_file_name(format!(".env.tmp-{}", uuid::Uuid::new_v4().simple()));
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

    let temp_path = path.with_file_name(format!(".env.tmp-{}", uuid::Uuid::new_v4().simple()));
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
