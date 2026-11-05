//! Release-owned executable validation for Unix Core helper launches.

#![cfg(unix)]

use crate::MAX_RELEASE_FILE;
use crate::unix_paths::UnixDirectoryGuard;
use hantong_contracts::{ErrorCode, wire_hex};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

const MAX_MANIFEST: u64 = 4 * 1024 * 1024;

/// Validates the installed release containing the current executable and
/// returns its sibling Core executable. The caller name is only a basename
/// selector; paths and arbitrary helper names are rejected.
pub fn verified_installed_core(caller_name: &str) -> Result<PathBuf, ErrorCode> {
    if !matches!(
        caller_name,
        "hantong" | "hantong-t" | "hantong-mcp" | "mail-hantong-core"
    ) || caller_name.contains('/')
        || caller_name.contains('\\')
    {
        return Err(ErrorCode::InvalidInput);
    }
    let caller = std::env::current_exe().map_err(|_| ErrorCode::CoreUnavailable)?;
    let caller_meta = open_owned(&caller)?
        .metadata()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if caller.file_name().and_then(|v| v.to_str()) != Some(caller_name) {
        return Err(ErrorCode::ScopeDenied);
    }
    let bin = caller.parent().ok_or(ErrorCode::CoreUnavailable)?;
    let version = bin.parent().ok_or(ErrorCode::CoreUnavailable)?;
    let manifest = version.join("release-manifest.json");
    validate_manifest(version, &manifest, caller_name)?;
    let core = bin.join("mail-hantong-core");
    let core_meta = open_owned(&core)?
        .metadata()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if core_meta.len() == 0 || caller_meta.len() == 0 {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(core)
}

fn validate_manifest(version: &Path, manifest: &Path, caller_name: &str) -> Result<(), ErrorCode> {
    let _version_guard = release_directory(version)?;
    let mut manifest_file = open_owned(manifest)?;
    let metadata = manifest_file
        .metadata()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if metadata.len() == 0 || metadata.len() > MAX_MANIFEST {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut bytes = Vec::new();
    (&mut manifest_file)
        .take(MAX_MANIFEST + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if bytes.len() as u64 != metadata.len() {
        return Err(ErrorCode::CoreUnavailable);
    }
    reject_duplicate_keys(&bytes)?;
    let object = parse_exact_object(
        &bytes,
        &[
            "format_version",
            "version",
            "package_version",
            "target",
            "files",
        ],
    )?;
    if object.get("format_version").and_then(Value::as_u64) != Some(1)
        || object
            .get("version")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || object
            .get("package_version")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || object.get("target").and_then(Value::as_str) != Some(unix_target())
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    let files = object
        .get("files")
        .and_then(Value::as_array)
        .ok_or(ErrorCode::CoreUnavailable)?;
    if files.is_empty() || files.len() > 4096 {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut paths = BTreeSet::new();
    for item in files {
        let record = item.as_object().ok_or(ErrorCode::CoreUnavailable)?;
        if record.len() != 3
            || !record.contains_key("path")
            || !record.contains_key("sha256")
            || !record.contains_key("size")
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        let relative = record
            .get("path")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::CoreUnavailable)?;
        let digest = record
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::CoreUnavailable)?;
        let expected_size = record
            .get("size")
            .and_then(Value::as_u64)
            .ok_or(ErrorCode::CoreUnavailable)?;
        if !safe_relative(relative)
            || digest.len() != 64
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            || expected_size > MAX_RELEASE_FILE
            || !paths.insert(relative.to_owned())
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        let path = version.join(relative);
        let mut file = open_owned(&path)?;
        if file
            .metadata()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .len()
            != expected_size
            || wire_hex::encode(&crate::unix_crypto::sha256_release_handle(&mut file)?) != digest
        {
            return Err(ErrorCode::CoreUnavailable);
        }
    }
    let core = version.join("bin/mail-hantong-core");
    let cli = version.join("bin/hantong");
    let caller_path = format!("bin/{caller_name}");
    if !paths.contains("bin/mail-hantong-core")
        || !paths.contains("bin/hantong")
        || !paths.contains(&caller_path)
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    open_owned(&core)?;
    open_owned(&cli)?;
    Ok(())
}

fn parse_exact_object(bytes: &[u8], keys: &[&str]) -> Result<Map<String, Value>, ErrorCode> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    let object = value.as_object().ok_or(ErrorCode::CoreUnavailable)?;
    let expected: BTreeSet<&str> = keys.iter().copied().collect();
    if object.len() != expected.len() || object.keys().any(|key| !expected.contains(key.as_str())) {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(object.clone())
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 512
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
}

fn open_owned(path: &Path) -> Result<File, ErrorCode> {
    let parent = path.parent().ok_or(ErrorCode::CoreUnavailable)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ErrorCode::CoreUnavailable)?;
    release_directory(parent)?.open_existing_release_file(name)
}

fn release_directory(path: &Path) -> Result<UnixDirectoryGuard, ErrorCode> {
    let guard = UnixDirectoryGuard::open(path, false).map_err(|error| match error {
        ErrorCode::CapabilityUnsupported | ErrorCode::ScopeDenied => ErrorCode::ScopeDenied,
        _ => ErrorCode::CoreUnavailable,
    })?;
    let held = guard
        .directory_fd()?
        .try_clone_to_owned()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let metadata = File::from(held)
        .metadata()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if metadata.uid() != crate::unix_peer::current_peer_context()?.effective_uid
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(guard)
}

fn unix_target() -> &'static str {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "linux-aarch64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x86_64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-aarch64"
    } else {
        "unix-unknown"
    }
}

// serde_json follows the last duplicate member. Release metadata must reject
// that ambiguity instead of allowing a later member to replace an earlier one.
fn reject_duplicate_keys(bytes: &[u8]) -> Result<(), ErrorCode> {
    const MAX_OBJECT_DEPTH: usize = 8;
    let mut stack: Vec<BTreeSet<String>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i = i.saturating_add(2);
                } else if bytes[i] == b'"' {
                    i += 1;
                    break;
                } else {
                    i += 1;
                }
            }
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len()
                && bytes[j] == b':'
                && let Some(set) = stack.last_mut()
            {
                let key: String = serde_json::from_slice(&bytes[start..i])
                    .map_err(|_| ErrorCode::CoreUnavailable)?;
                if !set.insert(key) {
                    return Err(ErrorCode::CoreUnavailable);
                }
            }
            continue;
        }
        if bytes[i] == b'{' {
            if stack.len() >= MAX_OBJECT_DEPTH {
                return Err(ErrorCode::CoreUnavailable);
            }
            stack.push(BTreeSet::new());
        } else if bytes[i] == b'}' && stack.pop().is_none() {
            return Err(ErrorCode::CoreUnavailable);
        }
        i += 1;
    }
    if !stack.is_empty() {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha256_release_file;
    use std::os::unix::fs::PermissionsExt;

    fn fixture() -> (std::path::PathBuf, std::path::PathBuf) {
        // macOS exposes `/var` as a symlink to `/private/var`; release
        // validation intentionally rejects symlinked ancestors, so construct
        // the fixture below the canonical temporary directory.
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp dir");
        let root = temp.join(format!(
            "hantong-release-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).expect("fixture directories");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
        for (name, bytes) in [
            ("hantong", b"cli".as_slice()),
            ("hantong-mcp", b"mcp".as_slice()),
            ("mail-hantong-core", b"core".as_slice()),
        ] {
            std::fs::write(bin.join(name), bytes).expect("fixture binary");
            std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let files = ["bin/hantong", "bin/hantong-mcp", "bin/mail-hantong-core"]
            .into_iter()
            .map(|path| {
                let file = root.join(path);
                serde_json::json!({
                    "path": path,
                    "sha256": wire_hex::encode(&sha256_release_file(&file).unwrap()),
                    "size": std::fs::metadata(file).unwrap().len(),
                })
            })
            .collect::<Vec<_>>();
        let manifest = root.join("release-manifest.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "format_version": 1,
                "version": "0.1.0-test",
                "package_version": "0.1.0-test",
                "target": unix_target(),
                "files": files,
            }))
            .unwrap(),
        )
        .unwrap();
        (root, manifest)
    }

    #[test]
    fn sec03_release_paths_reject_traversal_and_absolute_names() {
        for path in [
            "../core",
            "/tmp/core",
            "bin\\core",
            "bin//core",
            "bin/./core",
        ] {
            assert!(!safe_relative(path));
        }
        assert!(safe_relative("bin/mail-hantong-core"));
    }

    #[test]
    fn cli07_target_is_platform_specific() {
        assert!(
            unix_target().starts_with("linux-")
                || unix_target().starts_with("macos-")
                || unix_target() == "unix-unknown"
        );
    }

    #[test]
    fn sec03_release_manifest_rejects_duplicate_keys() {
        assert!(matches!(
            reject_duplicate_keys(br#"{"files":[],"files":[]}"#),
            Err(ErrorCode::CoreUnavailable)
        ));
    }

    #[test]
    fn sec03_release_manifest_binds_caller_and_file_hashes() {
        let (root, manifest) = fixture();
        assert_eq!(validate_manifest(&root, &manifest, "hantong"), Ok(()));
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&root, &alias).expect("ancestor symlink");
        assert_eq!(
            validate_manifest(&alias, &alias.join("release-manifest.json"), "hantong"),
            Err(ErrorCode::ScopeDenied)
        );
        for mode in [0o777, 0o1777] {
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(mode)).unwrap();
            assert_eq!(
                validate_manifest(&root, &manifest, "hantong"),
                Err(ErrorCode::ScopeDenied)
            );
        }
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            validate_manifest(&root, &manifest, "missing-caller"),
            Err(ErrorCode::CoreUnavailable)
        );
        std::fs::write(root.join("bin/mail-hantong-core"), b"changed").unwrap();
        assert_eq!(
            validate_manifest(&root, &manifest, "hantong"),
            Err(ErrorCode::CoreUnavailable)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_release_manifest_rejects_symlinked_bin_and_manifest() {
        let (root, manifest) = fixture();
        let bin = root.join("bin");
        let held = root.join("held-bin");
        std::fs::rename(&bin, &held).unwrap();
        std::os::unix::fs::symlink(&held, &bin).unwrap();
        assert_eq!(
            validate_manifest(&root, &manifest, "hantong"),
            Err(ErrorCode::ScopeDenied)
        );
        std::fs::remove_file(&bin).unwrap();
        std::fs::rename(&held, &bin).unwrap();
        let held = root.join("held-manifest.json");
        std::fs::rename(&manifest, &held).unwrap();
        std::os::unix::fs::symlink(&held, &manifest).unwrap();
        assert_eq!(
            validate_manifest(&root, &manifest, "hantong"),
            Err(ErrorCode::ScopeDenied)
        );
        std::fs::remove_file(&manifest).unwrap();
        std::fs::rename(&held, &manifest).unwrap();
        assert_eq!(validate_manifest(&root, &manifest, "hantong"), Ok(()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_release_hash_uses_the_owned_open_file_after_path_replacement() {
        let (root, manifest) = fixture();
        let core = root.join("bin/mail-hantong-core");
        let expected = sha256_release_file(&core).unwrap();
        let mut opened = open_owned(&core).unwrap();
        std::fs::rename(&core, root.join("held-core")).unwrap();
        std::fs::write(&core, b"changed").unwrap();
        std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            crate::unix_crypto::sha256_release_handle(&mut opened),
            Ok(expected)
        );
        assert_eq!(
            validate_manifest(&root, &manifest, "hantong"),
            Err(ErrorCode::CoreUnavailable)
        );
        drop(opened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_release_manifest_rejects_unsafe_path_and_deep_objects() {
        let (root, manifest) = fixture();
        let unsafe_manifest = format!(
            r#"{{"format_version":1,"version":"v","package_version":"p","target":"{}","files":[{{"path":"../escape","sha256":"{}","size":1}}]}}"#,
            unix_target(),
            "0".repeat(64)
        );
        std::fs::write(&manifest, unsafe_manifest).unwrap();
        assert_eq!(
            validate_manifest(&root, &manifest, "hantong"),
            Err(ErrorCode::CoreUnavailable)
        );
        let mut deep = String::new();
        for _ in 0..10 {
            deep.push('{');
        }
        deep.push_str("\"x\":1");
        for _ in 0..10 {
            deep.push('}');
        }
        assert_eq!(
            reject_duplicate_keys(deep.as_bytes()),
            Err(ErrorCode::CoreUnavailable)
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
