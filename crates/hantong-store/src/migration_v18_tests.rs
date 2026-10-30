// Synthetic encrypted databases only; no real profiles or network.
const V18_NEW_TABLES: [&str; 6] = [
    "imap_body_cache", "mail_drafts", "mail_send_reviews", "mail_outbox", "mcp_ai_consents", "mcp_registration_replacements",
];

fn assert_v18_tables_empty(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    for table in V18_NEW_TABLES {
        assert_eq!(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get::<_, i64>(0))?, 0,
            "new v18 table must not acquire data or consent during migration: {table}");
    }
    Ok(())
}

fn seed_v17_fixture(f: &Fixture) {
    seed_v16_fixture(f);
    Store { connection: f.keyed() }.migrate_v16_to_v17().unwrap();
    f.keyed().with_connection(|c| c.execute("UPDATE sqlite_sequence SET seq=1800 WHERE name='audit_starts'", [])).unwrap();
}

fn v17_product_rows(c: &rusqlite::Connection) -> rusqlite::Result<V14TableRows> {
    frozen_product_table_names(SCHEMA_V17)?.into_iter().filter(|name| name != "app_metadata")
        .map(|name| {
            let mut columns=c.prepare(&format!("PRAGMA table_info({name})"))?;
            let columns=columns.query_map([], |r| r.get::<_,String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((name.clone(), read_table_columns(c,&name,&columns)?))
        }).collect()
}

#[test]
fn ai07_sync07_v17_to_v18_preserves_all_rows_highwater_and_grants_no_consent() {
    let f=Fixture::new(); seed_v17_fixture(&f);
    let before=f.keyed().with_connection(v17_product_rows).unwrap();
    let store=Store { connection:f.keyed() }; store.migrate_v17_to_v18().unwrap();
    store.with_connection(|c| {
        assert_eq!(before,v17_product_rows(c)?);
        assert_eq!(product_table_names(c)?.len(),41);
        assert_eq!(c.query_row("SELECT seq FROM sqlite_sequence WHERE name='audit_starts'",[],|r|r.get::<_,i64>(0))?,1800);
        assert_v18_tables_empty(c)?;
        validate_schema(c,SCHEMA_V18,18)
    }).unwrap();
    assert!(f.0.join("data/backup-before-v17.db").is_file());
}

#[test]
fn sync07_v18_validation_failure_rolls_back_all_schema_rows_and_highwater() {
    let f=Fixture::new(); seed_v17_fixture(&f);
    let store=Store { connection:f.keyed() };
    let before=store.with_connection(v17_product_rows).unwrap();
    assert_eq!(store.migrate_with_backup(17,18,SCHEMA_V18,|c| {
        c.execute_batch(MIGRATION_V17_TO_V18)?;
        c.execute_batch("CREATE TABLE unexpected_v18_schema(value)")
    }),Err(ErrorCode::InternalError));
    store.with_connection(|c| {
        assert_eq!(before,v17_product_rows(c)?);
        assert_eq!(c.query_row("SELECT seq FROM sqlite_sequence WHERE name='audit_starts'",[],|r|r.get::<_,i64>(0))?,1800);
        validate_schema(c,SCHEMA_V17,17)
    }).unwrap();
}

#[test]
fn sync07_v18_backup_collision_preserves_original_database_and_backup() {
    let f=Fixture::new(); seed_v17_fixture(&f);
    let database=f.0.join("data/mail-hantong.db");
    let backup=f.0.join("data/backup-before-v17.db");
    let before=fs::read(&database).unwrap();
    fs::write(&backup,b"existing-v17-backup").unwrap();
    assert!(matches!(f.open(),Err(ErrorCode::Conflict)));
    assert_eq!(fs::read(database).unwrap(),before);
    assert_eq!(fs::read(backup).unwrap(),b"existing-v17-backup");
}
