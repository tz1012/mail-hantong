//! Validated Unix application and runtime paths.
//!
//! Path discovery is separate from guarded directory preparation and file
//! operations. Existing directory permissions are validated without repair.

#![cfg(unix)]
#![allow(unsafe_code)]

use hantong_contracts::ErrorCode;
use std::ffi::{CStr, OsStr};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const PRODUCT: &str = "mail-hantong";

#[derive(Clone, Eq, PartialEq)]
pub struct UnixAppDirectories {
    pub data: PathBuf,
    pub secrets: PathBuf,
    pub profiles: PathBuf,
}

impl std::fmt::Debug for UnixAppDirectories {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("UnixAppDirectories { <redacted> }")
    }
}

/// Resolve persistent data without trusting `$HOME`.
pub fn app_directories() -> Result<UnixAppDirectories, ErrorCode> {
    let base = persistent_base()?;
    let profiles = profiles_base()?;
    let root = base.join(PRODUCT);
    validate_parent(&base)?;
    validate_parent(&profiles)?;
    Ok(UnixAppDirectories {
        data: root.join("data"),
        secrets: root.join("secrets"),
        profiles: profiles.join(PRODUCT).join("profiles"),
    })
}

/// Return the application socket directory.  Linux requires the kernel's
/// per-user runtime directory when present. Headless sessions without one use
/// a fixed private directory below the OS passwd home, never arbitrary `/tmp`.
pub fn runtime_socket_dir() -> Result<PathBuf, ErrorCode> {
    #[cfg(target_os = "linux")]
    {
        if let Some(root) = std::env::var_os("XDG_RUNTIME_DIR") {
            let root = absolute_clean(root)?;
            validate_owned_private_dir(&root)?;
            return Ok(root.join(PRODUCT));
        }
        let home = passwd_home()?.ok_or(ErrorCode::CapabilityUnsupported)?;
        headless_runtime_dir(&home)
    }
    #[cfg(target_os = "macos")]
    {
        // `confstr(_CS_DARWIN_USER_TEMP_DIR)` is the OS-owned per-user
        // temporary directory.  Environment overrides are not consulted.
        let mut buffer = vec![0u8; 4096];
        let length = unsafe {
            libc::confstr(
                libc::_CS_DARWIN_USER_TEMP_DIR,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if length == 0 || length >= buffer.len() {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        buffer.truncate(length.saturating_sub(1));
        let root = absolute_clean(OsStr::from_bytes(&buffer).to_os_string())?;
        let root = std::fs::canonicalize(root).map_err(|_| ErrorCode::CapabilityUnsupported)?;
        validate_owned_private_dir(&root)?;
        Ok(root.join(PRODUCT))
    }
}

#[cfg(target_os = "linux")]
fn headless_runtime_dir(home: &Path) -> Result<PathBuf, ErrorCode> {
    let root =
        absolute_clean(home.as_os_str().to_os_string())?.join(".local/state/mail-hantong/run");
    // Creation is left to UnixDirectoryGuard::prepare, which pins each private
    // directory and enforces UID/mode/no-follow before a socket can be bound.
    validate_parent(&root)?;
    Ok(root)
}

pub fn socket_path() -> Result<PathBuf, ErrorCode> {
    Ok(runtime_socket_dir()?.join("core-v2.sock"))
}

#[cfg(target_os = "linux")]
fn persistent_base() -> Result<PathBuf, ErrorCode> {
    if let Some(value) = std::env::var_os("XDG_DATA_HOME") {
        return absolute_clean(value);
    }
    Ok(passwd_home()?
        .ok_or(ErrorCode::CapabilityUnsupported)?
        .join(".local")
        .join("share"))
}

#[cfg(target_os = "linux")]
fn profiles_base() -> Result<PathBuf, ErrorCode> {
    if let Some(value) = std::env::var_os("XDG_CONFIG_HOME") {
        return absolute_clean(value);
    }
    Ok(passwd_home()?
        .ok_or(ErrorCode::CapabilityUnsupported)?
        .join(".config"))
}

#[cfg(target_os = "macos")]
fn persistent_base() -> Result<PathBuf, ErrorCode> {
    Ok(passwd_home()?
        .ok_or(ErrorCode::CapabilityUnsupported)?
        .join("Library")
        .join("Application Support"))
}

#[cfg(target_os = "macos")]
fn profiles_base() -> Result<PathBuf, ErrorCode> {
    persistent_base()
}

fn passwd_home() -> Result<Option<PathBuf>, ErrorCode> {
    let uid = unsafe { libc::geteuid() };
    let mut buffer = vec![0u8; 1024];
    loop {
        let mut entry = unsafe { std::mem::zeroed::<libc::passwd>() };
        let mut result = std::ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                &mut entry,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE {
            if buffer.len() >= 64 * 1024 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if status != 0 {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        if result.is_null() || entry.pw_dir.is_null() {
            return Ok(None);
        }
        let bytes = unsafe { CStr::from_ptr(entry.pw_dir).to_bytes() };
        if bytes.is_empty() {
            return Ok(None);
        }
        return Ok(Some(absolute_clean(PathBuf::from(OsStr::from_bytes(
            bytes,
        )))?));
    }
}

fn absolute_clean(value: impl Into<std::ffi::OsString>) -> Result<PathBuf, ErrorCode> {
    let path = PathBuf::from(value.into());
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    Ok(path)
}

fn validate_parent(path: &Path) -> Result<(), ErrorCode> {
    validate_directory_chain(path, false)
}

fn validate_owned_private_dir(path: &Path) -> Result<(), ErrorCode> {
    validate_directory_chain(path, true)
}

fn validate_directory_chain(path: &Path, private: bool) -> Result<(), ErrorCode> {
    if !path.is_absolute() {
        return Err(ErrorCode::InvalidInput);
    }
    let mut fd = OwnedFd::open_root()?;
    let components: Vec<_> = path.components().skip(1).collect();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(name) = component else {
            return Err(ErrorCode::InvalidInput);
        };
        let bytes = name.as_bytes();
        let name = std::ffi::CString::new(bytes).map_err(|_| ErrorCode::InvalidInput)?;
        let next = unsafe {
            libc::openat(
                fd.0,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0,
            )
        };
        if next < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
                return Ok(());
            }
            return Err(ErrorCode::CapabilityUnsupported);
        }
        let next = OwnedFd(next);
        let stat = fstat(next.0)?;
        if stat.st_uid != 0 && stat.st_uid != unsafe { libc::geteuid() } {
            return Err(ErrorCode::ScopeDenied);
        }
        if !ancestor_allowed(&stat, private && index + 1 == components.len()) {
            return Err(ErrorCode::ScopeDenied);
        }
        fd = next;
    }
    Ok(())
}

fn ancestor_allowed(stat: &libc::stat, private: bool) -> bool {
    let owner_ok = if private {
        stat.st_uid == unsafe { libc::geteuid() }
    } else {
        stat.st_uid == 0 || stat.st_uid == unsafe { libc::geteuid() }
    };
    if !owner_ok {
        return false;
    }
    if private {
        stat.st_mode & 0o077 == 0
    } else {
        stat.st_mode & 0o022 == 0
            || (stat.st_uid == 0 && stat.st_mode & 0o1000 != 0 && stat.st_mode & 0o002 != 0)
    }
}

/// Holds the opened directory chain for a subsequent handle-relative
/// operation. Path-only validation is discovery and must not be treated as a
/// TOCTOU security proof.
pub struct UnixDirectoryGuard {
    descriptors: Vec<OwnedFd>,
}

impl UnixDirectoryGuard {
    pub fn directory_fd(&self) -> Result<BorrowedFd<'_>, ErrorCode> {
        let fd = self.descriptors.last().ok_or(ErrorCode::InternalError)?.0;
        Ok(unsafe { BorrowedFd::borrow_raw(fd) })
    }

    pub fn link_new(&self, source: &str, target: &str) -> Result<(), ErrorCode> {
        let source = leaf_name(source)?;
        let target = leaf_name(target)?;
        let parent = self.descriptors.last().ok_or(ErrorCode::InternalError)?;
        let result =
            unsafe { libc::linkat(parent.0, source.as_ptr(), parent.0, target.as_ptr(), 0) };
        if result == 0 {
            Ok(())
        } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
            Err(ErrorCode::Conflict)
        } else {
            Err(ErrorCode::CoreUnavailable)
        }
    }

    pub fn rename_under_lock(&self, source: &str, target: &str) -> Result<(), ErrorCode> {
        let source = leaf_name(source)?;
        let target = leaf_name(target)?;
        let parent = self.descriptors.last().ok_or(ErrorCode::InternalError)?;
        let result =
            unsafe { libc::renameat(parent.0, source.as_ptr(), parent.0, target.as_ptr()) };
        if result == 0 {
            Ok(())
        } else {
            Err(ErrorCode::CoreUnavailable)
        }
    }

    pub fn unlink_owned(&self, name: &str, held: &std::fs::File) -> Result<(), ErrorCode> {
        let name = leaf_name(name)?;
        let parent = self.descriptors.last().ok_or(ErrorCode::InternalError)?;
        let expected = fstat(held.as_raw_fd())?;
        let mut current = unsafe { std::mem::zeroed::<libc::stat>() };
        if unsafe {
            libc::fstatat(
                parent.0,
                name.as_ptr(),
                &mut current,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        if expected.st_dev != current.st_dev || expected.st_ino != current.st_ino {
            return Err(ErrorCode::Conflict);
        }
        if unsafe { libc::unlinkat(parent.0, name.as_ptr(), 0) } != 0 {
            return Err(ErrorCode::CoreUnavailable);
        }
        Ok(())
    }

    pub fn open(path: &Path, private: bool) -> Result<Self, ErrorCode> {
        if !path.is_absolute() {
            return Err(ErrorCode::InvalidInput);
        }
        let mut current = OwnedFd::open_root()?;
        let mut descriptors = vec![current.duplicate()?];
        let components: Vec<_> = path.components().skip(1).collect();
        if components.is_empty() {
            return Err(ErrorCode::ScopeDenied);
        }
        for (index, component) in components.iter().enumerate() {
            let std::path::Component::Normal(name) = component else {
                return Err(ErrorCode::InvalidInput);
            };
            let name =
                std::ffi::CString::new(name.as_bytes()).map_err(|_| ErrorCode::InvalidInput)?;
            let raw = unsafe {
                libc::openat(
                    current.0,
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                    0,
                )
            };
            if raw < 0 {
                return Err(match std::io::Error::last_os_error().raw_os_error() {
                    Some(libc::ENOENT) => ErrorCode::NotFound,
                    _ => ErrorCode::CapabilityUnsupported,
                });
            }
            let next = OwnedFd(raw);
            let stat = fstat(next.0)?;
            if !ancestor_allowed(&stat, private && index + 1 == components.len()) {
                return Err(ErrorCode::ScopeDenied);
            }
            descriptors.push(next.duplicate()?);
            current = next;
        }
        Ok(Self { descriptors })
    }

    /// Securely open or create a directory tree. Missing descendants are
    /// created with 0700; existing directories are never repaired.
    pub fn prepare(path: &Path) -> Result<Self, ErrorCode> {
        if !path.is_absolute() {
            return Err(ErrorCode::InvalidInput);
        }
        let mut current = OwnedFd::open_root()?;
        let mut descriptors = vec![current.duplicate()?];
        let components: Vec<_> = path.components().skip(1).collect();
        if components.is_empty() {
            return Err(ErrorCode::ScopeDenied);
        }
        let total = components.len();
        for (index, component) in components.into_iter().enumerate() {
            let std::path::Component::Normal(name) = component else {
                return Err(ErrorCode::InvalidInput);
            };
            let name =
                std::ffi::CString::new(name.as_bytes()).map_err(|_| ErrorCode::InvalidInput)?;
            let mut raw = unsafe {
                libc::openat(
                    current.0,
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                    0,
                )
            };
            if raw < 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
                if unsafe { libc::mkdirat(current.0, name.as_ptr(), 0o700) } != 0
                    && std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
                {
                    return Err(ErrorCode::CoreUnavailable);
                }
                raw = unsafe {
                    libc::openat(
                        current.0,
                        name.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                        0,
                    )
                };
            }
            if raw < 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            let next = OwnedFd(raw);
            let stat = fstat(next.0)?;
            if !ancestor_allowed(&stat, index + 1 == total) {
                return Err(ErrorCode::ScopeDenied);
            }
            descriptors.push(next.duplicate()?);
            current = next;
        }
        Ok(Self { descriptors })
    }

    /// Open one fixed regular file beneath this already-held directory. The
    /// existing file is never truncated, replaced, or permission-repaired.
    pub fn open_or_create_regular(&self, name: &str) -> Result<std::fs::File, ErrorCode> {
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(ErrorCode::InvalidInput);
        }
        let parent = self.descriptors.last().ok_or(ErrorCode::InternalError)?;
        let name = std::ffi::CString::new(name).map_err(|_| ErrorCode::InvalidInput)?;
        let raw = unsafe {
            libc::openat(
                parent.0,
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if raw < 0 {
            return Err(ErrorCode::CoreUnavailable);
        }
        let file = unsafe { std::fs::File::from_raw_fd(raw) };
        let stat = file.metadata().map_err(|_| ErrorCode::CoreUnavailable)?;
        if !stat.file_type().is_file()
            || stat.uid() != unsafe { libc::geteuid() }
            || stat.nlink() != 1
            || stat.mode() & 0o7777 != 0o600
        {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(file)
    }

    pub fn open_existing_regular(&self, name: &str) -> Result<std::fs::File, ErrorCode> {
        self.open_regular(name, libc::O_RDWR)
    }

    pub fn open_existing_readonly_regular(&self, name: &str) -> Result<std::fs::File, ErrorCode> {
        self.open_regular(name, libc::O_RDONLY)
    }

    /// Open a release asset relative to this already-held directory without
    /// following the final path component. Release files may use permissions
    /// other than the private 0600 profile/secret shape, but must still be a
    /// regular file owned by the current user and must not be group/world
    /// writable.
    pub(crate) fn open_existing_release_file(
        &self,
        name: &str,
    ) -> Result<std::fs::File, ErrorCode> {
        let name = leaf_name(name)?;
        let parent = self.descriptors.last().ok_or(ErrorCode::InternalError)?;
        let raw = unsafe {
            libc::openat(
                parent.0,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0,
            )
        };
        if raw < 0 {
            return Err(match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ELOOP) => ErrorCode::ScopeDenied,
                _ => ErrorCode::CoreUnavailable,
            });
        }
        let file = unsafe { std::fs::File::from_raw_fd(raw) };
        let stat = file.metadata().map_err(|_| ErrorCode::CoreUnavailable)?;
        if !stat.file_type().is_file()
            || stat.uid() != unsafe { libc::geteuid() }
            || stat.mode() & 0o022 != 0
        {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(file)
    }

    pub fn create_new_regular(&self, name: &str) -> Result<std::fs::File, ErrorCode> {
        self.open_regular(name, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL)
    }

    fn open_regular(&self, name: &str, flags: libc::c_int) -> Result<std::fs::File, ErrorCode> {
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(ErrorCode::InvalidInput);
        }
        let parent = self.descriptors.last().ok_or(ErrorCode::InternalError)?;
        let name = std::ffi::CString::new(name).map_err(|_| ErrorCode::InvalidInput)?;
        let raw = unsafe {
            libc::openat(
                parent.0,
                name.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600,
            )
        };
        if raw < 0 {
            return Err(match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOENT) => ErrorCode::NotFound,
                Some(libc::EEXIST) => ErrorCode::Conflict,
                _ => ErrorCode::CoreUnavailable,
            });
        }
        let file = unsafe { std::fs::File::from_raw_fd(raw) };
        let stat = file.metadata().map_err(|_| ErrorCode::CoreUnavailable)?;
        if !stat.file_type().is_file()
            || stat.uid() != unsafe { libc::geteuid() }
            || stat.nlink() != 1
            || stat.mode() & 0o7777 != 0o600
        {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(file)
    }
}

fn leaf_name(name: &str) -> Result<std::ffi::CString, ErrorCode> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(ErrorCode::InvalidInput);
    }
    std::ffi::CString::new(name).map_err(|_| ErrorCode::InvalidInput)
}

struct OwnedFd(libc::c_int);
impl OwnedFd {
    fn open_root() -> Result<Self, ErrorCode> {
        let path = std::ffi::CString::new("/").unwrap();
        let fd = unsafe {
            libc::open(
                path.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
                0,
            )
        };
        if fd < 0 {
            Err(ErrorCode::CapabilityUnsupported)
        } else {
            Ok(Self(fd))
        }
    }

    fn duplicate(&self) -> Result<Self, ErrorCode> {
        let fd = unsafe { libc::fcntl(self.0, libc::F_DUPFD_CLOEXEC, 0) };
        if fd < 0 {
            Err(ErrorCode::CoreUnavailable)
        } else {
            Ok(Self(fd))
        }
    }
}
impl Drop for OwnedFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

fn fstat(fd: libc::c_int) -> Result<libc::stat, ErrorCode> {
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    Ok(stat)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn sec03_rejects_relative_and_parent_paths() {
        assert!(absolute_clean("relative".to_owned()).is_err());
        assert!(absolute_clean("/tmp/../mail".to_owned()).is_err());
    }

    #[test]
    fn sec03_does_not_accept_home_environment_as_authority() {
        let passwd = passwd_home().unwrap();
        if let Some(home) = passwd {
            assert!(home.is_absolute());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sec03_headless_runtime_uses_fixed_passwd_home_subdirectory() {
        assert!(headless_runtime_dir(Path::new("relative")).is_err());
        assert!(headless_runtime_dir(Path::new("/tmp/../escape")).is_err());
        let home = passwd_home().unwrap().unwrap();
        assert_eq!(
            headless_runtime_dir(&home).unwrap(),
            home.join(".local/state/mail-hantong/run")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sec03_directory_walk_rejects_symlink_ancestor() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "hantong-platform-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let target = root.join("target");
        let link = root.join("link");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        std::fs::set_permissions(&target, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        symlink(&target, &link).unwrap();
        assert!(validate_directory_chain(&link.join("child"), false).is_err());
        let _ = std::fs::remove_file(link);
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sec03_prepare_and_regular_file_guard_preserve_data_and_reject_loose_or_symlink() {
        use std::io::{Read, Seek, SeekFrom, Write};
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(format!(
            "hantong-platform-guard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let target = root.join("mail-hantong").join("data");
        let guard = UnixDirectoryGuard::prepare(&target).unwrap();
        let mut file = guard.open_or_create_regular("db").unwrap();
        file.write_all(b"keep").unwrap();
        file.seek(SeekFrom::Start(0)).unwrap();
        let mut value = String::new();
        file.read_to_string(&mut value).unwrap();
        assert_eq!(value, "keep");
        drop(file);
        let mut reopened = UnixDirectoryGuard::open(&target, true)
            .unwrap()
            .open_or_create_regular("db")
            .unwrap();
        reopened.seek(SeekFrom::Start(0)).unwrap();
        let mut value = String::new();
        reopened.read_to_string(&mut value).unwrap();
        assert_eq!(value, "keep");
        drop(reopened);
        std::fs::set_permissions(target.join("db"), std::fs::Permissions::from_mode(0o644))
            .unwrap();
        assert!(matches!(
            UnixDirectoryGuard::open(&target, true)
                .unwrap()
                .open_or_create_regular("db"),
            Err(ErrorCode::ScopeDenied)
        ));
        std::fs::remove_file(target.join("db")).unwrap();
        symlink(root.join("other"), target.join("db")).unwrap();
        assert!(matches!(
            UnixDirectoryGuard::open(&target, true)
                .unwrap()
                .open_or_create_regular("db"),
            Err(ErrorCode::CoreUnavailable)
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
