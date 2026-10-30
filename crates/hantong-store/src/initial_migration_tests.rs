// Isolated SQLCipher fixtures only; no actual profile, account, mailbox, or network.

const INITIAL_PREVIEW_TABLES: [&str; 4] = [
    "imap_initial_preview_plans",
    "imap_initial_preview_requests",
    "imap_initial_preview_pages",
    "imap_initial_preview_messages",
];

fn seed_v16_fixture(f: &Fixture) {
    seed_v15_fixture(f);
    Store {
        connection: f.keyed(),
    }
    .migrate_with_backup(15, 16, SCHEMA_V16, apply_v16_migration)
    .unwrap();
}

fn product_table_names(c: &rusqlite::Connection) -> rusqlite::Result<Vec<String>> {
    let mut statement = c.prepare(
        "SELECT name FROM sqlite_master \
         WHERE type='table' AND name NOT GLOB 'sqlite_*' ORDER BY name",
    )?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

fn assert_initial_tables_empty(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    for table in INITIAL_PREVIEW_TABLES {
        let count = c.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get::<_, i64>(0)
        })?;
        assert_eq!(count, 0, "new v17 table must start empty: {table}");
    }
    Ok(())
}

fn assert_frozen_v16_rows(
    c: &rusqlite::Connection,
    expected: &V14TableRows,
) -> rusqlite::Result<()> {
    for (table, expected_rows) in expected {
        let mut columns = c.prepare(&format!("PRAGMA table_info({table})"))?;
        let columns = columns
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        assert_eq!(
            read_table_columns(c, table, &columns)?,
            *expected_rows,
            "v16 rows changed during migration: {table}"
        );
    }
    Ok(())
}

#[test]
fn sync01_v16_to_v17_preserves_all_frozen_rows_sequence_and_message_identity() {
    let f = Fixture::new();
    seed_v16_fixture(&f);
    f.keyed()
        .with_connection(|c| {
            c.execute(
                "UPDATE sqlite_sequence SET seq=1700 WHERE name='audit_starts'",
                [],
            )?;
            assert_eq!(product_table_names(c)?.len(), 31);
            validate_schema(c, SCHEMA_V16, 16)
        })
        .unwrap();

    let before_rows = f.keyed().with_connection(v15_product_rows).unwrap();
    let before_tables = f.keyed().with_connection(product_table_names).unwrap();
    let before_metadata = f
        .keyed()
        .with_connection(|c| {
            c.query_row(
                "SELECT singleton,database_format,created_at_utc_ms FROM app_metadata",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
        })
        .unwrap();
    let before_identity = f
        .keyed()
        .with_connection(|c| {
            c.query_row(
                "SELECT internal_message_id,account_id,mailbox_key,mailbox_encoding,uid_validity,uid \
                 FROM imap_message_identities",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
        })
        .unwrap();

    let store = Store {
        connection: f.keyed(),
    };
    store.migrate_v16_to_v17().unwrap();
    store
        .with_connection(|c| {
            assert_eq!(
                c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?,
                17
            );
            assert_frozen_v16_rows(c, &before_rows)?;
            let after_tables = product_table_names(c)?;
            assert_eq!(after_tables.len(), 35);
            assert!(before_tables.iter().all(|table| after_tables.contains(table)));
            assert_eq!(
                c.query_row(
                    "SELECT singleton,database_format,created_at_utc_ms FROM app_metadata",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    }
                )?,
                before_metadata
            );
            assert_eq!(
                c.query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='audit_starts'",
                    [],
                    |row| row.get::<_, i64>(0)
                )?,
                1700
            );
            let after_identity = c.query_row(
                "SELECT internal_message_id,account_id,mailbox_key,mailbox_encoding,uid_validity,uid \
                 FROM imap_message_identities",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )?;
            assert_eq!(after_identity, before_identity);
            assert_initial_tables_empty(c)?;
            validate_schema(c, SCHEMA_V17, 17)
        })
        .unwrap();
    assert!(f.0.join("data/backup-before-v16.db").is_file());
}

#[test]
fn sync01_fresh_v19_has_exact_table_count_and_empty_initial_preview_ledgers() {
    let f = Fixture::new();
    let store = f.open().unwrap();
    store
        .with_connection(|c| {
            assert_eq!(product_table_names(c)?.len(), 46);
            assert_initial_tables_empty(c)?;
            assert_v18_tables_empty(c)?;
            assert_v19_tables_empty(c)?;
            validate_schema(c, SCHEMA, 19)
        })
        .unwrap();
}

#[test]
fn sync01_v16_to_v17_validation_failure_rolls_back_schema_rows_and_sequence() {
    let f = Fixture::new();
    seed_v16_fixture(&f);
    f.keyed()
        .with_connection(|c| {
            c.execute(
                "UPDATE sqlite_sequence SET seq=1700 WHERE name='audit_starts'",
                [],
            )?;
            validate_schema(c, SCHEMA_V16, 16)
        })
        .unwrap();
    let store = Store {
        connection: f.keyed(),
    };
    let before_rows = store.with_connection(v15_product_rows).unwrap();
    let before_tables = store.with_connection(product_table_names).unwrap();
    let before_metadata = store
        .with_connection(|c| {
            c.query_row(
                "SELECT singleton,database_format,schema_version,created_at_utc_ms,updated_at_utc_ms \
                 FROM app_metadata",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )
        })
        .unwrap();

    assert_eq!(
        store.migrate_with_backup(16, 17, SCHEMA_V17, |c| {
            c.execute_batch(MIGRATION_V16_TO_V17)?;
            c.execute_batch("CREATE TABLE unexpected_initial_preview_schema(value)")
        }),
        Err(ErrorCode::InternalError)
    );

    store
        .with_connection(|c| {
            assert_frozen_v16_rows(c, &before_rows)?;
            assert_eq!(product_table_names(c)?, before_tables);
            assert_eq!(
                c.query_row(
                    "SELECT singleton,database_format,schema_version,created_at_utc_ms,updated_at_utc_ms \
                     FROM app_metadata",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    }
                )?,
                before_metadata
            );
            assert_eq!(
                c.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?,
                16
            );
            assert_eq!(
                c.query_row(
                    "SELECT seq FROM sqlite_sequence WHERE name='audit_starts'",
                    [],
                    |row| row.get::<_, i64>(0)
                )?,
                1700
            );
            for table in INITIAL_PREVIEW_TABLES {
                assert_eq!(
                    c.query_row(
                        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                        [table],
                        |row| row.get::<_, i64>(0)
                    )?,
                    0
                );
            }
            validate_schema(c, SCHEMA_V16, 16)
        })
        .unwrap();
    assert!(f.0.join("data/backup-before-v16.db").is_file());
}

fn frozen_product_table_names(schema: &str) -> rusqlite::Result<Vec<String>> {
    let reference = rusqlite::Connection::open_in_memory()?;
    reference.execute_batch(schema)?;
    product_table_names(&reference)
}
