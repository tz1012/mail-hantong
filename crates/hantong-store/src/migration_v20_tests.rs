fn seed_v19_fixture(f: &Fixture) {
    seed_v18_fixture(f);
    Store { connection: f.keyed() }.migrate_v18_to_v19().unwrap();
}
fn v19_product_rows(c: &rusqlite::Connection) -> rusqlite::Result<V14TableRows> {
    frozen_product_table_names(SCHEMA_V19)?.into_iter().filter(|n| n != "app_metadata").map(|name| {
        let mut q=c.prepare(&format!("PRAGMA table_info({name})"))?;
        let columns=q.query_map([],|r|r.get::<_,String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((name.clone(),read_table_columns(c,&name,&columns)?))
    }).collect()
}
#[test]
fn gmail_write_v20_migration_preserves_every_v19_table_and_grants() {
    let f=Fixture::new(); seed_v19_fixture(&f);
    let before=f.keyed().with_connection(v19_product_rows).unwrap();
    f.open().unwrap().with_connection(|c| {
        assert_eq!(before,v19_product_rows(c)?);
        assert_eq!(c.query_row("SELECT count(*) FROM mail_draft_attachments",[],|r|r.get::<_,i64>(0))?,0);
        validate_schema(c,SCHEMA,CURRENT_SCHEMA_VERSION)
    }).unwrap();
    assert!(f.0.join("data/backup-before-v19.db").is_file());
}
#[test]
fn gmail_write_v20_backup_collision_preserves_database_and_existing_backup() {
    let f=Fixture::new(); seed_v19_fixture(&f);
    let path=f.0.join("data/mail-hantong.db"); let before=fs::read(&path).unwrap();
    let backup=f.0.join("data/backup-before-v19.db");fs::write(&backup,b"existing synthetic backup").unwrap();
    assert!(matches!(f.open(),Err(ErrorCode::Conflict)));
    assert_eq!(fs::read(path).unwrap(),before);
    assert_eq!(fs::read(backup).unwrap(),b"existing synthetic backup");
}
#[test]
fn gmail_write_v20_validation_failure_rolls_back_without_partial_upgrade() {
    let f=Fixture::new();seed_v19_fixture(&f);
    let store=Store {connection:f.keyed()};
    let before=store.with_connection(v19_product_rows).unwrap();
    assert_eq!(store.migrate_with_backup(19,20,SCHEMA_V20,|c| {
        c.execute_batch(MIGRATION_V19_TO_V20)?;
        c.execute_batch("CREATE TABLE unexpected_write_schema(value)")
    }),Err(ErrorCode::InternalError));
    store.with_connection(|c| {assert_eq!(before,v19_product_rows(c)?);validate_schema(c,SCHEMA_V19,19)}).unwrap();
}
