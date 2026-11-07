#[test]
fn acc02_v27_to_v28_preserves_all_legacy_rows_and_grants_nothing() {
    let fixture = Fixture::new();
    let keyed = fixture.keyed();
    keyed.with_connection(|c| {
        c.execute_batch(SCHEMA_V27)?;
        c.execute("INSERT INTO app_metadata VALUES(1,'mail-hantong',27,1,1)", [])?;
        c.execute("INSERT INTO settings VALUES('synthetic.preservation','{\"kept\":true}',7,1)", [])?;
        c.execute("INSERT INTO accounts VALUES('synthetic_imap','imap',NULL,NULL,'synthetic',NULL,1,'ready','[]','[]',1)", [])?;
        c.pragma_update(None, "user_version", 27)?;
        validate_schema(c, SCHEMA_V27, 27)
    }).unwrap();
    let store = Store { connection: keyed };
    fn snapshot(
        c: &rusqlite::Connection,
    ) -> rusqlite::Result<Vec<(String, Vec<Vec<rusqlite::types::Value>>)>> {
        let mut names = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT GLOB 'sqlite_*' AND name<>'app_metadata' ORDER BY name")?;
        let names = names
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        names
            .into_iter()
            .map(|name| {
                let mut stmt = c.prepare(&format!("SELECT * FROM \"{name}\" ORDER BY rowid"))?;
                let count = stmt.column_count();
                let rows = stmt
                    .query_map([], |r| {
                        (0..count)
                            .map(|i| r.get(i))
                            .collect::<rusqlite::Result<Vec<_>>>()
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok((name, rows))
            })
            .collect()
    }
    let before = store.with_connection(snapshot).unwrap();
    store.migrate_v27_to_v28().unwrap();
    store
        .with_connection(|c| {
            let after = snapshot(c)?;
            for entry in before {
                assert_eq!(
                    after.iter().find(|(name, _)| *name == entry.0),
                    Some(&entry)
                );
            }
            for name in [
                "account_scope_v2",
                "account_scope_decisions_v2",
                "account_full_mail_readiness",
            ] {
                assert_eq!(
                    c.query_row(&format!("SELECT count(*) FROM {name}"), [], |r| r
                        .get::<_, i64>(0))?,
                    0
                );
            }
            assert!(
                c.prepare("PRAGMA foreign_key_check")?
                    .query([])?
                    .next()?
                    .is_none()
            );
            validate_schema(c, SCHEMA, CURRENT_SCHEMA_VERSION)
        })
        .unwrap();
}
