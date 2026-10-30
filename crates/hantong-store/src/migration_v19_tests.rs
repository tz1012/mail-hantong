// Synthetic encrypted fixtures only. Freeze all 41 v18 tables independently of
// the current schema so additive tables cannot alter historical projections.
const V19_NEW_TABLES: [&str; 5] = [
    "google_connections",
    "google_labels",
    "google_messages",
    "google_sync_state",
    "google_sync_requests",
];
fn assert_v19_tables_empty(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    for table in V19_NEW_TABLES {
        assert_eq!(
            c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))?,
            0,
            "migration must not create Google identities, jobs, or data: {table}"
        );
    }
    Ok(())
}
fn seed_v18_fixture(f: &Fixture) {
    seed_v17_fixture(f);
    Store {
        connection: f.keyed(),
    }
    .migrate_v17_to_v18()
    .unwrap();
    f.keyed()
        .with_connection(|c| {
            c.execute(
                "UPDATE sqlite_sequence SET seq=1900 WHERE name='audit_starts'",
                [],
            )
        })
        .unwrap();
}
fn v18_product_rows(c: &rusqlite::Connection) -> rusqlite::Result<V14TableRows> {
    frozen_product_table_names(SCHEMA_V18)?
        .into_iter()
        .filter(|name| name != "app_metadata")
        .map(|name| {
            let mut columns = c.prepare(&format!("PRAGMA table_info({name})"))?;
            let columns = columns
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((name.clone(), read_table_columns(c, &name, &columns)?))
        })
        .collect()
}
#[test]
fn acc01_sync02_v18_to_v19_preserves_every_frozen_table_highwater_and_no_consent() {
    let f = Fixture::new();
    seed_v18_fixture(&f);
    assert_eq!(
        f.keyed()
            .with_connection(product_table_names)
            .unwrap()
            .len(),
        41
    );
    let before = f.keyed().with_connection(v18_product_rows).unwrap();
    let store = f.open().unwrap();
    store
        .with_connection(|c| {
            assert_eq!(before, v18_product_rows(c)?);
            assert_eq!(product_table_names(c)?.len(), 46);
            assert_eq!(
                c.query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='audit_starts'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                1900
            );
            assert_v19_tables_empty(c)?;
            assert_v18_tables_empty(c)?;
            validate_schema(c, SCHEMA, 19)
        })
        .unwrap();
    assert!(f.0.join("data/backup-before-v18.db").is_file());
}
#[test]
fn acc01_v19_backup_collision_preserves_both_files() {
    let f = Fixture::new();
    seed_v18_fixture(&f);
    let path = f.0.join("data/mail-hantong.db");
    let before = fs::read(&path).unwrap();
    let backup = f.0.join("data/backup-before-v18.db");
    fs::write(&backup, b"existing-v18").unwrap();
    assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
    assert_eq!(before, fs::read(path).unwrap());
    assert_eq!(fs::read(backup).unwrap(), b"existing-v18");
}
#[test]
fn acc01_v19_validation_failure_rolls_back_ddl_rows_sequence_and_version() {
    let f = Fixture::new();
    seed_v18_fixture(&f);
    let store = Store {
        connection: f.keyed(),
    };
    let before = store.with_connection(v18_product_rows).unwrap();
    assert_eq!(
        store.migrate_with_backup(18, 19, SCHEMA, |c| {
            c.execute_batch(MIGRATION_V18_TO_V19)?;
            c.execute_batch("CREATE TABLE unexpected_google_schema(value)")
        }),
        Err(ErrorCode::InternalError)
    );
    store
        .with_connection(|c| {
            assert_eq!(before, v18_product_rows(c)?);
            assert_eq!(
                c.query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='audit_starts'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                1900
            );
            validate_schema(c, SCHEMA_V18, 18)
        })
        .unwrap();
}
