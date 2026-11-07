//! Adopted Linux release context and sealed execution admission (E1–E5).
#![cfg(target_os = "linux")]
#![allow(unsafe_code)]

use crate::{linux_sealed, unix_paths::UnixDirectoryGuard};
use hantong_contracts::{ErrorCode, wire_hex};
use serde_json::{Map, Value, json};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const CONTEXT_KEYS: &[&str] = &[
    "protocol_version",
    "target",
    "release_version",
    "source_commit",
    "canonical_source_sha256",
    "descriptor_sha256",
    "manifest_sha256",
    "asset_relative_path",
    "asset_size",
    "asset_sha256",
];
const DESCRIPTOR_KEYS: &[&str] = &[
    "schema_version",
    "version",
    "package_version",
    "target",
    "commit",
    "source_filename",
    "cargo_lock_sha256",
    "source_sha256",
    "source_archive_sha256",
    "package_filename",
    "package_sha256",
];

/// This is constructed from local independent release-pin evidence, never peer JSON.
struct ReleaseTrust {
    root: PathBuf,
    descriptor: Vec<u8>,
    manifest: Vec<u8>,
    commit: String,
    source: String,
    version: String,
    descriptor_sha: [u8; 32],
    manifest_sha: [u8; 32],
}

pub(crate) struct LaunchImage {
    pub command: Command,
    pub roles: [File; 5],
}

fn denied<T>() -> Result<T, ErrorCode> {
    Err(ErrorCode::ScopeDenied)
}
fn sha(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .expect("SHA256")
}
fn exact(bytes: &[u8], keys: &[&str]) -> Result<Map<String, Value>, ErrorCode> {
    crate::unix_release::reject_duplicate_keys(bytes)?;
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::ScopeDenied)?;
    let object = value.as_object().ok_or(ErrorCode::ScopeDenied)?;
    if object.len() != keys.len() || keys.iter().any(|key| !object.contains_key(*key)) {
        return denied();
    }
    Ok(object.clone())
}
fn string<'a>(map: &'a Map<String, Value>, key: &str) -> Result<&'a str, ErrorCode> {
    let text = map
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::ScopeDenied)?;
    if text.is_empty() || text.chars().any(char::is_control) {
        return denied();
    }
    Ok(text)
}
fn hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn digest(map: &Map<String, Value>, key: &str) -> Result<[u8; 32], ErrorCode> {
    let text = string(map, key)?;
    if !hex(text, 64) {
        return denied();
    }
    wire_hex::decode_fixed(text).map_err(|_| ErrorCode::ScopeDenied)
}
pub fn valid_version(text: &str) -> bool {
    if text.len() > 64 || text.is_empty() {
        return false;
    }
    let (base, suffix) = text
        .split_once('-')
        .map_or((text, None), |(a, b)| (a, Some(b)));
    let parts: Vec<_> = base.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
        && suffix.is_none_or(|s| {
            !s.is_empty()
                && s.split('.').all(|p| {
                    !p.is_empty()
                        && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        && (!p.bytes().all(|b| b.is_ascii_digit())
                            || p.len() == 1
                            || !p.starts_with('0'))
                })
        })
}
fn valid_asset(asset: &str) -> bool {
    matches!(
        asset,
        "bin/hantong" | "bin/hantong-t" | "bin/hantong-mcp" | "bin/mail-hantong-core"
    )
}
fn bounded(file: &mut File, max: u64) -> Result<Vec<u8>, ErrorCode> {
    let metadata = file.metadata().map_err(|_| ErrorCode::ScopeDenied)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > max {
        return denied();
    }
    file.seek(SeekFrom::Start(0))
        .map_err(|_| ErrorCode::ScopeDenied)?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::ScopeDenied)?;
    if bytes.len() as u64 != metadata.len() {
        return denied();
    }
    Ok(bytes)
}
fn public_file(path: &Path, max: u64) -> Result<Vec<u8>, ErrorCode> {
    let directory = UnixDirectoryGuard::open(path.parent().ok_or(ErrorCode::ScopeDenied)?, false)?;
    let name = path
        .file_name()
        .and_then(|p| p.to_str())
        .ok_or(ErrorCode::ScopeDenied)?;
    bounded(&mut directory.open_existing_release_file(name)?, max)
}
fn root_file(root: &Path) -> Result<File, ErrorCode> {
    let directory = UnixDirectoryGuard::open(root, false)?;
    let raw = unsafe {
        libc::openat(
            directory.directory_fd()?.as_raw_fd(),
            c".".as_ptr(),
            libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if raw < 0 {
        return denied();
    }
    let file = unsafe { File::from_raw_fd(raw) };
    let meta = file.metadata().map_err(|_| ErrorCode::ScopeDenied)?;
    if meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        return denied();
    }
    Ok(file)
}
fn object_equal(a: &File, b: &File) -> Result<bool, ErrorCode> {
    let a = a.metadata().map_err(|_| ErrorCode::ScopeDenied)?;
    let b = b.metadata().map_err(|_| ErrorCode::ScopeDenied)?;
    Ok(a.dev() == b.dev() && a.ino() == b.ino())
}
fn root_path(root: &File) -> Result<PathBuf, ErrorCode> {
    // This path is discovery only; the checked handle is compared again below.
    let path = std::fs::read_link(format!("/proc/self/fd/{}", root.as_raw_fd()))
        .map_err(|_| ErrorCode::ScopeDenied)?;
    if !path.is_absolute() || !object_equal(root, &root_file(&path)?)? {
        return denied();
    }
    Ok(path)
}

impl ReleaseTrust {
    fn load(root: &Path, version: &str, commit: &str) -> Result<Self, ErrorCode> {
        if !cfg!(target_arch = "x86_64")
            || !valid_version(version)
            || !hex(commit, 40)
            || root.file_name().and_then(|p| p.to_str()) != Some(version)
        {
            return denied();
        }
        let versions = root.parent().ok_or(ErrorCode::ScopeDenied)?;
        if versions.file_name().and_then(|p| p.to_str()) != Some("versions") {
            return denied();
        }
        let install = versions.parent().ok_or(ErrorCode::ScopeDenied)?;
        let evidence = install.join("release-pins").join(version).join(commit);
        let pin = exact(
            &public_file(&evidence.join("pin.json"), 4096)?,
            &[
                "format_version",
                "target",
                "version",
                "commit",
                "descriptor_sha256",
            ],
        )?;
        if pin.get("format_version").and_then(Value::as_u64) != Some(1)
            || string(&pin, "target")? != "linux-x86_64"
            || string(&pin, "version")? != version
            || string(&pin, "commit")? != commit
        {
            return denied();
        }
        let descriptor_sha = digest(&pin, "descriptor_sha256")?;
        let descriptor = public_file(&evidence.join("descriptor.json"), 256 * 1024)?;
        if sha(&descriptor) != descriptor_sha {
            return denied();
        }
        let desc = exact(&descriptor, DESCRIPTOR_KEYS)?;
        if desc.get("schema_version").and_then(Value::as_u64) != Some(1)
            || string(&desc, "target")? != "linux-x86_64"
            || string(&desc, "version")? != version
            || string(&desc, "commit")? != commit
            || !valid_version(string(&desc, "package_version")?)
            || string(&desc, "package_filename")?
                != format!("mail-hantong-cli-{version}-linux-x86_64.tar.gz")
            || string(&desc, "source_filename")? != format!("mail-hantong-source-{version}.zip")
        {
            return denied();
        }
        for key in [
            "cargo_lock_sha256",
            "source_sha256",
            "source_archive_sha256",
            "package_sha256",
        ] {
            digest(&desc, key)?;
        }
        let manifest = public_file(&root.join("release-manifest.json"), 4 * 1024 * 1024)?;
        let map = exact(
            &manifest,
            &[
                "format_version",
                "version",
                "package_version",
                "target",
                "files",
            ],
        )?;
        if map.get("format_version").and_then(Value::as_u64) != Some(1)
            || string(&map, "version")? != version
            || string(&map, "target")? != "linux-x86_64"
            || string(&map, "package_version")? != string(&desc, "package_version")?
        {
            return denied();
        }
        let metadata_bytes = public_file(&root.join("BUILD-METADATA.json"), 4 * 1024 * 1024)?;
        crate::unix_release::reject_duplicate_keys(&metadata_bytes)?;
        let metadata: Value =
            serde_json::from_slice(&metadata_bytes).map_err(|_| ErrorCode::ScopeDenied)?;
        if metadata.get("source_commit").and_then(Value::as_str) != Some(commit)
            || metadata.get("source_hash").and_then(Value::as_str)
                != Some(string(&desc, "source_sha256")?)
            || metadata.get("cargo_lock_sha256").and_then(Value::as_str)
                != Some(string(&desc, "cargo_lock_sha256")?)
            || metadata.get("build_performed").and_then(Value::as_bool) != Some(true)
        {
            return denied();
        }
        let source = string(&desc, "source_sha256")?.to_owned();
        Ok(Self {
            root: root.to_owned(),
            descriptor,
            manifest_sha: sha(&manifest),
            manifest,
            commit: commit.to_owned(),
            source,
            version: version.to_owned(),
            descriptor_sha,
        })
    }
    fn asset(&self, asset: &str) -> Result<(u64, [u8; 32]), ErrorCode> {
        if !valid_asset(asset) {
            return denied();
        }
        let manifest = exact(
            &self.manifest,
            &[
                "format_version",
                "version",
                "package_version",
                "target",
                "files",
            ],
        )?;
        let records = manifest["files"].as_array().ok_or(ErrorCode::ScopeDenied)?;
        if records.is_empty() || records.len() > 4096 {
            return denied();
        }
        let mut names = std::collections::BTreeSet::new();
        let mut result = None;
        for record in records {
            let map = record.as_object().ok_or(ErrorCode::ScopeDenied)?;
            if map.len() != 3 {
                return denied();
            }
            let name = string(map, "path")?;
            let size = map
                .get("size")
                .and_then(Value::as_u64)
                .ok_or(ErrorCode::ScopeDenied)?;
            let hash = digest(map, "sha256")?;
            if !crate::unix_release::safe_relative(name)
                || !names.insert(name.to_owned())
                || size > 64 * 1024 * 1024
            {
                return denied();
            }
            if name == asset {
                if size == 0 {
                    return denied();
                }
                result = Some((size, hash));
            }
        }
        result.ok_or(ErrorCode::ScopeDenied)
    }
    fn context(&self, asset: &str) -> Result<Vec<u8>, ErrorCode> {
        let (size, hash) = self.asset(asset)?;
        serde_json::to_vec(&json!({"protocol_version":2,"target":"linux-x86_64",
            "release_version":self.version,"source_commit":self.commit,"canonical_source_sha256":self.source,
            "descriptor_sha256":wire_hex::encode(&self.descriptor_sha),"manifest_sha256":wire_hex::encode(&self.manifest_sha),
            "asset_relative_path":asset,"asset_size":size,"asset_sha256":wire_hex::encode(&hash)})).map_err(|_|ErrorCode::ScopeDenied)
    }
    fn check_context(
        &self,
        context: &Map<String, Value>,
        expected_asset: &str,
    ) -> Result<(), ErrorCode> {
        let bytes = self.context(expected_asset)?;
        if exact(&bytes, CONTEXT_KEYS)? != *context {
            return denied();
        }
        Ok(())
    }
}

type RetainedRelease = (ReleaseTrust, Map<String, Value>, [File; 5]);

fn retained() -> Result<RetainedRelease, ErrorCode> {
    let mut roles = linux_sealed::checked_role_files(std::process::id())?;
    let context = exact(&bounded(&mut roles[2], 4096)?, CONTEXT_KEYS)?;
    if context.get("protocol_version").and_then(Value::as_u64) != Some(2)
        || string(&context, "target")? != "linux-x86_64"
    {
        return denied();
    }
    let root = root_path(&roles[0])?;
    let trust = ReleaseTrust::load(
        &root,
        string(&context, "release_version")?,
        string(&context, "source_commit")?,
    )?;
    let asset = string(&context, "asset_relative_path")?;
    trust.check_context(&context, asset)?;
    let (size, hash) = trust.asset(asset)?;
    linux_sealed::validate_sealed(&mut roles[1], size, hash, true)?;
    linux_sealed::validate_sealed(
        &mut roles[2],
        serde_json::to_vec(&context)
            .map_err(|_| ErrorCode::ScopeDenied)?
            .len() as u64,
        sha(&serde_json::to_vec(&context).map_err(|_| ErrorCode::ScopeDenied)?),
        false,
    )?;
    linux_sealed::validate_sealed(
        &mut roles[3],
        trust.manifest.len() as u64,
        trust.manifest_sha,
        false,
    )?;
    linux_sealed::validate_sealed(
        &mut roles[4],
        trust.descriptor.len() as u64,
        trust.descriptor_sha,
        false,
    )?;
    if !linux_sealed::proc_image_matches(std::process::id(), &roles[1])? {
        return denied();
    }
    Ok((trust, context, roles))
}

pub fn current_asset() -> Result<String, ErrorCode> {
    let (_, context, _) = retained()?;
    Ok(string(&context, "asset_relative_path")?.to_owned())
}
pub fn current_release_root() -> Result<PathBuf, ErrorCode> {
    Ok(retained()?.0.root)
}

pub fn peer_same_asset(pid: u32, expected_asset: &str) -> Result<bool, ErrorCode> {
    let (trust, _, own) = retained()?;
    let mut peer = linux_sealed::checked_role_files(pid)?;
    if !object_equal(&own[0], &peer[0])? || !linux_sealed::proc_image_matches(pid, &peer[1])? {
        return Ok(false);
    }
    let bytes = bounded(&mut peer[2], 4096)?;
    let context = exact(&bytes, CONTEXT_KEYS)?;
    trust.check_context(&context, expected_asset)?;
    let (size, hash) = trust.asset(expected_asset)?;
    linux_sealed::validate_sealed(&mut peer[1], size, hash, true)?;
    linux_sealed::validate_sealed(&mut peer[2], bytes.len() as u64, sha(&bytes), false)?;
    linux_sealed::validate_sealed(
        &mut peer[3],
        trust.manifest.len() as u64,
        trust.manifest_sha,
        false,
    )?;
    linux_sealed::validate_sealed(
        &mut peer[4],
        trust.descriptor.len() as u64,
        trust.descriptor_sha,
        false,
    )?;
    linux_sealed::proc_image_matches(pid, &peer[1])
}

fn sealed_bytes(bytes: &[u8]) -> Result<File, ErrorCode> {
    // A private anonymous staging file never serves as authority or executable.
    let fd = unsafe {
        libc::memfd_create(
            c"hantong-public-staging".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING | 0x0008,
        )
    };
    if fd < 0 {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    let mut source = unsafe { File::from_raw_fd(fd) };
    std::io::Write::write_all(&mut source, bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    linux_sealed::seal_copy(&mut source, bytes.len() as u64, sha(bytes), false)
}
fn launch(trust: ReleaseTrust, asset: &str) -> Result<LaunchImage, ErrorCode> {
    let (size, hash) = trust.asset(asset)?;
    let path = trust.root.join(asset);
    let parent = UnixDirectoryGuard::open(path.parent().ok_or(ErrorCode::ScopeDenied)?, false)?;
    let mut image = parent.open_existing_release_file(
        path.file_name()
            .and_then(|v| v.to_str())
            .ok_or(ErrorCode::ScopeDenied)?,
    )?;
    let image = linux_sealed::seal_copy(&mut image, size, hash, true)?;
    let context = trust.context(asset)?;
    let roles = [
        root_file(&trust.root)?,
        image,
        sealed_bytes(&context)?,
        sealed_bytes(&trust.manifest)?,
        sealed_bytes(&trust.descriptor)?,
    ];
    let mut command = Command::new("/proc/self/fd/198");
    command.arg0(&path);
    linux_sealed::configure_role_inheritance(&mut command, &roles)
        .map_err(|_| ErrorCode::ScopeDenied)?;
    Ok(LaunchImage { command, roles })
}
pub(crate) fn sibling(caller: &str, asset: &str) -> Result<LaunchImage, ErrorCode> {
    if !valid_asset(asset) {
        return Err(ErrorCode::InvalidInput);
    }
    let (trust, context, _) = retained()?;
    if string(&context, "asset_relative_path")? != format!("bin/{caller}") {
        return denied();
    }
    launch(trust, asset)
}

/// Only the native verified stager supplies these independent expected hashes.
pub(crate) fn candidate_command(
    root: &Path,
    version: &str,
    commit: &str,
    expected_descriptor_sha: [u8; 32],
    expected_manifest_sha: [u8; 32],
    asset: &str,
) -> Result<crate::unix_release::VerifiedCoreCommand, ErrorCode> {
    let trust = ReleaseTrust::load(root, version, commit)?;
    if trust.descriptor_sha != expected_descriptor_sha
        || trust.manifest_sha != expected_manifest_sha
    {
        return denied();
    }
    crate::unix_release::VerifiedCoreCommand::from_launch(launch(trust, asset)?)
}

/// All production binary entries call this before protected work. A malformed
/// inherited role is never treated as permission for legacy pathname execution.
pub fn ensure_entry(asset: &str) -> Result<(), ErrorCode> {
    if !valid_asset(asset) || !cfg!(target_arch = "x86_64") {
        return denied();
    }
    let mut occupied = false;
    for fd in 197..=201 {
        let rc = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        if rc >= 0 {
            occupied = true;
        } else if std::io::Error::last_os_error().raw_os_error() != Some(libc::EBADF) {
            return denied();
        }
    }
    if occupied {
        if current_asset()? != asset {
            return denied();
        }
        linux_sealed::mark_roles_cloexec()?;
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|_| ErrorCode::CoreUnavailable)?;
    if exe.file_name().and_then(|p| p.to_str()) != asset.strip_prefix("bin/") {
        return denied();
    }
    let root = exe
        .parent()
        .and_then(Path::parent)
        .ok_or(ErrorCode::ScopeDenied)?;
    let version = root
        .file_name()
        .and_then(|p| p.to_str())
        .ok_or(ErrorCode::ScopeDenied)?;
    let metadata = public_file(&root.join("BUILD-METADATA.json"), 4 * 1024 * 1024)?;
    crate::unix_release::reject_duplicate_keys(&metadata)?;
    let metadata: Value = serde_json::from_slice(&metadata).map_err(|_| ErrorCode::ScopeDenied)?;
    let commit = metadata
        .get("source_commit")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::ScopeDenied)?;
    let trust = ReleaseTrust::load(root, version, commit)?;
    let mut launch = launch(trust, asset)?;
    launch.command.args(std::env::args_os().skip(1));
    let failure = launch.command.exec();
    drop(launch.roles);
    let _ = failure;
    Err(ErrorCode::CoreUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Fixture {
        base: PathBuf,
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let base = std::env::temp_dir().join(format!(
                "hantong-r2-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&base).unwrap();
            std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700)).unwrap();
            let root = base.join("versions/0.1.0-r2-test");
            std::fs::create_dir_all(root.join("bin")).unwrap();
            let commit = "a".repeat(40);
            let source = "b".repeat(64);
            let lock = "c".repeat(64);
            let image = std::fs::read(std::env::current_exe().unwrap()).unwrap();
            let assets = ["hantong", "hantong-t", "hantong-mcp", "mail-hantong-core"];
            for name in assets {
                let path = root.join("bin").join(name);
                std::fs::write(&path, &image).unwrap();
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            let manifest = json!({"format_version":1,"version":"0.1.0-r2-test","package_version":"0.1.0","target":"linux-x86_64",
                "files":assets.map(|name|json!({"path":format!("bin/{name}"),"size":image.len(),"sha256":wire_hex::encode(&sha(&image))}))});
            std::fs::write(
                root.join("release-manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            std::fs::write(root.join("BUILD-METADATA.json"),serde_json::to_vec(&json!({"source_commit":commit,"source_hash":source,"cargo_lock_sha256":lock,"build_performed":true})).unwrap()).unwrap();
            let descriptor=serde_json::to_vec(&json!({"schema_version":1,"version":"0.1.0-r2-test","package_version":"0.1.0","target":"linux-x86_64","commit":commit,
                "source_filename":"mail-hantong-source-0.1.0-r2-test.zip","cargo_lock_sha256":lock,"source_sha256":source,"source_archive_sha256":"d".repeat(64),
                "package_filename":"mail-hantong-cli-0.1.0-r2-test-linux-x86_64.tar.gz","package_sha256":"e".repeat(64)})).unwrap();
            let evidence = base.join("release-pins/0.1.0-r2-test").join(&commit);
            std::fs::create_dir_all(&evidence).unwrap();
            std::fs::write(evidence.join("descriptor.json"), &descriptor).unwrap();
            std::fs::write(evidence.join("pin.json"),serde_json::to_vec(&json!({"format_version":1,"target":"linux-x86_64","version":"0.1.0-r2-test","commit":commit,"descriptor_sha256":wire_hex::encode(&sha(&descriptor))})).unwrap()).unwrap();
            Self { base, root }
        }
        fn trust(&self) -> ReleaseTrust {
            ReleaseTrust::load(&self.root, "0.1.0-r2-test", &"a".repeat(40)).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn sec03_product_sealed_entry_and_sibling_preserve_release_context() {
        if let Some(depth) = std::env::var_os("HANTONG_R2_CHILD_DEPTH") {
            ensure_entry("bin/mail-hantong-core").unwrap();
            assert_eq!(current_asset().unwrap(), "bin/mail-hantong-core");
            assert!(peer_same_asset(std::process::id(), "bin/mail-hantong-core").unwrap());
            if depth == "1" {
                let mut child = sibling("mail-hantong-core", "bin/mail-hantong-core").unwrap();
                assert!(child.command.args(["--exact","linux_identity::tests::sec03_product_sealed_entry_and_sibling_preserve_release_context"])
                    .env("HANTONG_R2_CHILD_DEPTH","2").status().unwrap().success());
            }
            return;
        }
        let f = Fixture::new();
        let mut child = launch(f.trust(), "bin/mail-hantong-core").unwrap();
        assert!(child.command.args(["--exact","linux_identity::tests::sec03_product_sealed_entry_and_sibling_preserve_release_context"])
            .env("HANTONG_R2_CHILD_DEPTH","1").status().unwrap().success());
    }
    #[test]
    fn sec03_product_peer_rejects_other_asset_or_root_and_descriptor_changes() {
        if std::env::var_os("HANTONG_R2_PEER_CHILD").is_some() {
            ensure_entry("bin/mail-hantong-core").unwrap();
            assert!(peer_same_asset(std::process::id(), "bin/hantong").is_err());
            let root = current_release_root().unwrap();
            let evidence = root
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("release-pins/0.1.0-r2-test")
                .join("a".repeat(40));
            std::fs::write(evidence.join("descriptor.json"), b"{}").unwrap();
            assert!(current_asset().is_err());
            return;
        }
        let f = Fixture::new();
        let mut child = launch(f.trust(), "bin/mail-hantong-core").unwrap();
        assert!(child.command.args(["--exact","linux_identity::tests::sec03_product_peer_rejects_other_asset_or_root_and_descriptor_changes"])
            .env("HANTONG_R2_PEER_CHILD","1").status().unwrap().success());
    }
    #[test]
    fn sec03_product_launch_rejects_changed_same_length_original() {
        let f = Fixture::new();
        let trust = f.trust();
        let path = f.root.join("bin/mail-hantong-core");
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(path, bytes).unwrap();
        assert!(launch(trust, "bin/mail-hantong-core").is_err());
    }
    #[test]
    fn sec03_product_peer_rejects_exec_after_admission() {
        use std::io::Write;
        use std::os::unix::net::{UnixListener, UnixStream};
        if let Some(mode) = std::env::var_os("HANTONG_R2_EXEC_TEST") {
            ensure_entry("bin/mail-hantong-core").unwrap();
            let root = current_release_root().unwrap();
            let socket = root
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("exec-race.sock");
            if mode == "switch" {
                let mut stream = UnixStream::connect(&socket).unwrap();
                stream.write_all(b"R").unwrap();
                let mut byte = [0];
                stream.read_exact(&mut byte).unwrap();
                let fd = stream.as_raw_fd();
                assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
                let script = format!("import os,time;os.write({fd},b'E');time.sleep(1)");
                let failure = Command::new("/usr/bin/python3")
                    .args(["-I", "-S", "-c", &script])
                    .exec();
                panic!("synthetic exec failed: {failure}");
            }
            let listener = UnixListener::bind(&socket).unwrap();
            listener.set_nonblocking(true).unwrap();
            let mut launch = sibling("mail-hantong-core", "bin/mail-hantong-core").unwrap();
            let mut child = launch
                .command
                .args([
                    "--exact",
                    "linux_identity::tests::sec03_product_peer_rejects_exec_after_admission",
                ])
                .env("HANTONG_R2_EXEC_TEST", "switch")
                .spawn()
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(value) => break value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if std::time::Instant::now() >= deadline
                            || child.try_wait().unwrap().is_some()
                        {
                            let _ = child.kill();
                            let _ = child.wait();
                            panic!("synthetic peer did not connect");
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(error) => panic!("synthetic accept: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            let peer = crate::unix_peer::observe_peer(stream.as_raw_fd()).unwrap();
            assert!(crate::unix_peer::peer_matches_current_image(&peer).unwrap());
            stream.write_all(b"X").unwrap();
            stream.read_exact(&mut byte).unwrap();
            assert_eq!(&byte, b"E");
            let after = crate::unix_peer::observe_peer(stream.as_raw_fd()).unwrap();
            assert_eq!(after.process_instance, peer.process_instance);
            assert!(!crate::unix_peer::peer_matches_current_image(&peer).unwrap_or(false));
            assert!(child.wait().unwrap().success());
            std::fs::remove_file(socket).unwrap();
            return;
        }
        let f = Fixture::new();
        let mut child = launch(f.trust(), "bin/mail-hantong-core").unwrap();
        assert!(
            child
                .command
                .args([
                    "--exact",
                    "linux_identity::tests::sec03_product_peer_rejects_exec_after_admission"
                ])
                .env("HANTONG_R2_EXEC_TEST", "parent")
                .status()
                .unwrap()
                .success()
        );
    }
    #[test]
    fn sec03_context_rejects_unknown_duplicate_or_noninteger() {
        assert!(
            exact(
                br#"{"protocol_version":2,"protocol_version":2}"#,
                &["protocol_version"]
            )
            .is_err()
        );
        assert!(
            exact(
                br#"{"protocol_version":2,"extra":0}"#,
                &["protocol_version"]
            )
            .is_err()
        );
        for value in [
            "",
            "../1",
            "1.2",
            "01.2.3",
            "1.2.3/evil",
            "1.2.3-",
            "1.2.3-a..b",
        ] {
            assert!(!valid_version(value));
        }
        assert!(valid_version("0.1.0-beta.30"));
    }
    #[test]
    fn sec03_identity_rejects_missing_roles_in_isolated_child() {
        if std::env::var_os("HANTONG_IDENTITY_MISSING_CHILD").is_some() {
            for fd in 197..=201 {
                unsafe {
                    libc::close(fd);
                }
            }
            assert!(current_asset().is_err());
            return;
        }
        let status = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_identity::tests::sec03_identity_rejects_missing_roles_in_isolated_child",
            ])
            .env("HANTONG_IDENTITY_MISSING_CHILD", "1")
            .status()
            .unwrap();
        assert!(status.success());
    }
}
