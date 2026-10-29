use super::DatabaseError;
use crate::{SecretError, SecretStore, SensitiveBuffer, windows_impl};
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const KEY_FILE: &str = "database-v1.secret";
const KEY_ENTROPY: &[u8] = b"mail-hantong\0private-database-key\0v1";
const DB_FILE: &str = "mail-hantong.db";

pub(super) struct Database {
    // Rust drops fields in declaration order: SQLite must close before guards.
    pub(super) connection: Connection,
    _database_file: File,
    _key_file: File,
    _key_lock: File,
    _lock: File,
    _root_handles: Vec<File>,
    _secrets: SecretStore,
    root: PathBuf,
    key_path: PathBuf,
}

fn internal<T>(value: Result<T, impl std::fmt::Debug>) -> Result<T, DatabaseError> {
    value.map_err(|_| DatabaseError::Internal)
}

impl Database {
    pub(super) fn open(root: &Path, key_root: &Path) -> Result<Self, DatabaseError> {
        let (root, root_handles) = internal(windows_impl::lock_root(root))?;
        let secrets = internal(SecretStore::open(key_root))?;
        let data_id = internal(windows_impl::file_identity(
            root_handles.last().ok_or(DatabaseError::Internal)?,
        ))?;
        let keys_id = internal(windows_impl::file_identity(
            secrets
                ._root_handles
                .last()
                .ok_or(DatabaseError::Internal)?,
        ))?;
        let data_ancestors = internal(
            root_handles
                .iter()
                .map(windows_impl::file_identity)
                .collect::<Result<Vec<_>, _>>(),
        )?;
        let key_ancestors = internal(
            secrets
                ._root_handles
                .iter()
                .map(windows_impl::file_identity)
                .collect::<Result<Vec<_>, _>>(),
        )?;
        if data_ancestors.contains(&keys_id) || key_ancestors.contains(&data_id) {
            return Err(DatabaseError::Internal);
        }
        let lock = windows_impl::lock_database(&root.join("database.lock")).map_err(|e| {
            if e.raw_os_error() == Some(32) {
                DatabaseError::Conflict
            } else {
                DatabaseError::Internal
            }
        })?;
        let key_lock = windows_impl::lock_database(&secrets.root.join("database-key.lock"))
            .map_err(|e| {
                if e.raw_os_error() == Some(32) {
                    DatabaseError::Conflict
                } else {
                    DatabaseError::Internal
                }
            })?;
        let path = root.join(DB_FILE);
        let exists = match fs::symlink_metadata(&path) {
            Ok(_) => true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err(DatabaseError::Internal),
        };
        // Refuse pre-existing sidecars rather than follow unverified paths or
        // silently discard a journal that may be needed for crash recovery.
        for suffix in ["-journal", "-wal", "-shm"] {
            require_absent(fs::symlink_metadata(
                root.join(format!("{DB_FILE}{suffix}")),
            ))?;
        }
        let key_path = secrets.root.join(KEY_FILE);
        match fs::symlink_metadata(&key_path) {
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !exists => {
                let mut key = SensitiveBuffer(vec![0; 32]);
                internal(windows_impl::random_bytes(&mut key.0))?;
                internal(windows_impl::store_scoped(
                    &key_path,
                    KEY_ENTROPY,
                    key.as_slice(),
                ))?;
            }
            Err(_) => return Err(DatabaseError::Internal),
        }
        let key_file = internal(windows_impl::open_checked(&key_path))?;
        let database_file = internal(windows_impl::pin_database_file(&path, !exists))?;
        if exists && internal(database_file.metadata())?.len() == 0 {
            return Err(DatabaseError::Internal);
        }
        let connection = open_keyed(&path, &key_path, false)?;
        configure_primary_connection(&connection)?;
        Ok(Self {
            connection,
            _database_file: database_file,
            _key_file: key_file,
            _key_lock: key_lock,
            _lock: lock,
            _root_handles: root_handles,
            _secrets: secrets,
            root,
            key_path,
        })
    }

    pub(super) fn backup(&self, current_version: u32) -> Result<(), DatabaseError> {
        let version: u32 = internal(self.connection.pragma_query_value(
            None,
            "user_version",
            |r| r.get(0),
        ))?;
        if current_version == 0 || version != current_version {
            return Err(DatabaseError::Internal);
        }
        let destination = self
            .root
            .join(format!("backup-before-v{current_version}.db"));
        match fs::symlink_metadata(&destination) {
            Ok(_) => return Err(DatabaseError::Conflict),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(DatabaseError::Internal),
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = self.root.join(format!(
            "backup-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        // Create-new proves ownership for cleanup. A foreign collision is never
        // truncated or removed. Root guards prevent ancestor replacement.
        let file = internal(windows_impl::pin_database_file(&temporary, true))?;
        let result = (|| {
            let mut target = open_keyed(&temporary, &self.key_path, false)?;
            {
                let backup = internal(Backup::new(&self.connection, &mut target))?;
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    match internal(backup.step(128))? {
                        StepResult::Done => break,
                        StepResult::More if Instant::now() < deadline => (),
                        StepResult::Busy | StepResult::Locked if Instant::now() < deadline => {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        _ => return Err(DatabaseError::Internal),
                    }
                }
            }
            drop(target);
            let verified = open_keyed(&temporary, &self.key_path, true)?;
            let copied_version: u32 =
                internal(verified.pragma_query_value(None, "user_version", |r| r.get(0)))?;
            if copied_version != current_version {
                return Err(DatabaseError::Internal);
            }
            drop(verified);
            internal(file.sync_all())?;
            // Same-volume link creation publishes atomically without replacing
            // an existing backup. The temporary name is removed after unpinning.
            internal(fs::hard_link(&temporary, &destination))
        })();
        drop(file);
        let _ = fs::remove_file(&temporary);
        result
    }
}

fn configure_primary_connection(connection: &Connection) -> Result<(), DatabaseError> {
    // The primary connection gates audit-start commits before any later effect.
    // EXTRA is required with a rollback/DELETE journal; FULL (the default) is not
    // sufficient to assert the last committed transaction survives power loss.
    internal(connection.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=OFF; \
         PRAGMA journal_mode=DELETE; PRAGMA synchronous=EXTRA;",
    ))?;
    for (name, expected) in [
        ("foreign_keys", 1),
        ("temp_store", 2),
        ("trusted_schema", 0),
        ("synchronous", 3),
    ] {
        let actual: i64 = internal(connection.pragma_query_value(None, name, |r| r.get(0)))?;
        if actual != expected {
            return Err(DatabaseError::Internal);
        }
    }
    let journal: String =
        internal(connection.pragma_query_value(None, "journal_mode", |r| r.get(0)))?;
    if journal != "delete" {
        return Err(DatabaseError::Internal);
    }
    Ok(())
}

fn require_absent(metadata: std::io::Result<fs::Metadata>) -> Result<(), DatabaseError> {
    match metadata {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(DatabaseError::Internal),
    }
}

fn open_keyed(path: &Path, key_path: &Path, read_only: bool) -> Result<Connection, DatabaseError> {
    let mode = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let connection = internal(Connection::open_with_flags(
        path,
        mode | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    ))?;
    internal(windows_impl::with_scoped_secret(
        key_path,
        KEY_ENTROPY,
        |key| {
            if key.len() != 32 {
                return Err(SecretError::InvalidCiphertext);
            }
            let raw = raw_key_literal(key);
            // SAFETY: the live owned connection handle is used synchronously without
            // another SQLite call. The 67-byte raw-key literal lives through key_v2;
            // SQLCipher copies key material into its own connection-owned context.
            let code = unsafe {
                rusqlite::ffi::sqlite3_key_v2(
                    connection.handle(),
                    c"main".as_ptr(),
                    raw.as_slice().as_ptr().cast(),
                    67,
                )
            };
            if code != rusqlite::ffi::SQLITE_OK {
                return Err(SecretError::Protection);
            }
            Ok(())
        },
    ))?;
    // Native codec diagnostics bypass the application's structured redaction
    // boundary. Keep them disabled; callers receive only DatabaseError codes.
    internal(connection.pragma_update(None, "cipher_log_level", "NONE"))?;
    let log_level: String =
        internal(connection.pragma_query_value(None, "cipher_log_level", |r| r.get(0)))?;
    if log_level != "NONE" {
        return Err(DatabaseError::Internal);
    }
    let cipher: String =
        internal(connection.pragma_query_value(None, "cipher_version", |r| r.get(0)))?;
    if cipher.is_empty() {
        return Err(DatabaseError::Internal);
    }
    let _: i64 =
        internal(connection.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0)))?;
    {
        let mut statement = internal(connection.prepare("PRAGMA cipher_integrity_check"))?;
        let mut rows = internal(statement.query([]))?;
        while let Some(row) = internal(rows.next())? {
            let status: String = internal(row.get(0))?;
            if status != "ok" {
                return Err(DatabaseError::Internal);
            }
        }
    }
    Ok(connection)
}

fn raw_key_literal(key: &[u8]) -> SensitiveBuffer {
    let mut result = SensitiveBuffer(vec![0; 67]);
    result.0[0] = b'x';
    result.0[1] = b'\'';
    result.0[66] = b'\'';
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, byte) in key.iter().enumerate() {
        result.0[2 + 2 * i] = HEX[(byte >> 4) as usize];
        result.0[3 + 2 * i] = HEX[(byte & 15) as usize];
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sec02_uncertain_sidecar_metadata_is_rejected() {
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::Other,
            std::io::ErrorKind::InvalidInput,
        ] {
            assert_eq!(
                require_absent(Err(std::io::Error::from(kind))),
                Err(DatabaseError::Internal)
            );
        }
        assert_eq!(
            require_absent(Err(std::io::Error::from(std::io::ErrorKind::NotFound))),
            Ok(())
        );
    }
    #[test]
    fn sec02_raw_key_literal_is_67_bytes_and_uses_no_string_allocation() {
        let mut key = [0u8; 32];
        key[0] = 0xab;
        key[31] = 0xff;
        let literal = raw_key_literal(&key);
        assert_eq!(literal.as_slice().len(), 67);
        assert_eq!(&literal.as_slice()[..4], b"x'ab");
        assert_eq!(&literal.as_slice()[64..], b"ff'");
        let mut bytes = [1u8; 67];
        crate::wipe(&mut bytes);
        assert_eq!(bytes, [0; 67]);
    }
}
