use super::*;

pub fn app_dir() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let home = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME"));
    #[cfg(not(windows))]
    let home = env::var_os("HOME");

    let home = home.ok_or_else(|| {
        "could not find the user home directory (HOME/USERPROFILE is not set)".to_owned()
    })?;
    Ok(app_dir_from_home(&PathBuf::from(home)))
}

pub fn initialize_app_dir(app_dir: &Path) -> io::Result<()> {
    ensure_private_dir(app_dir)?;
    let env_path = app_dir.join(".env");
    if env_path.exists() && fs::symlink_metadata(&env_path)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "refusing to use a symbolic link as the ExoRoute .env file",
        ));
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let result = match options.open(&env_path) {
        Ok(mut file) => file.write_all(include_bytes!("../../.env.example")),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    };
    result?;
    ensure_private_file(&env_path)
}

#[cfg(unix)]
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    ensure_no_reparse_path_components(path)?;
    use std::os::unix::fs::DirBuilderExt;
    let mut builder = fs::DirBuilder::new();
    match builder.recursive(true).mode(0o700).create(path) {
        Ok(()) => ensure_no_reparse_path_components(path),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::symlink_metadata(path)?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the ExoRoute data path must be a real directory",
                ));
            }
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
            ensure_no_reparse_path_components(path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    ensure_no_reparse_path_components(path)?;
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the ExoRoute data path must be a real directory",
            ));
        }
        restrict_windows_acl(path, true, &metadata)?;
        ensure_no_reparse_path_components(path)
    }
    #[cfg(not(windows))]
    {
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the ExoRoute data path must be a real directory",
            ));
        }
        ensure_no_reparse_path_components(path)
    }
}

pub fn ensure_private_file(path: &Path) -> io::Result<()> {
    ensure_no_reparse_path_components(path)?;
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "an ExoRoute secret or database path must be a regular file",
            ));
        }
        restrict_windows_acl(path, false, &metadata)
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "an ExoRoute secret or database path must be a regular file",
            ));
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(any(unix, windows)))]
    {
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "an ExoRoute secret or database path must be a regular file",
            ));
        }
        Ok(())
    }
}

/// Reject symlinks and Windows reparse points in every existing path component.
/// Checking only the final component is insufficient because filesystem APIs
/// follow links in parent directories before inspecting the leaf.
pub fn ensure_no_reparse_path_components(path: &Path) -> io::Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                current.push(component.as_os_str());
            }
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                current.pop();
                continue;
            }
            std::path::Component::Normal(part) => current.push(part),
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                let is_link = metadata.file_type().is_symlink();
                #[cfg(windows)]
                let is_reparse = {
                    use std::os::windows::fs::MetadataExt;
                    metadata.file_attributes() & 0x400 != 0
                };
                #[cfg(not(windows))]
                let is_reparse = false;
                if is_link || is_reparse {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "the ExoRoute data path must not contain a symlink or reparse point",
                    ));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(crate) fn resolve_path(base: &Path, value: String) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

#[cfg(not(windows))]
pub(crate) fn atomic_replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
pub(crate) fn atomic_replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // The temporary source is unique to this process and remains beside the
    // destination, so Windows can replace the secret atomically in place.
    let replaced = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(crate) fn app_dir_from_home(home: &Path) -> PathBuf {
    home.join(".exoroute")
}
