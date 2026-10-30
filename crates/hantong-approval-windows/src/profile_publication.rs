use hantong_contracts::ErrorCode;
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr;
use windows_sys::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, GetLastError, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce,
    GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
    IsValidAcl, IsValidSecurityDescriptor, IsValidSid, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, DELETE, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO,
    FILE_RENAME_INFO_0, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FileDispositionInfo,
    FileRenameInfo, FlushFileBuffers, READ_CONTROL, SetFileInformationByHandle,
};
use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;

const MAX_PROFILE_BYTES: usize = 32 * 1024;

pub(super) struct Publication {
    _file: File,
    _directories: Vec<File>,
    backup: Option<File>,
}

impl std::fmt::Debug for Publication {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Publication { <redacted> }")
    }
}

pub(super) fn publish(bytes: &[u8]) -> Result<Publication, ErrorCode> {
    publish_at(&hantong_platform::app_directories()?.profiles, bytes)
}

pub(super) fn publish_gui(bytes: &[u8]) -> Result<Publication, ErrorCode> {
    publish_fixed(
        &hantong_platform::app_directories()?.profiles,
        bytes,
        ProfileFile::Gui,
    )
}
pub(super) fn publish_mcp(bytes: &[u8]) -> Result<Publication, ErrorCode> {
    publish_fixed(
        &hantong_platform::app_directories()?.profiles,
        bytes,
        ProfileFile::Mcp,
    )
}

enum ProfileFile {
    Default,
    Gui,
    Mcp,
}
impl ProfileFile {
    fn name(&self) -> &'static str {
        match self {
            Self::Default => "default.json",
            Self::Gui => "gui.json",
            Self::Mcp => "mcp.json",
        }
    }
}

fn publish_at(profiles: &Path, bytes: &[u8]) -> Result<Publication, ErrorCode> {
    publish_fixed(profiles, bytes, ProfileFile::Default)
}
fn publish_fixed(
    profiles: &Path,
    bytes: &[u8],
    selected: ProfileFile,
) -> Result<Publication, ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_PROFILE_BYTES {
        return Err(ErrorCode::InvalidInput);
    }
    validate_path(profiles)?;
    let identity = hantong_platform::current_peer_context()?;
    let user = identity.user_sid();
    let security = creation_security(user)?;
    let directories = open_profile_directories(profiles, user, &security)?;
    let destination = profiles.join(selected.name());
    if let Some(file) = existing(&destination, bytes, user)? {
        return Ok(Publication {
            _file: file,
            _directories: directories,
            backup: None,
        });
    }
    let mut temporary = create_temporary(profiles, &security)?;
    let file = temporary.0.as_mut().ok_or(ErrorCode::ScopeDenied)?;
    check_type(file, false)?;
    check_security(file, user, true)?;
    file.write_all(bytes).map_err(|_| ErrorCode::ScopeDenied)?;
    // SAFETY: source handle remains open with write access. Flush completes
    // before the namespace rename; this is not a cross-Store transaction.
    if unsafe { FlushFileBuffers(file.as_raw_handle()) } == 0 {
        return Err(ErrorCode::ScopeDenied);
    }
    if rename_no_replace(file, &destination)? {
        let file = temporary.0.take().ok_or(ErrorCode::ScopeDenied)?;
        Ok(Publication {
            _file: file,
            _directories: directories,
            backup: None,
        })
    } else {
        let file = existing(&destination, bytes, user)?.ok_or(ErrorCode::Conflict)?;
        Ok(Publication {
            _file: file,
            _directories: directories,
            backup: None,
        })
    }
}

fn open_profile_directories(
    profiles: &Path,
    user: &str,
    security: &LocalAllocation,
) -> Result<Vec<File>, ErrorCode> {
    let product = profiles.parent().ok_or(ErrorCode::ScopeDenied)?;
    let mut directories = Vec::new();
    let mut current = PathBuf::new();
    for component in profiles.components() {
        current.push(component);
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let trusted = current == product || current == profiles;
        if trusted {
            let name = wide_path(&current)?;
            let attributes = security.attributes();
            // SAFETY: the SD and UTF-16 path live through this call. CREATE is
            // limited to the two fixed product directories, never ancestors.
            if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0
                && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS
            {
                return Err(ErrorCode::ScopeDenied);
            }
        }
        let directory = OpenOptions::new()
            .access_mode(
                FILE_READ_ATTRIBUTES | FILE_TRAVERSE | if trusted { READ_CONTROL } else { 0 },
            )
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&current)
            .map_err(|_| ErrorCode::ScopeDenied)?;
        check_type(&directory, true)?;
        if trusted {
            check_security(&directory, user, false)?;
        }
        directories.push(directory);
    }
    Ok(directories)
}

/// Replaces the fixed default profile only after proving the exact old bytes.
/// The old object is renamed through its DELETE-capable handle; it is never
/// closed and reopened by path. The returned publication retains both objects
/// until activation calls `cleanup_backup`.
pub(super) fn refresh(
    refresh_id: [u8; 32],
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<Publication, ErrorCode> {
    refresh_at(
        &hantong_platform::app_directories()?.profiles,
        refresh_id,
        old_bytes,
        new_bytes,
    )
}

fn refresh_at(
    profiles: &Path,
    refresh_id: [u8; 32],
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<Publication, ErrorCode> {
    refresh_fixed_at(
        profiles,
        refresh_id,
        old_bytes,
        new_bytes,
        ProfileFile::Default,
    )
}
pub(super) fn refresh_mcp(
    refresh_id: [u8; 32],
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<Publication, ErrorCode> {
    refresh_fixed_at(
        &hantong_platform::app_directories()?.profiles,
        refresh_id,
        old_bytes,
        new_bytes,
        ProfileFile::Mcp,
    )
}
fn refresh_fixed_at(
    profiles: &Path,
    refresh_id: [u8; 32],
    old_bytes: &[u8],
    new_bytes: &[u8],
    selected: ProfileFile,
) -> Result<Publication, ErrorCode> {
    if refresh_id == [0; 32]
        || old_bytes.is_empty()
        || old_bytes.len() > MAX_PROFILE_BYTES
        || new_bytes.is_empty()
        || new_bytes.len() > MAX_PROFILE_BYTES
        || old_bytes == new_bytes
    {
        return Err(ErrorCode::InvalidInput);
    }
    validate_path(profiles)?;
    let identity = hantong_platform::current_peer_context()?;
    let user = identity.user_sid();
    let security = creation_security(user)?;
    let directories = open_profile_directories(profiles, user, &security)?;

    let source_path = profiles.join(selected.name());
    let backup_path = profiles.join(format!(
        "{}.refresh-{}.bak",
        selected.name().trim_end_matches(".json"),
        hantong_contracts::wire_hex::encode(&refresh_id)
    ));
    let mut current = open_refresh_file(&source_path, user)?;
    let mut backup = open_refresh_file(&backup_path, user)?;
    match (current.is_some(), backup.is_some()) {
        (true, false) => {
            let default = current.as_ref().ok_or(ErrorCode::InternalError)?;
            let bytes = read_profile(default)?;
            if bytes == new_bytes {
                let file = current.take().ok_or(ErrorCode::InternalError)?;
                return Ok(Publication {
                    _file: file,
                    _directories: directories,
                    backup: None,
                });
            }
            if bytes != old_bytes {
                return Err(ErrorCode::Conflict);
            }
            if !rename_no_replace(default, &backup_path)? {
                return Err(ErrorCode::Conflict);
            }
            backup = current.take();
        }
        (false, true) => {
            let old = backup.as_ref().ok_or(ErrorCode::InternalError)?;
            if read_profile(old)? != old_bytes {
                return Err(ErrorCode::Conflict);
            }
        }
        (true, true) => {
            let default = current.as_ref().ok_or(ErrorCode::InternalError)?;
            let saved = backup.as_ref().ok_or(ErrorCode::InternalError)?;
            let default_bytes = read_profile(default)?;
            let saved_bytes = read_profile(saved)?;
            if default_bytes != new_bytes || saved_bytes != old_bytes {
                return Err(ErrorCode::Conflict);
            }
            let file = current.take().ok_or(ErrorCode::InternalError)?;
            return Ok(Publication {
                _file: file,
                _directories: directories,
                backup,
            });
        }
        (false, false) => return Err(ErrorCode::NotFound),
    }

    let file = if let Some(file) = current.take() {
        if read_profile(&file)? != new_bytes {
            return Err(ErrorCode::Conflict);
        }
        file
    } else {
        let mut temporary = create_temporary(profiles, &security)?;
        let file = temporary.0.as_mut().ok_or(ErrorCode::ScopeDenied)?;
        check_type(file, false)?;
        check_security(file, user, true)?;
        file.write_all(new_bytes)
            .map_err(|_| ErrorCode::ScopeDenied)?;
        if unsafe { FlushFileBuffers(file.as_raw_handle()) } == 0 {
            return Err(ErrorCode::ScopeDenied);
        }
        if !rename_no_replace(file, &source_path)? {
            return Err(ErrorCode::Conflict);
        }
        let file = temporary.0.take().ok_or(ErrorCode::ScopeDenied)?;
        if unsafe { FlushFileBuffers(file.as_raw_handle()) } == 0 {
            return Err(ErrorCode::ScopeDenied);
        }
        file
    };
    Ok(Publication {
        _file: file,
        _directories: directories,
        backup,
    })
}

fn open_refresh_file(path: &Path, user: &str) -> Result<Option<File>, ErrorCode> {
    let file = match OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | DELETE | READ_CONTROL)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ErrorCode::ScopeDenied),
    };
    check_type(&file, false)?;
    check_security(&file, user, true)?;
    Ok(Some(file))
}

fn read_profile(file: &File) -> Result<Vec<u8>, ErrorCode> {
    let mut actual = Vec::new();
    file.take((MAX_PROFILE_BYTES + 1) as u64)
        .read_to_end(&mut actual)
        .map_err(|_| ErrorCode::ScopeDenied)?;
    if actual.len() > MAX_PROFILE_BYTES {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(actual)
}

pub(super) fn cleanup_backup(publication: &mut Publication) -> Result<(), ErrorCode> {
    let Some(file) = publication.backup.take() else {
        return Ok(());
    };
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    let success = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    drop(file);
    if success == 0 {
        Err(ErrorCode::ScopeDenied)
    } else {
        Ok(())
    }
}

fn existing(path: &Path, expected: &[u8], user: &str) -> Result<Option<File>, ErrorCode> {
    let file = match OpenOptions::new()
        .access_mode(FILE_GENERIC_READ | READ_CONTROL)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ErrorCode::ScopeDenied),
    };
    check_type(&file, false)?;
    check_security(&file, user, true)?;
    if file.metadata().map_err(|_| ErrorCode::ScopeDenied)?.len() != expected.len() as u64 {
        return Err(ErrorCode::Conflict);
    }
    let mut actual = Vec::new();
    (&file)
        .take((MAX_PROFILE_BYTES + 1) as u64)
        .read_to_end(&mut actual)
        .map_err(|_| ErrorCode::ScopeDenied)?;
    if actual != expected {
        return Err(ErrorCode::Conflict);
    }
    Ok(Some(file))
}

struct Temporary(Option<File>);
impl Drop for Temporary {
    fn drop(&mut self) {
        if let Some(file) = &self.0 {
            let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
            // SAFETY: this still-owned CREATE_NEW handle has DELETE access.
            // Never close it then delete a possibly replaced path. A failed
            // cleanup leaves only an inert temp name, never default.json.
            unsafe {
                SetFileInformationByHandle(
                    file.as_raw_handle(),
                    FileDispositionInfo,
                    (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                    std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
                );
            }
        }
    }
}

fn create_temporary(profiles: &Path, security: &LocalAllocation) -> Result<Temporary, ErrorCode> {
    for _ in 0..8 {
        let nonce = hantong_platform::random_nonce()?;
        let hex = hantong_contracts::wire_hex::encode(&nonce);
        let name = wide_path(&profiles.join(format!("publish-{hex}.tmp")))?;
        let attributes = security.attributes();
        // SAFETY: paths and SD are live; CREATE_NEW never follows an existing
        // name. File receives sole ownership of the successful native handle.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | READ_CONTROL,
                FILE_SHARE_READ,
                &attributes,
                CREATE_NEW,
                FILE_FLAG_OPEN_REPARSE_POINT,
                ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            return Ok(Temporary(Some(unsafe { File::from_raw_handle(handle) })));
        }
        if !matches!(
            unsafe { GetLastError() },
            ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS
        ) {
            return Err(ErrorCode::ScopeDenied);
        }
    }
    Err(ErrorCode::Conflict)
}

fn rename_no_replace(file: &File, destination: &Path) -> Result<bool, ErrorCode> {
    let name = wide_path(destination)?;
    let size = std::mem::size_of::<FILE_RENAME_INFO>() + name.len() * std::mem::size_of::<u16>();
    // The variable-sized structure must be pointer-aligned, not Vec<u8> aligned.
    let mut buffer = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
    // SAFETY: allocation covers the complete header plus UTF-16 name and is
    // suitably aligned. RootDirectory=NULL uses the fixed absolute destination;
    // all ancestors and the source remain held throughout this call.
    let success = unsafe {
        let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        (*info).Anonymous = FILE_RENAME_INFO_0 {
            ReplaceIfExists: false,
        };
        (*info).RootDirectory = ptr::null_mut();
        (*info).FileNameLength = ((name.len() - 1) * std::mem::size_of::<u16>()) as u32;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            ptr::addr_of_mut!((*info).FileName).cast(),
            name.len(),
        );
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            info.cast(),
            size as u32,
        )
    };
    if success != 0 {
        return Ok(true);
    }
    if matches!(
        unsafe { GetLastError() },
        ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS
    ) {
        Ok(false)
    } else {
        Err(ErrorCode::ScopeDenied)
    }
}

fn wide_path(path: &Path) -> Result<Vec<u16>, ErrorCode> {
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.is_empty() || wide.len() > 32_767 || wide.contains(&0) {
        return Err(ErrorCode::ScopeDenied);
    }
    wide.push(0);
    Ok(wide)
}

fn creation_security(user: &str) -> Result<LocalAllocation, ErrorCode> {
    // No inherited entries: directory creation and file creation each receive
    // an explicit protected ACL and the current user as owner.
    let sddl = format!("O:{user}D:P(A;;FA;;;{user})(A;;FA;;;SY)(A;;FA;;;BA)");
    let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = ptr::null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            ptr::null_mut(),
        )
    } == 0
        || descriptor.is_null()
    {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(LocalAllocation(descriptor))
}

impl LocalAllocation {
    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0,
            bInheritHandle: 0,
        }
    }
}

fn check_security(file: &File, user: &str, require_protected: bool) -> Result<(), ErrorCode> {
    let mut descriptor = ptr::null_mut();
    // SAFETY: handle is live with READ_CONTROL; descriptor is LocalFree-owned.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    let descriptor = LocalAllocation(descriptor);
    if status != 0 || descriptor.0.is_null() {
        return Err(ErrorCode::ScopeDenied);
    }
    unsafe {
        validate_descriptor(descriptor.0, user, require_protected)?;
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<(), ErrorCode> {
    let mut parts = path.components();
    if !matches!(parts.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
        || !matches!(parts.next(), Some(Component::RootDir))
        || parts.any(|part| !matches!(part, Component::Normal(_)))
        || path.file_name() != Some(std::ffi::OsStr::new("profiles"))
        || path.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new("mail-hantong"))
    {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}

fn check_type(file: &File, directory: bool) -> Result<(), ErrorCode> {
    let metadata = file.metadata().map_err(|_| ErrorCode::ScopeDenied)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        }
    {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}

struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: only allocations returned by the Win32 security APIs are
        // wrapped here, and the allocation has exactly one owner.
        unsafe {
            LocalFree(self.0);
        }
    }
}

// Only the current user, SYSTEM, and local Administrators may own or appear
// in the allow-list DACL. Files must protect inheritance; directories may inherit
// a safe current allow-list. Unknown/conditional/object/deny ACEs are
// deliberately rejected rather than attempting broader access evaluation.
// This is not isolation from a malicious process running as the same user.
unsafe fn validate_descriptor(
    sd: PSECURITY_DESCRIPTOR,
    user: &str,
    require_protected: bool,
) -> Result<(), ErrorCode> {
    // SAFETY: caller provides a live OS-allocated descriptor. Each structure
    // is validated before traversal, and ACE/SID bounds are checked below.
    unsafe {
        if sd.is_null() || IsValidSecurityDescriptor(sd) == 0 {
            return Err(ErrorCode::ScopeDenied);
        }
        let mut control = 0;
        let mut revision = 0;
        let mut owner = ptr::null_mut();
        let mut defaulted = 0;
        let mut present = 0;
        let mut acl: *mut ACL = ptr::null_mut();
        if GetSecurityDescriptorControl(sd, &mut control, &mut revision) == 0
            || (require_protected && control & SE_DACL_PROTECTED == 0)
            || GetSecurityDescriptorOwner(sd, &mut owner, &mut defaulted) == 0
            || !allowed_sid(owner, user)?
            || GetSecurityDescriptorDacl(sd, &mut present, &mut acl, &mut defaulted) == 0
            || present == 0
            || acl.is_null()
            || IsValidAcl(acl) == 0
        {
            return Err(ErrorCode::ScopeDenied);
        }
        if (*acl).AceCount == 0 || (*acl).AceCount > 64 {
            return Err(ErrorCode::ScopeDenied);
        }
        let start = acl as usize;
        let end = start
            .checked_add((*acl).AclSize as usize)
            .ok_or(ErrorCode::ScopeDenied)?;
        for index in 0..(*acl).AceCount as u32 {
            let mut raw = ptr::null_mut();
            if GetAce(acl, index, &mut raw) == 0 || raw.is_null() {
                return Err(ErrorCode::ScopeDenied);
            }
            let address = raw as usize;
            if address < start + std::mem::size_of::<ACL>()
                || address
                    .checked_add(std::mem::size_of::<ACE_HEADER>())
                    .is_none_or(|n| n > end)
            {
                return Err(ErrorCode::ScopeDenied);
            }
            let header = ptr::read_unaligned(raw.cast::<ACE_HEADER>());
            let sid_offset = std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
            if header.AceType != ACCESS_ALLOWED_ACE_TYPE as u8
                || (header.AceSize as usize) < sid_offset + 8
                || address
                    .checked_add(header.AceSize as usize)
                    .is_none_or(|n| n > end)
            {
                return Err(ErrorCode::ScopeDenied);
            }
            let sid = raw.cast::<u8>().add(sid_offset);
            let count = *sid.add(1) as usize;
            if count > 15
                || sid_offset + 8 + 4 * count > header.AceSize as usize
                || !allowed_sid(sid.cast(), user)?
            {
                return Err(ErrorCode::ScopeDenied);
            }
        }
        Ok(())
    }
}

unsafe fn allowed_sid(sid: PSID, user: &str) -> Result<bool, ErrorCode> {
    // SAFETY: caller bounds-checks ACE SIDs or gets the owner from a valid
    // descriptor. Conversion returns its own terminated LocalFree buffer.
    unsafe {
        if sid.is_null() || IsValidSid(sid) == 0 {
            return Err(ErrorCode::ScopeDenied);
        }
        let mut text = ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 || text.is_null() {
            return Err(ErrorCode::ScopeDenied);
        }
        let _allocation = LocalAllocation(text.cast());
        let mut length = 0;
        while length < 184 && *text.add(length) != 0 {
            length += 1;
        }
        if length == 184 {
            return Err(ErrorCode::ScopeDenied);
        }
        let value = String::from_utf16(std::slice::from_raw_parts(text, length))
            .map_err(|_| ErrorCode::ScopeDenied)?;
        Ok(value == user || value == "S-1-5-18" || value == "S-1-5-32-544")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::path::PathBuf;
    use std::ptr;
    use std::sync::atomic::{AtomicU64, Ordering};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, SetFileSecurityW,
        UNPROTECTED_DACL_SECURITY_INFORMATION,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, READ_CONTROL,
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        product: PathBuf,
        profiles: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let base = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .unwrap()
                .join(".local/profile-publication-tests");
            fs::create_dir_all(&base).unwrap();
            let root = base.join(format!(
                "{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let product = root.join("mail-hantong");
            let profiles = product.join("profiles");
            Self {
                root,
                product,
                profiles,
            }
        }
        fn dirs(&self, protected: bool) {
            fs::create_dir(&self.product).unwrap();
            secure(&self.product, false, protected);
            fs::create_dir(&self.profiles).unwrap();
            secure(&self.profiles, false, protected);
        }
        fn existing(&self, bytes: &[u8], protected: bool) -> PathBuf {
            let file = self.profiles.join("default.json");
            fs::write(&file, bytes).unwrap();
            secure(&file, false, protected);
            file
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // Remove only exact owned entries non-recursively. Never follow a
            // reparse directory when cleaning an adversarial fixture.
            if let Ok(meta) = fs::symlink_metadata(&self.profiles) {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes()
                    & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                    == 0
                    && let Ok(entries) = fs::read_dir(&self.profiles)
                {
                    for entry in entries.flatten() {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
            for path in [
                &self.profiles,
                &self.product,
                &self.root.join("renamed"),
                &self.root.join("other"),
                &self.root,
            ] {
                let _ = fs::remove_dir(path);
            }
        }
    }

    fn secure(path: &Path, foreign: bool, protected: bool) {
        let identity = hantong_platform::current_peer_context().unwrap();
        let extra = if foreign { "(A;;FR;;;WD)" } else { "" };
        let protect = if protected { "P" } else { "" };
        let sddl = format!(
            "D:{protect}(A;;FA;;;{})(A;;FA;;;SY)(A;;FA;;;BA){extra}",
            identity.user_sid()
        );
        let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut sd = ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    ptr::null_mut(),
                )
            },
            0
        );
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let flags = DACL_SECURITY_INFORMATION
            | if protected {
                PROTECTED_DACL_SECURITY_INFORMATION
            } else {
                UNPROTECTED_DACL_SECURITY_INFORMATION
            };
        let status = unsafe { SetFileSecurityW(wide.as_ptr(), flags, sd) };
        unsafe {
            LocalFree(sd);
        }
        assert_ne!(status, 0);
    }

    #[test]
    fn inst04_profile_publication_creates_only_fixed_file_with_complete_bytes() {
        let fixture = Fixture::new();
        let bytes = br#"{"public_fixture":1}"#;
        let published = publish_at(&fixture.profiles, bytes).unwrap();
        assert_eq!(format!("{published:?}"), "Publication { <redacted> }");
        drop(published);
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            bytes
        );
        assert_eq!(fs::read_dir(&fixture.profiles).unwrap().count(), 1);
    }

    #[test]
    fn ai09_profile_publication_guard_prevents_mutation_until_activation() {
        let fixture = Fixture::new();
        let published = publish_at(&fixture.profiles, b"public").unwrap();
        let path = fixture.profiles.join("default.json");
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::remove_file(&path).is_err());
        assert!(fs::rename(&fixture.product, fixture.root.join("renamed")).is_err());
        drop(published);
        assert!(OpenOptions::new().write(true).open(path).is_ok());
    }

    #[test]
    fn ai09_profile_publication_recovers_only_exact_existing_bytes() {
        let fixture = Fixture::new();
        drop(publish_at(&fixture.profiles, b"canonical").unwrap());
        let recovered = publish_at(&fixture.profiles, b"canonical").unwrap();
        assert!(
            OpenOptions::new()
                .write(true)
                .open(fixture.profiles.join("default.json"))
                .is_err()
        );
        drop(recovered);
        assert_eq!(
            publish_at(&fixture.profiles, b"different").unwrap_err(),
            ErrorCode::Conflict
        );
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"canonical"
        );
    }

    #[test]
    fn inst04_profile_publication_rejects_empty_and_oversized_before_creation() {
        let fixture = Fixture::new();
        for bytes in [Vec::new(), vec![b'x'; MAX_PROFILE_BYTES + 1]] {
            assert_eq!(
                publish_at(&fixture.profiles, &bytes).unwrap_err(),
                ErrorCode::InvalidInput
            );
            assert!(!fixture.product.exists());
        }
        drop(publish_at(&fixture.profiles, &vec![b'x'; MAX_PROFILE_BYTES]).unwrap());
    }

    #[test]
    fn cli15_scope_refresh_moves_exact_old_profile_and_retains_backup_until_cleanup() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        let old = br#"{"profile_epoch":1,"account_ids":["old"]}"#;
        let new = br#"{"profile_epoch":2,"account_ids":["new"]}"#;
        fixture.existing(old, true);
        let id = [7; 32];
        let mut published = refresh_at(&fixture.profiles, id, old, new).unwrap();
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            new
        );
        let backup = fixture.profiles.join(format!(
            "default.refresh-{}.bak",
            hantong_contracts::wire_hex::encode(&id)
        ));
        assert_eq!(fs::read(&backup).unwrap(), old);
        assert!(cleanup_backup(&mut published).is_ok());
        assert!(!backup.exists());
        drop(published);
        // Exact new bytes with no backup are a permitted recovery state too.
        let mut recovered = refresh_at(&fixture.profiles, id, old, new).unwrap();
        assert!(
            OpenOptions::new()
                .write(true)
                .open(fixture.profiles.join("default.json"))
                .is_err()
        );
        cleanup_backup(&mut recovered).unwrap();
        drop(recovered);
        assert_eq!(fs::read_dir(&fixture.profiles).unwrap().count(), 1);
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            new
        );
    }

    #[test]
    fn cli15_scope_refresh_rejects_stale_or_duplicate_source_without_mutation() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        fixture.existing(b"old", true);
        assert_eq!(
            refresh_at(&fixture.profiles, [0; 32], b"old", b"new").unwrap_err(),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            refresh_at(&fixture.profiles, [1; 32], b"wrong", b"new").unwrap_err(),
            ErrorCode::Conflict
        );
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"old"
        );
        let backup = fixture.profiles.join(format!(
            "default.refresh-{}.bak",
            hantong_contracts::wire_hex::encode(&[2; 32])
        ));
        fs::write(&backup, b"occupied").unwrap();
        secure(&backup, false, true);
        assert_eq!(
            refresh_at(&fixture.profiles, [2; 32], b"old", b"new").unwrap_err(),
            ErrorCode::Conflict
        );
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"old"
        );
    }

    #[test]
    fn cli15_scope_refresh_recovers_missing_default_from_exact_old_backup() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        fixture.existing(b"old", true);
        let id = [8; 32];
        drop(refresh_at(&fixture.profiles, id, b"old", b"new").unwrap());
        fs::remove_file(fixture.profiles.join("default.json")).unwrap();
        let mut recovered = refresh_at(&fixture.profiles, id, b"old", b"new").unwrap();
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"new"
        );
        cleanup_backup(&mut recovered).unwrap();
    }

    #[test]
    fn cli15_scope_refresh_recovers_new_default_with_old_backup_without_republish() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        fixture.existing(b"old", true);
        let id = [9; 32];
        drop(refresh_at(&fixture.profiles, id, b"old", b"new").unwrap());
        let mut recovered = refresh_at(&fixture.profiles, id, b"old", b"new").unwrap();
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"new"
        );
        cleanup_backup(&mut recovered).unwrap();
    }

    #[test]
    fn cli15_scope_refresh_rejects_wrong_backup_even_when_new_default_exists() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        fixture.existing(b"old", true);
        let id = [14; 32];
        drop(refresh_at(&fixture.profiles, id, b"old", b"new").unwrap());
        let backup = fixture.profiles.join(format!(
            "default.refresh-{}.bak",
            hantong_contracts::wire_hex::encode(&id)
        ));
        fs::write(&backup, b"wrong").unwrap();
        secure(&backup, false, true);
        assert_eq!(
            refresh_at(&fixture.profiles, id, b"old", b"new").unwrap_err(),
            ErrorCode::Conflict
        );
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"new"
        );
    }

    #[test]
    fn cli15_scope_refresh_rejects_missing_both_and_both_old() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        fixture.existing(b"old", true);
        let id = [10; 32];
        drop(refresh_at(&fixture.profiles, id, b"old", b"new").unwrap());
        fs::remove_file(fixture.profiles.join("default.json")).unwrap();
        let backup = fixture.profiles.join(format!(
            "default.refresh-{}.bak",
            hantong_contracts::wire_hex::encode(&id)
        ));
        fs::remove_file(&backup).unwrap();
        assert_eq!(
            refresh_at(&fixture.profiles, id, b"old", b"new").unwrap_err(),
            ErrorCode::NotFound
        );

        fixture.existing(b"old", true);
        fs::write(&backup, b"old").unwrap();
        secure(&backup, false, true);
        assert_eq!(
            refresh_at(&fixture.profiles, id, b"old", b"new").unwrap_err(),
            ErrorCode::Conflict
        );
    }

    #[test]
    fn cli15_scope_refresh_reader_blocks_move_and_new_guard_blocks_mutation() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        fixture.existing(b"old", true);
        let reader = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(fixture.profiles.join("default.json"))
            .unwrap();
        assert_eq!(
            refresh_at(&fixture.profiles, [13; 32], b"old", b"new").unwrap_err(),
            ErrorCode::ScopeDenied
        );
        drop(reader);
        let mut published = refresh_at(&fixture.profiles, [13; 32], b"old", b"new").unwrap();
        let path = fixture.profiles.join("default.json");
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::remove_file(&path).is_err());
        cleanup_backup(&mut published).unwrap();
    }

    #[test]
    fn ai09_profile_publication_accepts_only_safe_directory_acls() {
        let fixture = Fixture::new();
        fixture.dirs(false);
        drop(publish_at(&fixture.profiles, b"public").unwrap());
        // Reuse accepts the same unchanged directories and exact file.
        drop(publish_at(&fixture.profiles, b"public").unwrap());
        let unsafe_fixture = Fixture::new();
        unsafe_fixture.dirs(false);
        secure(&unsafe_fixture.product, true, false);
        assert_eq!(
            publish_at(&unsafe_fixture.profiles, b"public").unwrap_err(),
            ErrorCode::ScopeDenied
        );
        assert!(!unsafe_fixture.profiles.join("default.json").exists());
    }

    #[test]
    fn sec03_gui_profile_publication_coexists_without_replacing_cli_or_gui_bytes() {
        let fixture = Fixture::new();
        let default = publish_at(&fixture.profiles, b"cli-public").unwrap();
        let gui = publish_fixed(&fixture.profiles, b"gui-public", ProfileFile::Gui).unwrap();
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"cli-public"
        );
        assert_eq!(
            fs::read(fixture.profiles.join("gui.json")).unwrap(),
            b"gui-public"
        );
        // Initial publication still owns a write-capable guard. A second
        // protected reader must fail until activation releases that guard.
        assert_eq!(
            publish_fixed(&fixture.profiles, b"gui-public", ProfileFile::Gui).unwrap_err(),
            ErrorCode::ScopeDenied
        );
        drop(gui);
        let gui = publish_fixed(&fixture.profiles, b"gui-public", ProfileFile::Gui).unwrap();
        assert_eq!(
            publish_fixed(&fixture.profiles, b"different", ProfileFile::Gui).unwrap_err(),
            ErrorCode::Conflict
        );
        assert_eq!(
            fs::read(fixture.profiles.join("gui.json")).unwrap(),
            b"gui-public"
        );
        assert!(
            OpenOptions::new()
                .write(true)
                .open(fixture.profiles.join("gui.json"))
                .is_err()
        );
        assert!(fs::remove_file(fixture.profiles.join("gui.json")).is_err());
        assert!(fs::rename(&fixture.profiles, fixture.root.join("renamed")).is_err());
        drop(gui);
        drop(default);
    }

    #[test]
    fn sec03_mcp_profile_publication_coexists_without_replacing_cli_or_mcp_bytes() {
        let fixture = Fixture::new();
        let default = publish_at(&fixture.profiles, b"cli-public").unwrap();
        let mcp = publish_fixed(&fixture.profiles, b"mcp-public", ProfileFile::Mcp).unwrap();
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"cli-public"
        );
        assert_eq!(
            fs::read(fixture.profiles.join("mcp.json")).unwrap(),
            b"mcp-public"
        );
        // Initial publication still owns a write-capable guard. A second
        // protected reader must fail until activation releases that guard.
        assert_eq!(
            publish_fixed(&fixture.profiles, b"mcp-public", ProfileFile::Mcp).unwrap_err(),
            ErrorCode::ScopeDenied
        );
        drop(mcp);
        let mcp = publish_fixed(&fixture.profiles, b"mcp-public", ProfileFile::Mcp).unwrap();
        assert_eq!(
            publish_fixed(&fixture.profiles, b"different", ProfileFile::Mcp).unwrap_err(),
            ErrorCode::Conflict
        );
        assert_eq!(
            fs::read(fixture.profiles.join("mcp.json")).unwrap(),
            b"mcp-public"
        );
        assert!(
            OpenOptions::new()
                .write(true)
                .open(fixture.profiles.join("mcp.json"))
                .is_err()
        );
        assert!(fs::remove_file(fixture.profiles.join("mcp.json")).is_err());
        assert!(fs::rename(&fixture.profiles, fixture.root.join("renamed")).is_err());
        drop(mcp);
        drop(default);
    }
    #[test]
    fn sec03_mcp_replacement_recovers_exact_backup_without_touching_cli() {
        let fixture = Fixture::new();
        drop(publish_at(&fixture.profiles, b"cli").unwrap());
        drop(publish_fixed(&fixture.profiles, b"legacy-mcp", ProfileFile::Mcp).unwrap());
        let id = [39; 32];
        drop(
            refresh_fixed_at(
                &fixture.profiles,
                id,
                b"legacy-mcp",
                b"full-mcp",
                ProfileFile::Mcp,
            )
            .unwrap(),
        );
        fs::remove_file(fixture.profiles.join("mcp.json")).unwrap();
        let mut published = refresh_fixed_at(
            &fixture.profiles,
            id,
            b"legacy-mcp",
            b"full-mcp",
            ProfileFile::Mcp,
        )
        .unwrap();
        assert_eq!(
            fs::read(fixture.profiles.join("mcp.json")).unwrap(),
            b"full-mcp"
        );
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"cli"
        );
        cleanup_backup(&mut published).unwrap();
    }
    #[test]
    fn sec03_gui_profile_publication_never_repairs_acl_or_uses_cli_as_existing_gui() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        fixture.existing(b"gui-public", true);
        let gui = fixture.profiles.join("gui.json");
        fs::write(&gui, b"gui-public").unwrap();
        secure(&gui, true, true);
        let file = File::open(&gui).unwrap();
        let before = security_snapshot(&file);
        assert_eq!(
            publish_fixed(&fixture.profiles, b"gui-public", ProfileFile::Gui).unwrap_err(),
            ErrorCode::ScopeDenied
        );
        assert_eq!(security_snapshot(&file), before);
        assert_eq!(
            fs::read(fixture.profiles.join("default.json")).unwrap(),
            b"gui-public"
        );
        assert_eq!(fs::read(&gui).unwrap(), b"gui-public");
    }

    #[test]
    fn ai09_profile_publication_never_repairs_existing_file_acl() {
        for foreign in [false, true] {
            let fixture = Fixture::new();
            fixture.dirs(true);
            let path = fixture.existing(b"public", foreign);
            secure(&path, foreign, foreign);
            assert_eq!(
                publish_at(&fixture.profiles, b"public").unwrap_err(),
                ErrorCode::ScopeDenied
            );
            assert_eq!(fs::read(path).unwrap(), b"public");
        }
    }

    #[test]
    fn ai09_profile_publication_rejects_junction_profiles() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        fs::remove_dir(&fixture.profiles).unwrap();
        let other = fixture.root.join("other");
        fs::create_dir(&other).unwrap();
        let status = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(&fixture.profiles)
            .arg(&other)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            publish_at(&fixture.profiles, b"public").unwrap_err(),
            ErrorCode::ScopeDenied
        );
        assert_eq!(fs::read_dir(&other).unwrap().count(), 0);
        assert_eq!(
            publish_fixed(&fixture.profiles, b"gui-public", ProfileFile::Gui).unwrap_err(),
            ErrorCode::ScopeDenied
        );
        assert_eq!(fs::read_dir(&other).unwrap().count(), 0);
    }

    #[test]
    fn ai09_profile_publication_renames_open_handle_without_replacement() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        let source = fixture.profiles.join("owned.tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | READ_CONTROL)
            .share_mode(FILE_SHARE_READ)
            .create_new(true)
            .open(&source)
            .unwrap();
        file.write_all(b"complete").unwrap();
        file.sync_all().unwrap();
        let target = fixture.profiles.join("default.json");
        assert!(rename_no_replace(&file, &target).unwrap());
        assert!(!source.exists());
        assert!(fs::remove_file(&target).is_err());
        drop(file);
        let another = OpenOptions::new()
            .write(true)
            .access_mode(FILE_GENERIC_READ | FILE_GENERIC_WRITE | DELETE | READ_CONTROL)
            .share_mode(FILE_SHARE_READ)
            .create_new(true)
            .open(&source)
            .unwrap();
        assert!(!rename_no_replace(&another, &target).unwrap());
        assert_eq!(fs::read(&target).unwrap(), b"complete");
    }

    #[test]
    fn inst04_profile_publication_rejects_nonfixed_path_shapes() {
        for path in [
            r"relative\profiles",
            r"\\server\share\mail-hantong\profiles",
            r"C:\fixture\..\mail-hantong\profiles",
            r"C:\fixture\other\profiles",
        ] {
            assert_eq!(
                publish_at(Path::new(path), b"public").unwrap_err(),
                ErrorCode::ScopeDenied
            );
        }
    }

    fn security_snapshot(file: &File) -> Vec<u8> {
        let mut sd = ptr::null_mut();
        assert_eq!(
            unsafe {
                GetSecurityInfo(
                    file.as_raw_handle(),
                    SE_FILE_OBJECT,
                    OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut sd,
                )
            },
            0
        );
        let sd = LocalAllocation(sd);
        let length =
            unsafe { windows_sys::Win32::Security::GetSecurityDescriptorLength(sd.0) } as usize;
        assert!((1..=65_536).contains(&length));
        unsafe { std::slice::from_raw_parts(sd.0.cast::<u8>(), length) }.to_vec()
    }

    #[test]
    fn ai09_profile_publication_failure_preserves_existing_file_security() {
        let fixture = Fixture::new();
        fixture.dirs(false);
        let path = fixture.existing(b"public", false);
        let file = File::open(&path).unwrap();
        let before = security_snapshot(&file);
        assert_eq!(
            publish_at(&fixture.profiles, b"public").unwrap_err(),
            ErrorCode::ScopeDenied
        );
        assert!(
            before == security_snapshot(&file),
            "publication must not alter existing file security"
        );
    }

    #[test]
    fn ai09_profile_publication_owned_temporary_cleanup_uses_open_handle() {
        let fixture = Fixture::new();
        fixture.dirs(true);
        let user = hantong_platform::current_peer_context().unwrap();
        let security = creation_security(user.user_sid()).unwrap();
        {
            let mut temporary = create_temporary(&fixture.profiles, &security).unwrap();
            temporary
                .0
                .as_mut()
                .unwrap()
                .write_all(b"incomplete")
                .unwrap();
            assert_eq!(fs::read_dir(&fixture.profiles).unwrap().count(), 1);
        }
        assert_eq!(fs::read_dir(&fixture.profiles).unwrap().count(), 0);
        assert!(!fixture.profiles.join("default.json").exists());
    }

    #[test]
    fn ai09_profile_publication_created_objects_match_loader_acl_policy() {
        let fixture = Fixture::new();
        let published = publish_at(&fixture.profiles, b"public").unwrap();
        let user = hantong_platform::current_peer_context().unwrap();
        check_security(&published._file, user.user_sid(), true).unwrap();
        for directory in published._directories.iter().rev().take(2) {
            check_security(directory, user.user_sid(), true).unwrap();
        }
    }

    #[test]
    fn ai09_profile_publication_success_preserves_existing_security_descriptors() {
        let fixture = Fixture::new();
        fixture.dirs(false);
        let open_directory = |path: &Path| {
            OpenOptions::new()
                .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES | FILE_TRAVERSE)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)
                .unwrap()
        };
        let product = open_directory(&fixture.product);
        let profiles = open_directory(&fixture.profiles);
        let product_before = security_snapshot(&product);
        let profiles_before = security_snapshot(&profiles);
        drop(publish_at(&fixture.profiles, b"public").unwrap());
        let file = File::open(fixture.profiles.join("default.json")).unwrap();
        let file_before = security_snapshot(&file);
        drop(publish_at(&fixture.profiles, b"public").unwrap());
        assert!(
            product_before == security_snapshot(&product),
            "product security changed"
        );
        assert!(
            profiles_before == security_snapshot(&profiles),
            "profiles security changed"
        );
        assert!(
            file_before == security_snapshot(&file),
            "file security changed"
        );
    }
}
