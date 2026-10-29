#![cfg(windows)]

use hantong_secrets::{DatabaseError, KeyedConnection};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-cipher-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn open(&self) -> Result<KeyedConnection, DatabaseError> {
        KeyedConnection::open(self.0.join("data"), self.0.join("keys"))
    }
    fn seed(&self) -> KeyedConnection {
        let db = self.open().unwrap();
        db.with_connection(|c| {
            c.execute_batch("CREATE TABLE fixture(value TEXT); PRAGMA user_version=1;")
        })
        .unwrap();
        db.with_connection(|c| {
            c.execute(
                "INSERT INTO fixture VALUES (?1)",
                ["cipher-sentinel-본문-123456789"],
            )
        })
        .unwrap();
        db
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn sec02_encrypted_roundtrip_backup_and_keyless_copy_rejection() {
    let f = Fixture::new();
    let db = f.seed();
    db.backup_before_migration(1).unwrap();
    drop(db);
    let db = f.open().unwrap();
    assert_eq!(
        db.with_connection(
            |c| c.query_row("SELECT value FROM fixture", [], |r| r.get::<_, String>(0))
        )
        .unwrap(),
        "cipher-sentinel-본문-123456789"
    );
    for path in [
        f.0.join("data/mail-hantong.db"),
        f.0.join("data/backup-before-v1.db"),
    ] {
        let bytes = fs::read(&path).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3"));
        assert!(
            !bytes
                .windows("cipher-sentinel-본문-123456789".len())
                .any(|w| w == "cipher-sentinel-본문-123456789".as_bytes())
        );
        let copied_path = f.0.join(format!(
            "copied-{}",
            path.file_name().unwrap().to_string_lossy()
        ));
        fs::copy(&path, &copied_path).unwrap();
        let copy = rusqlite::Connection::open_with_flags(
            copied_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        assert!(
            copy.query_row("SELECT value FROM fixture", [], |r| r.get::<_, String>(0))
                .is_err()
        );
    }
}

#[test]
fn sec02_missing_or_wrong_key_never_recreates_existing_database() {
    let f = Fixture::new();
    drop(f.seed());
    let path = f.0.join("data/mail-hantong.db");
    let before = fs::read(&path).unwrap();
    let key = f.0.join("keys/database-v1.secret");
    let original_key = fs::read(&key).unwrap();
    fs::remove_file(&key).unwrap();
    assert!(matches!(f.open(), Err(DatabaseError::Internal)));
    assert!(!key.exists());
    let other = Fixture::new();
    drop(other.seed());
    fs::copy(other.0.join("keys/database-v1.secret"), &key).unwrap();
    assert!(matches!(f.open(), Err(DatabaseError::Internal)));
    assert_eq!(fs::read(&path).unwrap(), before);
    fs::write(key, original_key).unwrap();
    assert!(f.open().is_ok());
}

#[test]
fn sec02_plaintext_corrupt_and_zero_length_existing_databases_fail_closed() {
    for content in [b"SQLite format 3\0plaintext".as_slice(), b"corrupt", b""] {
        let f = Fixture::new();
        drop(f.seed());
        let path = f.0.join("data/mail-hantong.db");
        fs::write(&path, content).unwrap();
        assert!(matches!(f.open(), Err(DatabaseError::Internal)));
        assert_eq!(fs::read(path).unwrap(), content);
    }
}

#[test]
fn sec02_lifetime_lock_pins_data_key_and_root_and_releases_on_drop() {
    let f = Fixture::new();
    let db = f.seed();
    assert!(matches!(f.open(), Err(DatabaseError::Conflict)));
    assert!(fs::rename(f.0.join("data/mail-hantong.db"), f.0.join("moved.db")).is_err());
    assert!(fs::remove_file(f.0.join("keys/database-v1.secret")).is_err());
    assert!(fs::rename(f.0.join("data"), f.0.join("moved")).is_err());
    drop(db);
    assert!(f.open().is_ok());
}

#[test]
fn sec02_pragma_policy_and_errors_do_not_expose_values() {
    let f = Fixture::new();
    let db = f.seed();
    assert_eq!(
        db.with_connection(
            |c| c.pragma_query_value(None, "cipher_log_level", |r| r.get::<_, String>(0))
        )
        .unwrap(),
        "NONE"
    );
    for (name, expected) in [
        ("foreign_keys", 1),
        ("temp_store", 2),
        ("trusted_schema", 0),
        // M1-08's pre-effect audit commit requires the SQLite rollback journal
        // durability mode, not the rollback-journal default of FULL (2).
        ("synchronous", 3),
    ] {
        assert_eq!(
            db.with_connection(|c| c.pragma_query_value(None, name, |r| r.get::<_, i64>(0)))
                .unwrap(),
            expected
        );
    }
    assert_eq!(
        db.with_connection(
            |c| c.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
        )
        .unwrap(),
        "delete"
    );
    let error = db
        .with_connection(|c| c.execute_batch("SELECT secret_sentinel_value FROM missing_table"))
        .unwrap_err();
    assert_eq!(error, DatabaseError::Internal);
    assert!(!format!("{db:?} {error:?} {error}").contains("sentinel"));
    assert!(!format!("{db:?}").contains(&f.0.to_string_lossy().to_string()));
}

#[test]
fn sec02_backup_conflict_preserves_previous_backup_and_source() {
    let f = Fixture::new();
    let db = f.seed();
    db.backup_before_migration(1).unwrap();
    let backup = f.0.join("data/backup-before-v1.db");
    let bytes = fs::read(&backup).unwrap();
    assert_eq!(db.backup_before_migration(1), Err(DatabaseError::Conflict));
    assert_eq!(fs::read(backup).unwrap(), bytes);
    assert_eq!(
        db.with_connection(
            |c| c.query_row("SELECT count(*) FROM fixture", [], |r| r.get::<_, i64>(0))
        )
        .unwrap(),
        1
    );
}

#[test]
fn sec02_data_and_secret_roots_must_be_separate() {
    let f = Fixture::new();
    assert!(matches!(
        KeyedConnection::open(f.0.join("same"), f.0.join("same")),
        Err(DatabaseError::Internal)
    ));
    assert!(matches!(
        KeyedConnection::open(f.0.join("parent"), f.0.join("parent/child")),
        Err(DatabaseError::Internal)
    ));
}

#[test]
fn sec02_sibling_prefix_roots_are_allowed_and_case_alias_is_rejected() {
    let f = Fixture::new();
    let db = KeyedConnection::open(f.0.join("data"), f.0.join("data2")).unwrap();
    drop(db);
    assert!(matches!(
        KeyedConnection::open(f.0.join("Same"), f.0.join("same")),
        Err(DatabaseError::Internal)
    ));
}

#[test]
fn sec02_existing_sidecars_fail_without_changing_database() {
    for suffix in ["-journal", "-wal", "-shm"] {
        let f = Fixture::new();
        drop(f.seed());
        let db_path = f.0.join("data/mail-hantong.db");
        let before = fs::read(&db_path).unwrap();
        let sidecar = f.0.join(format!("data/mail-hantong.db{suffix}"));
        fs::write(&sidecar, b"uncertain-recovery-data").unwrap();
        assert!(matches!(f.open(), Err(DatabaseError::Internal)));
        assert_eq!(fs::read(db_path).unwrap(), before);
        assert_eq!(fs::read(sidecar).unwrap(), b"uncertain-recovery-data");
    }
}

#[test]
fn sec02_key_committed_before_database_creation_is_reused() {
    let f = Fixture::new();
    drop(f.open().unwrap());
    let key_path = f.0.join("keys/database-v1.secret");
    let key = fs::read(&key_path).unwrap();
    fs::remove_file(f.0.join("data/mail-hantong.db")).unwrap();
    drop(f.seed());
    assert_eq!(fs::read(key_path).unwrap(), key);
}

#[test]
fn sec02_database_leaf_junction_is_rejected_without_target_changes() {
    let f = Fixture::new();
    drop(f.seed());
    let path = f.0.join("data/mail-hantong.db");
    fs::remove_file(&path).unwrap();
    let target = f.0.join("target");
    fs::create_dir(&target).unwrap();
    let result = std::process::Command::new("powershell.exe")
        .args(["-NoProfile","-NonInteractive","-Command","$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:HANTONG_TEST_LINK -Target $env:HANTONG_TEST_TARGET | Out-Null"])
        .env("HANTONG_TEST_LINK",&path).env("HANTONG_TEST_TARGET",&target).output().unwrap();
    assert!(result.status.success(), "junction fixture creation failed");
    assert!(matches!(f.open(), Err(DatabaseError::Internal)));
    assert_eq!(fs::read_dir(&target).unwrap().count(), 0);
    fs::remove_dir(&path).unwrap();
}

#[test]
fn sec02_active_journal_and_memory_temp_table_do_not_expose_sentinel() {
    let f = Fixture::new();
    let db = f.seed();
    db.with_connection(|connection| {
        let transaction = rusqlite::Transaction::new_unchecked(
            connection,
            rusqlite::TransactionBehavior::Immediate,
        )?;
        transaction.execute(
            "UPDATE fixture SET value=?1",
            ["changed-private-sentinel-123456789"],
        )?;
        transaction.execute_batch("CREATE TEMP TABLE temp_fixture(value TEXT)")?;
        transaction.execute(
            "INSERT INTO temp_fixture VALUES (?1)",
            ["temp-private-sentinel-123456789"],
        )?;
        assert!(f.0.join("data/mail-hantong.db-journal").is_file());
        for entry in fs::read_dir(f.0.join("data")).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == "database.lock" {
                continue;
            }
            let bytes = fs::read(entry.path()).unwrap();
            for sentinel in [
                "cipher-sentinel-본문-123456789",
                "changed-private-sentinel-123456789",
                "temp-private-sentinel-123456789",
            ] {
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|w| w == sentinel.as_bytes())
                );
            }
        }
        transaction.rollback()
    })
    .unwrap();
    assert!(!f.0.join("data/mail-hantong.db-journal").exists());
}
