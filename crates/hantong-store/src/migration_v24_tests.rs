#[test]
fn acc02_v24_to_v25_preserves_rows_foreign_keys_and_sequence() {
    let fixture = Fixture::new();
    seed_v21_fixture(&fixture);
    let store = Store { connection: fixture.keyed() };
    store.migrate_v21_to_v22().unwrap();
    store.migrate_v22_to_v23().unwrap();
    store.migrate_v23_to_v24().unwrap();
    let before = store.with_connection(|c| {
        Ok::<_, rusqlite::Error>((
            c.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))?,
            c.query_row("SELECT count(*) FROM client_registration_requests", [], |r| r.get::<_, i64>(0))?,
            c.query_row("SELECT count(*) FROM client_registrations", [], |r| r.get::<_, i64>(0))?,
            c.query_row("SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='audit_starts'),-1)", [], |r| r.get::<_, i64>(0))?,
        ))
    }).unwrap();
    store.migrate_v24_to_v25().unwrap();
    store.with_connection(|c| {
        assert_eq!(before.0, c.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))?);
        assert_eq!(before.1, c.query_row("SELECT count(*) FROM client_registration_requests", [], |r| r.get::<_, i64>(0))?);
        assert_eq!(before.2, c.query_row("SELECT count(*) FROM client_registrations", [], |r| r.get::<_, i64>(0))?);
        assert_eq!(before.3, c.query_row("SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='audit_starts'),-1)", [], |r| r.get::<_, i64>(0))?);
        assert!(c.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_none());
        validate_schema(c, SCHEMA_V25, 25)
    }).unwrap();
}
