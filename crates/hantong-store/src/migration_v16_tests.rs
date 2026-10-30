// Isolated SQLCipher fixtures only; no actual client, account or network.
fn seed_v15_fixture(f: &Fixture) {
    seed_v14_fixture(f);
    Store { connection: f.keyed() }
        .migrate_with_backup(14, 15,
            include_str!("../../../docs/contracts/migrations/schema-v15.sql"),
            apply_v15_migration)
        .unwrap();
}

fn v15_product_rows(c: &rusqlite::Connection) -> rusqlite::Result<V14TableRows> {
    let mut result = v14_product_rows(c)?;
    result.push(("imap_message_identities".into(), read_table_columns(c,
        "imap_message_identities", &["internal_message_id", "account_id", "mailbox_key",
        "mailbox_encoding", "uid_validity", "uid"].map(String::from))?));
    Ok(result)
}

#[test]
fn cli16_ops05_v15_to_v16_preserves_every_row_identity_and_audit_highwater() {
    let f = Fixture::new();
    seed_v15_fixture(&f);
    f.keyed().with_connection(|c| c.execute("UPDATE sqlite_sequence SET seq=1500 WHERE name='audit_starts'", [])).unwrap();
    let before = f.keyed().with_connection(v15_product_rows).unwrap();
    let store = Store { connection: f.keyed() };
    store.migrate_v15_to_v16().unwrap();
    store.with_connection(|c| {
        assert_eq!(c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?, 16);
        assert_eq!(before, v15_product_rows(c)?);
        assert_eq!(c.query_row("SELECT seq FROM sqlite_sequence WHERE name='audit_starts'", [], |r| r.get::<_,i64>(0))?, 1500);
        assert_eq!(c.query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT GLOB 'sqlite_*'", [], |r| r.get::<_,i64>(0))?, 31);
        validate_schema(c, SCHEMA_V16, 16)
    }).unwrap();
    assert!(f.0.join("data/backup-before-v15.db").is_file());
}

#[test]
fn cli16_ops05_v15_to_v16_preserves_absent_audit_sequence_and_empty_store() {
    let f = Fixture::new();
    f.keyed().with_connection(|c| {
        c.execute_batch(include_str!("../../../docs/contracts/migrations/schema-v15.sql"))?;
        c.execute("INSERT INTO app_metadata VALUES(1,'mail-hantong',15,1,1)", [])?;
        c.pragma_update(None,"user_version",15)
    }).unwrap();
    let store = Store { connection: f.keyed() };
    store.migrate_v15_to_v16().unwrap();
    store.with_connection(|c| {
        assert_eq!(c.pragma_query_value(None,"user_version",|r|r.get::<_,i64>(0))?,16);
        assert_eq!(c.query_row("SELECT count(*) FROM sqlite_sequence WHERE name='audit_starts'",[],|r|r.get::<_,i64>(0))?,0);
        assert_eq!(c.query_row("SELECT count(*) FROM audit_starts",[],|r|r.get::<_,i64>(0))?,0);
        validate_schema(c,SCHEMA_V16,16)
    }).unwrap();
}

#[test]
fn cli16_ops05_v16_failure_and_backup_collision_preserve_v15() {
    let f=Fixture::new();seed_v15_fixture(&f);
    let store=Store { connection:f.keyed() };
    let before=store.with_connection(v15_product_rows).unwrap();
    assert_eq!(store.migrate_with_backup(15,16,SCHEMA_V16,|c| {
        apply_v16_migration(c)?;
        c.execute_batch("CREATE TABLE unexpected_app_schema(value)")
    }),Err(ErrorCode::InternalError));
    store.with_connection(|c| { assert_eq!(before,v15_product_rows(c)?);validate_schema(c,SCHEMA_V15,15) }).unwrap();
    drop(store);
    let original=fs::read(f.0.join("data/mail-hantong.db")).unwrap();
    let backup=fs::read(f.0.join("data/backup-before-v15.db")).unwrap();
    assert!(matches!(f.open(),Err(ErrorCode::Conflict)));
    assert_eq!(fs::read(f.0.join("data/mail-hantong.db")).unwrap(),original);
    assert_eq!(fs::read(f.0.join("data/backup-before-v15.db")).unwrap(),backup);
}
