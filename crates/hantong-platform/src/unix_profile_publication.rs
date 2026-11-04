//! Unix public profile publication. This stores already-authenticated profile
//! bytes; it never creates a registration, approval, or signing key.
#![allow(unsafe_code)]

use crate::unix_paths::UnixDirectoryGuard;
use hantong_contracts::{ErrorCode, registration::AiProvider};
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::FileExt;
use std::path::Path;

const MAX_PROFILE_BYTES: usize = 32 * 1024;
const LOCK_NAME: &str = "profile-publication.lock";

pub struct UnixPublishedProfile {
    _profile: File,
    backup: Option<File>,
    _lock: File,
    _directory: UnixDirectoryGuard,
    backup_name: Option<String>,
}

impl std::fmt::Debug for UnixPublishedProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UnixPublishedProfile { <redacted> }")
    }
}

#[derive(Clone, Copy)]
enum ProfileFile {
    Default,
    Gui,
    Mcp,
    AiCodex,
    AiClaude,
}

impl ProfileFile {
    fn name(self) -> &'static str {
        match self {
            Self::Default => "default.json",
            Self::Gui => "gui.json",
            Self::Mcp => "mcp.json",
            Self::AiCodex => "ai-codex.json",
            Self::AiClaude => "ai-claude.json",
        }
    }
}

pub fn publish_default_profile(bytes: &[u8]) -> Result<UnixPublishedProfile, ErrorCode> {
    publish_new_at(
        &crate::app_directories()?.profiles,
        ProfileFile::Default,
        bytes,
    )
}

pub fn publish_gui_profile(bytes: &[u8]) -> Result<UnixPublishedProfile, ErrorCode> {
    publish_new_at(&crate::app_directories()?.profiles, ProfileFile::Gui, bytes)
}

pub fn publish_mcp_profile(bytes: &[u8]) -> Result<UnixPublishedProfile, ErrorCode> {
    publish_new_at(&crate::app_directories()?.profiles, ProfileFile::Mcp, bytes)
}

pub fn publish_ai_profile(
    provider: AiProvider,
    bytes: &[u8],
) -> Result<UnixPublishedProfile, ErrorCode> {
    publish_new_at(
        &crate::app_directories()?.profiles,
        match provider {
            AiProvider::Codex => ProfileFile::AiCodex,
            AiProvider::Claude => ProfileFile::AiClaude,
        },
        bytes,
    )
}

pub fn refresh_default_profile(
    refresh_id: [u8; 32],
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<UnixPublishedProfile, ErrorCode> {
    refresh_at(
        &crate::app_directories()?.profiles,
        ProfileFile::Default,
        refresh_id,
        old_bytes,
        new_bytes,
    )
}

pub fn refresh_mcp_profile(
    refresh_id: [u8; 32],
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<UnixPublishedProfile, ErrorCode> {
    refresh_at(
        &crate::app_directories()?.profiles,
        ProfileFile::Mcp,
        refresh_id,
        old_bytes,
        new_bytes,
    )
}

pub fn cleanup_profile_refresh_backup(
    publication: &mut UnixPublishedProfile,
) -> Result<(), ErrorCode> {
    let Some(name) = publication.backup_name.as_ref() else {
        return Ok(());
    };
    let backup = publication
        .backup
        .as_ref()
        .ok_or(ErrorCode::InternalError)?;
    let expected = read_all(backup)?;
    if expected.is_empty() || expected.len() > MAX_PROFILE_BYTES {
        return Err(ErrorCode::InternalError);
    }
    publication._directory.unlink_owned(name, backup)?;
    let backup_name = publication
        .backup_name
        .take()
        .ok_or(ErrorCode::InternalError)?;
    let backup = publication.backup.take().ok_or(ErrorCode::InternalError)?;
    drop(backup);
    drop(backup_name);
    Ok(())
}

fn publish_new_at(
    profiles: &Path,
    file: ProfileFile,
    bytes: &[u8],
) -> Result<UnixPublishedProfile, ErrorCode> {
    validate_bytes(bytes)?;
    let directory = UnixDirectoryGuard::prepare(profiles)?;
    let lock = open_lock(&directory)?;
    match directory.open_existing_regular(file.name()) {
        Ok(existing) => {
            if read_all(&existing)? != bytes {
                return Err(ErrorCode::Conflict);
            }
            let temporary = temp_name(file.name(), [0; 32]);
            if let Ok(orphan) = directory.open_existing_regular(&temporary) {
                if read_all(&orphan)? != bytes {
                    return Err(ErrorCode::Conflict);
                }
                directory.unlink_owned(&temporary, &orphan)?;
            }
            return Ok(UnixPublishedProfile {
                _profile: existing,
                backup: None,
                _lock: lock,
                _directory: directory,
                backup_name: None,
            });
        }
        Err(ErrorCode::NotFound) => {}
        Err(error) => return Err(error),
    }
    let temporary = temp_name(file.name(), [0; 32]);
    let temporary_file = match directory.open_existing_regular(&temporary) {
        Ok(existing) => {
            if read_all(&existing)? != bytes {
                return Err(ErrorCode::Conflict);
            }
            existing
        }
        Err(ErrorCode::NotFound) => write_new(&directory, &temporary, bytes)?,
        Err(error) => return Err(error),
    };
    directory.link_new(&temporary, file.name())?;
    directory.unlink_owned(&temporary, &temporary_file)?;
    drop(temporary_file);
    let published = directory.open_existing_regular(file.name())?;
    Ok(UnixPublishedProfile {
        _profile: published,
        backup: None,
        _lock: lock,
        _directory: directory,
        backup_name: None,
    })
}

fn refresh_at(
    profiles: &Path,
    file: ProfileFile,
    refresh_id: [u8; 32],
    old_bytes: &[u8],
    new_bytes: &[u8],
) -> Result<UnixPublishedProfile, ErrorCode> {
    if refresh_id == [0; 32] {
        return Err(ErrorCode::InvalidInput);
    }
    validate_bytes(old_bytes)?;
    validate_bytes(new_bytes)?;
    let directory = UnixDirectoryGuard::prepare(profiles)?;
    let lock = open_lock(&directory)?;
    let current = directory.open_existing_readonly_regular(file.name())?;
    let current_bytes = read_all(&current)?;
    let backup_name = temp_name("profile-backup", refresh_id);
    let existing_backup = match directory.open_existing_regular(&backup_name) {
        Ok(existing) => {
            if read_all(&existing)? != old_bytes {
                return Err(ErrorCode::Conflict);
            }
            Some(existing)
        }
        Err(ErrorCode::NotFound) => None,
        Err(error) => return Err(error),
    };
    if current_bytes == new_bytes {
        let backup = existing_backup.ok_or(ErrorCode::Conflict)?;
        let published = directory.open_existing_regular(file.name())?;
        return Ok(UnixPublishedProfile {
            _profile: published,
            backup: Some(backup),
            _lock: lock,
            _directory: directory,
            backup_name: Some(backup_name),
        });
    }
    if current_bytes != old_bytes {
        return Err(ErrorCode::Conflict);
    }
    let backup = match existing_backup {
        Some(existing) => existing,
        None => write_new(&directory, &backup_name, old_bytes)?,
    };
    let temporary = temp_name(file.name(), refresh_id);
    let temporary_file = match directory.open_existing_regular(&temporary) {
        Ok(existing) => {
            if read_all(&existing)? != new_bytes {
                return Err(ErrorCode::Conflict);
            }
            existing
        }
        Err(ErrorCode::NotFound) => write_new(&directory, &temporary, new_bytes)?,
        Err(error) => return Err(error),
    };
    // The writer lock serializes publication. Rename is atomic for the active
    // fixed leaf; on failure, the original remains available via backup.
    directory.rename_under_lock(&temporary, file.name())?;
    drop(temporary_file);
    let published = directory.open_existing_regular(file.name())?;
    Ok(UnixPublishedProfile {
        _profile: published,
        backup: Some(backup),
        _lock: lock,
        _directory: directory,
        backup_name: Some(backup_name),
    })
}

fn open_lock(directory: &UnixDirectoryGuard) -> Result<File, ErrorCode> {
    let lock = directory.open_or_create_regular(LOCK_NAME)?;
    lock.try_lock().map_err(|_| ErrorCode::Conflict)?;
    Ok(lock)
}

fn write_new(directory: &UnixDirectoryGuard, name: &str, bytes: &[u8]) -> Result<File, ErrorCode> {
    let mut file = directory.create_new_regular(name)?;
    file.write_all(bytes)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    file.sync_all().map_err(|_| ErrorCode::CoreUnavailable)?;
    Ok(file)
}

fn read_all(file: &File) -> Result<Vec<u8>, ErrorCode> {
    let mut bytes = Vec::new();
    let mut offset = 0u64;
    let mut chunk = [0u8; 8192];
    loop {
        let count = file
            .read_at(&mut chunk, offset)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > MAX_PROFILE_BYTES {
            return Err(ErrorCode::InvalidInput);
        }
        offset = offset
            .checked_add(count as u64)
            .ok_or(ErrorCode::InvalidInput)?;
    }
    validate_bytes(&bytes)?;
    Ok(bytes)
}

fn validate_bytes(bytes: &[u8]) -> Result<(), ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_PROFILE_BYTES {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

fn temp_name(prefix: &str, id: [u8; 32]) -> String {
    let mut name = String::with_capacity(prefix.len() + 65);
    name.push('.');
    name.push_str(prefix);
    name.push('-');
    for byte in id {
        name.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        name.push(char::from(b"0123456789abcdef"[(byte & 0xf) as usize]));
    }
    name.push_str(".tmp");
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn fixture() -> (PathBuf, PathBuf) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::fs::canonicalize(std::env::temp_dir()).expect("temp root");
        let root = base.join(format!(
            "hantong-profile-publication-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("fixture root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).expect("root mode");
        (root.clone(), root)
    }

    #[test]
    fn sec03_unix_publication_is_atomic_idempotent_and_refresh_bound() {
        let (root, profiles) = fixture();
        let id = [7; 32];
        let published = publish_new_at(&profiles, ProfileFile::Default, b"old").unwrap();
        drop(published);
        let published = publish_new_at(&profiles, ProfileFile::Default, b"old").unwrap();
        assert!(matches!(
            publish_new_at(&profiles, ProfileFile::Default, b"other"),
            Err(ErrorCode::Conflict)
        ));
        drop(published);
        assert!(matches!(
            refresh_at(&profiles, ProfileFile::Default, id, b"wrong", b"new"),
            Err(ErrorCode::Conflict)
        ));
        let refreshed = refresh_at(&profiles, ProfileFile::Default, id, b"old", b"new")
            .expect("idempotent recovery");
        drop(refreshed);
        let mut recovered =
            refresh_at(&profiles, ProfileFile::Default, id, b"old", b"new").expect("recovery");
        cleanup_profile_refresh_backup(&mut recovered).expect("cleanup");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_unix_publication_recovers_orphan_and_rejects_oversize() {
        let (root, profiles) = fixture();
        let directory = UnixDirectoryGuard::prepare(&profiles).expect("directory");
        let orphan_name = temp_name("default.json", [0; 32]);
        let orphan = write_new(&directory, &orphan_name, b"orphan").expect("orphan");
        drop(orphan);
        let published =
            publish_new_at(&profiles, ProfileFile::Default, b"orphan").expect("orphan recovery");
        drop(published);
        assert!(matches!(
            publish_new_at(
                &profiles,
                ProfileFile::Gui,
                &vec![b'x'; MAX_PROFILE_BYTES + 1]
            ),
            Err(ErrorCode::InvalidInput)
        ));
        let _ = std::fs::remove_dir_all(root);
    }
}
