//! Release-owned executable validation for Unix Core helper launches.

#![cfg(unix)]

#[cfg(any(not(target_os = "linux"), test))]
use crate::{MAX_RELEASE_FILE, unix_paths::UnixDirectoryGuard};
use hantong_contracts::ErrorCode;
#[cfg(any(not(target_os = "linux"), test))]
use hantong_contracts::wire_hex;
#[cfg(any(not(target_os = "linux"), test))]
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::fs::File;
#[cfg(any(not(target_os = "linux"), test))]
use std::io::Read;
use std::io::{Seek, SeekFrom};
use std::os::fd::AsRawFd;
#[cfg(any(not(target_os = "linux"), test))]
use std::os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, PermissionsExt},
    process::CommandExt,
};
#[cfg(any(not(target_os = "linux"), test))]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Output, Stdio};

#[cfg(any(not(target_os = "linux"), test))]
const MAX_MANIFEST: u64 = 4 * 1024 * 1024;

/// Validates the installed release containing the current executable and
/// returns its sibling Core executable. The caller name is only a basename
/// selector; paths and arbitrary helper names are rejected.
pub fn verified_installed_core(caller_name: &str) -> Result<PathBuf, ErrorCode> {
    #[cfg(target_os = "linux")]
    {
        if crate::linux_identity::current_asset()? != format!("bin/{caller_name}") {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(crate::linux_identity::current_release_root()?.join("bin/mail-hantong-core"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let (path, _, _, _) = verified_core(caller_name)?;
        Ok(path)
    }
}

/// Builds a command retaining the exact Core file hashed by the manifest.
/// Linux executes through that descriptor, never reopening the executable by
/// package path. macOS has no fexecve: its final path check narrows, but cannot
/// eliminate, the same-UID check-to-exec race. Neither permits a path fallback.
pub fn verified_core_command(caller_name: &str) -> Result<VerifiedCoreCommand, ErrorCode> {
    verified_sibling_command(caller_name, "bin/mail-hantong-core")
}

/// Only these fixed package assets may be launched; arbitrary paths are rejected.
pub fn verified_sibling_command(
    caller_name: &str,
    asset: &str,
) -> Result<VerifiedCoreCommand, ErrorCode> {
    #[cfg(target_os = "linux")]
    {
        VerifiedCoreCommand::from_launch(crate::linux_identity::sibling(caller_name, asset)?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        if !matches!(
            asset,
            "bin/mail-hantong-core" | "bin/hantong" | "bin/hantong-t" | "update.sh"
        ) {
            return Err(ErrorCode::InvalidInput);
        }
        let (path, file, snapshot, digest) = verified_asset(caller_name, asset)?;
        guarded_command(&path, file, snapshot, digest)
    }
}

/// No raw Command is exposed: each launch rehashes the retained image in the
/// parent, before fork. The child only performs async-signal-safe checks.
pub struct VerifiedCoreCommand {
    command: Command,
    file: File,
    snapshot: FileSnapshot,
    digest: [u8; 32],
    #[cfg(target_os = "linux")]
    roles: Option<[File; 5]>,
}

impl VerifiedCoreCommand {
    #[cfg(target_os = "linux")]
    pub(crate) fn from_launch(
        launch: crate::linux_identity::LaunchImage,
    ) -> Result<Self, ErrorCode> {
        let file = launch.roles[1]
            .try_clone()
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        let snapshot = FileSnapshot::read(&file)?;
        let mut measured = file.try_clone().map_err(|_| ErrorCode::CoreUnavailable)?;
        // Sealed readback leaves the shared file description at EOF.
        measured
            .seek(SeekFrom::Start(0))
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        let digest = crate::unix_crypto::sha256_release_handle(&mut measured)?;
        Ok(Self {
            command: launch.command,
            file,
            snapshot,
            digest,
            roles: Some(launch.roles),
        })
    }
    pub fn arg(&mut self, arg: impl AsRef<std::ffi::OsStr>) -> &mut Self {
        self.command.arg(arg);
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        self.command.args(args);
        self
    }

    pub fn stdin(&mut self, value: impl Into<Stdio>) -> &mut Self {
        self.command.stdin(value);
        self
    }

    pub fn stdout(&mut self, value: impl Into<Stdio>) -> &mut Self {
        self.command.stdout(value);
        self
    }

    pub fn stderr(&mut self, value: impl Into<Stdio>) -> &mut Self {
        self.command.stderr(value);
        self
    }

    fn recheck(&mut self) -> std::io::Result<()> {
        let denied = || std::io::Error::from_raw_os_error(libc::EPERM);
        #[cfg(target_os = "linux")]
        if let Some(roles) = &mut self.roles {
            crate::linux_sealed::validate_sealed(
                &mut roles[1],
                self.snapshot.size as u64,
                self.digest,
                true,
            )
            .map_err(|_| denied())?;
        }
        self.file.seek(SeekFrom::Start(0))?;
        if FileSnapshot::read(&self.file).map_err(|_| denied())? != self.snapshot
            || crate::unix_crypto::sha256_release_handle(&mut self.file).map_err(|_| denied())?
                != self.digest
            || FileSnapshot::read(&self.file).map_err(|_| denied())? != self.snapshot
        {
            return Err(denied());
        }
        Ok(())
    }

    pub fn spawn(&mut self) -> std::io::Result<Child> {
        self.recheck()?;
        self.command.spawn()
    }

    pub fn status(&mut self) -> std::io::Result<ExitStatus> {
        self.spawn()?.wait()
    }

    pub fn output(&mut self) -> std::io::Result<Output> {
        self.recheck()?;
        self.command.output()
    }
}

#[cfg(not(target_os = "linux"))]
fn verified_core(caller_name: &str) -> Result<(PathBuf, File, FileSnapshot, [u8; 32]), ErrorCode> {
    verified_asset(caller_name, "bin/mail-hantong-core")
}

#[cfg(not(target_os = "linux"))]
fn verified_asset(
    caller_name: &str,
    asset: &str,
) -> Result<(PathBuf, File, FileSnapshot, [u8; 32]), ErrorCode> {
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
    let (core_file, snapshot, digest) =
        validate_manifest_asset(version, &manifest, caller_name, asset)?;
    let core = version.join(asset);
    if snapshot.size == 0 || caller_meta.len() == 0 {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok((core, core_file, snapshot, digest))
}

#[cfg(test)]
fn validate_manifest(version: &Path, manifest: &Path, caller_name: &str) -> Result<(), ErrorCode> {
    validate_manifest_file(version, manifest, caller_name).map(drop)
}

#[cfg(test)]
fn validate_manifest_file(
    version: &Path,
    manifest: &Path,
    caller_name: &str,
) -> Result<(File, FileSnapshot, [u8; 32]), ErrorCode> {
    validate_manifest_asset(version, manifest, caller_name, "bin/mail-hantong-core")
}

#[cfg(any(not(target_os = "linux"), test))]
fn validate_manifest_asset(
    version: &Path,
    manifest: &Path,
    caller_name: &str,
    asset: &str,
) -> Result<(File, FileSnapshot, [u8; 32]), ErrorCode> {
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
    let mut core_file = None;
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
        let before = FileSnapshot::read(&file)?;
        if file
            .metadata()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .len()
            != expected_size
            || wire_hex::encode(&crate::unix_crypto::sha256_release_handle(&mut file)?) != digest
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        if FileSnapshot::read(&file)? != before {
            return Err(ErrorCode::ScopeDenied);
        }
        if relative == asset {
            core_file = Some((
                file,
                before,
                wire_hex::decode_fixed::<32>(digest).map_err(|_| ErrorCode::CoreUnavailable)?,
            ));
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
    core_file.ok_or(ErrorCode::CoreUnavailable)
}

#[derive(Clone, Copy, Eq, PartialEq)]
struct FileSnapshot {
    device: libc::dev_t,
    inode: libc::ino_t,
    mode: libc::mode_t,
    owner: libc::uid_t,
    group: libc::gid_t,
    links: libc::nlink_t,
    size: libc::off_t,
    modified: libc::time_t,
    modified_ns: libc::c_long,
    changed: libc::time_t,
    changed_ns: libc::c_long,
}

impl FileSnapshot {
    fn from_stat(value: &libc::stat) -> Self {
        Self {
            device: value.st_dev,
            inode: value.st_ino,
            mode: value.st_mode,
            owner: value.st_uid,
            group: value.st_gid,
            links: value.st_nlink,
            size: value.st_size,
            modified: value.st_mtime,
            modified_ns: value.st_mtime_nsec,
            changed: value.st_ctime,
            changed_ns: value.st_ctime_nsec,
        }
    }

    #[allow(unsafe_code)]
    fn read(file: &File) -> Result<Self, ErrorCode> {
        let mut value = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstat initializes the output on success; the File owns fd.
        if unsafe { libc::fstat(file.as_raw_fd(), value.as_mut_ptr()) } != 0 {
            return Err(ErrorCode::CoreUnavailable);
        }
        let value = unsafe { value.assume_init() };
        Ok(Self::from_stat(&value))
    }
}

#[allow(unsafe_code)]
#[cfg(any(not(target_os = "linux"), test))]
fn guarded_command(
    path: &Path,
    file: File,
    expected: FileSnapshot,
    digest: [u8; 32],
) -> Result<VerifiedCoreCommand, ErrorCode> {
    if expected.mode & 0o111 == 0 {
        return Err(ErrorCode::ScopeDenied);
    }
    let mut paths = Vec::new();
    let script = path.file_name().is_some_and(|name| name == "update.sh");
    let bin = path.parent().ok_or(ErrorCode::CoreUnavailable)?;
    let version = if script {
        bin
    } else {
        bin.parent().ok_or(ErrorCode::CoreUnavailable)?
    };
    for name in [version, bin, path] {
        let held = if name == path {
            file.try_clone().map_err(|_| ErrorCode::CoreUnavailable)?
        } else {
            File::from(
                release_directory(name)?
                    .directory_fd()?
                    .try_clone_to_owned()
                    .map_err(|_| ErrorCode::CoreUnavailable)?,
            )
        };
        let snapshot = if name == path {
            expected
        } else {
            FileSnapshot::read(&held)?
        };
        let name = std::ffi::CString::new(name.as_os_str().as_bytes())
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        paths.push((name, snapshot));
    }
    let held = file.try_clone().map_err(|_| ErrorCode::CoreUnavailable)?;
    #[cfg(target_os = "linux")]
    let fd_path = format!("/proc/self/fd/{}", held.as_raw_fd());
    #[cfg(not(target_os = "linux"))]
    let fd_path = format!("/dev/fd/{}", held.as_raw_fd());
    let mut command = if script {
        let mut shell = Command::new("/bin/sh");
        shell.arg(&fd_path);
        shell
    } else {
        #[cfg(target_os = "linux")]
        {
            Command::new(&fd_path)
        }
        #[cfg(not(target_os = "linux"))]
        {
            Command::new(path)
        }
    };
    if !script {
        command.arg0(path);
    }
    // SAFETY: all allocation, path encoding and hashing precede fork. Only
    // async-signal-safe fstat/lstat and comparisons run in the child. Capturing
    // File keeps its CLOEXEC descriptor alive through the kernel exec lookup.
    unsafe {
        command.pre_exec(move || {
            let denied = || std::io::Error::from_raw_os_error(libc::EPERM);
            let mut value = std::mem::MaybeUninit::<libc::stat>::uninit();
            if libc::fstat(held.as_raw_fd(), value.as_mut_ptr()) != 0
                || FileSnapshot::from_stat(&value.assume_init()) != expected
            {
                return Err(denied());
            }
            for (name, snapshot) in &paths {
                if libc::lstat(name.as_ptr(), value.as_mut_ptr()) != 0
                    || FileSnapshot::from_stat(&value.assume_init()) != *snapshot
                {
                    return Err(denied());
                }
            }
            // The fixed OS shell must read this public script fd after exec.
            // Only the public asset descriptor loses CLOEXEC; no secret fd does.
            if script
                && (libc::lseek(held.as_raw_fd(), 0, libc::SEEK_SET) != 0
                    || libc::fcntl(held.as_raw_fd(), libc::F_SETFD, 0) != 0)
            {
                return Err(denied());
            }
            Ok(())
        });
    }
    Ok(VerifiedCoreCommand {
        command,
        file,
        snapshot: expected,
        digest,
        #[cfg(target_os = "linux")]
        roles: None,
    })
}

#[cfg(any(not(target_os = "linux"), test))]
fn parse_exact_object(bytes: &[u8], keys: &[&str]) -> Result<Map<String, Value>, ErrorCode> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    let object = value.as_object().ok_or(ErrorCode::CoreUnavailable)?;
    let expected: BTreeSet<&str> = keys.iter().copied().collect();
    if object.len() != expected.len() || object.keys().any(|key| !expected.contains(key.as_str())) {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(object.clone())
}

pub(crate) fn safe_relative(path: &str) -> bool {
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

#[cfg(any(not(target_os = "linux"), test))]
fn open_owned(path: &Path) -> Result<File, ErrorCode> {
    let parent = path.parent().ok_or(ErrorCode::CoreUnavailable)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ErrorCode::CoreUnavailable)?;
    release_directory(parent)?.open_existing_release_file(name)
}

#[cfg(any(not(target_os = "linux"), test))]
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

#[cfg(any(not(target_os = "linux"), test))]
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
pub(crate) fn reject_duplicate_keys(bytes: &[u8]) -> Result<(), ErrorCode> {
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    fn replacement_program() -> &'static str {
        if cfg!(target_os = "macos") {
            "/usr/bin/false"
        } else {
            "/bin/false"
        }
    }

    fn fixture() -> (std::path::PathBuf, std::path::PathBuf) {
        // macOS exposes `/var` as a symlink to `/private/var`; release
        // validation intentionally rejects symlinked ancestors, so construct
        // the fixture below the canonical temporary directory.
        let temp = std::fs::canonicalize(std::env::temp_dir()).expect("canonical temp dir");
        let root = temp.join(format!(
            "hantong-release-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let bin = root.join("bin");
        std::fs::create_dir(&root).expect("fixture root");
        std::fs::create_dir(&bin).expect("fixture bin");
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

    fn executable_fixture() -> (PathBuf, File, FileSnapshot, [u8; 32]) {
        let (root, manifest) = fixture();
        let core = root.join("bin/mail-hantong-core");
        std::fs::copy("/bin/echo", &core).unwrap();
        std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut value: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        let record = value["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|record| record["path"] == "bin/mail-hantong-core")
            .unwrap();
        record["size"] = Value::from(std::fs::metadata(&core).unwrap().len());
        record["sha256"] = Value::from(wire_hex::encode(&sha256_release_file(&core).unwrap()));
        std::fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let (file, snapshot, digest) = validate_manifest_file(&root, &manifest, "hantong").unwrap();
        (root, file, snapshot, digest)
    }

    #[test]
    fn sec03_verified_launch_preserves_native_arguments_and_pipes() {
        let (root, file, snapshot, digest) = executable_fixture();
        let output = guarded_command(&root.join("bin/mail-hantong-core"), file, snapshot, digest)
            .unwrap()
            .arg("synthetic-native-launch")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"synthetic-native-launch\n");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sec03_verified_launch_rejects_replacement_symlink_write_and_mode_changes() {
        use std::io::{Seek, SeekFrom, Write};
        for change in 0..5 {
            let (root, file, snapshot, digest) = executable_fixture();
            let core = root.join("bin/mail-hantong-core");
            let mut command = guarded_command(&core, file, snapshot, digest).unwrap();
            match change {
                0 => {
                    std::fs::rename(&core, root.join("held-core")).unwrap();
                    std::fs::copy(replacement_program(), &core).unwrap();
                }
                1 => {
                    std::fs::rename(&core, root.join("held-core")).unwrap();
                    std::os::unix::fs::symlink(root.join("held-core"), &core).unwrap();
                }
                2 => {
                    let mut writer = std::fs::OpenOptions::new().write(true).open(&core).unwrap();
                    writer.seek(SeekFrom::End(-1)).unwrap();
                    writer.write_all(b"x").unwrap();
                    writer.sync_all().unwrap();
                }
                3 => {
                    std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o777)).unwrap()
                }
                _ => {
                    std::fs::rename(root.join("bin"), root.join("held-bin")).unwrap();
                    std::fs::create_dir(root.join("bin")).unwrap();
                    std::fs::copy(replacement_program(), &core).unwrap();
                }
            }
            assert!(
                command.output().is_err(),
                "changed launch must fail: {change}"
            );
            drop(command);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn sec03_verified_launch_keeps_hash_snapshot_before_command_construction() {
        let (root, file, snapshot, digest) = executable_fixture();
        let core = root.join("bin/mail-hantong-core");
        std::fs::copy(replacement_program(), &core).unwrap();
        let mut command = guarded_command(&core, file, snapshot, digest).unwrap();
        assert!(command.output().is_err());
        drop(command);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sec03_competing_replacement_never_executes_unverified_program() {
        for _ in 0..16 {
            let (root, file, snapshot, digest) = executable_fixture();
            let core = root.join("bin/mail-hantong-core");
            let replacement = root.join("replacement");
            std::fs::copy(replacement_program(), &replacement).unwrap();
            let mut command = guarded_command(&core, file, snapshot, digest).unwrap();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let concurrent = barrier.clone();
            let writer = std::thread::spawn(move || {
                concurrent.wait();
                std::fs::rename(replacement, core).unwrap();
            });
            barrier.wait();
            let result = command.arg("original-only").output();
            writer.join().unwrap();
            if let Ok(output) = result {
                assert!(output.status.success(), "replacement must never execute");
                assert_eq!(output.stdout, b"original-only\n");
            }
            drop(command);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn sec03_sibling_launch_rejects_arbitrary_asset_paths() {
        for asset in [
            "../update.sh",
            "/bin/sh",
            "bin/untrusted-tui",
            "publisher/google-desktop.json",
        ] {
            assert!(matches!(
                verified_sibling_command("hantong", asset),
                Err(ErrorCode::InvalidInput)
            ));
        }
    }

    #[test]
    fn sec03_cli_and_tui_assets_reject_replacement_after_manifest_validation() {
        for asset in ["bin/hantong", "bin/hantong-t"] {
            let (root, manifest) = fixture();
            let path = root.join(asset);
            std::fs::copy("/bin/echo", &path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut value: Value =
                serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
            let entries = value["files"].as_array_mut().unwrap();
            entries.retain(|entry| entry["path"] != asset);
            entries.push(serde_json::json!({
                "path":asset, "size":std::fs::metadata(&path).unwrap().len(),
                "sha256":wire_hex::encode(&sha256_release_file(&path).unwrap()),
            }));
            std::fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
            let (file, snapshot, digest) =
                validate_manifest_asset(&root, &manifest, "hantong", asset).unwrap();
            let mut command = guarded_command(&path, file, snapshot, digest).unwrap();
            std::fs::rename(&path, root.join("held-asset")).unwrap();
            std::fs::copy(replacement_program(), &path).unwrap();
            assert!(command.output().is_err());
            drop(command);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    #[allow(unsafe_code)]
    fn sec03_updater_uses_manifest_bound_script_fd_even_after_final_path_replacement() {
        let (root, manifest) = fixture();
        let script = root.join("update.sh");
        std::fs::write(
            &script,
            b"#!/bin/sh\nprintf 'verified-script:%s\\n' \"$1\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut value: Value = serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
        value["files"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "path":"update.sh", "size":std::fs::metadata(&script).unwrap().len(),
                "sha256":wire_hex::encode(&sha256_release_file(&script).unwrap()),
            }));
        std::fs::write(&manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        let replacement = root.join("replacement-script");
        std::fs::write(&replacement, b"#!/bin/sh\nexit 99\n").unwrap();
        let (file, snapshot, digest) =
            validate_manifest_asset(&root, &manifest, "hantong", "update.sh").unwrap();
        let mut command = guarded_command(&script, file, snapshot, digest).unwrap();
        let replacement = std::ffi::CString::new(replacement.as_os_str().as_bytes()).unwrap();
        let destination = std::ffi::CString::new(script.as_os_str().as_bytes()).unwrap();
        unsafe {
            command.command.pre_exec(move || {
                if libc::rename(replacement.as_ptr(), destination.as_ptr()) != 0 {
                    return Err(std::io::Error::from_raw_os_error(libc::EIO));
                }
                Ok(())
            });
        }
        let output = command.arg("argument-preserved").output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"verified-script:argument-preserved\n");
        drop(command);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[allow(unsafe_code)]
    fn sec03_descriptor_exec_does_not_execute_replacement_after_final_check() {
        let (root, file, snapshot, digest) = executable_fixture();
        let core = root.join("bin/mail-hantong-core");
        let replacement = root.join("replacement");
        std::fs::copy(replacement_program(), &replacement).unwrap();
        let replacement = std::ffi::CString::new(replacement.as_os_str().as_bytes()).unwrap();
        let destination = std::ffi::CString::new(core.as_os_str().as_bytes()).unwrap();
        let mut command = guarded_command(&core, file, snapshot, digest).unwrap();
        // Runs after the production pre_exec guard: force the exact remaining
        // rename race, rather than relying on a scheduler to hit the window.
        unsafe {
            command.command.pre_exec(move || {
                if libc::rename(replacement.as_ptr(), destination.as_ptr()) != 0 {
                    return Err(std::io::Error::from_raw_os_error(libc::EIO));
                }
                Ok(())
            });
        }
        let output = command.arg("held-inode").output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"held-inode\n");
        drop(command);
        std::fs::remove_dir_all(root).unwrap();
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
