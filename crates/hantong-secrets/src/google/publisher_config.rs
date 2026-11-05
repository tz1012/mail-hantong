//! Optional release-owned Google Desktop client configuration.
//!
//! This module is deliberately the only place that turns the downloaded
//! Google client JSON into the opaque client credential used by OAuth.

use super::GoogleClientSecret;
#[cfg(any(windows, unix, test))]
use super::SecretJson;
#[cfg(any(windows, unix, test))]
use crate::SensitiveBuffer;
use hantong_contracts::ErrorCode;
#[cfg(any(windows, unix, test))]
use serde_json::Value;
#[cfg(any(windows, unix, test))]
use std::collections::BTreeSet;
#[cfg(any(windows, unix, test))]
use std::fs;
#[cfg(any(windows, unix, test))]
use std::io::Read;
#[cfg(any(windows, unix, test))]
use std::path::{Path, PathBuf};

#[cfg(any(windows, unix, test))]
const ASSET: &str = "publisher/google-desktop.json";
#[cfg(any(windows, unix, test))]
const MANIFEST: &str = "release-manifest.json";
#[cfg(any(windows, unix, test))]
const MAX_MANIFEST: u64 = 4 * 1024 * 1024;
#[cfg(any(windows, unix, test))]
const MAX_ASSET: u64 = 16 * 1024;

pub struct InstalledGooglePublisher {
    pub client_id: String,
    pub client_secret: GoogleClientSecret,
}

impl std::fmt::Debug for InstalledGooglePublisher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InstalledGooglePublisher(<redacted>)")
    }
}

/// Load the optional publisher asset next to the installed Core executable.
/// Missing asset is the normal result for source builds and packages that do
/// not opt in. A present but invalid asset fails closed.
pub fn load_installed_publisher_configuration()
-> Result<Option<InstalledGooglePublisher>, ErrorCode> {
    #[cfg(all(not(windows), not(unix), not(test)))]
    {
        Ok(None)
    }
    #[cfg(any(windows, unix, test))]
    {
        #[cfg(unix)]
        {
            let core =
                hantong_platform::unix_release::verified_installed_core("mail-hantong-core")?;
            let bin = core.parent().ok_or(ErrorCode::CoreUnavailable)?;
            if core.file_name().and_then(|name| name.to_str()) != Some("mail-hantong-core")
                || bin.file_name().and_then(|name| name.to_str()) != Some("bin")
            {
                return Ok(None);
            }
            load_from_root(bin.parent().ok_or(ErrorCode::CoreUnavailable)?)
        }
        #[cfg(windows)]
        {
            let exe = std::env::current_exe().map_err(|_| ErrorCode::CoreUnavailable)?;
            let bin = exe.parent().ok_or(ErrorCode::CoreUnavailable)?;
            if exe.file_name().and_then(|name| name.to_str()) != Some("mail-hantong-core.exe")
                || bin.file_name().and_then(|name| name.to_str()) != Some("bin")
            {
                return Ok(None);
            }
            let root = bin.parent().ok_or(ErrorCode::CoreUnavailable)?;
            load_from_root(root)
        }
    }
}

#[cfg(any(windows, unix, test))]
fn load_from_root(root: &Path) -> Result<Option<InstalledGooglePublisher>, ErrorCode> {
    validate_ancestors(root)?;
    validate_ancestors(&root.join("publisher"))?;
    let asset = root.join(ASSET);
    if !asset.exists() {
        return Ok(None);
    }
    validate_regular(&asset)?;
    let manifest = root.join(MANIFEST);
    validate_regular(&manifest)?;
    let manifest_bytes = read_public_bounded(&manifest, MAX_MANIFEST)?;
    reject_duplicate_keys(&manifest_bytes)?;
    let manifest_json: Value =
        serde_json::from_slice(&manifest_bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    let manifest_object = manifest_json
        .as_object()
        .ok_or(ErrorCode::CoreUnavailable)?;
    if manifest_object.len() != 5
        || manifest_object
            .get("format_version")
            .and_then(Value::as_u64)
            != Some(1)
        || manifest_object
            .get("version")
            .and_then(Value::as_str)
            .is_none()
        || manifest_object
            .get("package_version")
            .and_then(Value::as_str)
            .is_none()
        || manifest_object.get("target").and_then(Value::as_str) != Some(expected_target())
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    let records = manifest_object
        .get("files")
        .and_then(Value::as_array)
        .ok_or(ErrorCode::CoreUnavailable)?;
    if records.is_empty()
        || records.iter().any(|entry| {
            let Some(object) = entry.as_object() else {
                return true;
            };
            object.len() != 3
                || object.get("path").and_then(Value::as_str).is_none()
                || object.get("sha256").and_then(Value::as_str).is_none()
                || object.get("size").and_then(Value::as_u64).is_none()
        })
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    let matching: Vec<&Value> = records
        .iter()
        .filter(|entry| entry.get("path").and_then(Value::as_str) == Some(ASSET))
        .collect();
    if matching.len() != 1 {
        return Err(ErrorCode::CoreUnavailable);
    }
    let record = matching[0];
    let size = record
        .get("size")
        .and_then(Value::as_u64)
        .ok_or(ErrorCode::CoreUnavailable)?;
    let expected_hash = record
        .get("sha256")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::CoreUnavailable)?;
    if size > MAX_ASSET || !is_sha256(expected_hash) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let bytes = read_secret_bounded(&asset, MAX_ASSET)?;
    if bytes.as_slice().len() as u64 != size {
        return Err(ErrorCode::CoreUnavailable);
    }
    let actual = hantong_contracts::wire_hex::encode(&super::crypto::sha256(bytes.as_slice())?);
    if !actual.eq_ignore_ascii_case(expected_hash) {
        return Err(ErrorCode::CoreUnavailable);
    }
    parse_installed(bytes.as_slice()).map(Some)
}

#[cfg(any(windows, unix, test))]
fn expected_target() -> &'static str {
    if cfg!(windows) {
        "windows-x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
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

#[cfg(any(windows, unix, test))]
fn parse_installed(bytes: &[u8]) -> Result<InstalledGooglePublisher, ErrorCode> {
    if bytes.len() as u64 > MAX_ASSET {
        return Err(ErrorCode::QuotaExceeded);
    }
    reject_duplicate_keys(bytes)?;
    let mut json = SecretJson::parse(bytes)?;
    if json.0.as_object().is_none_or(|object| object.len() != 1) {
        return Err(ErrorCode::AuthRequired);
    }
    let installed = json.0.get_mut("installed").ok_or(ErrorCode::AuthRequired)?;
    let object = installed.as_object().ok_or(ErrorCode::AuthRequired)?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "client_id"
                | "client_secret"
                | "project_id"
                | "auth_uri"
                | "token_uri"
                | "auth_provider_x509_cert_url"
                | "redirect_uris"
        )
    }) {
        return Err(ErrorCode::AuthRequired);
    }
    let client_id = object
        .get("client_id")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::AuthRequired)?
        .to_owned();
    let auth_uri = object
        .get("auth_uri")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::AuthRequired)?;
    let token_uri = object
        .get("token_uri")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::AuthRequired)?;
    if auth_uri != "https://accounts.google.com/o/oauth2/auth"
        || token_uri != "https://oauth2.googleapis.com/token"
    {
        return Err(ErrorCode::AuthRequired);
    }
    let redirects = object
        .get("redirect_uris")
        .and_then(Value::as_array)
        .ok_or(ErrorCode::AuthRequired)?;
    if !redirects
        .iter()
        .any(|v| v.as_str() == Some("http://localhost"))
    {
        return Err(ErrorCode::AuthRequired);
    }
    let secret = object
        .get("client_secret")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::AuthRequired)?
        .as_bytes()
        .to_vec();
    hantong_contracts::google::validate_google_client_id(&client_id)?;
    let client_secret = GoogleClientSecret::from_capture(&client_id, SensitiveBuffer(secret))?;
    Ok(InstalledGooglePublisher {
        client_id,
        client_secret,
    })
}

#[cfg(any(windows, unix, test))]
fn validate_regular(path: &Path) -> Result<(), ErrorCode> {
    let meta = fs::symlink_metadata(path).map_err(|_| ErrorCode::CoreUnavailable)?;
    if !meta.file_type().is_file() || meta.file_type().is_symlink() {
        return Err(ErrorCode::CoreUnavailable);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(ErrorCode::CoreUnavailable);
        }
    }
    Ok(())
}

#[cfg(any(windows, unix, test))]
fn validate_ancestors(path: &Path) -> Result<(), ErrorCode> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        let Ok(meta) = fs::symlink_metadata(&current) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            return Err(ErrorCode::CoreUnavailable);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(ErrorCode::CoreUnavailable);
            }
        }
    }
    Ok(())
}

#[cfg(any(windows, unix, test))]
fn read_public_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, ErrorCode> {
    validate_regular(path)?;
    let mut file = fs::File::open(path).map_err(|_| ErrorCode::CoreUnavailable)?;
    let metadata = file.metadata().map_err(|_| ErrorCode::CoreUnavailable)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut bytes = Vec::new();
    file.by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if bytes.len() as u64 > limit
        || file
            .metadata()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .len()
            != bytes.len() as u64
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(bytes)
}

#[cfg(any(windows, unix, test))]
fn read_secret_bounded(path: &Path, limit: u64) -> Result<SensitiveBuffer, ErrorCode> {
    validate_regular(path)?;
    let mut file = fs::File::open(path).map_err(|_| ErrorCode::CoreUnavailable)?;
    let metadata = file.metadata().map_err(|_| ErrorCode::CoreUnavailable)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(ErrorCode::CoreUnavailable);
    }
    let bytes =
        SensitiveBuffer::read_limited(file.by_ref().take(limit.saturating_add(1)), limit as usize)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
    if bytes.as_slice().len() as u64 > limit
        || file
            .metadata()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .len()
            != bytes.as_slice().len() as u64
    {
        return Err(ErrorCode::CoreUnavailable);
    }
    Ok(bytes)
}

#[cfg(any(windows, unix, test))]
fn is_sha256(v: &str) -> bool {
    v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())
}

// serde_json follows the last duplicate member. Release metadata and secret
// input must reject that ambiguity instead.
#[cfg(any(windows, unix, test))]
fn reject_duplicate_keys(bytes: &[u8]) -> Result<(), ErrorCode> {
    // Dynamic parsing cannot reveal duplicates, so inspect raw object members
    // with a small lexical pass that records keys at every object depth.
    let mut stack: Vec<BTreeSet<String>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    i += 2;
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
                    .map_err(|_| ErrorCode::InvalidInput)?;
                if !set.insert(key) {
                    return Err(ErrorCode::InvalidInput);
                }
            }
            continue;
        } else if bytes[i] == b'{' {
            stack.push(BTreeSet::new());
        } else if bytes[i] == b'}' {
            stack.pop().ok_or(ErrorCode::InvalidInput)?;
        }
        i += 1;
    }
    if !stack.is_empty() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path, body: &str) {
        fs::create_dir_all(root.join("publisher")).unwrap();
        fs::write(root.join(ASSET), body).unwrap();
        let hash = hantong_contracts::wire_hex::encode(
            &super::super::crypto::sha256(body.as_bytes()).unwrap(),
        );
        let manifest = format!(
            r#"{{"format_version":1,"version":"0.1.0","package_version":"0.1.0","target":"{target}","files":[{{"path":"{ASSET}","sha256":"{hash}","size":{size}}}]}}"#,
            target = expected_target(),
            size = body.len()
        );
        fs::write(root.join(MANIFEST), manifest).unwrap();
    }
    #[test]
    fn acc01_loads_matching_installed_desktop_asset_without_exposing_secret_debug() {
        let root = std::env::temp_dir().join(format!("hantong-publisher-{}", std::process::id()));
        let body = r#"{"installed":{"client_id":"fixture.apps.googleusercontent.com","client_secret":"fixture-secret","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token","redirect_uris":["http://localhost"]}}"#;
        fixture(&root, body);
        // macOS temp_dir may use /var -> /private/var. Validate the actual
        // fixture location without weakening the product's symlink guard.
        #[cfg(unix)]
        let root = fs::canonicalize(root).unwrap();
        let config = load_from_root(&root).unwrap().unwrap();
        assert_eq!(config.client_id, "fixture.apps.googleusercontent.com");
        assert!(!format!("{config:?}").contains("fixture-secret"));
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn acc01_rejects_duplicate_or_tampered_asset() {
        let root =
            std::env::temp_dir().join(format!("hantong-publisher-dup-{}", std::process::id()));
        fixture(
            &root,
            r#"{"installed":{"client_id":"fixture.apps.googleusercontent.com","client_id":"other","client_secret":"x","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token","redirect_uris":["http://localhost"]}}"#,
        );
        #[cfg(unix)]
        let root = fs::canonicalize(root).unwrap();
        assert!(matches!(
            load_from_root(&root),
            Err(ErrorCode::InvalidInput)
        ));
        fs::write(root.join(ASSET), b"tampered").unwrap();
        assert!(matches!(
            load_from_root(&root),
            Err(ErrorCode::CoreUnavailable)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn acc01_json_member_order_does_not_change_valid_configuration() {
        let body = br#"{"installed":{"redirect_uris":["http://localhost"],"client_id":"fixture.apps.googleusercontent.com","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token","client_secret":"fixture-secret"}}"#;
        let config = parse_installed(body).unwrap();
        assert_eq!(config.client_id, "fixture.apps.googleusercontent.com");
    }

    #[test]
    fn inst03_manifest_size_must_match_even_when_hash_is_valid() {
        let root =
            std::env::temp_dir().join(format!("hantong-publisher-size-{}", std::process::id()));
        fixture(&root, r#"{"installed":{}}"#);
        #[cfg(unix)]
        let root = fs::canonicalize(root).unwrap();
        let path = root.join(MANIFEST);
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["files"][0]["size"] = 1.into();
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(matches!(
            load_from_root(&root),
            Err(ErrorCode::CoreUnavailable)
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sec01_publisher_web_clients_and_non_google_endpoints_are_rejected() {
        assert!(
            parse_installed(br#"{"web":{"client_id":"fixture.apps.googleusercontent.com"}}"#)
                .is_err()
        );
        let body = br#"{"installed":{"redirect_uris":["http://localhost"],"client_id":"fixture.apps.googleusercontent.com","auth_uri":"https://untrusted.invalid/auth","token_uri":"https://oauth2.googleapis.com/token","client_secret":"fixture-secret"}}"#;
        assert!(parse_installed(body).is_err());
        assert!(parse_installed(&vec![b' '; 16385]).is_err());
    }
}
