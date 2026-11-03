//! Bounded first-run starter for the installed Windows CLI.
//!
//! This module deliberately does not create accounts, registrations, or
//! credentials.  It only attaches to an existing core or starts the verified
//! sibling shipped in the same release directory and covered by its release
//! manifest hashes.

use hantong_contracts::ErrorCode;

#[cfg(windows)]
const CORE_NAME: &str = "mail-hantong-core.exe";
#[cfg(windows)]
const CLI_NAME: &str = "hantong.exe";
#[cfg(windows)]
const MANIFEST_NAME: &str = "release-manifest.json";
#[cfg(windows)]
const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
#[cfg(windows)]
const MAX_FILE: u64 = hantong_platform::MAX_RELEASE_FILE;

pub(super) fn ensure_started() -> Result<(), ErrorCode> {
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    {
        ensure_started_with(hantong_ipc_client::core_status, start_installed_core)
    }
}

/// Run the package-verified Core helper in this console. No native window and
/// no credential plaintext pass through the CLI.
pub(super) fn review(
    kind: hantong_contracts::terminal_review::TerminalReviewKind,
    request_id: [u8; 32],
    account_id: Option<&str>,
    payload_hash: Option<[u8; 32]>,
    lang: super::Lang,
) -> Result<(), ErrorCode> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(ErrorCode::HumanStepRequired);
    }
    hantong_contracts::terminal_review::TerminalReviewGet::new(
        kind,
        request_id,
        account_id.map(str::to_owned),
        payload_hash.unwrap_or([0; 32]),
    )?;
    #[cfg(not(windows))]
    {
        let _ = lang;
        Err(ErrorCode::CapabilityUnsupported)
    }
    #[cfg(windows)]
    {
        use std::process::{Command, Stdio};
        let current = canonical_regular_file(
            &std::env::current_exe().map_err(|_| ErrorCode::CoreUnavailable)?,
        )?;
        if current.file_name().and_then(|v| v.to_str()) != Some(CLI_NAME) {
            return Err(ErrorCode::CoreUnavailable);
        }
        let bin = current.parent().ok_or(ErrorCode::CoreUnavailable)?;
        validate_package(bin)?;
        let mut child = Command::new(bin.join(CORE_NAME));
        child.args([
            "terminal-review",
            "--kind",
            kind.as_str(),
            "--request-id",
            &hantong_contracts::wire_hex::encode(&request_id),
            "--lang",
            if matches!(lang, super::Lang::Ko) {
                "ko"
            } else {
                "en"
            },
        ]);
        if let Some(account) = account_id {
            child.args(["--account-id", account]);
        }
        if let Some(hash) = payload_hash {
            child.args([
                "--payload-hash",
                &hantong_contracts::wire_hex::encode(&hash),
            ]);
        }
        let status = child
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        if status.success() {
            Ok(())
        } else {
            Err(ErrorCode::ALL
                .into_iter()
                .find(|v| Some(v.exit_code()) == status.code())
                .unwrap_or(ErrorCode::CoreUnavailable))
        }
    }
}

#[cfg(windows)]
fn ensure_started_with<S, F>(mut status: S, mut start: F) -> Result<(), ErrorCode>
where
    S: FnMut() -> Result<hantong_ipc_client::Readiness, ErrorCode>,
    F: FnMut() -> Result<(), ErrorCode>,
{
    match status() {
        Ok(readiness) if readiness.transport_ready => Ok(()),
        Ok(_) => Err(ErrorCode::CoreUnavailable),
        Err(ErrorCode::CoreUnavailable) => start(),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn start_installed_core() -> Result<(), ErrorCode> {
    use std::env;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let current = env::current_exe().map_err(|_| ErrorCode::CoreUnavailable)?;
    let current = canonical_regular_file(&current)?;
    if current.file_name().and_then(|n| n.to_str()) != Some(CLI_NAME) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let bin = current.parent().ok_or(ErrorCode::CoreUnavailable)?;
    validate_package(bin)?;
    let core = bin.join(CORE_NAME);
    validate_regular_file(&core)?;

    let mut child = Command::new(&core);
    child
        .arg("run")
        .creation_flags(0x0800_0000)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = child.spawn().map_err(|_| ErrorCode::CoreUnavailable)?;
    wait_for_readiness(
        child,
        hantong_ipc_client::core_status,
        Duration::from_secs(10),
    )
}

#[cfg(windows)]
fn wait_for_readiness<F>(
    child: std::process::Child,
    mut status: F,
    timeout: std::time::Duration,
) -> Result<(), ErrorCode>
where
    F: FnMut() -> Result<hantong_ipc_client::Readiness, ErrorCode>,
{
    use std::thread;
    use std::time::{Duration, Instant};

    let mut child = ChildGuard::new(child);
    let deadline = Instant::now() + timeout;
    loop {
        if child
            .try_wait()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .is_some()
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        match status() {
            Ok(readiness) if readiness.transport_ready => {
                child.disarm();
                return Ok(());
            }
            Ok(_) | Err(ErrorCode::CoreUnavailable) => {}
            Err(error) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Err(ErrorCode::CoreUnavailable);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(windows)]
struct ChildGuard {
    child: Option<std::process::Child>,
}

#[cfg(windows)]
impl ChildGuard {
    fn new(child: std::process::Child) -> Self {
        Self { child: Some(child) }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child
            .as_mut()
            .expect("child guard is armed")
            .try_wait()
    }

    fn disarm(mut self) {
        let _ = self.child.take();
    }
}

#[cfg(windows)]
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        if child.try_wait().ok().flatten().is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }
}

#[cfg(windows)]
pub(super) fn validate_package(bin: &std::path::Path) -> Result<(), ErrorCode> {
    use std::fs;
    let root = bin.parent().ok_or(ErrorCode::CoreUnavailable)?;
    validate_ancestors(root)?;
    let manifest = root.join(MANIFEST_NAME);
    validate_regular_file(&manifest)?;
    let metadata = fs::metadata(&manifest).map_err(|_| ErrorCode::CoreUnavailable)?;
    if metadata.len() > MAX_MANIFEST {
        return Err(ErrorCode::CoreUnavailable);
    }
    let bytes = fs::read(&manifest).map_err(|_| ErrorCode::CoreUnavailable)?;
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    assert_no_duplicate_keys(&bytes)?;
    let object = value.as_object().ok_or(ErrorCode::CoreUnavailable)?;
    let allowed = [
        "format_version",
        "version",
        "package_version",
        "target",
        "files",
    ];
    if object.len() != allowed.len()
        || object.keys().any(|key| !allowed.contains(&key.as_str()))
        || object
            .get("format_version")
            .and_then(serde_json::Value::as_u64)
            != Some(1)
        || object.get("target").and_then(serde_json::Value::as_str) != Some("windows-x86_64")
        || object
            .get("version")
            .and_then(serde_json::Value::as_str)
            .is_none()
        || object
            .get("package_version")
            .and_then(serde_json::Value::as_str)
            .is_none()
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    let files = object
        .get("files")
        .and_then(serde_json::Value::as_array)
        .ok_or(ErrorCode::CoreUnavailable)?;
    if files.is_empty() || files.len() > 4096 {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut seen = std::collections::BTreeSet::new();
    for entry in files {
        let record = entry.as_object().ok_or(ErrorCode::CoreUnavailable)?;
        if record
            .keys()
            .any(|key| !["path", "sha256", "size"].contains(&key.as_str()))
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        let path = record
            .get("path")
            .and_then(serde_json::Value::as_str)
            .ok_or(ErrorCode::CoreUnavailable)?;
        let hash = record
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or(ErrorCode::CoreUnavailable)?;
        let size = record
            .get("size")
            .and_then(serde_json::Value::as_u64)
            .ok_or(ErrorCode::CoreUnavailable)?;
        if size > MAX_FILE
            || !is_safe_relative(path)
            || !is_sha256(hash)
            || !seen.insert(path.to_ascii_lowercase())
        {
            return Err(ErrorCode::CoreUnavailable);
        }
        let file = safe_join(root, path)?;
        validate_regular_file(&file)?;
        let actual = fs::metadata(&file).map_err(|_| ErrorCode::CoreUnavailable)?;
        if actual.len() != size || hex_hash(&file)? != hash.to_ascii_lowercase() {
            return Err(ErrorCode::CoreUnavailable);
        }
    }
    if !seen.contains("bin/hantong.exe") || !seen.contains("bin/mail-hantong-core.exe") {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(())
}

#[cfg(windows)]
fn assert_no_duplicate_keys(bytes: &[u8]) -> Result<(), ErrorCode> {
    fn whitespace(bytes: &[u8], cursor: &mut usize) {
        while bytes
            .get(*cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace())
        {
            *cursor += 1;
        }
    }
    fn string(bytes: &[u8], cursor: &mut usize) -> Result<String, ErrorCode> {
        if bytes.get(*cursor) != Some(&b'"') {
            return Err(ErrorCode::CoreUnavailable);
        }
        let start = *cursor;
        *cursor += 1;
        while let Some(byte) = bytes.get(*cursor) {
            *cursor += 1;
            if *byte == b'\\' {
                *cursor += 1;
                continue;
            }
            if *byte == b'"' {
                return serde_json::from_slice(&bytes[start..*cursor])
                    .map_err(|_| ErrorCode::CoreUnavailable);
            }
        }
        Err(ErrorCode::CoreUnavailable)
    }
    fn value(bytes: &[u8], cursor: &mut usize) -> Result<(), ErrorCode> {
        whitespace(bytes, cursor);
        match bytes.get(*cursor) {
            Some(b'{') => {
                *cursor += 1;
                whitespace(bytes, cursor);
                let mut keys = std::collections::BTreeSet::new();
                if bytes.get(*cursor) == Some(&b'}') {
                    *cursor += 1;
                    return Ok(());
                }
                loop {
                    whitespace(bytes, cursor);
                    let key = string(bytes, cursor)?;
                    if !keys.insert(key) {
                        return Err(ErrorCode::CoreUnavailable);
                    }
                    whitespace(bytes, cursor);
                    if bytes.get(*cursor) != Some(&b':') {
                        return Err(ErrorCode::CoreUnavailable);
                    }
                    *cursor += 1;
                    value(bytes, cursor)?;
                    whitespace(bytes, cursor);
                    match bytes.get(*cursor) {
                        Some(b',') => *cursor += 1,
                        Some(b'}') => {
                            *cursor += 1;
                            break;
                        }
                        _ => return Err(ErrorCode::CoreUnavailable),
                    }
                }
                Ok(())
            }
            Some(b'[') => {
                *cursor += 1;
                whitespace(bytes, cursor);
                if bytes.get(*cursor) == Some(&b']') {
                    *cursor += 1;
                    return Ok(());
                }
                loop {
                    value(bytes, cursor)?;
                    whitespace(bytes, cursor);
                    match bytes.get(*cursor) {
                        Some(b',') => *cursor += 1,
                        Some(b']') => {
                            *cursor += 1;
                            break;
                        }
                        _ => return Err(ErrorCode::CoreUnavailable),
                    }
                }
                Ok(())
            }
            Some(b'"') => string(bytes, cursor).map(|_| ()),
            Some(_) => {
                let start = *cursor;
                while bytes
                    .get(*cursor)
                    .is_some_and(|byte| !b",]} \t\r\n".contains(byte))
                {
                    *cursor += 1;
                }
                if start == *cursor {
                    Err(ErrorCode::CoreUnavailable)
                } else {
                    Ok(())
                }
            }
            None => Err(ErrorCode::CoreUnavailable),
        }
    }
    let mut cursor = 0;
    value(bytes, &mut cursor)?;
    whitespace(bytes, &mut cursor);
    if cursor == bytes.len() {
        Ok(())
    } else {
        Err(ErrorCode::CoreUnavailable)
    }
}

#[cfg(windows)]
fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.chars().any(|character| character.is_control())
        && !path.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with('.')
                || part.ends_with(' ')
        })
}

#[cfg(windows)]
fn safe_join(root: &std::path::Path, path: &str) -> Result<std::path::PathBuf, ErrorCode> {
    let joined = root.join(path);
    let canonical = canonical_regular_file(&joined)?;
    if !canonical.starts_with(root) {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(canonical)
}

#[cfg(windows)]
fn validate_regular_file(path: &std::path::Path) -> Result<(), ErrorCode> {
    use std::fs;
    let metadata = fs::symlink_metadata(path).map_err(|_| ErrorCode::CoreUnavailable)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ErrorCode::CoreUnavailable);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(ErrorCode::CoreUnavailable);
        }
    }
    validate_ancestors(path)
}

#[cfg(windows)]
fn validate_ancestors(path: &std::path::Path) -> Result<(), ErrorCode> {
    use std::fs;
    let mut cursor = path.to_path_buf();
    while let Some(parent) = cursor.parent() {
        let metadata = fs::symlink_metadata(parent).map_err(|_| ErrorCode::CoreUnavailable)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(ErrorCode::CoreUnavailable);
            }
        }
        if parent == cursor {
            break;
        }
        cursor = parent.to_path_buf();
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn canonical_regular_file(
    path: &std::path::Path,
) -> Result<std::path::PathBuf, ErrorCode> {
    validate_regular_file(path)?;
    path.canonicalize().map_err(|_| ErrorCode::CoreUnavailable)
}

#[cfg(windows)]
fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(windows)]
fn hex_hash(path: &std::path::Path) -> Result<String, ErrorCode> {
    let digest =
        hantong_platform::sha256_release_file(path).map_err(|_| ErrorCode::CoreUnavailable)?;
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_first_run_non_windows_is_explicitly_unsupported() {
        #[cfg(not(windows))]
        assert_eq!(ensure_started(), Err(ErrorCode::CapabilityUnsupported));
    }

    #[test]
    fn cli_first_run_only_core_unavailable_may_spawn() {
        fn decide(status: Result<(), ErrorCode>, spawned: &mut usize) -> Result<(), ErrorCode> {
            match status {
                Ok(()) => Ok(()),
                Err(ErrorCode::CoreUnavailable) => {
                    *spawned += 1;
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        let mut count = 0;
        assert_eq!(decide(Ok(()), &mut count), Ok(()));
        assert_eq!(
            decide(Err(ErrorCode::AuthRequired), &mut count),
            Err(ErrorCode::AuthRequired)
        );
        assert_eq!(decide(Err(ErrorCode::CoreUnavailable), &mut count), Ok(()));
        assert_eq!(count, 1);
    }

    #[cfg(windows)]
    fn readiness(transport_ready: bool) -> hantong_ipc_client::Readiness {
        hantong_ipc_client::Readiness {
            transport_ready,
            mail_ready: false,
            authenticated: false,
            protocol_version: 1,
        }
    }

    #[cfg(windows)]
    #[test]
    fn cli_first_run_lifecycle_never_spawns_for_ready_not_ready_or_auth_error() {
        let mut starts = 0;
        let mut start = || {
            starts += 1;
            Ok(())
        };
        assert_eq!(
            ensure_started_with(|| Ok(readiness(true)), &mut start),
            Ok(())
        );
        assert_eq!(
            ensure_started_with(|| Ok(readiness(false)), &mut start),
            Err(ErrorCode::CoreUnavailable)
        );
        assert_eq!(
            ensure_started_with(|| Err(ErrorCode::AuthRequired), &mut start),
            Err(ErrorCode::AuthRequired)
        );
        assert_eq!(starts, 0);
    }

    #[cfg(windows)]
    #[test]
    fn cli_first_run_lifecycle_starts_at_most_once_after_unavailable() {
        let mut starts = 0;
        let result = ensure_started_with(
            || Err(ErrorCode::CoreUnavailable),
            || {
                starts += 1;
                Err(ErrorCode::CoreUnavailable)
            },
        );
        assert_eq!(result, Err(ErrorCode::CoreUnavailable));
        assert_eq!(starts, 1);
    }

    #[cfg(windows)]
    fn synthetic_package() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let root = std::env::temp_dir().join(format!(
            "hantong-core-start-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let root = root.canonicalize().unwrap();
        let bin = root.join("bin");
        let cli = bin.join(CLI_NAME);
        let core = bin.join(CORE_NAME);
        std::fs::write(&cli, b"synthetic-cli").unwrap();
        std::fs::write(&core, b"synthetic-core").unwrap();
        let records = [&cli, &core]
            .into_iter()
            .map(|path| {
                let data = std::fs::read(path).unwrap();
                let digest = hantong_platform::sha256(&data).unwrap();
                serde_json::json!({
                    "path": format!("bin/{}", path.file_name().unwrap().to_string_lossy()),
                    "sha256": digest.iter().map(|b| format!("{b:02x}")).collect::<String>(),
                    "size": data.len()
                })
            })
            .collect::<Vec<_>>();
        let manifest = serde_json::json!({
            "format_version": 1,
            "version": "0.1.0-preview.1",
            "package_version": "0.1.0",
            "target": "windows-x86_64",
            "files": records
        });
        std::fs::write(
            root.join(MANIFEST_NAME),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        (root, cli, core)
    }

    #[cfg(windows)]
    #[test]
    fn cli_first_run_validates_synthetic_package_and_detects_tamper() {
        let (root, _cli, core) = synthetic_package();
        assert!(validate_package(&root.join("bin")).is_ok());
        std::fs::write(core, b"tampered-core").unwrap();
        assert_eq!(
            validate_package(&root.join("bin")),
            Err(ErrorCode::CoreUnavailable)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn cli_first_run_rejects_duplicate_nested_and_traversal_manifest_data() {
        let (root, _cli, _core) = synthetic_package();
        let manifest = root.join(MANIFEST_NAME);
        let duplicate = br#"{"format_version":1,"format_version":1,"version":"0.1.0","package_version":"0.1.0","target":"windows-x86_64","files":[]}"#;
        std::fs::write(&manifest, duplicate).unwrap();
        assert_eq!(
            validate_package(&root.join("bin")),
            Err(ErrorCode::CoreUnavailable)
        );
        let mut nested = "[".repeat(128);
        nested.push('0');
        nested.push_str("]".repeat(128).as_str());
        std::fs::write(&manifest, nested).unwrap();
        assert_eq!(
            validate_package(&root.join("bin")),
            Err(ErrorCode::CoreUnavailable)
        );
        let traversal = serde_json::json!({
            "format_version": 1,
            "version": "0.1.0-preview.1",
            "package_version": "0.1.0",
            "target": "windows-x86_64",
            "files": [{"path": "../escape.exe", "sha256": "0".repeat(64), "size": 0}]
        });
        std::fs::write(&manifest, serde_json::to_vec(&traversal).unwrap()).unwrap();
        assert_eq!(
            validate_package(&root.join("bin")),
            Err(ErrorCode::CoreUnavailable)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn cli_first_run_child_guard_cleans_failed_start_and_keeps_ready_start_alive() {
        use std::process::Command;
        use std::thread;
        use std::time::Duration;

        let failed = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .spawn()
            .unwrap();
        let failed_pid = failed.id();
        assert_eq!(
            wait_for_readiness(
                failed,
                || Err(ErrorCode::CoreUnavailable),
                Duration::from_millis(20),
            ),
            Err(ErrorCode::CoreUnavailable)
        );
        let tasklist = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {failed_pid}")])
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&tasklist.stdout).contains(&failed_pid.to_string()));

        let ready = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .spawn()
            .unwrap();
        let ready_pid = ready.id();
        assert_eq!(
            wait_for_readiness(ready, || Ok(readiness(true)), Duration::from_secs(1),),
            Ok(())
        );
        thread::sleep(Duration::from_millis(100));
        let tasklist = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {ready_pid}")])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&tasklist.stdout).contains(&ready_pid.to_string()));
        let _ = Command::new("taskkill")
            .args(["/PID", &ready_pid.to_string(), "/T", "/F"])
            .output();
    }
}
