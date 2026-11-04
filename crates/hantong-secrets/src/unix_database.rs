//! SQLCipher boundary for native Unix stores. Keys never leave Secrets.
#![allow(unsafe_code)]

use super::database::DatabaseError;
use crate::SensitiveBuffer;
use crate::unix_secret_store::{
    ProtectedBytes, UnixSecretError, UnixSecretKind, UnixSecretSelector, UnixSecretStore,
};
use hantong_platform::unix_paths::UnixDirectoryGuard;
use ring::rand::SecureRandom;
use rusqlite::{Connection, OpenFlags};
use std::fs::File;
use std::os::fd::AsRawFd;
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const DATABASE: &str = "mail-hantong.db";

pub(super) struct Database {
    pub(super) connection: Connection,
    key: ProtectedBytes,
    _file: File,
    _database_lock: File,
    _key_lock: File,
    data: UnixDirectoryGuard,
    _secrets: UnixDirectoryGuard,
}

fn internal<T>(value: Result<T, impl std::fmt::Debug>) -> Result<T, DatabaseError> {
    value.map_err(|_| DatabaseError::Internal)
}

trait KeySource {
    fn read(&mut self) -> Result<ProtectedBytes, UnixSecretError>;
    fn create(&mut self, bytes: &[u8]) -> Result<(), UnixSecretError>;
}

impl Database {
    pub(super) fn open(data: &Path, secrets: &Path) -> Result<Self, DatabaseError> {
        // The production key-store adapter is created only after root validation.
        let roots = prepare_roots(data, secrets)?;
        internal(UnixSecretStore::install_platform_store())?;
        let store = UnixSecretStore::new();
        let selector = internal(UnixSecretSelector::new(
            UnixSecretKind::DatabaseKey,
            None,
            1,
        ))?;
        let mut source = OsKeys {
            store,
            selector,
            root: &roots.1,
        };
        Self::open_prepared(&roots.0, &mut source).map(|opened| Self {
            connection: opened.connection,
            key: opened.key,
            _file: opened.file,
            _database_lock: opened.database_lock,
            _key_lock: opened.key_lock,
            data: roots.0,
            _secrets: roots.1,
        })
    }

    fn open_prepared(
        data: &UnixDirectoryGuard,
        keys: &mut impl KeySource,
    ) -> Result<Opened, DatabaseError> {
        let database_lock = internal(data.open_or_create_regular("database.lock"))?;
        database_lock
            .try_lock()
            .map_err(|_| DatabaseError::Conflict)?;
        let key_lock = internal(data.open_or_create_regular("database-key.lock"))?;
        key_lock.try_lock().map_err(|_| DatabaseError::Conflict)?;
        let existing = match data.open_existing_regular(DATABASE) {
            Ok(file) => {
                if internal(file.metadata())?.len() == 0 {
                    return Err(DatabaseError::Internal);
                }
                Some(file)
            }
            Err(hantong_contracts::ErrorCode::NotFound) => None,
            Err(_) => return Err(DatabaseError::Internal),
        };
        let key = match keys.read() {
            Ok(key) => key,
            Err(UnixSecretError::NotFound) if existing.is_none() => {
                let mut generated = SensitiveBuffer(vec![0; 32]);
                internal(ring::rand::SystemRandom::new().fill(&mut generated.0))?;
                internal(keys.create(generated.as_slice()))?;
                // Re-read to validate OS item persistence and its exact kind/size.
                internal(keys.read())?
            }
            Err(_) => return Err(DatabaseError::Internal),
        };
        if key.as_bytes_in_secrets().len() != 32 {
            return Err(DatabaseError::Internal);
        }
        let file = match existing {
            Some(file) => file,
            None => internal(data.create_new_regular(DATABASE))?,
        };
        let descriptor = descriptor_path(data, DATABASE)?;
        ensure_same_file(&file, &descriptor)?;
        let connection = open_keyed(&descriptor, &key, false)?;
        ensure_same_file(&file, &descriptor)?;
        configure(&connection)?;
        Ok(Opened {
            connection,
            key,
            file,
            database_lock,
            key_lock,
        })
    }

    pub(super) fn backup(&self, version: u32) -> Result<(), DatabaseError> {
        let actual: u32 = internal(self.connection.pragma_query_value(
            None,
            "user_version",
            |row| row.get(0),
        ))?;
        if version == 0 || version != actual {
            return Err(DatabaseError::Internal);
        }
        let name = format!("backup-before-v{version}.db");
        match self.data.open_existing_regular(&name) {
            Ok(_) => return Err(DatabaseError::Conflict),
            Err(hantong_contracts::ErrorCode::NotFound) => (),
            Err(_) => return Err(DatabaseError::Internal),
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let temporary = format!(
            "backup-{0}-{1}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let file = internal(self.data.create_new_regular(&temporary))?;
        let result = (|| {
            let temporary_path = descriptor_path(&self.data, &temporary)?;
            ensure_same_file(&file, &temporary_path)?;
            let mut target = open_keyed(&temporary_path, &self.key, false)?;
            ensure_same_file(&file, &temporary_path)?;
            {
                let backup =
                    internal(rusqlite::backup::Backup::new(&self.connection, &mut target))?;
                let deadline = Instant::now() + Duration::from_secs(10);
                loop {
                    match internal(backup.step(128))? {
                        rusqlite::backup::StepResult::Done => break,
                        rusqlite::backup::StepResult::More if Instant::now() < deadline => (),
                        rusqlite::backup::StepResult::Busy
                        | rusqlite::backup::StepResult::Locked
                            if Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        _ => return Err(DatabaseError::Internal),
                    }
                }
            }
            drop(target);
            let verified_path = descriptor_path(&self.data, &temporary)?;
            let verified = open_keyed(&verified_path, &self.key, true)?;
            ensure_same_file(&file, &verified_path)?;
            verify_backup(&verified, version)?;
            drop(verified);
            internal(file.sync_all())?;
            internal(std::fs::hard_link(
                descriptor_path(&self.data, &temporary)?,
                descriptor_path(&self.data, &name)?,
            ))
        })();
        drop(file);
        let _ = std::fs::remove_file(descriptor_path(&self.data, &temporary)?);
        result
    }
}

fn verify_backup(connection: &Connection, version: u32) -> Result<(), DatabaseError> {
    let cipher: String =
        internal(connection.pragma_query_value(None, "cipher_version", |row| row.get(0)))?;
    if cipher.is_empty() {
        return Err(DatabaseError::Internal);
    }
    let copied: u32 =
        internal(connection.pragma_query_value(None, "user_version", |row| row.get(0)))?;
    if copied != version {
        return Err(DatabaseError::Internal);
    }
    let integrity: String =
        internal(connection.query_row("PRAGMA integrity_check", [], |row| row.get(0)))?;
    if integrity != "ok" {
        return Err(DatabaseError::Internal);
    }
    let mut foreign = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(|_| DatabaseError::Internal)?;
    let mut rows = internal(foreign.query([]))?;
    if internal(rows.next())?.is_some() {
        return Err(DatabaseError::Internal);
    }
    Ok(())
}

struct OsKeys<'a> {
    store: UnixSecretStore,
    selector: UnixSecretSelector,
    root: &'a UnixDirectoryGuard,
}
impl KeySource for OsKeys<'_> {
    fn read(&mut self) -> Result<ProtectedBytes, UnixSecretError> {
        self.store.read(&self.selector)
    }
    fn create(&mut self, bytes: &[u8]) -> Result<(), UnixSecretError> {
        let guard = crate::unix_secret_store::UnixSecretMutationGuard::lock(self.root)?;
        self.store
            .create(&guard, &self.selector, bytes, UnixSecretKind::DatabaseKey)
    }
}

struct Opened {
    connection: Connection,
    key: ProtectedBytes,
    file: File,
    database_lock: File,
    key_lock: File,
}

fn prepare_roots(
    data: &Path,
    secrets: &Path,
) -> Result<(UnixDirectoryGuard, UnixDirectoryGuard), DatabaseError> {
    if data.starts_with(secrets) || secrets.starts_with(data) {
        return Err(DatabaseError::Internal);
    }
    Ok((
        internal(UnixDirectoryGuard::prepare(data))?,
        internal(UnixDirectoryGuard::prepare(secrets))?,
    ))
}

fn descriptor_path(root: &UnixDirectoryGuard, name: &str) -> Result<PathBuf, DatabaseError> {
    let physical = physical_root(root)?;
    Ok(physical.join(name))
}

fn physical_root(root: &UnixDirectoryGuard) -> Result<PathBuf, DatabaseError> {
    let fd = internal(root.directory_fd())?.as_raw_fd();
    #[cfg(target_os = "linux")]
    let path = internal(std::fs::read_link(format!("/proc/self/fd/{fd}")))?;
    #[cfg(target_os = "macos")]
    let path = {
        let mut bytes = [0_i8; libc::PATH_MAX as usize];
        if unsafe { libc::fcntl(fd, libc::F_GETPATH, bytes.as_mut_ptr()) } != 0 {
            return Err(DatabaseError::Internal);
        }
        let c = unsafe { std::ffi::CStr::from_ptr(bytes.as_ptr()) };
        PathBuf::from(std::ffi::OsStr::from_bytes(c.to_bytes()))
    };
    if !path.is_absolute()
        || path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
    {
        return Err(DatabaseError::Conflict);
    }
    let fresh = internal(UnixDirectoryGuard::open(&path, true))?;
    if directory_identity(root)? != directory_identity(&fresh)? {
        return Err(DatabaseError::Conflict);
    }
    Ok(path)
}

fn directory_identity(root: &UnixDirectoryGuard) -> Result<(u64, u64), DatabaseError> {
    let fd = internal(root.directory_fd())?.as_raw_fd();
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return Err(DatabaseError::Internal);
    }
    let stat = unsafe { stat.assume_init() };
    #[cfg(target_os = "linux")]
    let device = stat.st_dev;
    #[cfg(target_os = "macos")]
    let device = stat.st_dev as u64;
    let inode = stat.st_ino;
    Ok((device, inode))
}

fn open_keyed(
    path: &Path,
    key: &ProtectedBytes,
    read_only: bool,
) -> Result<Connection, DatabaseError> {
    let mode = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let connection = internal(Connection::open_with_flags(
        path,
        mode | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    ))?;
    let bytes = key.as_bytes_in_secrets();
    if bytes.len() != 32 {
        return Err(DatabaseError::Internal);
    }
    let mut raw = SensitiveBuffer(vec![0; 67]);
    raw.0[0..2].copy_from_slice(b"x'");
    raw.0[66] = b'\'';
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, byte) in bytes.iter().enumerate() {
        raw.0[2 + 2 * i] = HEX[(byte >> 4) as usize];
        raw.0[3 + 2 * i] = HEX[(byte & 15) as usize];
    }
    // SAFETY: connection and bounded raw key remain alive throughout the synchronous call.
    if unsafe {
        rusqlite::ffi::sqlite3_key_v2(
            connection.handle(),
            c"main".as_ptr(),
            raw.as_slice().as_ptr().cast(),
            67,
        )
    } != rusqlite::ffi::SQLITE_OK
    {
        return Err(DatabaseError::Internal);
    }
    internal(connection.pragma_update(None, "cipher_log_level", "NONE"))?;
    let cipher: String =
        internal(connection.pragma_query_value(None, "cipher_version", |row| row.get(0)))?;
    if cipher.is_empty() {
        return Err(DatabaseError::Internal);
    }
    let _: i64 =
        internal(connection.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0)))?;
    Ok(connection)
}

/// The directory FD protects the parent walk, while SQLite opens the leaf by
/// descriptor path. Compare identities before configuration or backup writes;
/// this is defense in depth and does not claim to defeat a same-UID ABA race.
fn ensure_same_file(file: &File, path: &Path) -> Result<(), DatabaseError> {
    let expected = internal(file.metadata())?;
    let actual = internal(std::fs::symlink_metadata(path))?;
    if expected.dev() != actual.dev()
        || expected.ino() != actual.ino()
        || expected.nlink() != actual.nlink()
    {
        return Err(DatabaseError::Conflict);
    }
    Ok(())
}

fn configure(connection: &Connection) -> Result<(), DatabaseError> {
    internal(connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=OFF; PRAGMA journal_mode=DELETE; PRAGMA synchronous=EXTRA;"))?;
    for (name, expected) in [
        ("foreign_keys", 1),
        ("temp_store", 2),
        ("trusted_schema", 0),
        ("synchronous", 3),
    ] {
        let actual: i64 = internal(connection.pragma_query_value(None, name, |row| row.get(0)))?;
        if actual != expected {
            return Err(DatabaseError::Internal);
        }
    }
    let journal_mode: String =
        internal(connection.pragma_query_value(None, "journal_mode", |row| row.get(0)))?;
    if !journal_mode.eq_ignore_ascii_case("delete") {
        return Err(DatabaseError::Internal);
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FakeKey {
        bytes: Option<Vec<u8>>,
        creates: usize,
    }

    impl FakeKey {
        fn new(bytes: Option<Vec<u8>>) -> Self {
            Self { bytes, creates: 0 }
        }
    }

    impl KeySource for FakeKey {
        fn read(&mut self) -> Result<ProtectedBytes, UnixSecretError> {
            self.bytes
                .clone()
                .ok_or(UnixSecretError::NotFound)
                .and_then(ProtectedBytes::from_backend)
        }
        fn create(&mut self, bytes: &[u8]) -> Result<(), UnixSecretError> {
            self.creates += 1;
            self.bytes = Some(bytes.to_vec());
            Ok(())
        }
    }

    struct Fixture {
        root: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "hantong-unix-db-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }
        fn roots(&self) -> (PathBuf, PathBuf) {
            (self.root.join("data"), self.root.join("secrets"))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn open(
        fixture: &Fixture,
        key: Option<Vec<u8>>,
    ) -> (Opened, FakeKey, UnixDirectoryGuard, UnixDirectoryGuard) {
        let (data, secrets) = fixture.roots();
        let data_guard = UnixDirectoryGuard::prepare(&data).unwrap();
        let secrets_guard = UnixDirectoryGuard::prepare(&secrets).unwrap();
        let mut source = FakeKey::new(key);
        let opened = Database::open_prepared(&data_guard, &mut source).unwrap();
        (opened, source, data_guard, secrets_guard)
    }

    #[test]
    fn sec03_unix_database_encrypts_known_plaintext_and_reopens_with_same_key() {
        let fixture = Fixture::new();
        let (opened, mut source, data, secrets) = open(&fixture, Some(vec![7; 32]));
        opened.connection.execute_batch("CREATE TABLE secret_value (value TEXT); INSERT INTO secret_value VALUES ('known-plaintext-marker');").unwrap();
        let path = descriptor_path(&data, DATABASE).unwrap();
        let raw = std::fs::read(&path).unwrap();
        assert!(
            !raw.windows(22)
                .any(|part| part == b"known-plaintext-marker")
        );
        drop(opened);
        drop(data);
        drop(secrets);
        let (data, secrets) = fixture.roots();
        let dg = UnixDirectoryGuard::prepare(&data).unwrap();
        let _sg = UnixDirectoryGuard::prepare(&secrets).unwrap();
        let reopened = Database::open_prepared(&dg, &mut source).unwrap();
        let value: String = reopened
            .connection
            .query_row("SELECT value FROM secret_value", [], |r| r.get(0))
            .unwrap();
        assert_eq!(value, "known-plaintext-marker");
    }

    #[test]
    fn sec03_existing_database_never_creates_missing_or_wrong_key() {
        let fixture = Fixture::new();
        let (opened, _, data, secrets) = open(&fixture, Some(vec![8; 32]));
        opened
            .connection
            .execute_batch(
                "CREATE TABLE protected_value (value TEXT); INSERT INTO protected_value VALUES ('stable');",
            )
            .unwrap();
        drop(opened);
        drop(data);
        drop(secrets);
        let (data_path, secrets_path) = fixture.roots();
        let database_path = data_path.join(DATABASE);
        let before = std::fs::read(&database_path).unwrap();
        assert!(!before.is_empty());
        let dg = UnixDirectoryGuard::prepare(&data_path).unwrap();
        let sg = UnixDirectoryGuard::prepare(&secrets_path).unwrap();
        let mut missing = FakeKey::new(None);
        assert!(Database::open_prepared(&dg, &mut missing).is_err());
        assert_eq!(missing.creates, 0);
        assert_eq!(std::fs::read(&database_path).unwrap(), before);
        let mut wrong = FakeKey::new(Some(vec![9; 32]));
        assert!(Database::open_prepared(&dg, &mut wrong).is_err());
        assert_eq!(wrong.creates, 0);
        assert_eq!(std::fs::read(&database_path).unwrap(), before);
        drop(sg);
    }

    #[test]
    fn sec03_database_lock_contention_is_conflict() {
        let fixture = Fixture::new();
        let (opened, _, data, secrets) = open(&fixture, Some(vec![10; 32]));
        let (data_path, secrets_path) = fixture.roots();
        let dg = UnixDirectoryGuard::prepare(&data_path).unwrap();
        let _sg = UnixDirectoryGuard::prepare(&secrets_path).unwrap();
        let mut source = FakeKey::new(Some(vec![10; 32]));
        assert!(matches!(
            Database::open_prepared(&dg, &mut source),
            Err(DatabaseError::Conflict)
        ));
        drop(opened);
        drop(data);
        drop(secrets);
    }

    #[test]
    fn sec03_backup_verifies_cipher_integrity_and_preserves_existing_backup() {
        let fixture = Fixture::new();
        let (opened, _, data, secrets) = open(&fixture, Some(vec![11; 32]));
        opened.connection.execute_batch("PRAGMA user_version=7; CREATE TABLE parent (id INTEGER PRIMARY KEY); CREATE TABLE child (parent_id INTEGER REFERENCES parent(id)); INSERT INTO parent VALUES (1); INSERT INTO child VALUES (1);").unwrap();
        let db = Database {
            connection: opened.connection,
            key: opened.key,
            _file: opened.file,
            _database_lock: opened.database_lock,
            _key_lock: opened.key_lock,
            data,
            _secrets: secrets,
        };
        db.backup(7).unwrap();
        let backup = descriptor_path(&db.data, "backup-before-v7.db").unwrap();
        let checked = open_keyed(&backup, &db.key, true).unwrap();
        verify_backup(&checked, 7).unwrap();
        assert_eq!(db.backup(7), Err(DatabaseError::Conflict));
    }
}
