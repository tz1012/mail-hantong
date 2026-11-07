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
            validate_schema(c, SCHEMA_V26, 26)
        })
        .unwrap();
}

#[test]
fn acc02_v26_to_v27_preserves_every_table_row_and_adds_no_event_authority() {
    let fixture = Fixture::new();
    seed_v21_fixture(&fixture);
    let store = Store { connection: fixture.keyed() };
    store.migrate_v21_to_v22().unwrap();
    store.migrate_v22_to_v23().unwrap();
    store.migrate_v23_to_v24().unwrap();
    store.migrate_v24_to_v25().unwrap();
    store.migrate_v25_to_v26().unwrap();
    let before = store.with_connection(|c| {
        let mut stmt = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
        let names = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        names.into_iter().map(|name| {
            let count: i64 = c.query_row(&format!("SELECT count(*) FROM \"{name}\""), [], |r| r.get(0))?;
            Ok((name, count))
        }).collect::<rusqlite::Result<Vec<_>>>()
    }).unwrap();
    store.migrate_v26_to_v27().unwrap();
    store.with_connection(|c| {
        for (name, count) in &before {
            assert_eq!(*count, c.query_row(&format!("SELECT count(*) FROM \"{name}\""), [], |r| r.get::<_, i64>(0))?, "{name}");
        }
        validate_schema(c, SCHEMA_V27, 27)?;
        assert!(c.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_none());
        for name in ["mail_event_log", "mail_event_tracking", "mail_event_sources", "mail_event_cursors"] {
            assert_eq!(c.query_row(&format!("SELECT count(*) FROM {name}"), [], |r| r.get::<_, i64>(0))?, 0);
        }
        assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?, 27);
        Ok(())
    }).unwrap();
}
