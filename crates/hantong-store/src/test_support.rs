//! Shared Linux-only test setup for the encrypted store's headless fixtures.
//!
//! Product code never links this module.  A single synthetic backend keeps
//! tests deterministic when several store fixtures are created in one test
//! process, without consulting or changing a user's real secret service.

use std::{fs, os::unix::fs::PermissionsExt, sync::OnceLock};

pub(crate) fn ensure_headless_backend() {
    static READY: OnceLock<()> = OnceLock::new();
    READY.get_or_init(|| {
        let root =
            std::env::temp_dir().join(format!("hantong-store-test-vault-{}", std::process::id()));
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
    });
}
