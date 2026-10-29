//! Public metadata for the CNG server key. This module never sees private key
//! bytes and does not generate keys or grant client access.
use crate::Store;
use hantong_contracts::ErrorCode;
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub enum ServerIdentity {
    Creating {
        key_id: [u8; 32],
        key_epoch: u64,
        version: u64,
    },
    Active {
        key_id: [u8; 32],
        public_key: [u8; 72],
        key_epoch: u64,
        version: u64,
    },
}
impl fmt::Debug for ServerIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ServerIdentity { <redacted> }")
    }
}

impl Store {
    pub fn load_server_identity(&self) -> Result<Option<ServerIdentity>, ErrorCode> {
        #[cfg(windows)]
        {
            self.with_connection(read_identity)
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
    }

    /// Persists the public CNG selector before the coordinator creates a key.
    /// An existing identity is never replaced, including a Creating identity.
    pub fn stage_server_identity(&self, key_id: [u8; 32]) -> Result<ServerIdentity, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = key_id;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            let changed=self.with_connection(|c| {
                let now=crate::utc_millis()?;
                c.execute("INSERT INTO server_identity(singleton,key_id,state,public_key,key_epoch,version,created_at_utc_ms,updated_at_utc_ms) VALUES (1,?1,'creating',NULL,1,1,?2,?2) ON CONFLICT(singleton) DO NOTHING",rusqlite::params![key_id.as_slice(),now])
            })?;
            if changed != 1 {
                return Err(ErrorCode::Conflict);
            }
            Ok(ServerIdentity::Creating {
                key_id,
                key_epoch: 1,
                version: 1,
            })
        }
    }

    /// Records public metadata after the platform coordinator has verified the
    /// CNG key. This is server identity publication, never client registration.
    pub fn activate_server_identity(
        &self,
        key_id: [u8; 32],
        public_key: [u8; 72],
        expected_version: u64,
    ) -> Result<ServerIdentity, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = (key_id, public_key, expected_version);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            if !valid_header(&public_key) {
                return Err(ErrorCode::InvalidInput);
            }
            if expected_version == 0 || expected_version >= i64::MAX as u64 {
                return Err(ErrorCode::Conflict);
            }
            let updated=self.with_connection(|c| {
                let transaction=rusqlite::Transaction::new_unchecked(c,rusqlite::TransactionBehavior::Immediate)?;
                let count=transaction.execute("UPDATE server_identity SET state='active',public_key=?1,version=version+1,updated_at_utc_ms=?2 WHERE singleton=1 AND key_id=?3 AND state='creating' AND version=?4",rusqlite::params![public_key.as_slice(),crate::utc_millis()?,key_id.as_slice(),expected_version as i64])?;
                if count!=1 {return Ok(None);}
                let identity=read_identity(&transaction)?;
                transaction.commit()?;
                Ok(identity)
            })?;
            updated.ok_or(ErrorCode::Conflict)
        }
    }
}

#[cfg(windows)]
fn valid_header(public_key: &[u8; 72]) -> bool {
    &public_key[..4] == b"ECS1" && public_key[4..8] == 32u32.to_le_bytes()
}

#[cfg(windows)]
fn read_identity(c: &rusqlite::Connection) -> rusqlite::Result<Option<ServerIdentity>> {
    use rusqlite::OptionalExtension;
    c.query_row(
        "SELECT key_id,state,public_key,key_epoch,version FROM server_identity WHERE singleton=1",
        [],
        |row| {
            let key_id: [u8; 32] = row
                .get::<_, Vec<u8>>(0)?
                .try_into()
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            let state: String = row.get(1)?;
            let public: Option<Vec<u8>> = row.get(2)?;
            let key_epoch = row.get::<_, i64>(3)?;
            let version = row.get::<_, i64>(4)?;
            if key_epoch <= 0 || version <= 0 {
                return Err(rusqlite::Error::InvalidQuery);
            }
            let key_epoch = key_epoch as u64;
            let version = version as u64;
            match (state.as_str(), public) {
                ("creating", None) => Ok(ServerIdentity::Creating {
                    key_id,
                    key_epoch,
                    version,
                }),
                ("active", Some(blob)) => {
                    let public_key: [u8; 72] =
                        blob.try_into().map_err(|_| rusqlite::Error::InvalidQuery)?;
                    if !valid_header(&public_key) {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                    Ok(ServerIdentity::Active {
                        key_id,
                        public_key,
                        key_epoch,
                        version,
                    })
                }
                _ => Err(rusqlite::Error::InvalidQuery),
            }
        },
    )
    .optional()
}

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "hantong-server-identity-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
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
    // A structural public-blob fixture. CNG point validation belongs to the
    // platform coordinator and is tested with real keys there.
    fn public() -> [u8; 72] {
        let mut v = [1; 72];
        v[..4].copy_from_slice(b"ECS1");
        v[4..8].copy_from_slice(&32u32.to_le_bytes());
        v
    }

    #[test]
    fn sec03_server_identity_is_staged_before_activation_and_survives_reopen() {
        let fixture = Fixture::new();
        let store = fixture.open();
        assert_eq!(store.load_server_identity().unwrap(), None);
        assert_eq!(
            store.stage_server_identity([1; 32]),
            Ok(ServerIdentity::Creating {
                key_id: [1; 32],
                key_epoch: 1,
                version: 1
            })
        );
        assert_eq!(
            store.stage_server_identity([2; 32]),
            Err(ErrorCode::Conflict)
        );
        drop(store);
        let store = fixture.open();
        assert_eq!(
            store.load_server_identity().unwrap(),
            Some(ServerIdentity::Creating {
                key_id: [1; 32],
                key_epoch: 1,
                version: 1
            })
        );
        let active = store
            .activate_server_identity([1; 32], public(), 1)
            .unwrap();
        assert_eq!(
            active,
            ServerIdentity::Active {
                key_id: [1; 32],
                public_key: public(),
                key_epoch: 1,
                version: 2
            }
        );
        assert_eq!(
            store.activate_server_identity([1; 32], public(), 2),
            Err(ErrorCode::Conflict)
        );
        drop(store);
        assert_eq!(fixture.open().load_server_identity().unwrap(), Some(active));
    }
    #[test]
    fn sec03_wrong_identity_version_or_public_header_cannot_replace_key() {
        let fixture = Fixture::new();
        let store = fixture.open();
        let before = store.stage_server_identity([7; 32]).unwrap();
        assert_eq!(
            store.activate_server_identity([8; 32], public(), 1),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            store.activate_server_identity([7; 32], public(), 2),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            store.activate_server_identity([7; 32], [0; 72], 1),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            store.activate_server_identity([7; 32], public(), u64::MAX),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(store.load_server_identity().unwrap(), Some(before));
    }
}
