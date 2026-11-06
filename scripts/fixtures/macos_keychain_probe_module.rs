//! Temporary macOS-only Keychain probe injected by the compatibility harness.
//!
//! This module is intentionally test-only. It reads one synthetic OAuth item
//! through Security.framework, never creates, updates, deletes, or prints the
//! item, and keeps all returned bytes inside this Secrets test boundary.

#![cfg(target_os = "macos")]

use super::MailCredentialSlot;
use super::unix_secret_store::{SERVICE, UnixSecretKind, UnixSecretSelector};
use security_framework::os::macos::keychain::SecKeychain;
use security_framework::os::macos::passwords::find_generic_password;
use std::env;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::PathBuf;

const ACCOUNT: &str = "synthetic-upgrade-account";
const COMPAT_ROOT_ENV: &str = "HANTONG_COMPAT_ROOT";
const KEYCHAIN_ENV: &str = "HANTONG_TEST_KEYCHAIN";
const EXPECTED_IMAP: &[u8] = b"synthetic-old-imap";

fn error_category(status: i32) -> &'static str {
    match status {
        -25308 => "interaction_not_allowed",
        -25291 | -25294 => "unavailable",
        -25293 => "auth_failed",
        -25300 => "not_found",
        _ => "other_error",
    }
}

fn marker(stage: &'static str, value: &'static str) {
    eprintln!("\nHANTONG_KEYCHAIN_PROBE stage={stage} category={value}");
}

fn isolated_keychain() -> Result<PathBuf, &'static str> {
    let root = PathBuf::from(env::var_os(COMPAT_ROOT_ENV).ok_or("other_error")?);
    let keychain = PathBuf::from(env::var_os(KEYCHAIN_ENV).ok_or("other_error")?);
    let runner = PathBuf::from(env::var_os("RUNNER_TEMP").ok_or("other_error")?);
    if !root.is_absolute()
        || !keychain.is_absolute()
        || !runner.is_absolute()
        || env::var("CI").as_deref() != Ok("true")
        || env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
    {
        return Err("other_error");
    }
    let root_meta = fs::symlink_metadata(&root).map_err(|_| "other_error")?;
    let keychain_meta = fs::symlink_metadata(&keychain).map_err(|_| "other_error")?;
    if !root_meta.is_dir()
        || root_meta.file_type().is_symlink()
        || root_meta.permissions().mode() & 0o077 != 0
        || !keychain_meta.is_file()
        || keychain_meta.file_type().is_symlink()
    {
        return Err("other_error");
    }
    let runner = fs::canonicalize(runner).map_err(|_| "other_error")?;
    let owner = fs::metadata(&runner).map_err(|_| "other_error")?.uid();
    let root = fs::canonicalize(root).map_err(|_| "other_error")?;
    let keychain = fs::canonicalize(keychain).map_err(|_| "other_error")?;
    if !root.starts_with(&runner)
        || root.file_name().and_then(|s| s.to_str()) != Some("keychain-compat-secrets")
        || keychain.parent() != Some(runner.as_path())
        || keychain.file_name().and_then(|s| s.to_str()) != Some("hantong-ci.keychain-db")
        || root_meta.uid() != owner
        || keychain_meta.uid() != owner
    {
        return Err("other_error");
    }
    Ok(keychain)
}

fn read_synthetic_imap_oauth() -> Result<(), &'static str> {
    match SecKeychain::user_interaction_allowed() {
        Ok(true) => {}
        Ok(false) | Err(_) => {
            marker("guard", "other_error");
            return Err("other_error");
        }
    }
    let interaction_lock = match SecKeychain::disable_user_interaction() {
        Ok(lock) => lock,
        Err(error) => {
            let value = error_category(error.code());
            marker("guard", value);
            return Err(value);
        }
    };
    if SecKeychain::user_interaction_allowed().ok() != Some(false) {
        marker("guard", "other_error");
        return restore(interaction_lock, Err("other_error"));
    }
    marker("guard", "ready");

    let probe_result = match isolated_keychain() {
        Err(value) => {
            marker("store_open", value);
            Err(value)
        }
        Ok(keychain_path) => match SecKeychain::open(keychain_path) {
            Err(error) => {
                let value = error_category(error.code());
                marker("store_open", value);
                Err(value)
            }
            Ok(keychain) => {
                marker("store_open", "ready");
                let selector = match UnixSecretSelector::new(
                    UnixSecretKind::MailCredential(MailCredentialSlot::ImapOAuthRefresh),
                    Some(ACCOUNT),
                    1,
                ) {
                    Ok(selector) => selector,
                    Err(_) => {
                        marker("read", "other_error");
                        return restore(interaction_lock, Err("other_error"));
                    }
                };
                match find_generic_password(
                    Some(std::slice::from_ref(&keychain)),
                    SERVICE,
                    selector.as_str(),
                ) {
                    Ok((password, _item)) if password.as_ref() == EXPECTED_IMAP => {
                        marker("read", "ok");
                        Ok(())
                    }
                    Ok((_password, _item)) => {
                        marker("read", "invalid_item");
                        Ok(())
                    }
                    Err(error) => {
                        marker("read", error_category(error.code()));
                        Ok(())
                    }
                }
            }
        },
    };
    restore(interaction_lock, probe_result)
}

fn restore(
    interaction_lock: security_framework::os::macos::keychain::KeychainUserInteractionLock,
    result: Result<(), &'static str>,
) -> Result<(), &'static str> {
    drop(interaction_lock);
    match SecKeychain::user_interaction_allowed() {
        Ok(true) => marker("restore", "ok"),
        Ok(false) | Err(_) => {
            marker("restore", "other_error");
            return Err("other_error");
        }
    }
    result
}

#[test]
fn mac_keychain_read_probe() {
    assert_eq!(error_category(-25308), "interaction_not_allowed");
    assert_eq!(error_category(-25291), "unavailable");
    assert_eq!(error_category(-25294), "unavailable");
    assert_eq!(error_category(-25293), "auth_failed");
    assert_eq!(error_category(-25300), "not_found");
    assert_eq!(error_category(-1), "other_error");
    if read_synthetic_imap_oauth().is_err() {
        panic!("macOS Keychain probe failed");
    }
}
