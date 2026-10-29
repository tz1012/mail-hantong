#![cfg(windows)]
use hantong_contracts::ErrorCode;
use hantong_store::Store;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn open(&self) -> Store {
        Store::open(self.0.join("data"), self.0.join("keys")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn inst07_config_is_persisted_atomically_with_matching_version() {
    let f = Fixture::new();
    let store = f.open();
    let config = store.initialize_config("Asia/Seoul").unwrap();
    assert_eq!(config.settings_version, 1);
    let mut json = serde_json::to_value(config).unwrap();
    json["cli"]["language"] = "ko".into();
    json["gui"]["theme"] = "dark".into();
    let next = store
        .update_config(&serde_json::to_vec(&json).unwrap(), 1)
        .unwrap();
    assert_eq!(next.settings_version, 2);
    assert!(!next.security.external_ai_enabled);
    assert_eq!(
        store.update_config(&serde_json::to_vec(&json).unwrap(), 1),
        Err(ErrorCode::Conflict)
    );
    drop(store);
    let reopened = f.open();
    assert_eq!(reopened.load_config().unwrap(), Some(next.clone()));
    assert_eq!(reopened.initialize_config("UTC").unwrap(), next);
}

#[test]
fn inst07_invalid_and_protected_updates_preserve_previous_configuration() {
    let f = Fixture::new();
    let store = f.open();
    let before = store.initialize_config("UTC").unwrap();
    let mut json = serde_json::to_value(&before).unwrap();
    json["security"]["external_ai_enabled"] = true.into();
    assert_eq!(
        store.update_config(&serde_json::to_vec(&json).unwrap(), 1),
        Err(ErrorCode::PolicyBlocked)
    );
    for input in [
        b"not-json".to_vec(),
        vec![b' '; 65537],
        br#"{"schema_version":999}"#.to_vec(),
    ] {
        assert_eq!(store.update_config(&input, 1), Err(ErrorCode::InvalidInput));
        assert_eq!(store.load_config().unwrap(), Some(before.clone()));
    }
    assert_eq!(store.load_config().unwrap(), Some(before));
}

#[test]
fn inst07_corrupt_persisted_version_is_not_exposed_or_repaired() {
    let f = Fixture::new();
    let store = f.open();
    store.initialize_config("UTC").unwrap();
    drop(store);
    let connection =
        hantong_secrets::KeyedConnection::open(f.0.join("data"), f.0.join("keys")).unwrap();
    connection
        .with_connection(|c| c.execute_batch("UPDATE settings SET version=2"))
        .unwrap();
    drop(connection);
    assert_eq!(f.open().load_config(), Err(ErrorCode::InternalError));
}
