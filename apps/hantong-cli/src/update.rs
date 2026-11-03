//! Start the package-owned updater from an installed CLI.
//!
//! The updater is deliberately a separate PowerShell bootstrap.  The CLI
//! only starts it after validating the currently running version against its
//! release manifest; it never accepts a source URL or arbitrary script path.

use hantong_contracts::ErrorCode;

pub(super) fn run(
    command: &[String],
    lang: super::Lang,
    json: bool,
    no_input: bool,
) -> Option<i32> {
    if command.first().map(String::as_str) != Some("update") {
        return None;
    }
    if command.len() != 1 {
        return Some(super::emit_error(ErrorCode::InvalidInput, lang, json));
    }
    if json
        || no_input
        || !std::io::IsTerminal::is_terminal(&std::io::stdin())
        || !std::io::IsTerminal::is_terminal(&std::io::stdout())
    {
        return Some(super::emit_error(ErrorCode::HumanStepRequired, lang, json));
    }
    #[cfg(not(windows))]
    {
        Some(super::emit_error(
            ErrorCode::CapabilityUnsupported,
            lang,
            false,
        ))
    }
    #[cfg(windows)]
    {
        Some(match run_installed_updater() {
            Ok(()) => 0,
            Err(ErrorCode::InternalError) => {
                eprintln!("INTERNAL_ERROR: {}", super::resource(lang, "update_failed"));
                ErrorCode::InternalError.exit_code()
            }
            Err(code) => super::emit_error(code, lang, false),
        })
    }
}

#[cfg(windows)]
fn run_installed_updater() -> Result<(), ErrorCode> {
    use std::process::{Command, Stdio};

    let executable = std::env::current_exe().map_err(|_| ErrorCode::CoreUnavailable)?;
    let plan = resolve_update_plan(&executable)?;
    let powershell =
        super::core_start::canonical_regular_file(&hantong_platform::system_powershell()?)?;
    let status = Command::new(powershell)
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&plan.updater)
        .args(["-InstallRoot"])
        .arg(&plan.root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|_| ErrorCode::InternalError)?;
    if status.success() {
        Ok(())
    } else {
        Err(ErrorCode::InternalError)
    }
}

#[cfg(windows)]
struct UpdatePlan {
    root: std::path::PathBuf,
    updater: std::path::PathBuf,
}

#[cfg(windows)]
fn resolve_update_plan(executable: &std::path::Path) -> Result<UpdatePlan, ErrorCode> {
    let executable = super::core_start::canonical_regular_file(executable)?;
    if executable.file_name().and_then(|v| v.to_str()) != Some("hantong.exe") {
        return Err(ErrorCode::CoreUnavailable);
    }
    let bin = executable.parent().ok_or(ErrorCode::CoreUnavailable)?;
    if bin.file_name().and_then(|v| v.to_str()) != Some("bin") {
        return Err(ErrorCode::CoreUnavailable);
    }
    let version_dir = bin.parent().ok_or(ErrorCode::CoreUnavailable)?;
    let versions_dir = version_dir.parent().ok_or(ErrorCode::CoreUnavailable)?;
    if versions_dir.file_name().and_then(|v| v.to_str()) != Some("versions") {
        return Err(ErrorCode::CoreUnavailable);
    }
    let root = versions_dir
        .parent()
        .ok_or(ErrorCode::CoreUnavailable)?
        .to_path_buf();
    let root = regular_dir(&root)?;
    let marker = regular_file(&root.join(".mail-hantong-owner"))?;
    if read_bounded(&marker, 64)? != b"mail-hantong-cli-v1" {
        return Err(ErrorCode::CoreUnavailable);
    }
    let current = regular_file(&root.join("current.txt"))?;
    let active =
        String::from_utf8(read_bounded(&current, 128)?).map_err(|_| ErrorCode::CoreUnavailable)?;
    if active.trim()
        != version_dir
            .file_name()
            .and_then(|v| v.to_str())
            .unwrap_or("")
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    super::core_start::validate_package(bin)?;
    let updater = version_dir.join("update.ps1");
    let updater = regular_file(&updater)?;
    validate_updater_binding(version_dir, &updater)?;
    Ok(UpdatePlan { root, updater })
}

#[cfg(windows)]
fn validate_updater_binding(
    version_dir: &std::path::Path,
    updater: &std::path::Path,
) -> Result<(), ErrorCode> {
    let manifest = regular_file(&version_dir.join("release-manifest.json"))?;
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(manifest).map_err(|_| ErrorCode::CoreUnavailable)?)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
    let files = value
        .get("files")
        .and_then(serde_json::Value::as_array)
        .ok_or(ErrorCode::CoreUnavailable)?;
    let records: Vec<_> = files
        .iter()
        .filter(|entry| {
            entry
                .get("path")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|path| path.eq_ignore_ascii_case("update.ps1"))
        })
        .collect();
    if records.len() != 1 {
        return Err(ErrorCode::CoreUnavailable);
    }
    let record = records[0];
    let size = record
        .get("size")
        .and_then(serde_json::Value::as_u64)
        .ok_or(ErrorCode::CoreUnavailable)?;
    let hash = record
        .get("sha256")
        .and_then(serde_json::Value::as_str)
        .ok_or(ErrorCode::CoreUnavailable)?;
    let actual = std::fs::metadata(updater).map_err(|_| ErrorCode::CoreUnavailable)?;
    if actual.len() != size
        || !hash.eq_ignore_ascii_case(&hantong_contracts::wire_hex::encode(
            &hantong_platform::sha256_release_file(updater)
                .map_err(|_| ErrorCode::CoreUnavailable)?,
        ))
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(())
}

#[cfg(windows)]
fn regular_file(path: &std::path::Path) -> Result<std::path::PathBuf, ErrorCode> {
    super::core_start::canonical_regular_file(path)
}

#[cfg(windows)]
fn read_bounded(path: &std::path::Path, limit: u64) -> Result<Vec<u8>, ErrorCode> {
    let metadata = std::fs::metadata(path).map_err(|_| ErrorCode::CoreUnavailable)?;
    if metadata.len() > limit {
        return Err(ErrorCode::CoreUnavailable);
    }
    std::fs::read(path).map_err(|_| ErrorCode::CoreUnavailable)
}

#[cfg(windows)]
fn regular_dir(path: &std::path::Path) -> Result<std::path::PathBuf, ErrorCode> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ErrorCode::CoreUnavailable)?;
    if !metadata.file_type().is_dir() {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    #[test]
    fn update_requires_exact_cli_command_before_any_effect() {
        let invalid = super::run(
            &["update".into(), "hantong".into()],
            super::super::Lang::En,
            false,
            false,
        );
        assert_eq!(
            invalid,
            Some(hantong_contracts::ErrorCode::InvalidInput.exit_code())
        );
    }

    #[test]
    fn update_headless_mode_stops_before_installer() {
        let result = super::run(&["update".into()], super::super::Lang::En, true, false);
        assert_eq!(
            result,
            Some(hantong_contracts::ErrorCode::HumanStepRequired.exit_code())
        );
    }

    #[cfg(windows)]
    fn fixture(
        missing_updater_record: bool,
        tamper_updater: bool,
        current: &str,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        use std::fs;
        let root = std::env::temp_dir().join(format!(
            "mail-hantong-update-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let version = root.join("versions/1.2.3");
        let bin = version.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(root.join(".mail-hantong-owner"), b"mail-hantong-cli-v1").unwrap();
        fs::write(root.join("current.txt"), current).unwrap();
        fs::write(bin.join("hantong.exe"), b"cli").unwrap();
        fs::write(bin.join("mail-hantong-core.exe"), b"core").unwrap();
        fs::write(version.join("license"), b"license").unwrap();
        let updater = version.join("update.ps1");
        fs::write(&updater, b"synthetic updater").unwrap();
        let mut records = Vec::new();
        for relative in ["bin/hantong.exe", "bin/mail-hantong-core.exe", "license"] {
            let path = version.join(relative);
            let hash = hantong_platform::sha256_release_file(&path).unwrap();
            records.push(serde_json::json!({
                "path": relative,
                "sha256": hantong_contracts::wire_hex::encode(&hash),
                "size": fs::metadata(path).unwrap().len()
            }));
        }
        if !missing_updater_record {
            let hash = hantong_platform::sha256_release_file(&updater).unwrap();
            records.push(serde_json::json!({
                "path": "update.ps1",
                "sha256": hantong_contracts::wire_hex::encode(&hash),
                "size": fs::metadata(&updater).unwrap().len()
            }));
        }
        fs::write(
            version.join("release-manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "format_version": 1,
                "version": "1.2.3",
                "package_version": "1.2.3",
                "target": "windows-x86_64",
                "files": records
            }))
            .unwrap(),
        )
        .unwrap();
        if tamper_updater {
            fs::write(&updater, b"tampered updater").unwrap();
        }
        (root, bin.join("hantong.exe"))
    }

    #[cfg(windows)]
    #[test]
    fn installed_update_plan_binds_current_version_root_and_updater() {
        let (root, executable) = fixture(false, false, "1.2.3");
        let plan = super::resolve_update_plan(&executable).unwrap();
        let canonical_root = root.canonicalize().unwrap();
        assert_eq!(plan.root, canonical_root);
        assert_eq!(
            plan.updater,
            canonical_root.join("versions/1.2.3/update.ps1")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(windows)]
    #[test]
    fn installed_update_plan_rejects_wrong_current_or_unbound_updater() {
        for (missing_record, tampered, current) in [
            (false, false, "9.9.9"),
            (true, false, "1.2.3"),
            (false, true, "1.2.3"),
        ] {
            let (root, executable) = fixture(missing_record, tampered, current);
            assert!(super::resolve_update_plan(&executable).is_err());
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[cfg(windows)]
    #[test]
    fn inst03_update_plan_rejects_junction_parent_before_canonicalization() {
        let (root, _) = fixture(false, false, "1.2.3");
        let link = root.with_extension("junction");
        let native_path = |path: &std::path::Path| {
            path.to_string_lossy()
                .strip_prefix(r"\\?\")
                .unwrap_or(&path.to_string_lossy())
                .to_owned()
        };
        let powershell = hantong_platform::system_powershell().unwrap();
        let cmd = powershell
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("cmd.exe");
        let created = std::process::Command::new(cmd)
            .args(["/d", "/c", "mklink", "/J"])
            .arg(native_path(&link))
            .arg(native_path(&root))
            .output()
            .unwrap();
        assert!(created.status.success(), "junction fixture creation failed");
        let result = super::resolve_update_plan(&link.join("versions/1.2.3/bin/hantong.exe"));
        std::fs::remove_dir(&link).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(result.is_err());
    }
}
