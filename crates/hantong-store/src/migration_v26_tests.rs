#[test]
fn acc02_v25_to_v26_preserves_rows_foreign_keys_and_creates_trash_ledger() {
    let fixture = Fixture::new();
    seed_v21_fixture(&fixture);
    let store = Store { connection: fixture.keyed() };
    store.migrate_v21_to_v22().unwrap();
    store.migrate_v22_to_v23().unwrap();
    store.migrate_v23_to_v24().unwrap();
    store.migrate_v24_to_v25().unwrap();

    let before = store
        .with_connection(|c| {
            Ok::<_, rusqlite::Error>((
                c.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))?,
                c.query_row(
                    "SELECT count(*) FROM client_registrations",
                    [],
                    |r| r.get::<_, i64>(0),
                )?,
                c.query_row(
                    "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='audit_starts'),-1)",
                    [],
                    |r| r.get::<_, i64>(0),
                )?,
            ))
        })
        .unwrap();

    store.migrate_v25_to_v26().unwrap();
    store
        .with_connection(|c| {
            assert_eq!(
                before.0,
                c.query_row("SELECT count(*) FROM accounts", [], |r| r.get::<_, i64>(0))?
            );
            assert_eq!(
                before.1,
                c.query_row("SELECT count(*) FROM client_registrations", [], |r| {
                    r.get::<_, i64>(0)
                })?
            );
            assert_eq!(
                before.2,
                c.query_row(
                    "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='audit_starts'),-1)",
                    [],
                    |r| r.get::<_, i64>(0),
                )?
            );
            assert!(c
                .prepare("PRAGMA foreign_key_check")?
                .query([])?
                .next()?
                .is_none());
            assert_eq!(
                c.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('imap_trash_reviews','imap_trash_outbox')",
                    [],
                    |r| r.get::<_, i64>(0),
                )?,
                2
            );
            validate_schema(c, SCHEMA, CURRENT_SCHEMA_VERSION)
        })
        .unwrap();
}
