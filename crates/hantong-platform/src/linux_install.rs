#![allow(unsafe_code)]

//! Linux beta.29 one-time activation guard.
//!
//! This module performs only local staging/pointer activation.  It never starts
//! Core, reads the database or key, changes permissions, removes old releases,
//! or grants account scope.

use hantong_contracts::{ErrorCode, wire_hex};
use serde_json::Value;
use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
const MAX_MEMBER: u64 = 64 * 1024 * 1024;

#[derive(Clone, Eq, PartialEq)]
struct ReleaseVersion {
    numeric: [u64; 3],
    pre: Vec<PreId>,
}

#[derive(Clone, Eq, PartialEq)]
enum PreId {
    Numeric(u64),
    Text(String),
}

impl Ord for PreId {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Numeric(left), Self::Numeric(right)) => left.cmp(right),
            (Self::Numeric(_), Self::Text(_)) => Ordering::Less,
            (Self::Text(_), Self::Numeric(_)) => Ordering::Greater,
            (Self::Text(left), Self::Text(right)) => left.cmp(right),
        }
    }
}
impl PartialOrd for PreId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ReleaseVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.numeric.cmp(&other.numeric).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => self.pre.cmp(&other.pre),
            }
        })
    }
}
impl PartialOrd for ReleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn version(value: &str) -> Result<ReleaseVersion, ErrorCode> {
    if !crate::linux_identity::valid_version(value) {
        return Err(ErrorCode::InvalidInput);
    }
    let (core, pre) = value.split_once('-').map_or((value, ""), |parts| parts);
    let nums: Vec<_> = core.split('.').collect();
    if nums.len() != 3 || nums.iter().any(|part| part.is_empty()) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut numeric = [0; 3];
    for (slot, value) in numeric.iter_mut().zip(nums) {
        *slot = value.parse().map_err(|_| ErrorCode::InvalidInput)?;
    }
    let pre = if pre.is_empty() {
        Vec::new()
    } else {
        pre.split('.')
            .map(|part| {
                if part.is_empty()
                    || !part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                {
                    return Err(ErrorCode::InvalidInput);
                }
                if part.bytes().all(|byte| byte.is_ascii_digit()) {
                    if part.len() > 1 && part.starts_with('0') {
                        return Err(ErrorCode::InvalidInput);
                    }
                    Ok(PreId::Numeric(
                        part.parse().map_err(|_| ErrorCode::InvalidInput)?,
                    ))
                } else {
                    Ok(PreId::Text(part.to_owned()))
                }
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    Ok(ReleaseVersion { numeric, pre })
}

pub(crate) fn read_public(root: &Path, relative: &str, limit: u64) -> Result<Vec<u8>, ErrorCode> {
    if !crate::unix_release::safe_relative(relative) {
        return Err(ErrorCode::InvalidInput);
    }
    let path = root.join(relative);
    let guard = crate::unix_paths::UnixDirectoryGuard::open(
        path.parent().ok_or(ErrorCode::ScopeDenied)?,
        false,
    )?;
    let mut file = guard.open_existing_release_file(
        path.file_name()
            .and_then(|p| p.to_str())
            .ok_or(ErrorCode::ScopeDenied)?,
    )?;
    let metadata = file.metadata().map_err(|_| ErrorCode::CoreUnavailable)?;
    if metadata.nlink() != 1 || metadata.len() > limit {
        return Err(ErrorCode::ScopeDenied);
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(ErrorCode::Conflict);
    }
    Ok(bytes)
}
fn hash(bytes: &[u8]) -> String {
    wire_hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}
pub(crate) fn inventory(root: &Path) -> Result<(), ErrorCode> {
    let manifest = read_public(root, "release-manifest.json", MAX_MANIFEST)?;
    crate::unix_release::reject_duplicate_keys(&manifest)?;
    let value: Value = serde_json::from_slice(&manifest).map_err(|_| ErrorCode::InvalidInput)?;
    let object = value.as_object().ok_or(ErrorCode::InvalidInput)?;
    if object.len() != 5
        || object.get("format_version").and_then(Value::as_u64) != Some(1)
        || object.get("target").and_then(Value::as_str) != Some("linux-x86_64")
        || object.get("version").and_then(Value::as_str)
            != root.file_name().and_then(|p| p.to_str())
        || object
            .get("package_version")
            .and_then(Value::as_str)
            .is_none_or(|s| !crate::linux_identity::valid_version(s))
    {
        return Err(ErrorCode::InvalidInput);
    }
    let rows = object
        .get("files")
        .and_then(Value::as_array)
        .ok_or(ErrorCode::InvalidInput)?;
    if rows.is_empty() || rows.len() > 4096 {
        return Err(ErrorCode::InvalidInput);
    }
    let mut expected = std::collections::BTreeSet::new();
    let mut hashes = std::collections::BTreeMap::new();
    for row in rows {
        let row = row.as_object().ok_or(ErrorCode::InvalidInput)?;
        if row.len() != 3 {
            return Err(ErrorCode::InvalidInput);
        }
        let name = row
            .get("path")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?;
        let size = row
            .get("size")
            .and_then(Value::as_u64)
            .ok_or(ErrorCode::InvalidInput)?;
        let digest = row
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?;
        if !crate::unix_release::safe_relative(name)
            || !expected.insert(name.to_owned())
            || size > MAX_MEMBER
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ErrorCode::InvalidInput);
        }
        let bytes = read_public(root, name, MAX_MEMBER)?;
        if bytes.len() as u64 != size || hash(&bytes) != digest {
            return Err(ErrorCode::Conflict);
        }
        hashes.insert(name.to_owned(), digest.to_owned());
    }
    for name in [
        "VERSION",
        "BUILD-METADATA.json",
        "installed-files.txt",
        "bin/hantong",
        "bin/hantong-t",
        "bin/hantong-mcp",
        "bin/mail-hantong-core",
    ] {
        if !expected.contains(name) {
            return Err(ErrorCode::InvalidInput);
        }
    }
    if expected.contains("release-manifest.json") || expected.contains("FILES.sha256") {
        return Err(ErrorCode::InvalidInput);
    }
    hashes.insert("release-manifest.json".into(), hash(&manifest));
    expected.extend([
        "release-manifest.json".to_owned(),
        "FILES.sha256".to_owned(),
    ]);
    let inventory = read_public(root, "installed-files.txt", MAX_MANIFEST)?;
    let mut installed = std::collections::BTreeSet::new();
    for name in std::str::from_utf8(&inventory)
        .map_err(|_| ErrorCode::InvalidInput)?
        .lines()
    {
        if !crate::unix_release::safe_relative(name) || !installed.insert(name.to_owned()) {
            return Err(ErrorCode::InvalidInput);
        }
    }
    if installed != expected {
        return Err(ErrorCode::Conflict);
    }
    let sums = read_public(root, "FILES.sha256", MAX_MANIFEST)?;
    let mut observed = std::collections::BTreeMap::new();
    for line in std::str::from_utf8(&sums)
        .map_err(|_| ErrorCode::InvalidInput)?
        .lines()
    {
        let (digest, name) = line.split_once("  ").ok_or(ErrorCode::InvalidInput)?;
        if observed
            .insert(name.to_owned(), digest.to_owned())
            .is_some()
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    if observed != hashes {
        return Err(ErrorCode::Conflict);
    }
    let guard = crate::unix_paths::UnixDirectoryGuard::open(root, false)?;
    let file = File::from(
        guard
            .directory_fd()?
            .try_clone_to_owned()
            .map_err(|_| ErrorCode::CoreUnavailable)?,
    );
    let mut actual = std::collections::BTreeSet::new();
    walk_files(&file, "", &expected, &mut actual, 0)?;
    if actual != expected {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
fn walk_files(
    directory: &File,
    prefix: &str,
    expected: &std::collections::BTreeSet<String>,
    actual: &mut std::collections::BTreeSet<String>,
    depth: usize,
) -> Result<(), ErrorCode> {
    if depth > 16 {
        return Err(ErrorCode::QuotaExceeded);
    }
    for item in fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))
        .map_err(|_| ErrorCode::CoreUnavailable)?
    {
        let item = item.map_err(|_| ErrorCode::CoreUnavailable)?;
        let leaf = item
            .file_name()
            .into_string()
            .map_err(|_| ErrorCode::ScopeDenied)?;
        let name = if prefix.is_empty() {
            leaf.clone()
        } else {
            format!("{prefix}/{leaf}")
        };
        if !crate::unix_release::safe_relative(&name) {
            return Err(ErrorCode::ScopeDenied);
        }
        let leaf = std::ffi::CString::new(leaf).map_err(|_| ErrorCode::InvalidInput)?;
        let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
        if unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                leaf.as_ptr(),
                &mut stat,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        if stat.st_uid != unsafe { libc::geteuid() } || stat.st_mode & 0o022 != 0 {
            return Err(ErrorCode::ScopeDenied);
        }
        if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
            if !expected
                .iter()
                .any(|p| p.starts_with(&(name.clone() + "/")))
            {
                return Err(ErrorCode::Conflict);
            }
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    leaf.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd < 0 {
                return Err(ErrorCode::ScopeDenied);
            }
            let child = unsafe { <File as std::os::fd::FromRawFd>::from_raw_fd(fd) };
            let metadata = child.metadata().map_err(|_| ErrorCode::CoreUnavailable)?;
            if metadata.dev() != stat.st_dev || metadata.ino() != stat.st_ino {
                return Err(ErrorCode::Conflict);
            }
            walk_files(&child, &name, expected, actual, depth + 1)?;
        } else if stat.st_mode & libc::S_IFMT == libc::S_IFREG {
            if stat.st_nlink != 1
                || !expected.contains(&name)
                || !actual.insert(name)
                || actual.len() > 4096
            {
                return Err(ErrorCode::Conflict);
            }
        } else {
            return Err(ErrorCode::ScopeDenied);
        }
    }
    Ok(())
}
fn current_pointer(install: &Path) -> Result<String, ErrorCode> {
    let bytes = read_public(install, "current.txt", 256)?;
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| ErrorCode::InvalidInput)?
        .trim();
    if !crate::linux_identity::valid_version(text) {
        return Err(ErrorCode::InvalidInput);
    }
    version(text)?;
    Ok(text.to_owned())
}

fn socket_absent(path: &Path) -> Result<(), ErrorCode> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ErrorCode::CoreUnavailable),
        Ok(_) => Err(ErrorCode::Conflict),
    }
}

/// A post-rename durability error is not an ordinary pre-activation failure.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Activation {
    Activated,
    DurabilityUncertain,
}

fn sync_directory(guard: &crate::unix_paths::UnixDirectoryGuard) -> Result<(), ErrorCode> {
    if unsafe { libc::fsync(guard.directory_fd()?.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(ErrorCode::CoreUnavailable)
    }
}
fn activate_at(
    install: &Path,
    old_version: &str,
    candidate_root: &Path,
    socket: &Path,
    data: &Path,
) -> Result<Activation, ErrorCode> {
    activate_with_sync(
        install,
        old_version,
        candidate_root,
        socket,
        data,
        sync_directory,
    )
}

fn read_activation_record(
    guard: &crate::unix_paths::UnixDirectoryGuard,
    name: &str,
) -> Result<Option<Vec<u8>>, ErrorCode> {
    match guard.open_existing_readonly_regular(name) {
        Ok(mut file) => {
            let metadata = file.metadata().map_err(|_| ErrorCode::CoreUnavailable)?;
            if metadata.len() > 256 {
                return Err(ErrorCode::Conflict);
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|_| ErrorCode::CoreUnavailable)?;
            if bytes.len() as u64 != metadata.len() {
                return Err(ErrorCode::Conflict);
            }
            Ok(Some(bytes))
        }
        Err(ErrorCode::NotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

fn activate_resume_at(
    install: &Path,
    candidate_root: &Path,
    socket: &Path,
    data: &Path,
) -> Result<Activation, ErrorCode> {
    let candidate_version = candidate_root
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ErrorCode::ScopeDenied)?;
    let candidate = version(candidate_version)?;
    inventory(candidate_root)?;
    socket_absent(socket)?;
    let runtime = crate::unix_paths::UnixDirectoryGuard::open(
        socket.parent().ok_or(ErrorCode::InvalidInput)?,
        true,
    )?;
    let socket_lock = runtime.open_existing_regular("core-v2.lock")?;
    socket_lock.try_lock().map_err(|_| ErrorCode::Conflict)?;
    socket_absent(socket)?;
    let data_guard = crate::unix_paths::UnixDirectoryGuard::open(data, true)?;
    let database_lock = data_guard.open_existing_regular("database.lock")?;
    let key_lock = data_guard.open_existing_regular("database-key.lock")?;
    for file in [&database_lock, &key_lock] {
        file.try_lock().map_err(|_| ErrorCode::Conflict)?;
    }
    socket_absent(socket)?;
    let guard = crate::unix_paths::UnixDirectoryGuard::open(install, true)?;
    let pointer = current_pointer(install)?;
    let before_name = format!("activation-before-{candidate_version}");
    let expected_before = if pointer == candidate_version {
        let before = read_activation_record(&guard, &before_name)?.ok_or(ErrorCode::Conflict)?;
        let old = std::str::from_utf8(&before)
            .map_err(|_| ErrorCode::Conflict)?
            .trim();
        if !crate::linux_identity::valid_version(old) || version(old)? >= candidate {
            return Err(ErrorCode::Conflict);
        }
        before
    } else {
        let old = version(&pointer)?;
        if candidate <= old {
            return Err(ErrorCode::VersionMismatch);
        }
        let expected = pointer.as_bytes().to_vec();
        match read_activation_record(&guard, &before_name)? {
            Some(existing) if existing != expected => return Err(ErrorCode::Conflict),
            Some(_) => {}
            None => {
                let mut before = guard.create_new_regular(&before_name)?;
                before
                    .write_all(&expected)
                    .map_err(|_| ErrorCode::CoreUnavailable)?;
                before.sync_all().map_err(|_| ErrorCode::CoreUnavailable)?;
            }
        }
        expected
    };
    if read_activation_record(&guard, "current.txt.tmp")?
        .is_some_and(|bytes| bytes != candidate_version.as_bytes())
    {
        return Err(ErrorCode::Conflict);
    }
    if pointer == candidate_version {
        sync_directory(&guard)?;
        return Ok(Activation::Activated);
    }
    let temporary = match read_activation_record(&guard, "current.txt.tmp")? {
        Some(existing) if existing != candidate_version.as_bytes() => {
            return Err(ErrorCode::Conflict);
        }
        Some(_) => true,
        None => {
            let mut file = guard.create_new_regular("current.txt.tmp")?;
            file.write_all(candidate_version.as_bytes())
                .map_err(|_| ErrorCode::CoreUnavailable)?;
            file.sync_all().map_err(|_| ErrorCode::CoreUnavailable)?;
            true
        }
    };
    if !temporary || expected_before != pointer.as_bytes() {
        return Err(ErrorCode::Conflict);
    }
    sync_directory(&guard)?;
    guard.rename_under_lock("current.txt.tmp", "current.txt")?;
    if sync_directory(&guard).is_err() {
        return Ok(Activation::DurabilityUncertain);
    }
    Ok(Activation::Activated)
}

/// Resume an interrupted activation after an explicit operator retry.
pub fn activate_resume() -> Result<Activation, ErrorCode> {
    let current_root = crate::linux_identity::current_release_root()?;
    if crate::linux_identity::current_asset()? != "bin/mail-hantong-core" {
        return Err(ErrorCode::ScopeDenied);
    }
    let install = current_root
        .parent()
        .and_then(Path::parent)
        .ok_or(ErrorCode::ScopeDenied)?;
    let candidate_root = &current_root;
    let socket = crate::unix_paths::socket_path()?;
    activate_resume_at(
        install,
        candidate_root,
        &socket,
        &crate::unix_paths::app_directories()?.data,
    )
}
fn activate_with_sync(
    install: &Path,
    old_version: &str,
    candidate_root: &Path,
    socket: &Path,
    data: &Path,
    mut sync: impl FnMut(&crate::unix_paths::UnixDirectoryGuard) -> Result<(), ErrorCode>,
) -> Result<Activation, ErrorCode> {
    let candidate_version = candidate_root
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(ErrorCode::ScopeDenied)?;
    if version(candidate_version)? <= version(old_version)? {
        return Err(ErrorCode::VersionMismatch);
    }
    inventory(candidate_root)?;
    socket_absent(socket)?;
    let runtime = crate::unix_paths::UnixDirectoryGuard::open(
        socket.parent().ok_or(ErrorCode::InvalidInput)?,
        true,
    )?;
    let socket_lock = runtime.open_existing_regular("core-v2.lock")?;
    socket_lock.try_lock().map_err(|_| ErrorCode::Conflict)?;
    socket_absent(socket)?;
    let data_guard = crate::unix_paths::UnixDirectoryGuard::open(data, true)?;
    let database_lock = data_guard.open_existing_regular("database.lock")?;
    let key_lock = data_guard.open_existing_regular("database-key.lock")?;
    for file in [&database_lock, &key_lock] {
        file.try_lock().map_err(|_| ErrorCode::Conflict)?;
    }
    socket_absent(socket)?;
    let guard = crate::unix_paths::UnixDirectoryGuard::open(install, true)?;
    if current_pointer(install)? != old_version {
        return Err(ErrorCode::Conflict);
    }
    // Preserve the pre-activation pointer as synced public recovery evidence.
    // Never overwrite an earlier interrupted activation record.
    let mut before = guard.create_new_regular(&format!("activation-before-{candidate_version}"))?;
    before
        .write_all(old_version.as_bytes())
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    before.sync_all().map_err(|_| ErrorCode::CoreUnavailable)?;
    sync(&guard)?;
    let mut pointer = guard.create_new_regular("current.txt.tmp")?;
    pointer
        .write_all(candidate_version.as_bytes())
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pointer.sync_all().map_err(|_| ErrorCode::CoreUnavailable)?;
    guard.rename_under_lock("current.txt.tmp", "current.txt")?;
    if sync(&guard).is_err() {
        // Do not start Core, downgrade, or describe this as pointer-preserving.
        return Ok(Activation::DurabilityUncertain);
    }
    drop(database_lock);
    drop(key_lock);
    drop(socket_lock);
    Ok(Activation::Activated)
}

/// Validate and atomically advance the current release pointer.
pub fn activate_bootstrap() -> Result<Activation, ErrorCode> {
    let current_root = crate::linux_identity::current_release_root()?;
    if crate::linux_identity::current_asset()? != "bin/mail-hantong-core" {
        return Err(ErrorCode::ScopeDenied);
    }
    let install = current_root
        .parent()
        .and_then(Path::parent)
        .ok_or(ErrorCode::ScopeDenied)?;
    let old_version = current_pointer(install)?;
    let socket = crate::unix_paths::socket_path()?;
    activate_at(
        install,
        &old_version,
        &current_root,
        &socket,
        &crate::unix_paths::app_directories()?.data,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fixture(version_name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "hantong-install-{}-{}-{}",
            std::process::id(),
            version_name.replace('.', "_"),
            {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            }
        ));
        let install = root.join("install");
        let candidate = install.join("versions").join(version_name);
        let data = root.join("data");
        let runtime = root.join("runtime");
        fs::create_dir_all(candidate.join("bin")).unwrap();
        fs::create_dir_all(&data).unwrap();
        fs::create_dir_all(&runtime).unwrap();
        for directory in [
            &root,
            &install,
            &data,
            &runtime,
            candidate.parent().unwrap(),
        ] {
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::write(install.join("current.txt"), b"0.1.0-beta.9").unwrap();
        for name in ["database.lock", "database-key.lock"] {
            fs::write(data.join(name), b"").unwrap();
            fs::set_permissions(data.join(name), fs::Permissions::from_mode(0o600)).unwrap();
        }
        fs::write(install.join("current.txt"), b"0.1.0-beta.9").unwrap();
        fs::write(runtime.join("core-v2.lock"), b"").unwrap();
        fs::set_permissions(
            runtime.join("core-v2.lock"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        let mut files = std::collections::BTreeMap::new();
        for name in [
            "bin/hantong",
            "bin/hantong-t",
            "bin/hantong-mcp",
            "bin/mail-hantong-core",
        ] {
            files.insert(name.to_owned(), b"synthetic".to_vec());
        }
        files.insert("VERSION".into(), version_name.as_bytes().to_vec());
        files.insert("BUILD-METADATA.json".into(), b"{}".to_vec());
        let mut names = files.keys().cloned().collect::<Vec<_>>();
        names.extend([
            "release-manifest.json".into(),
            "FILES.sha256".into(),
            "installed-files.txt".into(),
        ]);
        names.sort();
        files.insert(
            "installed-files.txt".into(),
            (names.join("\n") + "\n").into_bytes(),
        );
        let rows=files.iter().map(|(name,bytes)|serde_json::json!({"path":name,"size":bytes.len(),"sha256":hash(bytes)})).collect::<Vec<_>>();
        let manifest = serde_json::json!({"format_version":1,"target":"linux-x86_64","version":version_name,"package_version":"0.1.0","files":rows});
        files.insert(
            "release-manifest.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        let sums = files
            .iter()
            .map(|(name, bytes)| format!("{}  {name}\n", hash(bytes)))
            .collect::<String>();
        files.insert("FILES.sha256".into(), sums.into_bytes());
        for (name, bytes) in files {
            fs::write(candidate.join(name), bytes).unwrap();
        }
        let socket = runtime.join("core-v2.sock");
        (root, install, candidate, socket)
    }

    #[test]
    fn sec03_post_rename_sync_failure_is_explicit_and_preserves_recovery_pointer() {
        let (root, install, candidate, socket) = fixture("0.1.0-beta.29");
        let data = root.join("data");
        fs::write(data.join("user-data"), b"preserve").unwrap();
        let mut calls = 0;
        let outcome = activate_with_sync(
            &install,
            "0.1.0-beta.9",
            &candidate,
            &socket,
            &data,
            |guard| {
                calls += 1;
                if calls == 2 {
                    Err(ErrorCode::CoreUnavailable)
                } else {
                    sync_directory(guard)
                }
            },
        )
        .unwrap();
        assert_eq!(outcome, Activation::DurabilityUncertain);
        assert_eq!(current_pointer(&install).unwrap(), "0.1.0-beta.29");
        assert_eq!(
            fs::read(install.join("activation-before-0.1.0-beta.29")).unwrap(),
            b"0.1.0-beta.9"
        );
        assert_eq!(fs::read(data.join("user-data")).unwrap(), b"preserve");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_resume_after_post_rename_sync_failure_preserves_record_and_data() {
        let (root, install, candidate, socket) = fixture("0.1.0-beta.29");
        let data = root.join("data");
        fs::write(data.join("user-data"), b"preserve").unwrap();
        let mut calls = 0;
        let outcome = activate_with_sync(
            &install,
            "0.1.0-beta.9",
            &candidate,
            &socket,
            &data,
            |guard| {
                calls += 1;
                if calls == 2 {
                    Err(ErrorCode::CoreUnavailable)
                } else {
                    sync_directory(guard)
                }
            },
        )
        .unwrap();
        assert_eq!(outcome, Activation::DurabilityUncertain);
        assert_eq!(
            activate_resume_at(&install, &candidate, &socket, &data),
            Ok(Activation::Activated)
        );
        assert_eq!(current_pointer(&install).unwrap(), "0.1.0-beta.29");
        assert_eq!(fs::read(data.join("user-data")).unwrap(), b"preserve");
        assert_eq!(
            fs::read(install.join("activation-before-0.1.0-beta.29")).unwrap(),
            b"0.1.0-beta.9"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_resume_active_candidate_refuses_conflicting_temporary_pointer() {
        let (root, install, candidate, socket) = fixture("0.1.0-beta.29");
        let data = root.join("data");
        assert_eq!(
            activate_at(&install, "0.1.0-beta.9", &candidate, &socket, &data),
            Ok(Activation::Activated)
        );
        fs::write(install.join("current.txt.tmp"), b"0.1.0-beta.8").unwrap();
        fs::set_permissions(
            install.join("current.txt.tmp"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert_eq!(
            activate_resume_at(&install, &candidate, &socket, &data),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(current_pointer(&install).unwrap(), "0.1.0-beta.29");
        assert_eq!(
            fs::read(install.join("current.txt.tmp")).unwrap(),
            b"0.1.0-beta.8"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_resume_rejects_tampered_record_or_tmp_or_busy_socket() {
        for mode in 0..3 {
            let (root, install, candidate, socket) = fixture(&format!("0.1.0-resume.{mode}"));
            match mode {
                0 => fs::write(
                    install.join("activation-before-0.1.0-resume.0"),
                    b"0.1.0-beta.8",
                )
                .unwrap(),
                1 => fs::write(install.join("current.txt.tmp"), b"wrong").unwrap(),
                _ => fs::write(&socket, b"busy").unwrap(),
            }
            assert!(activate_resume_at(&install, &candidate, &socket, &root.join("data")).is_err());
            assert_eq!(current_pointer(&install).unwrap(), "0.1.0-beta.9");
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn sec03_beta9_is_before_beta29() {
        assert!(version("0.1.0-beta.9").unwrap() < version("0.1.0-beta.29").unwrap());
        assert!(version("0.1.0-beta.29").unwrap() < version("0.1.0").unwrap());
    }

    #[test]
    fn sec03_activate_exact_candidate_preserves_data() {
        let (root, install, candidate, socket) = fixture("0.1.0-beta.29");
        let data = root.join("data");
        fs::write(data.join("user-data"), b"preserve").unwrap();
        let result = activate_at(&install, "0.1.0-beta.9", &candidate, &socket, &data);
        assert_eq!(result, Ok(Activation::Activated));
        assert_eq!(
            fs::read_to_string(install.join("current.txt")).unwrap(),
            "0.1.0-beta.29"
        );
        assert_eq!(fs::read(data.join("user-data")).unwrap(), b"preserve");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_stale_socket_rejects_without_pointer_change() {
        let (root, install, candidate, socket) = fixture("0.1.0-beta.29");
        fs::write(&socket, b"stale").unwrap();
        let result = activate_at(
            &install,
            "0.1.0-beta.9",
            &candidate,
            &socket,
            &root.join("data"),
        );
        assert!(matches!(
            result,
            Err(ErrorCode::CoreUnavailable | ErrorCode::Conflict | ErrorCode::ScopeDenied)
        ));
        assert_eq!(
            fs::read_to_string(install.join("current.txt")).unwrap(),
            "0.1.0-beta.9"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_downgrade_rejects_without_pointer_change() {
        let (root, install, candidate, socket) = fixture("0.1.0-beta.8");
        let result = activate_at(
            &install,
            "0.1.0-beta.9",
            &candidate,
            &socket,
            &root.join("data"),
        );
        assert_eq!(result, Err(ErrorCode::VersionMismatch));
        assert_eq!(
            fs::read_to_string(install.join("current.txt")).unwrap(),
            "0.1.0-beta.9"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_busy_storage_or_socket_lock_preserves_pointer() {
        for (i, name) in ["database.lock", "database-key.lock", "core-v2.lock"]
            .iter()
            .enumerate()
        {
            let (root, install, candidate, socket) = fixture(&format!("0.1.0-lock.{i}"));
            let location = if *name == "core-v2.lock" {
                socket.parent().unwrap().to_owned()
            } else {
                root.join("data")
            };
            let lock = crate::unix_paths::UnixDirectoryGuard::open(&location, true)
                .unwrap()
                .open_existing_regular(name)
                .unwrap();
            lock.try_lock().unwrap();
            assert_eq!(
                activate_at(
                    &install,
                    "0.1.0-beta.9",
                    &candidate,
                    &socket,
                    &root.join("data")
                ),
                Err(ErrorCode::Conflict)
            );
            assert_eq!(
                fs::read(install.join("current.txt")).unwrap(),
                b"0.1.0-beta.9"
            );
            drop(lock);
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn sec03_extra_link_or_interrupted_pointer_refuses_without_data_change() {
        for i in 0..3 {
            let (root, install, candidate, socket) = fixture(&format!("0.1.0-reject.{i}"));
            match i {
                0 => fs::write(candidate.join("unlisted"), b"untrusted").unwrap(),
                1 => std::os::unix::fs::symlink("hantong", candidate.join("bin/unlisted-link"))
                    .unwrap(),
                _ => fs::write(install.join("current.txt.tmp"), b"interrupted").unwrap(),
            }
            assert!(
                activate_at(
                    &install,
                    "0.1.0-beta.9",
                    &candidate,
                    &socket,
                    &root.join("data")
                )
                .is_err()
            );
            assert_eq!(
                fs::read(install.join("current.txt")).unwrap(),
                b"0.1.0-beta.9"
            );
            let _ = fs::remove_dir_all(root);
        }
    }
}
