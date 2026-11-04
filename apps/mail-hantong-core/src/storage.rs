use hantong_contracts::ErrorCode;
use hantong_platform::AppDirectories;
use hantong_store::Store;

// Only the trusted startup composition supplies these directories. There is no
// command-line or request parameter that can redirect the product database.
pub(super) fn initialize_storage(directories: &AppDirectories) -> Result<Store, ErrorCode> {
    let store = Store::open(&directories.data, &directories.secrets)?;
    store.initialize_config("UTC")?;
    Ok(store)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use hantong_platform::ServerSigningKey;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture {
        root: PathBuf,
        directories: AppDirectories,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "hantong-core-startup-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            let directories = AppDirectories {
                data: root.join("data"),
                secrets: root.join("keys"),
                profiles: root.join("profiles"),
            };
            Self { root, directories }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    struct UnicodeStartupFixture {
        root: PathBuf,
        directories: AppDirectories,
        key_ids: Vec<[u8; 32]>,
    }

    impl UnicodeStartupFixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join("한통 사용자").join(format!(
                "Local App Data {} - {}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            let product = root.join("mail-hantong");
            let directories = AppDirectories {
                data: product.join("data"),
                secrets: product.join("secrets"),
                profiles: product.join("profiles"),
            };
            assert!(!product.exists());
            assert!(!directories.profiles.exists());
            Self {
                root,
                directories,
                key_ids: Vec::new(),
            }
        }
    }

    impl Drop for UnicodeStartupFixture {
        fn drop(&mut self) {
            for id in self.key_ids.drain(..) {
                let _ = ServerSigningKey::open(id).and_then(|key| key.delete());
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn inst01_inst07_startup_retains_store_lock_and_preserves_saved_config() {
        let fixture = Fixture::new();
        let original =
            Store::open(&fixture.directories.data, &fixture.directories.secrets).unwrap();
        let saved = original.initialize_config("Asia/Seoul").unwrap();
        drop(original);
        let running = initialize_storage(&fixture.directories).unwrap();
        assert_eq!(running.load_config().unwrap(), Some(saved.clone()));
        assert!(matches!(
            initialize_storage(&fixture.directories),
            Err(ErrorCode::Conflict)
        ));
        drop(running);
        let restarted = initialize_storage(&fixture.directories).unwrap();
        assert_eq!(restarted.load_config().unwrap(), Some(saved));
    }

    #[test]
    fn inst01_sec02_startup_rejects_missing_key_without_replacing_database() {
        let fixture = Fixture::new();
        let running = initialize_storage(&fixture.directories).unwrap();
        let config = running.load_config().unwrap().unwrap();
        assert_eq!(config.settings_version, 1);
        assert_eq!(config.locale.timezone_id, "UTC");
        assert!(!config.security.external_ai_enabled);
        drop(running);
        let db = fixture.directories.data.join("mail-hantong.db");
        let before = fs::read(&db).unwrap();
        fs::remove_file(fixture.directories.secrets.join("database-v1.secret")).unwrap();
        assert!(matches!(
            initialize_storage(&fixture.directories),
            Err(ErrorCode::InternalError)
        ));
        assert_eq!(fs::read(db).unwrap(), before);
        assert!(
            !fixture
                .directories
                .secrets
                .join("database-v1.secret")
                .exists()
        );
    }

    #[test]
    fn inst01_inst07_startup_supports_unicode_user_path_and_identity_restart() {
        let mut fixture = UnicodeStartupFixture::new();

        let profile_guard =
            hantong_approval_windows::prepare_profile_directories(&fixture.directories).unwrap();
        let store = initialize_storage(&fixture.directories).unwrap();
        let first = crate::identity::initialize_server_identity(&store).unwrap();
        let key_id = first.id();
        let public_key = first.public_key().unwrap();
        fixture.key_ids.push(key_id);
        drop(first);
        drop(store);
        drop(profile_guard);

        let profile_guard =
            hantong_approval_windows::prepare_profile_directories(&fixture.directories).unwrap();
        let store = initialize_storage(&fixture.directories).unwrap();
        let second = crate::identity::initialize_server_identity(&store).unwrap();
        assert_eq!(second.id(), key_id);
        assert_eq!(second.public_key().unwrap(), public_key);
        drop(second);
        drop(store);
        drop(profile_guard);
    }
}
