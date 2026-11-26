use super::*;

#[cfg(windows)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowsFileIdentity {
    path: PathBuf,
    creation_time: u64,
}

#[cfg(windows)]
pub(crate) const MAX_WINDOWS_ACL_CACHE_ENTRIES: usize = 32;

#[cfg(windows)]
pub(crate) static WINDOWS_ACL_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::VecDeque<WindowsFileIdentity>>,
> = std::sync::OnceLock::new();

#[cfg(windows)]
pub(crate) fn windows_acl_cache()
-> &'static std::sync::Mutex<std::collections::VecDeque<WindowsFileIdentity>> {
    WINDOWS_ACL_CACHE.get_or_init(|| {
        std::sync::Mutex::new(std::collections::VecDeque::with_capacity(
            MAX_WINDOWS_ACL_CACHE_ENTRIES,
        ))
    })
}

#[cfg(windows)]
pub(crate) fn windows_file_identity(
    path: &Path,
    metadata: &fs::Metadata,
) -> Option<WindowsFileIdentity> {
    use std::os::windows::fs::MetadataExt;

    let creation_time = metadata.creation_time();
    (creation_time != 0).then(|| WindowsFileIdentity {
        path: path.to_path_buf(),
        creation_time,
    })
}

#[cfg(windows)]
pub(crate) fn windows_acl_was_applied(identity: &WindowsFileIdentity) -> bool {
    let cache = windows_acl_cache();
    let mut entries = match cache.lock() {
        Ok(entries) => entries,
        Err(poisoned) => {
            let mut entries = poisoned.into_inner();
            entries.clear();
            cache.clear_poison();
            return false;
        }
    };
    let Some(index) = entries.iter().position(|entry| entry == identity) else {
        return false;
    };
    let Some(entry) = entries.remove(index) else {
        return false;
    };
    entries.push_back(entry);
    true
}

#[cfg(windows)]
pub(crate) fn remember_windows_acl(identity: WindowsFileIdentity) {
    let cache = windows_acl_cache();
    let mut entries = match cache.lock() {
        Ok(entries) => entries,
        Err(poisoned) => {
            let mut entries = poisoned.into_inner();
            entries.clear();
            cache.clear_poison();
            entries
        }
    };
    if let Some(index) = entries.iter().position(|entry| entry == &identity) {
        let _ = entries.remove(index);
    } else if entries.len() == MAX_WINDOWS_ACL_CACHE_ENTRIES {
        let _ = entries.pop_front();
    }
    entries.push_back(identity);
}

/// Clears only the short-lived ACL memoization used before the listener is ready.
/// Runtime secret rewrites then reapply their ACL even when they reuse a file identity.
#[cfg(windows)]
pub fn clear_windows_acl_cache() {
    let cache = windows_acl_cache();
    let mut entries = match cache.lock() {
        Ok(entries) => entries,
        Err(poisoned) => {
            let entries = poisoned.into_inner();
            cache.clear_poison();
            entries
        }
    };
    entries.clear();
}

#[cfg(windows)]
pub(crate) fn restrict_windows_acl(
    path: &Path,
    directory: bool,
    metadata: &fs::Metadata,
) -> io::Result<()> {
    use std::{
        mem::size_of,
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER, GetLastError, LocalFree},
        Security::Authorization::{
            BuildTrusteeWithSidW, EXPLICIT_ACCESS_W, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW,
            SetNamedSecurityInfoW, TRUSTEE_W,
        },
        Security::{
            DACL_SECURITY_INFORMATION, GetTokenInformation, IsValidSid, NO_INHERITANCE,
            PROTECTED_DACL_SECURITY_INFORMATION, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY,
            TOKEN_USER, TokenUser,
        },
        Storage::FileSystem::FILE_ALL_ACCESS,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let identity = windows_file_identity(path, metadata);
    if identity.as_ref().is_some_and(windows_acl_was_applied) {
        return Ok(());
    }

    struct HandleGuard(windows_sys::Win32::Foundation::HANDLE);

    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: this guard owns the process-token handle returned by OpenProcessToken.
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }

    struct LocalMemoryGuard(*mut std::ffi::c_void);

    impl Drop for LocalMemoryGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the guarded pointer is allocated by SetEntriesInAclW, which documents
                // LocalFree as its matching deallocator.
                unsafe {
                    let _ = LocalFree(self.0);
                }
            }
        }
    }

    // TokenUser data contains one SID and is small. Bound the kernel-reported size before
    // allocating aligned storage for the TOKEN_USER structure and its trailing SID bytes.
    const MAX_TOKEN_USER_INFO_BYTES: u32 = 4096;

    let mut token = null_mut();
    // SAFETY: GetCurrentProcess returns a pseudo-handle valid for OpenProcessToken; OpenProcessToken
    // writes a process-owned token handle into `token` on success.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if token.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned an empty process-token handle",
        ));
    }
    let _token = HandleGuard(token);

    let mut required = 0u32;
    // SAFETY: this is the documented size-query form of GetTokenInformation.
    let query_succeeded =
        unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut required) };
    if query_succeeded != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows unexpectedly returned token-user data for a zero-length buffer",
        ));
    }
    // Read the error immediately because later Windows API calls may replace the thread error.
    // SAFETY: GetLastError is valid on this thread immediately after GetTokenInformation failed.
    let query_error = unsafe { GetLastError() };
    if query_error != ERROR_INSUFFICIENT_BUFFER {
        return Err(io::Error::from_raw_os_error(query_error as i32));
    }
    if required < size_of::<TOKEN_USER>() as u32 || required > MAX_TOKEN_USER_INFO_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned an invalid process-token user size",
        ));
    }

    let word_count = (required as usize).div_ceil(size_of::<usize>());
    let storage_bytes = word_count * size_of::<usize>();
    let mut token_user_storage = Vec::<usize>::new();
    token_user_storage
        .try_reserve_exact(word_count)
        .map_err(|error| {
            io::Error::other(format!("could not allocate token information: {error}"))
        })?;
    token_user_storage.resize(word_count, 0);

    let mut returned = 0u32;
    // SAFETY: storage is aligned for TOKEN_USER, its length is at least the queried size, and
    // GetTokenInformation writes no more than the supplied buffer length.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            token_user_storage.as_mut_ptr().cast(),
            storage_bytes as u32,
            &mut returned,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if returned < size_of::<TOKEN_USER>() as u32 || returned as usize > storage_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned truncated process-token user data",
        ));
    }

    // SAFETY: the successful query wrote a complete TOKEN_USER into aligned storage.
    let token_user = unsafe { &*token_user_storage.as_ptr().cast::<TOKEN_USER>() };
    let sid = token_user.User.Sid;
    // SAFETY: SID points into the successful TOKEN_USER result and remains alive until this
    // function returns. IsValidSid validates that the pointer describes a complete SID.
    if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned an invalid process-token SID",
        ));
    }

    let mut trustee = TRUSTEE_W::default();
    // SAFETY: sid was validated above and remains alive while SetEntriesInAclW copies it.
    unsafe { BuildTrusteeWithSidW(&mut trustee, sid) };
    let access = EXPLICIT_ACCESS_W {
        grfAccessPermissions: FILE_ALL_ACCESS,
        grfAccessMode: SET_ACCESS,
        grfInheritance: if directory {
            SUB_CONTAINERS_AND_OBJECTS_INHERIT
        } else {
            NO_INHERITANCE
        },
        Trustee: trustee,
    };
    let mut acl = null_mut();
    // SAFETY: access contains one initialized trustee backed by the live token SID; SetEntriesInAclW
    // creates a new ACL and returns its allocation through `acl`.
    let acl_error = unsafe { SetEntriesInAclW(1, &access, null(), &mut acl) };
    if acl_error != 0 {
        return Err(io::Error::from_raw_os_error(acl_error as i32));
    }
    if acl.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Windows returned an empty private ACL",
        ));
    }
    let _acl = LocalMemoryGuard(acl.cast());

    let mut wide_path: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide_path.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the ExoRoute data path contains a null character",
        ));
    }
    wide_path.push(0);

    let inheritance = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: wide_path is NUL-terminated; acl is a valid ACL created above. The protected DACL
    // contains only the current user's full-control ACE, with child inheritance for directories.
    let error = unsafe {
        SetNamedSecurityInfoW(
            wide_path.as_mut_ptr(),
            SE_FILE_OBJECT,
            inheritance,
            null_mut(),
            null_mut(),
            acl,
            null(),
        )
    };
    if error == 0 {
        if let Some(identity) = identity {
            remember_windows_acl(identity);
        }
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(error as i32))
    }
}
