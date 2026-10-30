//! Read-only access to the fixed public client profile. Bytes remain untrusted
//! data; callers must validate the codec, existing key, and server authentication.

use hantong_contracts::ErrorCode;
use std::fmt;

#[cfg(windows)]
const MAX_PROFILE_BYTES: usize = 32 * 1024;

/// Immutable public bytes, with the Windows file and ancestor handles retained
/// until this value is dropped. This value does not grant authorization.
pub struct LoadedClientProfile {
    bytes: Vec<u8>,
    #[cfg(windows)]
    _file: std::fs::File,
    #[cfg(windows)]
    _directories: Vec<std::fs::File>,
}

impl LoadedClientProfile {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl fmt::Debug for LoadedClientProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LoadedClientProfile { <redacted> }")
    }
}

/// Reads only Known Folder/`mail-hantong/profiles/default.json`; never creates or
/// repairs a directory, file, ACL, or key. Keep the returned guard through auth.
pub fn load_default_client_profile() -> Result<LoadedClientProfile, ErrorCode> {
    #[cfg(windows)]
    {
        windows::load(&crate::app_directories()?.profiles)
    }
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Reads only Known Folder/`mail-hantong/profiles/gui.json`, without a CLI fallback.
pub fn load_gui_client_profile() -> Result<LoadedClientProfile, ErrorCode> {
    #[cfg(windows)]
    {
        windows::load_gui(&crate::app_directories()?.profiles)
    }
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn load_mcp_client_profile() -> Result<LoadedClientProfile, ErrorCode> {
    #[cfg(windows)]
    {
        windows::load_mcp(&crate::app_directories()?.profiles)
    }
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::io::{ErrorKind, Read};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use std::path::{Component, Path, PathBuf, Prefix};
    use std::ptr;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
        IsValidAcl, IsValidSecurityDescriptor, IsValidSid, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, SE_DACL_PROTECTED,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_GENERIC_READ, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE,
        READ_CONTROL,
    };
    use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;

    pub(super) fn load(profiles: &Path) -> Result<LoadedClientProfile, ErrorCode> {
        load_fixed(profiles, ProfileFile::Default)
    }
    pub(super) fn load_mcp(profiles: &Path) -> Result<LoadedClientProfile, ErrorCode> {
        load_fixed(profiles, ProfileFile::Mcp)
    }
    pub(super) fn load_gui(profiles: &Path) -> Result<LoadedClientProfile, ErrorCode> {
        load_fixed(profiles, ProfileFile::Gui)
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
    fn load_fixed(
        profiles: &Path,
        selected: ProfileFile,
    ) -> Result<LoadedClientProfile, ErrorCode> {
        validate_path(profiles)?;
        let user = crate::current_peer_context()?;
        let product = profiles.parent().ok_or(ErrorCode::ScopeDenied)?;
        let mut directories = Vec::new();
        let mut current = PathBuf::new();
        for component in profiles.components() {
            current.push(component);
            if matches!(component, Component::Prefix(_)) {
                continue;
            }
            let protected = current == product || current == profiles;
            let access =
                FILE_READ_ATTRIBUTES | FILE_TRAVERSE | if protected { READ_CONTROL } else { 0 };
            // Pin each ancestor before resolving its child. No SHARE_DELETE
            // means an already opened directory cannot be renamed/replaced.
            let directory = OpenOptions::new()
                .access_mode(access)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&current)
                .map_err(open_error)?;
            check_type(&directory, true)?;
            if protected {
                check_security(&directory, user.user_sid(), false)?;
            }
            directories.push(directory);
        }
        // Denying SHARE_WRITE/DELETE both detects pre-existing conflicting
        // writer handles and prevents new writers/replacement while retained.
        let file = OpenOptions::new()
            .access_mode(FILE_GENERIC_READ | READ_CONTROL)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(profiles.join(selected.name()))
            .map_err(open_error)?;
        check_type(&file, false)?;
        check_security(&file, user.user_sid(), true)?;
        if file.metadata().map_err(|_| ErrorCode::ScopeDenied)?.len() > MAX_PROFILE_BYTES as u64 {
            return Err(ErrorCode::InvalidInput);
        }
        let mut bytes = Vec::new();
        (&file)
            .take((MAX_PROFILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| ErrorCode::ScopeDenied)?;
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(LoadedClientProfile {
            bytes,
            _file: file,
            _directories: directories,
        })
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

    fn open_error(error: std::io::Error) -> ErrorCode {
        if error.kind() == ErrorKind::NotFound {
            ErrorCode::ClientNotRegistered
        } else {
            ErrorCode::ScopeDenied
        }
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

    fn check_security(file: &File, user: &str, require_protected: bool) -> Result<(), ErrorCode> {
        let mut descriptor = ptr::null_mut();
        // SAFETY: file is held open with READ_CONTROL. The API allocates a
        // descriptor; its internal pointers live until LocalAllocation drops.
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
        // SAFETY: GetSecurityInfo returned an allocated security descriptor.
        unsafe { validate_descriptor(descriptor.0, user, require_protected) }
    }

    // Only the current user, SYSTEM, and local Administrators may own or appear
    // in the allow-list DACL. Files must also protect inheritance. Directories
    // may inherit a safe current allow-list. Unknown/conditional/object/deny ACEs are
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
        use std::fs::{self, File, OpenOptions};
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
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
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
                    .join(".local/profile-loader-tests");
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
                fs::create_dir(&product).unwrap();
                secure(&product, false, true);
                let profiles = product.join("profiles");
                fs::create_dir(&profiles).unwrap();
                secure(&profiles, false, true);
                Self {
                    root,
                    product,
                    profiles,
                }
            }

            fn file(&self, contents: &[u8]) -> PathBuf {
                let path = self.profiles.join("default.json");
                fs::write(&path, contents).unwrap();
                secure(&path, false, true);
                path
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                // Enumerate only our exact fixture children and remove each
                // non-recursively; a junction must never be traversed.
                for path in [
                    self.profiles.join("default.json"),
                    self.profiles.join("gui.json"),
                    self.profiles.join("ignored.tmp"),
                ] {
                    if path.is_dir() {
                        let _ = fs::remove_dir(path);
                    } else {
                        let _ = fs::remove_file(path);
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
            let user = crate::current_peer_context().unwrap();
            let extra = if foreign { "(A;;FR;;;WD)" } else { "" };
            let protect = if protected { "P" } else { "" };
            let sddl = format!(
                "D:{protect}(A;;FA;;;{})(A;;FA;;;SY)(A;;FA;;;BA){extra}",
                user.user_sid()
            );
            let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
            let mut sd = ptr::null_mut();
            // SAFETY: inputs are terminated, and LocalFree owns the returned SD.
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
            let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            let flags = DACL_SECURITY_INFORMATION
                | if protected {
                    PROTECTED_DACL_SECURITY_INFORMATION
                } else {
                    UNPROTECTED_DACL_SECURITY_INFORMATION
                };
            let success = unsafe { SetFileSecurityW(path.as_ptr(), flags, sd) };
            unsafe {
                LocalFree(sd);
            }
            assert_ne!(success, 0);
        }

        #[test]
        fn inst01_profile_reads_bytes_without_interpreting_or_logging_them() {
            let fixture = Fixture::new();
            fixture.file(b"public-profile-fixture");
            let loaded = load(&fixture.profiles).unwrap();
            assert_eq!(loaded.bytes(), b"public-profile-fixture");
            assert_eq!(format!("{loaded:?}"), "LoadedClientProfile { <redacted> }");
        }

        #[test]
        fn sec03_gui_profile_loader_uses_only_its_fixed_file_and_retains_guards() {
            let fixture = Fixture::new();
            fixture.file(b"cli-public");
            assert_eq!(
                load_gui(&fixture.profiles).unwrap_err(),
                ErrorCode::ClientNotRegistered
            );
            let gui = fixture.profiles.join("gui.json");
            fs::write(&gui, b"gui-public").unwrap();
            secure(&gui, false, true);
            let loaded = load_gui(&fixture.profiles).unwrap();
            assert_eq!(loaded.bytes(), b"gui-public");
            assert_eq!(load(&fixture.profiles).unwrap().bytes(), b"cli-public");
            assert!(OpenOptions::new().write(true).open(&gui).is_err());
            assert!(fs::remove_file(&gui).is_err());
            assert!(fs::rename(&fixture.profiles, fixture.root.join("renamed")).is_err());
            drop(loaded);
            fs::remove_file(fixture.profiles.join("default.json")).unwrap();
            assert_eq!(
                load(&fixture.profiles).unwrap_err(),
                ErrorCode::ClientNotRegistered
            );
            assert_eq!(load_gui(&fixture.profiles).unwrap().bytes(), b"gui-public");
        }

        #[test]
        fn sec03_mcp_profile_loader_uses_only_its_fixed_file_and_retains_guards() {
            let fixture = Fixture::new();
            fixture.file(b"cli-public");
            assert_eq!(
                load_mcp(&fixture.profiles).unwrap_err(),
                ErrorCode::ClientNotRegistered
            );
            let mcp = fixture.profiles.join("mcp.json");
            fs::write(&mcp, b"mcp-public").unwrap();
            secure(&mcp, false, true);
            let loaded = load_mcp(&fixture.profiles).unwrap();
            assert_eq!(loaded.bytes(), b"mcp-public");
            assert_eq!(load(&fixture.profiles).unwrap().bytes(), b"cli-public");
            assert!(OpenOptions::new().write(true).open(&mcp).is_err());
            assert!(fs::remove_file(&mcp).is_err());
            assert!(fs::rename(&fixture.profiles, fixture.root.join("renamed")).is_err());
            drop(loaded);
            fs::remove_file(fixture.profiles.join("default.json")).unwrap();
            assert_eq!(
                load(&fixture.profiles).unwrap_err(),
                ErrorCode::ClientNotRegistered
            );
            assert_eq!(load_mcp(&fixture.profiles).unwrap().bytes(), b"mcp-public");
        }
        #[test]
        fn sec03_gui_profile_loader_keeps_acl_type_and_size_checks_without_cli_fallback() {
            let fixture = Fixture::new();
            fixture.file(b"cli-public");
            let gui = fixture.profiles.join("gui.json");
            fs::write(&gui, b"gui-public").unwrap();
            secure(&gui, true, true);
            assert_eq!(
                load_gui(&fixture.profiles).unwrap_err(),
                ErrorCode::ScopeDenied
            );
            secure(&gui, false, true);
            fs::write(&gui, vec![b' '; MAX_PROFILE_BYTES + 1]).unwrap();
            assert_eq!(
                load_gui(&fixture.profiles).unwrap_err(),
                ErrorCode::InvalidInput
            );
            fs::remove_file(&gui).unwrap();
            fs::create_dir(&gui).unwrap();
            assert_eq!(
                load_gui(&fixture.profiles).unwrap_err(),
                ErrorCode::ScopeDenied
            );
            assert_eq!(load(&fixture.profiles).unwrap().bytes(), b"cli-public");
        }

        #[test]
        fn inst01_profile_missing_is_not_registered_and_never_created() {
            let fixture = Fixture::new();
            fs::write(fixture.profiles.join("ignored.tmp"), b"incomplete").unwrap();
            assert_eq!(
                load(&fixture.profiles).unwrap_err(),
                ErrorCode::ClientNotRegistered
            );
            assert!(!fixture.profiles.join("default.json").exists());
        }

        #[test]
        fn inst01_profile_accepts_exact_byte_limit_and_rejects_oversize() {
            let fixture = Fixture::new();
            let path = fixture.file(&vec![b' '; MAX_PROFILE_BYTES]);
            assert_eq!(
                load(&fixture.profiles).unwrap().bytes().len(),
                MAX_PROFILE_BYTES
            );
            OpenOptions::new()
                .append(true)
                .open(path)
                .unwrap()
                .write_all(b"x")
                .unwrap();
            assert_eq!(
                load(&fixture.profiles).unwrap_err(),
                ErrorCode::InvalidInput
            );
        }

        #[test]
        fn ai09_profile_guard_pins_file_and_directories_until_drop() {
            let fixture = Fixture::new();
            let path = fixture.file(b"fixed");
            let loaded = load(&fixture.profiles).unwrap();
            assert!(OpenOptions::new().write(true).open(&path).is_err());
            assert!(fs::remove_file(&path).is_err());
            assert!(fs::rename(&fixture.product, fixture.root.join("renamed")).is_err());
            drop(loaded);
            assert!(OpenOptions::new().write(true).open(&path).is_ok());
        }

        #[test]
        fn ai09_profile_rejects_an_already_open_writer() {
            let fixture = Fixture::new();
            let path = fixture.file(b"fixed");
            let _writer = OpenOptions::new()
                .write(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .open(path)
                .unwrap();
            assert_eq!(load(&fixture.profiles).unwrap_err(), ErrorCode::ScopeDenied);
        }

        #[test]
        fn ai09_profile_rejects_foreign_acl_on_each_trusted_object() {
            for level in 0..3 {
                let fixture = Fixture::new();
                let path = fixture.file(b"fixed");
                let target = [&fixture.product, &fixture.profiles, &path][level];
                secure(target, true, true);
                assert_eq!(load(&fixture.profiles).unwrap_err(), ErrorCode::ScopeDenied);
            }
        }

        #[test]
        fn ai09_profile_rejects_inheritable_acl_and_directory_as_file() {
            let fixture = Fixture::new();
            let path = fixture.file(b"fixed");
            secure(&path, false, false);
            assert_eq!(load(&fixture.profiles).unwrap_err(), ErrorCode::ScopeDenied);
            fs::remove_file(&path).unwrap();
            fs::create_dir(&path).unwrap();
            secure(&path, false, true);
            assert_eq!(load(&fixture.profiles).unwrap_err(), ErrorCode::ScopeDenied);
        }

        #[test]
        fn ai09_profile_rejects_junction_directory() {
            let fixture = Fixture::new();
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
            assert!(
                status.success(),
                "isolated fixture junction creation failed"
            );
            assert_eq!(load(&fixture.profiles).unwrap_err(), ErrorCode::ScopeDenied);
            assert_eq!(
                load_gui(&fixture.profiles).unwrap_err(),
                ErrorCode::ScopeDenied
            );
        }

        #[test]
        fn inst01_profile_rejects_nonlocal_or_nonfixed_path_shapes() {
            for path in [
                r"relative\profiles",
                r"\\server\share\mail-hantong\profiles",
                r"C:\fixture\..\mail-hantong\profiles",
                r"C:\fixture\other\profiles",
            ] {
                assert_eq!(load(Path::new(path)).unwrap_err(), ErrorCode::ScopeDenied);
                assert_eq!(
                    load_gui(Path::new(path)).unwrap_err(),
                    ErrorCode::ScopeDenied
                );
            }
        }

        #[test]
        fn ai09_profile_existing_reader_is_allowed() {
            let fixture = Fixture::new();
            let path = fixture.file(b"fixed");
            let _reader = File::open(path).unwrap();
            assert_eq!(load(&fixture.profiles).unwrap().bytes(), b"fixed");
        }

        #[test]
        fn ai09_profile_security_rejects_foreign_owner_null_and_unsupported_acl() {
            let identity = crate::current_peer_context().unwrap();
            let user = identity.user_sid();
            for sddl in [
                format!("O:WDD:P(A;;FA;;;{user})"),
                format!("O:{user}D:NO_ACCESS_CONTROL"),
                format!("O:{user}D:P"),
                format!("O:{user}D:P(D;;FA;;;WD)(A;;FA;;;{user})"),
                format!("O:{user}D:P(A;;FA;;;CO)"),
            ] {
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
                let sd = LocalAllocation(sd);
                assert_eq!(
                    unsafe { validate_descriptor(sd.0, user, true) },
                    Err(ErrorCode::ScopeDenied)
                );
            }
        }

        #[test]
        fn ai09_profile_security_accepts_only_allowed_owners() {
            let identity = crate::current_peer_context().unwrap();
            let user = identity.user_sid();
            for owner in [user, "SY", "BA"] {
                let sddl = format!("O:{owner}D:P(A;;FA;;;{user})(A;;FA;;;SY)(A;;FA;;;BA)");
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
                let sd = LocalAllocation(sd);
                assert_eq!(unsafe { validate_descriptor(sd.0, user, true) }, Ok(()));
            }
        }

        #[test]
        fn ai09_profile_rejects_reparse_above_product_root() {
            let fixture = Fixture::new();
            fixture.file(b"fixed");
            let alias = fixture.root.join("other");
            let status = std::process::Command::new("cmd.exe")
                .args(["/c", "mklink", "/J"])
                .arg(&alias)
                .arg(&fixture.root)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(
                status.success(),
                "isolated fixture junction creation failed"
            );
            assert_eq!(
                load(&alias.join("mail-hantong/profiles")).unwrap_err(),
                ErrorCode::ScopeDenied
            );
            assert_eq!(
                load_gui(&alias.join("mail-hantong/profiles")).unwrap_err(),
                ErrorCode::ScopeDenied
            );
        }

        #[test]
        fn inst01_profile_missing_directories_are_never_created() {
            let fixture = Fixture::new();
            fs::remove_dir(&fixture.profiles).unwrap();
            fs::remove_dir(&fixture.product).unwrap();
            assert_eq!(
                load(&fixture.profiles).unwrap_err(),
                ErrorCode::ClientNotRegistered
            );
            assert!(!fixture.product.exists());
        }

        #[test]
        fn ai09_profile_accepts_safe_inherited_directory_acl() {
            let fixture = Fixture::new();
            let path = fixture.file(b"public");
            secure(&fixture.product, false, false);
            secure(&fixture.profiles, false, false);
            assert_eq!(load(&fixture.profiles).unwrap().bytes(), b"public");
            secure(&path, false, false);
            assert_eq!(load(&fixture.profiles).unwrap_err(), ErrorCode::ScopeDenied);
        }
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn inst01_profile_is_unsupported_outside_windows() {
        assert_eq!(
            load_default_client_profile().unwrap_err(),
            ErrorCode::CapabilityUnsupported
        );
        assert_eq!(
            load_gui_client_profile().unwrap_err(),
            ErrorCode::CapabilityUnsupported
        );
    }
}
