#![cfg(target_os = "linux")]
#![allow(unsafe_code)]

//! Native, pinned Linux update staging (D1-D4/E6).
//!
//! This module deliberately stops at a verified, private release directory.  It
//! does not change `current.txt`, stop Core, open the database, or touch secrets.

use hantong_contracts::{ErrorCode, wire_hex};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::linux_archive::{gzip_tar, source_zip};

const TARGET: &str = "linux-x86_64";
const MAX_DESCRIPTOR: usize = 256 * 1024;
const MAX_PACKAGE: usize = 256 * 1024 * 1024;
const MAX_SOURCE: usize = 128 * 1024 * 1024;
const MAX_MEMBER: usize = 64 * 1024 * 1024;
const MAX_FILES: usize = 4096;
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

#[derive(Debug, Clone)]
pub struct StagedUpdate {
    root: PathBuf,
    version: String,
    commit: String,
    descriptor_hash: [u8; 32],
    manifest_hash: [u8; 32],
}
impl StagedUpdate {
    pub fn core_command(&self) -> Result<crate::unix_release::VerifiedCoreCommand, ErrorCode> {
        crate::linux_identity::candidate_command(
            &self.root,
            &self.version,
            &self.commit,
            self.descriptor_hash,
            self.manifest_hash,
            "bin/mail-hantong-core",
        )
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn descriptor_sha256(&self) -> &[u8; 32] {
        &self.descriptor_hash
    }
    pub fn manifest_sha256(&self) -> &[u8; 32] {
        &self.manifest_hash
    }
}

fn bad<T>() -> Result<T, ErrorCode> {
    Err(ErrorCode::InvalidInput)
}
fn sha(bytes: &[u8]) -> Result<String, ErrorCode> {
    Ok(wire_hex::encode(
        ring::digest::digest(&ring::digest::SHA256, bytes).as_ref(),
    ))
}
fn hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes().all(|b| b.is_ascii_hexdigit())
        && s.bytes().all(|b| !b.is_ascii_uppercase())
}
fn hex40(s: &str) -> bool {
    s.len() == 40
        && s.bytes().all(|b| b.is_ascii_hexdigit())
        && s.bytes().all(|b| !b.is_ascii_uppercase())
}
fn valid_version(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    let (base, pre) = s.split_once('-').map_or((s, None), |(a, b)| (a, Some(b)));
    let n: Vec<_> = base.split('.').collect();
    n.len() == 3
        && n.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
        && pre.is_some_and(|p| {
            !p.is_empty()
                && p.split('.').all(|x| {
                    !x.is_empty()
                        && x.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                        && !(x.bytes().all(|b| b.is_ascii_digit())
                            && x.len() > 1
                            && x.starts_with('0'))
                })
        })
}
fn exact(bytes: &[u8], keys: &[&str]) -> Result<Map<String, Value>, ErrorCode> {
    crate::unix_release::reject_duplicate_keys(bytes)?;
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    let map = value.as_object().ok_or(ErrorCode::InvalidInput)?;
    if map.len() != keys.len() || keys.iter().any(|k| !map.contains_key(*k)) {
        return bad();
    }
    Ok(map.clone())
}
fn text<'a>(m: &'a Map<String, Value>, k: &str) -> Result<&'a str, ErrorCode> {
    let s = m
        .get(k)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)?;
    if s.is_empty() || s.chars().any(char::is_control) {
        bad()
    } else {
        Ok(s)
    }
}
fn digest(m: &Map<String, Value>, k: &str) -> Result<String, ErrorCode> {
    let s = text(m, k)?;
    if !hex64(s) {
        return bad();
    }
    Ok(s.to_owned())
}
fn curl() -> Result<PathBuf, ErrorCode> {
    let path = fs::canonicalize("/usr/bin/curl").map_err(|_| ErrorCode::CapabilityUnsupported)?;
    let meta = fs::symlink_metadata(&path).map_err(|_| ErrorCode::ScopeDenied)?;
    if !meta.is_file() || meta.uid() != 0 || meta.mode() & 0o6022 != 0 || meta.mode() & 0o111 == 0 {
        return Err(ErrorCode::ScopeDenied);
    }
    let mut cursor = path.parent();
    while let Some(current) = cursor {
        let meta = fs::symlink_metadata(current).map_err(|_| ErrorCode::ScopeDenied)?;
        if !meta.is_dir() || meta.uid() != 0 || meta.mode() & 0o6022 != 0 {
            return Err(ErrorCode::ScopeDenied);
        }
        cursor = current.parent();
    }
    Ok(path)
}
fn rename_noreplace(
    parent: &crate::unix_paths::UnixDirectoryGuard,
    from: &str,
    to: &str,
) -> Result<(), ErrorCode> {
    let from = std::ffi::CString::new(from).map_err(|_| ErrorCode::InvalidInput)?;
    let to = std::ffi::CString::new(to).map_err(|_| ErrorCode::InvalidInput)?;
    let directory = parent.directory_fd()?;
    // Both names are validated single components in a held private directory.
    let rc = unsafe {
        libc::renameat2(
            directory.as_raw_fd(),
            from.as_ptr(),
            directory.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc == 0 {
        Ok(())
    } else if std::io::Error::last_os_error().raw_os_error() == Some(libc::EEXIST) {
        Err(ErrorCode::Conflict)
    } else {
        Err(ErrorCode::CoreUnavailable)
    }
}
fn write_new(
    directory: &crate::unix_paths::UnixDirectoryGuard,
    name: &str,
    bytes: &[u8],
    executable: bool,
) -> Result<(), ErrorCode> {
    let mut file = directory.create_new_regular(name)?;
    file.write_all(bytes)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if executable {
        file.set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(|_| ErrorCode::CoreUnavailable)?;
    }
    file.sync_all().map_err(|_| ErrorCode::CoreUnavailable)
}
fn persist_public(
    guard: &crate::unix_paths::UnixDirectoryGuard,
    name: &str,
    bytes: &[u8],
) -> Result<(), ErrorCode> {
    match guard.open_existing_readonly_regular(name) {
        Ok(mut file) => {
            let mut current = Vec::new();
            Read::by_ref(&mut file)
                .take(MAX_DESCRIPTOR as u64 + 1)
                .read_to_end(&mut current)
                .map_err(|_| ErrorCode::CoreUnavailable)?;
            if current != bytes {
                return Err(ErrorCode::Conflict);
            }
            Ok(())
        }
        Err(ErrorCode::NotFound) => write_new(guard, name, bytes, false),
        Err(error) => Err(error),
    }
}
fn download_response(out: &[u8], limit: usize) -> Result<Vec<u8>, ErrorCode> {
    let response = out.strip_suffix(b"\n200").ok_or(ErrorCode::Offline)?;
    let sep = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or(ErrorCode::Offline)?;
    if sep > 65536 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let line = response
        .split(|b| *b == b'\n')
        .next()
        .ok_or(ErrorCode::Offline)?;
    let fields = line.split(|b| *b == b' ').collect::<Vec<_>>();
    if fields.len() < 2
        || ![b"HTTP/1.0".as_slice(), b"HTTP/1.1", b"HTTP/2", b"HTTP/3"].contains(&fields[0])
        || fields[1].strip_suffix(b"\r").unwrap_or(fields[1]) != b"200"
    {
        return Err(ErrorCode::Offline);
    }
    let body = &response[sep + 4..];
    if body.len() > limit {
        return Err(ErrorCode::QuotaExceeded);
    }
    Ok(body.to_vec())
}
fn fetch(url: &str, limit: usize) -> Result<Vec<u8>, ErrorCode> {
    use std::os::fd::AsRawFd;
    if !url.starts_with("https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/")
        || url.contains('?')
        || url.contains('#')
    {
        return bad();
    }
    let mut child = Command::new(curl()?)
        .env_clear()
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--noproxy",
            "*",
            "--connect-timeout",
            "15",
            "--max-time",
            "300",
            "--dump-header",
            "-",
            "--output",
            "-",
            "--write-out",
            "\n%{http_code}",
            url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ErrorCode::Offline)?;
    let mut reaped = false;
    let result = (|| {
        let mut pipe = child.stdout.take().ok_or(ErrorCode::Offline)?;
        let fd = pipe.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(ErrorCode::Offline);
        }
        let deadline = Instant::now() + Duration::from_secs(300);
        let mut out = Vec::new();
        let mut buffer = [0; 65536];
        loop {
            if Instant::now() >= deadline {
                return Err(ErrorCode::Offline);
            }
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if out.len() + count > limit + 65540 {
                        return Err(ErrorCode::QuotaExceeded);
                    }
                    out.extend_from_slice(&buffer[..count]);
                    if out.len() > 65540 && !out.windows(4).take(65537).any(|w| w == b"\r\n\r\n") {
                        return Err(ErrorCode::QuotaExceeded);
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    let mut poll = libc::pollfd {
                        fd,
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    let rc = unsafe { libc::poll(&mut poll, 1, 100) };
                    if rc < 0
                        && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                    {
                        return Err(ErrorCode::Offline);
                    }
                }
                Err(_) => return Err(ErrorCode::Offline),
            }
        }
        loop {
            if let Some(status) = child.try_wait().map_err(|_| ErrorCode::Offline)? {
                reaped = true;
                if !status.success() {
                    return Err(ErrorCode::Offline);
                }
                return download_response(&out, limit);
            }
            if Instant::now() >= deadline {
                return Err(ErrorCode::Offline);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if !reaped {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
fn canonical_source(m: &BTreeMap<String, Vec<u8>>) -> Result<String, ErrorCode> {
    let suffix = [
        ".rs",
        ".py",
        ".toml",
        ".txt",
        ".eml",
        ".mbox",
        ".ps1",
        ".sh",
        ".cert",
        ".fullchain",
        ".key",
        ".manifest",
        ".html",
    ];
    let fixed = [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "install.ps1",
        "install.sh",
        "tests/Corpus/index.json",
        "scripts/native-tools.json",
        "scripts/check-all",
        "scripts/traceability",
        "scripts/test-mail-server",
        "packaging/licenses/index.json",
    ];
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    digest.update(b"mail-hantong.source.lf.v2\0");
    for (name, bytes) in m {
        let parts = name.split('/').collect::<Vec<_>>();
        let leaf = parts.last().copied().unwrap_or("");
        let ext = Path::new(leaf)
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| format!(".{}", s.to_ascii_lowercase()))
            .unwrap_or_default();
        let ordinary = matches!(
            parts.first().copied(),
            Some("crates" | "apps" | "tests" | "scripts" | "packaging")
        ) && suffix.contains(&ext.as_str())
            && !matches!(leaf, "traceability.json" | "execution.json")
            && !parts.contains(&"__pycache__")
            && !parts
                .get(parts.len().saturating_sub(2))
                .is_some_and(|s| matches!(*s, "target" | "artifacts" | ".local"));
        let contract =
            name.starts_with("docs/contracts/") && [".md", ".json", ".sql"].contains(&ext.as_str());
        if !fixed.contains(&name.as_str()) && !ordinary && !contract {
            continue;
        }
        let normalized;
        let data = if name.starts_with("tests/Corpus/") {
            bytes.as_slice()
        } else {
            let mut out = Vec::with_capacity(bytes.len());
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                out.push(bytes[i]);
                i += 1;
            }
            normalized = out;
            normalized.as_slice()
        };
        digest.update(&(name.len() as u32).to_be_bytes());
        digest.update(name.as_bytes());
        digest.update(&(data.len() as u64).to_be_bytes());
        digest.update(data);
    }
    Ok(wire_hex::encode(digest.finish().as_ref()))
}

type VerifiedBundle = (BTreeMap<String, Vec<u8>>, String, String);

fn verify(
    descriptor: &[u8],
    package: &[u8],
    source: &[u8],
    version: &str,
    commit: &str,
    expected: &str,
) -> Result<VerifiedBundle, ErrorCode> {
    if descriptor.is_empty()
        || descriptor.len() > MAX_DESCRIPTOR
        || package.len() > MAX_PACKAGE
        || source.len() > MAX_SOURCE
        || !hex64(expected)
        || sha(descriptor)? != expected
    {
        return Err(ErrorCode::Conflict);
    }
    let d = exact(descriptor, DESCRIPTOR_KEYS)?;
    if d.get("schema_version").and_then(Value::as_u64) != Some(1)
        || text(&d, "target")? != TARGET
        || text(&d, "version")? != version
        || text(&d, "commit")? != commit
        || !crate::linux_identity::valid_version(text(&d, "package_version")?)
        || !hex40(commit)
    {
        return bad();
    }
    if !valid_version(version)
        || text(&d, "package_filename")? != format!("mail-hantong-cli-{version}-{TARGET}.tar.gz")
        || text(&d, "source_filename")? != format!("mail-hantong-source-{version}.zip")
    {
        return bad();
    }
    for k in [
        "cargo_lock_sha256",
        "source_sha256",
        "source_archive_sha256",
        "package_sha256",
    ] {
        digest(&d, k)?;
    }
    if sha(descriptor)? != expected || sha(package)? != digest(&d, "package_sha256")? {
        return Err(ErrorCode::Conflict);
    }
    let sm = source_zip(source)?;
    if sm.get("SOURCE-COMMIT").map(Vec::as_slice) != Some(format!("{commit}\n").as_bytes())
        || !sm.contains_key("Cargo.lock")
        || sha(source)? != digest(&d, "source_archive_sha256")?
        || sha(sm["Cargo.lock"].as_slice())? != digest(&d, "cargo_lock_sha256")?
        || canonical_source(&sm)? != digest(&d, "source_sha256")?
    {
        return Err(ErrorCode::Conflict);
    }
    let pm = gzip_tar(package)?;
    for n in [
        "VERSION",
        "BUILD-METADATA.json",
        "release-manifest.json",
        "FILES.sha256",
        "installed-files.txt",
    ] {
        if !pm.contains_key(n) {
            return bad();
        }
    }
    if pm.get("VERSION").map(Vec::as_slice) != Some(format!("{version}\n").as_bytes()) {
        return bad();
    }
    let manifest = exact(
        &pm["release-manifest.json"],
        &[
            "format_version",
            "version",
            "package_version",
            "target",
            "files",
        ],
    )?;
    if manifest.get("format_version").and_then(Value::as_u64) != Some(1)
        || text(&manifest, "target")? != TARGET
        || text(&manifest, "version")? != version
        || text(&manifest, "package_version")? != text(&d, "package_version")?
    {
        return bad();
    }
    let mut expected_files = BTreeSet::new();
    let rows = manifest["files"]
        .as_array()
        .ok_or(ErrorCode::InvalidInput)?;
    if rows.len() > MAX_FILES {
        return bad();
    }
    for row in rows {
        let r = row.as_object().ok_or(ErrorCode::InvalidInput)?;
        if r.len() != 3 {
            return bad();
        }
        let n = r
            .get("path")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?;
        if n.is_empty()
            || n.contains('\\')
            || n.starts_with('/')
            || n.split('/').any(|x| x.is_empty() || x == "." || x == "..")
            || !expected_files.insert(n.to_owned())
        {
            return bad();
        }
        let sz = r
            .get("size")
            .and_then(Value::as_u64)
            .ok_or(ErrorCode::InvalidInput)? as usize;
        let h = r
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?;
        if sz > MAX_MEMBER
            || !hex64(h)
            || pm
                .get(n)
                .is_none_or(|v| v.len() != sz || sha(v) != Ok(h.to_owned()))
        {
            return Err(ErrorCode::Conflict);
        }
    }
    crate::unix_release::reject_duplicate_keys(&pm["BUILD-METADATA.json"])?;
    let metadata_value: Value =
        serde_json::from_slice(&pm["BUILD-METADATA.json"]).map_err(|_| ErrorCode::InvalidInput)?;
    let metadata = metadata_value.as_object().ok_or(ErrorCode::InvalidInput)?;
    if metadata.get("format_version").and_then(Value::as_u64) != Some(1)
        || text(metadata, "release_label")? != version
        || text(metadata, "target")? != "x86_64-unknown-linux-gnu"
        || text(metadata, "source_hash")? != digest(&d, "source_sha256")?
        || metadata.get("build_performed") != Some(&Value::Bool(true))
        || text(metadata, "source_relation")? != "built_from_current_workspace"
        || text(metadata, "cargo_lock_sha256")? != digest(&d, "cargo_lock_sha256")?
        || text(metadata, "source_commit")? != commit
        || metadata
            .get("linux_release_readiness")
            .and_then(Value::as_str)
            == Some("blocked_identity_and_origin")
    {
        return Err(ErrorCode::Conflict);
    }
    let binaries = metadata
        .get("binaries")
        .ok_or(ErrorCode::InvalidInput)?
        .as_object()
        .ok_or(ErrorCode::InvalidInput)?;
    for name in [
        "bin/hantong",
        "bin/hantong-t",
        "bin/hantong-mcp",
        "bin/mail-hantong-core",
    ] {
        let bytes = pm.get(name).ok_or(ErrorCode::InvalidInput)?;
        let expected = binaries
            .get(name)
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?;
        if sha(bytes)? != expected
            || bytes.len() < 64
            || bytes[..6] != *b"\x7fELF\x02\x01"
            || u16::from_le_bytes([bytes[18], bytes[19]]) != 62
        {
            return Err(ErrorCode::Conflict);
        }
    }
    let mut sums = BTreeMap::new();
    for line in std::str::from_utf8(&pm["FILES.sha256"])
        .map_err(|_| ErrorCode::InvalidInput)?
        .lines()
    {
        let (h, n) = line.split_once("  ").ok_or(ErrorCode::InvalidInput)?;
        if !hex64(h) || sums.insert(n.to_owned(), h.to_owned()).is_some() {
            return bad();
        }
    }
    if sums.contains_key("FILES.sha256")
        || sums.len() + 1 != pm.len()
        || sums.keys().any(|n| !pm.contains_key(n))
        || sums
            .iter()
            .any(|(n, h)| sha(&pm[n]).map_or(true, |v| v != *h))
    {
        return Err(ErrorCode::Conflict);
    }
    let mut package_rows = pm.keys().cloned().collect::<BTreeSet<_>>();
    package_rows.remove("release-manifest.json");
    package_rows.remove("FILES.sha256");
    if expected_files != package_rows {
        return Err(ErrorCode::Conflict);
    }
    let lines = std::str::from_utf8(&pm["installed-files.txt"])
        .map_err(|_| ErrorCode::InvalidInput)?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let listed = lines.iter().cloned().collect::<BTreeSet<_>>();
    if listed.len() != lines.len() || listed != pm.keys().cloned().collect() {
        return Err(ErrorCode::Conflict);
    }
    let manifest_sha = sha(&pm["release-manifest.json"])?;
    Ok((pm, text(&d, "package_version")?.to_owned(), manifest_sha))
}

pub fn stage(version: &str, commit: &str, descriptor_sha: &str) -> Result<StagedUpdate, ErrorCode> {
    retrieve_and_stage(version, commit, descriptor_sha, false)
}
/// Explicit recovery re-fetches the independently pinned bundle; an installed
/// manifest by itself is insufficient evidence of the original package bytes.
pub fn resume(
    version: &str,
    commit: &str,
    descriptor_sha: &str,
) -> Result<StagedUpdate, ErrorCode> {
    retrieve_and_stage(version, commit, descriptor_sha, true)
}
fn retrieve_and_stage(
    version: &str,
    commit: &str,
    expected_descriptor_sha: &str,
    resume: bool,
) -> Result<StagedUpdate, ErrorCode> {
    if !valid_version(version)
        || !hex40(commit)
        || !hex64(expected_descriptor_sha)
        || !cfg!(target_arch = "x86_64")
    {
        return Err(ErrorCode::InvalidInput);
    }
    let current = crate::linux_identity::current_release_root()?;
    let install = current
        .parent()
        .and_then(Path::parent)
        .ok_or(ErrorCode::ScopeDenied)?;
    let base =
        format!("https://tz1012.github.io/mail-hantong/artifacts/linux-x86_64/{version}/{commit}");
    let descriptor = fetch(
        &format!("{base}/linux-pinned-candidate.json"),
        MAX_DESCRIPTOR,
    )?;
    if sha(&descriptor)? != expected_descriptor_sha {
        return Err(ErrorCode::Conflict);
    }
    let package_name = format!("mail-hantong-cli-{version}-{TARGET}.tar.gz");
    let source_name = format!("mail-hantong-source-{version}.zip");
    let package = fetch(&format!("{base}/{package_name}"), MAX_PACKAGE)?;
    let source = fetch(&format!("{base}/{source_name}"), MAX_SOURCE)?;
    if resume {
        resume_at(
            install,
            &descriptor,
            &package,
            &source,
            version,
            commit,
            expected_descriptor_sha,
        )
    } else {
        stage_at(
            install,
            &descriptor,
            &package,
            &source,
            version,
            commit,
            expected_descriptor_sha,
        )
    }
}

fn resume_at(
    install: &Path,
    descriptor: &[u8],
    package: &[u8],
    source: &[u8],
    version: &str,
    commit: &str,
    expected_descriptor_sha: &str,
) -> Result<StagedUpdate, ErrorCode> {
    let (files, _, manifest_hash) = verify(
        descriptor,
        package,
        source,
        version,
        commit,
        expected_descriptor_sha,
    )?;
    let root = install.join("versions").join(version);
    let held = crate::unix_paths::UnixDirectoryGuard::open(&root, true)?;
    let before = File::from(
        held.directory_fd()?
            .try_clone_to_owned()
            .map_err(|_| ErrorCode::CoreUnavailable)?,
    )
    .metadata()
    .map_err(|_| ErrorCode::CoreUnavailable)?;
    crate::linux_install::inventory(&root)?;
    for (name, bytes) in &files {
        if crate::linux_install::read_public(&root, name, MAX_MEMBER as u64)? != *bytes {
            return Err(ErrorCode::Conflict);
        }
    }
    let pin = install.join("release-pins").join(version).join(commit);
    let pin_expected=serde_json::to_vec(&serde_json::json!({"format_version":1,"target":TARGET,"version":version,"commit":commit,"descriptor_sha256":expected_descriptor_sha})).map_err(|_|ErrorCode::InternalError)?;
    if crate::linux_install::read_public(&pin, "descriptor.json", MAX_DESCRIPTOR as u64)?
        != descriptor
        || crate::linux_install::read_public(&pin, "pin.json", MAX_DESCRIPTOR as u64)?
            != pin_expected
    {
        return Err(ErrorCode::Conflict);
    }
    let after = fs::symlink_metadata(&root).map_err(|_| ErrorCode::ScopeDenied)?;
    if before.dev() != after.dev() || before.ino() != after.ino() {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(StagedUpdate {
        root,
        version: version.to_owned(),
        commit: commit.to_owned(),
        descriptor_hash: wire_hex::decode_fixed(expected_descriptor_sha)
            .map_err(|_| ErrorCode::InvalidInput)?,
        manifest_hash: wire_hex::decode_fixed(&manifest_hash)
            .map_err(|_| ErrorCode::InvalidInput)?,
    })
}

fn stage_at(
    install: &Path,
    descriptor: &[u8],
    package: &[u8],
    source: &[u8],
    version: &str,
    commit: &str,
    expected_descriptor_sha: &str,
) -> Result<StagedUpdate, ErrorCode> {
    let (files, _package_version, manifest_hash) = verify(
        descriptor,
        package,
        source,
        version,
        commit,
        expected_descriptor_sha,
    )?;
    let versions = install.join("versions");
    let versions_guard = crate::unix_paths::UnixDirectoryGuard::open(&versions, true)?;
    let mut nonce = [0_u8; 16];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce)
        .map_err(|_| ErrorCode::InternalError)?;
    let stage_name = format!(".{version}-stage-{}", wire_hex::encode(&nonce));
    let stage = versions.join(&stage_name);
    let name = std::ffi::CString::new(stage_name.as_str()).map_err(|_| ErrorCode::InvalidInput)?;
    // Exclusive mkdir under the held versions directory; never repair or reuse a stage.
    if unsafe {
        libc::mkdirat(
            versions_guard.directory_fd()?.as_raw_fd(),
            name.as_ptr(),
            0o700,
        )
    } != 0
    {
        return Err(ErrorCode::Conflict);
    }
    let stage_guard = crate::unix_paths::UnixDirectoryGuard::open(&stage, true)?;
    let held = stage_guard
        .directory_fd()?
        .try_clone_to_owned()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let held_meta = File::from(held)
        .metadata()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    for (name, data) in &files {
        let path = stage.join(name);
        let parent = path.parent().ok_or(ErrorCode::InvalidInput)?;
        let parent_guard = crate::unix_paths::UnixDirectoryGuard::prepare(parent)?;
        write_new(
            &parent_guard,
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or(ErrorCode::InvalidInput)?,
            data,
            name.starts_with("bin/"),
        )?;
        File::from(
            parent_guard
                .directory_fd()?
                .try_clone_to_owned()
                .map_err(|_| ErrorCode::CoreUnavailable)?,
        )
        .sync_all()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    }
    // Reopen and compare the stage inode before publishing its exact name.
    let observed = fs::symlink_metadata(&stage).map_err(|_| ErrorCode::ScopeDenied)?;
    if observed.dev() != held_meta.dev() || observed.ino() != held_meta.ino() {
        return Err(ErrorCode::ScopeDenied);
    }
    let pin = install.join("release-pins").join(version).join(commit);
    let pin_guard = crate::unix_paths::UnixDirectoryGuard::prepare(&pin)?;
    for entry in fs::read_dir(format!(
        "/proc/self/fd/{}",
        pin_guard.directory_fd()?.as_raw_fd()
    ))
    .map_err(|_| ErrorCode::CoreUnavailable)?
    {
        let name = entry.map_err(|_| ErrorCode::CoreUnavailable)?.file_name();
        if name != "descriptor.json" && name != "pin.json" {
            return Err(ErrorCode::Conflict);
        }
    }
    persist_public(&pin_guard, "descriptor.json", descriptor)?;
    let pin_json=serde_json::to_vec(&serde_json::json!({"format_version":1,"target":TARGET,"version":version,"commit":commit,"descriptor_sha256":expected_descriptor_sha})).map_err(|_|ErrorCode::InternalError)?;
    persist_public(&pin_guard, "pin.json", &pin_json)?;
    File::from(
        pin_guard
            .directory_fd()?
            .try_clone_to_owned()
            .map_err(|_| ErrorCode::CoreUnavailable)?,
    )
    .sync_all()
    .map_err(|_| ErrorCode::CoreUnavailable)?;
    File::from(
        versions_guard
            .directory_fd()?
            .try_clone_to_owned()
            .map_err(|_| ErrorCode::CoreUnavailable)?,
    )
    .sync_all()
    .map_err(|_| ErrorCode::CoreUnavailable)?;
    rename_noreplace(&versions_guard, &stage_name, version)?;
    let final_root = versions.join(version);
    let final_guard = crate::unix_paths::UnixDirectoryGuard::open(&final_root, true)?;
    let final_meta = File::from(
        final_guard
            .directory_fd()?
            .try_clone_to_owned()
            .map_err(|_| ErrorCode::CoreUnavailable)?,
    )
    .metadata()
    .map_err(|_| ErrorCode::CoreUnavailable)?;
    if final_meta.dev() != held_meta.dev() || final_meta.ino() != held_meta.ino() {
        return Err(ErrorCode::ScopeDenied);
    }
    File::from(
        versions_guard
            .directory_fd()?
            .try_clone_to_owned()
            .map_err(|_| ErrorCode::CoreUnavailable)?,
    )
    .sync_all()
    .map_err(|_| ErrorCode::CoreUnavailable)?;
    // Failed stages are preserved, never recursively removed through a mutable path.
    Ok(StagedUpdate {
        root: final_root,
        version: version.to_owned(),
        commit: commit.to_owned(),
        descriptor_hash: wire_hex::decode_fixed(expected_descriptor_sha)
            .map_err(|_| ErrorCode::InvalidInput)?,
        manifest_hash: wire_hex::decode_fixed(&manifest_hash)
            .map_err(|_| ErrorCode::InvalidInput)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    const VERSION: &str = "0.1.0-beta.29";
    const TEST_COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn fixture(flags: &[&str]) -> (PathBuf, Vec<u8>, Vec<u8>, Vec<u8>) {
        let root = std::env::temp_dir().join(format!(
            "hantong-linux-artifacts-{}-{}",
            std::process::id(),
            {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            }
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/Fakes/linux_update_artifacts.py");
        let status = Command::new("/usr/bin/python3")
            .args(["-I", "-S"])
            .arg(&script)
            .arg(&root)
            .args(flags)
            .status()
            .unwrap();
        assert!(status.success());
        let descriptor = std::fs::read(root.join("descriptor.json")).unwrap();
        let package =
            std::fs::read(root.join(format!("mail-hantong-cli-{VERSION}-{TARGET}.tar.gz")))
                .unwrap();
        let source =
            std::fs::read(root.join(format!("mail-hantong-source-{VERSION}.zip"))).unwrap();
        (root, descriptor, package, source)
    }

    fn install_fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!("hantong-stage-{}-{}", std::process::id(), {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        }));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("versions")).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(root.join("versions"), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(root.join("current.txt"), b"0.1.0-beta.28\n").unwrap();
        fs::write(root.join("user-data"), b"preserve-me").unwrap();
        root
    }

    fn stage_fixture(flags: &[&str]) -> (PathBuf, PathBuf, Vec<u8>, Vec<u8>, Vec<u8>) {
        let (artifacts, descriptor, package, source) = fixture(flags);
        let install = install_fixture();
        (install, artifacts, descriptor, package, source)
    }

    #[test]
    fn sec03_linux_update_rejects_unpinned_tuple() {
        assert!(valid_version("0.1.0-beta.29"));
        assert!(!valid_version("0.1.0"));
        assert!(!valid_version("0.01.0-beta.1"));
        assert!(!valid_version("0.1.0-beta.01"));
        assert!(hex40(&"a".repeat(40)));
        assert!(!hex40(&"A".repeat(40)));
    }

    #[test]
    fn sec03_linux_update_rejects_wrong_descriptor_digest() {
        let descriptor = br#"{"schema_version":1,"version":"0.1.0-beta.29","package_version":"0.1.0-beta.29","target":"linux-x86_64","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_filename":"mail-hantong-source-0.1.0-beta.29.zip","cargo_lock_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","source_sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","source_archive_sha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","package_filename":"mail-hantong-cli-0.1.0-beta.29-linux-x86_64.tar.gz","package_sha256":"eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}"#;
        assert!(crate::unix_release::reject_duplicate_keys(descriptor).is_ok());
        assert_ne!(sha(descriptor).unwrap(), "f".repeat(64));
    }

    #[test]
    fn sec03_linux_update_rejects_duplicate_descriptor_fields() {
        let bytes = br#"{"schema_version":1,"schema_version":1}"#;
        assert!(crate::unix_release::reject_duplicate_keys(bytes).is_err());
    }

    #[test]
    fn sec03_linux_update_verifies_complete_synthetic_artifact() {
        let (root, descriptor, package, source) = fixture(&[]);
        let expected = sha(&descriptor).unwrap();
        let result = verify(
            &descriptor,
            &package,
            &source,
            VERSION,
            TEST_COMMIT,
            &expected,
        );
        assert!(result.is_ok(), "complete fixture rejected: {result:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_linux_update_rejects_tampered_artifact_layers() {
        for flags in [
            &["--tamper-descriptor"][..],
            &["--tamper-package"][..],
            &["--tamper-source"][..],
            &["--bad-metadata"][..],
            &["--bad-binary"][..],
            &["--duplicate-inventory"][..],
        ] {
            let (root, descriptor, package, source) = fixture(flags);
            let expected = sha(&descriptor).unwrap();
            assert!(
                verify(
                    &descriptor,
                    &package,
                    &source,
                    VERSION,
                    TEST_COMMIT,
                    &expected
                )
                .is_err(),
                "accepted {flags:?}"
            );
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn sec03_linux_update_rejects_extra_package_file() {
        let (root, descriptor, package, source) = fixture(&["--extra-file"]);
        let expected = sha(&descriptor).unwrap();
        assert!(
            verify(
                &descriptor,
                &package,
                &source,
                VERSION,
                TEST_COMMIT,
                &expected
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_linux_update_preserves_canonical_crlf_rule() {
        let (root, descriptor, package, source) = fixture(&["--crlf-source"]);
        let expected = sha(&descriptor).unwrap();
        assert!(
            verify(
                &descriptor,
                &package,
                &source,
                VERSION,
                TEST_COMMIT,
                &expected
            )
            .is_ok()
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_linux_stage_at_preserves_pointer_and_userdata() {
        let (install, artifacts, descriptor, package, source) = stage_fixture(&[]);
        let expected = sha(&descriptor).unwrap();
        let staged = stage_at(
            &install,
            &descriptor,
            &package,
            &source,
            VERSION,
            TEST_COMMIT,
            &expected,
        )
        .unwrap();
        assert_eq!(
            fs::read(install.join("current.txt")).unwrap(),
            b"0.1.0-beta.28\n"
        );
        assert_eq!(fs::read(install.join("user-data")).unwrap(), b"preserve-me");
        assert_eq!(
            staged.root().file_name().and_then(|v| v.to_str()),
            Some(VERSION)
        );
        assert!(
            install
                .join("release-pins")
                .join(VERSION)
                .join(TEST_COMMIT)
                .join("pin.json")
                .is_file()
        );
        assert_eq!(
            fs::metadata(staged.root()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(staged.root().join("VERSION"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(install);
        let _ = fs::remove_dir_all(artifacts);
    }

    #[test]
    fn sec03_explicit_resume_revalidates_pinned_files_without_overwrite() {
        let (install, artifacts, d, pkg, source) = stage_fixture(&[]);
        let digest = sha(&d).unwrap();
        let staged = stage_at(&install, &d, &pkg, &source, VERSION, TEST_COMMIT, &digest).unwrap();
        assert!(resume_at(&install, &d, &pkg, &source, VERSION, TEST_COMMIT, &digest).is_ok());
        let binary = staged.root().join("bin/hantong");
        let mut tampered = fs::read(&binary).unwrap();
        tampered[63] ^= 1;
        fs::write(&binary, tampered).unwrap();
        assert!(resume_at(&install, &d, &pkg, &source, VERSION, TEST_COMMIT, &digest).is_err());
        assert_eq!(
            fs::read(install.join("current.txt")).unwrap(),
            b"0.1.0-beta.28\n"
        );
        let _ = fs::remove_dir_all(install);
        let _ = fs::remove_dir_all(artifacts);
    }

    #[test]
    fn sec03_linux_stage_at_duplicate_version_preserves_existing_release() {
        let (install, artifacts, descriptor, package, source) = stage_fixture(&[]);
        let expected = sha(&descriptor).unwrap();
        let first = stage_at(
            &install,
            &descriptor,
            &package,
            &source,
            VERSION,
            TEST_COMMIT,
            &expected,
        )
        .unwrap();
        let pointer = fs::read(install.join("current.txt")).unwrap();
        assert!(
            stage_at(
                &install,
                &descriptor,
                &package,
                &source,
                VERSION,
                TEST_COMMIT,
                &expected
            )
            .is_err()
        );
        assert_eq!(fs::read(install.join("current.txt")).unwrap(), pointer);
        assert_eq!(
            fs::read(first.root().join("VERSION")).unwrap(),
            format!("{VERSION}\n").as_bytes()
        );
        let _ = fs::remove_dir_all(install);
        let _ = fs::remove_dir_all(artifacts);
    }

    #[test]
    fn sec03_linux_stage_at_rejects_pin_ancestor_symlink_without_external_write() {
        let (install, artifacts, descriptor, package, source) = stage_fixture(&[]);
        let outside = install.with_extension("pin-outside");
        fs::create_dir(&outside).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, install.join("release-pins")).unwrap();
        let expected = sha(&descriptor).unwrap();
        assert!(
            stage_at(
                &install,
                &descriptor,
                &package,
                &source,
                VERSION,
                TEST_COMMIT,
                &expected
            )
            .is_err()
        );
        assert!(!outside.join(VERSION).exists());
        assert_eq!(
            fs::read(install.join("current.txt")).unwrap(),
            b"0.1.0-beta.28\n"
        );
        let _ = fs::remove_dir_all(install);
        let _ = fs::remove_dir_all(outside);
        let _ = fs::remove_dir_all(artifacts);
    }

    #[test]
    fn sec03_linux_stage_at_rejects_malformed_package_without_stage() {
        let (install, artifacts, descriptor, mut package, source) = stage_fixture(&[]);
        let middle = package.len() / 2;
        package[middle] ^= 0x01;
        let expected = sha(&descriptor).unwrap();
        assert!(
            stage_at(
                &install,
                &descriptor,
                &package,
                &source,
                VERSION,
                TEST_COMMIT,
                &expected
            )
            .is_err()
        );
        assert_eq!(
            fs::read(install.join("current.txt")).unwrap(),
            b"0.1.0-beta.28\n"
        );
        assert_eq!(fs::read(install.join("user-data")).unwrap(), b"preserve-me");
        assert_eq!(fs::read_dir(install.join("versions")).unwrap().count(), 0);
        let _ = fs::remove_dir_all(install);
        let _ = fs::remove_dir_all(artifacts);
    }
}
