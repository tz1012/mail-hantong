// Included inside the existing isolated SQLCipher test fixture module.
fn seed_v12_fixture(f: &Fixture) {
    seed_v11_fixture(f);
    let store = Store {
        connection: f.keyed(),
    };
    store
        .migrate_with_backup(11, 12, SCHEMA_V12, |c| {
            c.execute_batch(MIGRATION_V11_TO_V12)
        })
        .unwrap();
    use hantong_contracts::uid_page::{UidMailboxEncoding, UidMailboxIdentity, UidPageRequest};
    let request = UidPageRequest::new(
        "v5-account",
        UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap(),
        "v12-pending",
        "v12-key",
        1,
        1,
        &"00".repeat(32),
    )
    .unwrap();
    let intent = hantong_platform::sha256(&request.intent_bytes()).unwrap();
    store.with_connection(|c| {
        c.execute("INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,client_kind,display_label,tool_code,started_at_utc_ms) VALUES(?1,?2,1,'cli','v5-fixture','mail.sync.page',13)", rusqlite::params![vec![42u8;32],vec![22u8;16]])?;
        c.execute("INSERT INTO audit_start_accounts VALUES(?1,'v5-account')", [vec![42u8;32]])?;
        c.execute("UPDATE audit_capacity SET retained_executions=retained_executions+1,retained_account_refs=retained_account_refs+1 WHERE singleton=1", [])?;
        c.execute("INSERT INTO imap_uid_page_requests(execution_id,operation_id,account_id,idempotency_key,mailbox_key,mailbox_encoding,intent_hash,expected_catalog_generation,expected_cursor_version,bound_account_version,bound_endpoint_version,bound_endpoint_hash,expected_uid_validity,after_uid,state,admitted_at_utc_ms) VALUES(?1,'v12-pending','v5-account','v12-key','494e424f58','modified_utf7',?2,1,1,2,2,zeroblob(32),9,1,'admitted',13)", rusqlite::params![vec![42u8;32],intent.as_slice()])?;
        validate_schema(c, SCHEMA_V12, 12)
    }).unwrap();
}
fn v12_rows(c: &rusqlite::Connection) -> rusqlite::Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
    let mut rows = v11_rows(c)?;
    let mut q = c.prepare("SELECT * FROM imap_uid_page_requests ORDER BY rowid")?;
    let columns = q.column_count();
    rows.push(
        q.query_map([], |r| {
            (0..columns)
                .map(|i| r.get(i))
                .collect::<rusqlite::Result<Vec<_>>>()
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?,
    );
    Ok(rows)
}
#[test]
fn ext04_v12_to_v13_preserves_base_pending_jobs_audits_and_scope_history() {
    let f = Fixture::new();
    seed_v12_fixture(&f);
    let before = f.keyed().with_connection(v12_rows).unwrap();
    let store = f.open().unwrap();
    assert_eq!(before, store.with_connection(v12_rows).unwrap());
    assert_eq!(
        store
            .with_connection(|c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)))
            .unwrap(),
        13
    );
    for table in [
        "imap_uid_reconcile_heads",
        "imap_uid_reconcile_requests",
        "imap_uid_reconcile_messages",
    ] {
        assert_eq!(
            store
                .with_connection(|c| c.query_row(
                    &format!("SELECT count(*) FROM {table}"),
                    [],
                    |r| r.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
    }
    assert!(
        store
            .load_audit([42; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    assert!(f.0.join("data/backup-before-v12.db").is_file());
}
#[test]
fn ext04_v12_to_v13_backup_collision_preserves_original_bytes() {
    let f = Fixture::new();
    seed_v12_fixture(&f);
    let db = f.0.join("data/mail-hantong.db");
    let before = fs::read(&db).unwrap();
    let backup = f.0.join("data/backup-before-v12.db");
    fs::write(&backup, b"preserve-v12").unwrap();
    assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
    assert_eq!(before, fs::read(db).unwrap());
    assert_eq!(fs::read(backup).unwrap(), b"preserve-v12");
}
#[test]
fn ext04_v12_to_v13_real_ddl_failure_rolls_back_all_rows_and_schema() {
    let f = Fixture::new();
    seed_v12_fixture(&f);
    let before = f.keyed().with_connection(v12_rows).unwrap();
    let store = Store {
        connection: f.keyed(),
    };
    assert_eq!(
        store.migrate_with_backup(12, 13, SCHEMA, |c| {
            c.execute_batch(MIGRATION_V12_TO_V13)?;
            c.execute_batch("CREATE TABLE unexpected_reconcile_schema(value)")
        }),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(before, store.with_connection(v12_rows).unwrap());
    store
        .with_connection(|c| validate_schema(c, SCHEMA_V12, 12))
        .unwrap();
    assert!(f.0.join("data/backup-before-v12.db").is_file());
}
