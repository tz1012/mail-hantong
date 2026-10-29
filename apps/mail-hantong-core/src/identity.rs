use hantong_contracts::ErrorCode;
use hantong_platform::{ServerSigningKey, random_nonce, validate_public_key};
use hantong_store::{ServerIdentity, Store};

/// Server identity coordinator. The database row is staged before the CNG key
/// is created so a crash cannot silently replace an existing server identity.
pub(crate) fn initialize_server_identity(store: &Store) -> Result<ServerSigningKey, ErrorCode> {
    match store.load_server_identity()? {
        None => {
            let key_id = random_nonce()?;
            store.stage_server_identity(key_id)?;
            let key = ServerSigningKey::create(key_id)?;
            let public = key.public_key()?;
            validate_public_key(&public)?;
            store.activate_server_identity(key_id, public, 1)?;
            Ok(key)
        }
        Some(ServerIdentity::Creating {
            key_id, version, ..
        }) => {
            let key = match ServerSigningKey::open(key_id) {
                Ok(key) => key,
                Err(ErrorCode::NotFound) => ServerSigningKey::create(key_id)?,
                Err(code) => return Err(code),
            };
            let public = key.public_key()?;
            validate_public_key(&public)?;
            store.activate_server_identity(key_id, public, version)?;
            Ok(key)
        }
        Some(ServerIdentity::Active {
            key_id, public_key, ..
        }) => {
            let key = ServerSigningKey::open(key_id).map_err(|_| ErrorCode::InternalError)?;
            let current = key.public_key().map_err(|_| ErrorCode::InternalError)?;
            if current != public_key {
                return Err(ErrorCode::InternalError);
            }
            Ok(key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct Fixture {
        root: PathBuf,
        keys: Vec<[u8; 32]>,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "hantong-core-identity-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            Self {
                root,
                keys: Vec::new(),
            }
        }
        fn store(&self) -> Store {
            Store::open(self.root.join("data"), self.root.join("keys")).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            for id in self.keys.drain(..) {
                let _ = ServerSigningKey::open(id).and_then(|k| k.delete());
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn inst01_server_identity_stages_and_reopens_same_key() {
        let mut fixture = Fixture::new();
        let store = fixture.store();
        let first = initialize_server_identity(&store).unwrap();
        let id = first.id();
        fixture.keys.push(id);
        let public = first.public_key().unwrap();
        drop(first);
        drop(store);
        let store = fixture.store();
        let second = initialize_server_identity(&store).unwrap();
        assert_eq!(second.public_key().unwrap(), public);
    }

    #[test]
    fn inst01_missing_active_key_fails_closed_without_replacement() {
        let mut fixture = Fixture::new();
        let store = fixture.store();
        let first = initialize_server_identity(&store).unwrap();
        let id = first.id();
        let db = fixture.root.join("data/mail-hantong.db");
        let before = fs::read(&db).unwrap();
        first.delete().unwrap();
        fixture.keys.clear();
        drop(store);
        let store = fixture.store();
        assert!(matches!(
            initialize_server_identity(&store),
            Err(ErrorCode::InternalError)
        ));
        assert_eq!(fs::read(db).unwrap(), before);
        assert!(ServerSigningKey::open(id).is_err());
    }

    #[test]
    fn inst01_creating_row_recovers_with_same_key_id() {
        let mut fixture = Fixture::new();
        let store = fixture.store();
        let id = random_nonce().unwrap();
        store.stage_server_identity(id).unwrap();
        let key = ServerSigningKey::create(id).unwrap();
        fixture.keys.push(id);
        let expected = key.public_key().unwrap();
        assert_eq!(
            store.activate_server_identity(id, expected, 2),
            Err(ErrorCode::Conflict)
        );
        drop(key);
        let runtime = initialize_server_identity(&store).unwrap();
        assert_eq!(runtime.public_key().unwrap(), expected);
        assert!(matches!(
            store.load_server_identity().unwrap(),
            Some(ServerIdentity::Active { .. })
        ));
    }

    #[test]
    fn inst01_creating_row_without_key_recovers_only_its_staged_id() {
        let mut fixture = Fixture::new();
        let store = fixture.store();
        let id = random_nonce().unwrap();
        fixture.keys.push(id);
        store.stage_server_identity(id).unwrap();
        assert!(matches!(
            ServerSigningKey::open(id),
            Err(ErrorCode::NotFound)
        ));
        let runtime = initialize_server_identity(&store).unwrap();
        assert_eq!(runtime.id(), id);
        assert!(
            matches!(store.load_server_identity().unwrap(),Some(ServerIdentity::Active{key_id,..}) if key_id==id)
        );
    }

    #[test]
    fn sec03_active_public_key_mismatch_preserves_database_and_original_key() {
        let mut fixture = Fixture::new();
        let store = fixture.store();
        let id = random_nonce().unwrap();
        fixture.keys.push(id);
        store.stage_server_identity(id).unwrap();
        let real = ServerSigningKey::create(id).unwrap();
        let original = real.public_key().unwrap();
        let other_id = random_nonce().unwrap();
        fixture.keys.push(other_id);
        let other = ServerSigningKey::create(other_id).unwrap();
        store
            .activate_server_identity(id, other.public_key().unwrap(), 1)
            .unwrap();
        let db = fixture.root.join("data/mail-hantong.db");
        let before = fs::read(&db).unwrap();
        assert!(matches!(
            initialize_server_identity(&store),
            Err(ErrorCode::InternalError)
        ));
        assert_eq!(fs::read(db).unwrap(), before);
        assert_eq!(
            ServerSigningKey::open(id).unwrap().public_key().unwrap(),
            original
        );
    }
}
