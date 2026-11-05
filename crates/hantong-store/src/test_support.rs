//! Shared Unix test setup for the encrypted store's fixtures.
//!
//! Product code never links this module.  A single synthetic backend keeps
//! tests deterministic when several store fixtures are created in one test
//! process. macOS requires an explicitly isolated native test Keychain.

use std::{fs, os::unix::fs::PermissionsExt, sync::OnceLock};

pub(crate) fn ensure_test_backend() {
    static READY: OnceLock<()> = OnceLock::new();
    READY.get_or_init(|| {
        #[cfg(target_os = "linux")]
        install_headless_backend();
        #[cfg(target_os = "macos")]
        require_isolated_keychain();

        // KeyedConnection uses one fixed database-key selector.  Seed it
        // through the public Store API before parallel fixtures open their
        // own isolated databases, otherwise simultaneous first opens race on
        // the key creation and one loses with AlreadyExists.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "hantong-store-test-seed-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let store = crate::Store::open(root.join("data"), root.join("keys")).unwrap();
        drop(store);
        fs::remove_dir_all(root).unwrap();
    });
}

#[cfg(target_os = "macos")]
fn require_isolated_keychain() {
    let expected = std::env::var_os("HANTONG_TEST_KEYCHAIN")
        .expect("Store tests require an explicitly isolated HANTONG_TEST_KEYCHAIN");
    let expected = fs::canonicalize(expected).unwrap();
    let read_paths = |command| {
        let output = std::process::Command::new("/usr/bin/security")
            .args([command, "-d", "user"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| fs::canonicalize(line.trim().trim_matches('"')).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        read_paths("default-keychain").as_slice(),
        std::slice::from_ref(&expected)
    );
    assert_eq!(
        read_paths("list-keychains").as_slice(),
        std::slice::from_ref(&expected)
    );
}

#[cfg(target_os = "linux")]
fn install_headless_backend() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "hantong-store-test-vault-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let vault = hantong_secrets::headless_vault::HeadlessVault::initialize(
        &root,
        "synthetic-store-test-passphrase".to_owned().into(),
    )
    .unwrap();
    match hantong_secrets::unix_secret_store::UnixSecretStore::install_headless_backend(
        vault.into_shared(),
    ) {
        Ok(()) => {}
        Err(hantong_secrets::unix_secret_store::UnixSecretError::Conflict) => {
            let _ = fs::remove_dir_all(&root);
        }
        Err(error) => panic!("install synthetic store test vault: {error}"),
    }
}
