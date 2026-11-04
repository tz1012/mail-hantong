//! Unix-domain socket admission helpers.

#![cfg(unix)]
#![allow(unsafe_code)]

use crate::unix_paths::UnixDirectoryGuard;
use hantong_contracts::ErrorCode;
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const SOCKET_NAME: &str = "core-v2.sock";
const LOCK_NAME: &str = "core-v2.lock";
const SOCKET_MODE: u32 = 0o600;

pub fn validate_socket_endpoint(path: &Path) -> Result<(), ErrorCode> {
    let parent = socket_parent(path)?;
    let _guard = UnixDirectoryGuard::open(parent, true)?;
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(ErrorCode::CoreUnavailable),
        Ok(metadata) => {
            if !metadata.file_type().is_socket()
                || metadata.uid() != unsafe { libc::geteuid() }
                || metadata.mode() & 0o777 != SOCKET_MODE
            {
                return Err(ErrorCode::ScopeDenied);
            }
        }
    }
    Ok(())
}

pub fn open_socket_lock(path: &Path) -> Result<std::fs::File, ErrorCode> {
    let parent = socket_parent(path)?;
    let guard = UnixDirectoryGuard::open(parent, true)?;
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(ErrorCode::CoreUnavailable),
        Ok(metadata)
            if metadata.file_type().is_socket() && metadata.uid() == unsafe { libc::geteuid() } => {
        }
        Ok(_) => return Err(ErrorCode::ScopeDenied),
    }
    let file = guard.open_or_create_regular(LOCK_NAME)?;
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        return Err(
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EWOULDBLOCK) {
                ErrorCode::Conflict
            } else {
                ErrorCode::CoreUnavailable
            },
        );
    }
    Ok(file)
}

fn socket_parent(path: &Path) -> Result<&Path, ErrorCode> {
    if !path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
        || path.file_name().and_then(|value| value.to_str()) != Some(SOCKET_NAME)
    {
        return Err(ErrorCode::InvalidInput);
    }
    path.parent().ok_or(ErrorCode::InvalidInput)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec03_socket_path_is_fixed_and_absolute() {
        assert_eq!(
            validate_socket_endpoint(Path::new("core.sock")),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            validate_socket_endpoint(Path::new("/tmp/../core.sock")),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            validate_socket_endpoint(Path::new("/tmp/other.sock")),
            Err(ErrorCode::InvalidInput)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sec03_lock_is_exclusive_and_reusable_after_drop() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "hantong-platform-lock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let endpoint = root.join(SOCKET_NAME);
        let first = open_socket_lock(&endpoint).unwrap();
        assert!(matches!(
            open_socket_lock(&endpoint),
            Err(ErrorCode::Conflict)
        ));
        drop(first);
        let second = open_socket_lock(&endpoint).unwrap();
        drop(second);
        let _ = std::fs::remove_file(root.join(LOCK_NAME));
        let _ = std::fs::remove_dir(root);
    }
}
