// Isolated SQLCipher fixtures; no actual profile, account, or network.
fn seed_v14_fixture(f: &Fixture) {
    seed_v13_fixture(f);
    Store { connection: f.keyed() }
        .migrate_with_backup(
            13,
            14,
            include_str!("../../../docs/contracts/migrations/schema-v14.sql"),
            apply_v14_migration,
        )
        .unwrap();
}

#[test]
fn mail03_v14_to_v15_preserves_all_existing_rows_and_backfills_identity() {
    let f = Fixture::new();
    seed_v14_fixture(&f);
    f.keyed().with_connection(|c| c.execute("UPDATE sqlite_sequence SET seq=1000 WHERE name='audit_starts'", [])).unwrap();
    let before = f.keyed().with_connection(v14_product_rows).unwrap();
    let store = f.open().unwrap();
    store.with_connection(|c| {
        assert_eq!(c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?, i64::from(CURRENT_SCHEMA_VERSION));
        assert_eq!(before, v14_product_rows(c)?);
        let identity: (String, String, String, String, i64, i64) = c.query_row(
            "SELECT internal_message_id,account_id,mailbox_key,mailbox_encoding,uid_validity,uid FROM imap_message_identities", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
        assert_eq!((&identity.1, &identity.2, &identity.3, identity.4, identity.5),
                   (&"v5-account".to_string(), &"494e424f58".to_string(), &"modified_utf7".to_string(), 9, 1));
        assert_eq!(identity.0, "msg_c171f70eef26402c3b49dbc25510a950640d91ba8ca006beb9a9e929a306c6c1");
        assert_eq!(c.query_row("SELECT seq FROM sqlite_sequence WHERE name='audit_starts'", [], |r| r.get::<_,i64>(0))?, 1000);
        Ok(())
    }).unwrap();
    assert!(f.0.join("data/backup-before-v14.db").is_file());
}

#[test]
fn mail03_v14_to_v15_preserves_absent_audit_sequence_and_empty_cache() {
    let f = Fixture::new();
    f.keyed().with_connection(|c| {
        c.execute_batch(include_str!("../../../docs/contracts/migrations/schema-v14.sql"))?;
        c.execute("INSERT INTO app_metadata VALUES(1,'mail-hantong',14,1,1)", [])?;
        c.pragma_update(None, "user_version", 14)?;
        validate_schema(c, SCHEMA_V14, 14)
    }).unwrap();
    let store = f.open().unwrap();
    store.with_connection(|c| {
        assert_eq!(c.query_row("SELECT count(*) FROM sqlite_sequence WHERE name='audit_starts'", [], |r| r.get::<_,i64>(0))?, 0);
        assert_eq!(c.query_row("SELECT count(*) FROM imap_message_identities", [], |r| r.get::<_,i64>(0))?, 0);
        validate_schema(c, SCHEMA, CURRENT_SCHEMA_VERSION)
    }).unwrap();
}

#[test]
fn mail03_v14_to_v15_backup_collision_keeps_original_file_bytes() {
    let f = Fixture::new();
    seed_v14_fixture(&f);
    let database = f.0.join("data/mail-hantong.db");
    let backup = f.0.join("data/backup-before-v14.db");
    let before = fs::read(&database).unwrap();
    fs::write(&backup, b"existing-v14-backup").unwrap();
    assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
    assert_eq!(fs::read(database).unwrap(), before);
    assert_eq!(fs::read(backup).unwrap(), b"existing-v14-backup");
}

#[test]
fn mail03_v14_to_v15_backfill_failure_rolls_back_every_rebuilt_table() {
    for fault in [
        "CREATE TRIGGER identity_fault BEFORE INSERT ON imap_message_identities BEGIN SELECT RAISE(IGNORE); END",
        "CREATE TRIGGER identity_fault BEFORE INSERT ON imap_message_identities BEGIN SELECT RAISE(ABORT,'fixture'); END",
        "CREATE TRIGGER identity_fault AFTER INSERT ON imap_message_identities BEGIN UPDATE imap_message_identities SET internal_message_id='msg_'||printf('%064d',0) WHERE internal_message_id=NEW.internal_message_id; END",
    ] {
        let f = Fixture::new();
        seed_v14_fixture(&f);
        let store = Store { connection: f.keyed() };
        let before = store.with_connection(v14_product_rows).unwrap();
        assert_eq!(store.migrate_with_backup(14, 15, SCHEMA_V15, |c| {
            c.execute_batch(MIGRATION_V14_TO_V15)?;
            c.execute_batch(fault)?;
            // Check the insertion failure itself, independently of final DDL validation.
            let result = crate::message_identities::backfill_v14(c);
            assert!(result.is_err(), "backfill must reject the injected write corruption");
            result
        }), Err(ErrorCode::InternalError));
        store.with_connection(|c| {
            assert_eq!(before, v14_product_rows(c)?);
            validate_schema(c, SCHEMA_V14, 14)
        }).unwrap();
        assert!(f.0.join("data/backup-before-v14.db").is_file());
    }
}

#[test]
fn mail03_v14_to_v15_post_backfill_schema_failure_preserves_source() {
    let f = Fixture::new();
    seed_v14_fixture(&f);
    let store = Store { connection: f.keyed() };
    let before = store.with_connection(v14_product_rows).unwrap();
    assert_eq!(store.migrate_with_backup(14, 15, SCHEMA_V15, |c| {
        apply_v15_migration(c)?;
        c.execute_batch("CREATE TABLE unexpected_metadata_schema(value)")
    }), Err(ErrorCode::InternalError));
    store.with_connection(|c| {
        assert_eq!(before, v14_product_rows(c)?);
        validate_schema(c, SCHEMA_V14, 14)
    }).unwrap();
}

type V14TableRows = Vec<(String, Vec<Vec<rusqlite::types::Value>>)>;

#[test]
fn mail03_v14_to_v15_preserves_recovery_cycles_and_retired_epoch_observations() {
    use crate::uid_reconcile_jobs::tests::{Fixture as UidFixture, append, identity, metadata, reconcile};
    use hantong_contracts::{uid_page::{UidPageStatusRequest, UidPageState}, uid_recovery::UidRecoveryRequest};
    use crate::uid_page_jobs::UidQueueAdmission;
    use crate::uid_recovery_jobs::{UidRecoveryAdmission, UidRecoveryStart};
    use hantong_domain::{imap_recovery::ImapUidRecoveryPage, imap_sync::{ImapUidPage, ImapUidPageRequest}};
    let source = UidFixture::new();
    append(&source, 0, 0, 11, &[1,3], "migration-source");
    reconcile(&source, 21, 1, &[(1, &["\\Flagged"])]);
    let request = |id: &str, version| {
        let raw = UidRecoveryRequest::new("acct", identity(b"INBOX"), id, id, 1, version, &"0".repeat(64)).unwrap();
        let digest = hantong_platform::sha256(&raw.intent_bytes()).unwrap();
        UidRecoveryRequest::new("acct", identity(b"INBOX"), id, id, 1, version, &hantong_contracts::wire_hex::encode(&digest)).unwrap()
    };
    source.begin_for(31,9);
    assert!(matches!(source.store().admit_uid_recovery([31;32], &request("migration-recovery",1), UidQueueAdmission::Reserved).unwrap(), UidRecoveryAdmission::Admitted(_)));
    let UidRecoveryStart::Ready(job) = source.store().start_uid_recovery([31;32]).unwrap() else { panic!("recovery must start") };
    let fetch = job.fetch_request();
    let page = ImapUidPage::new(ImapUidPageRequest::new(fetch.account_id(),fetch.endpoint_version(),fetch.mailbox().clone(),Some(9),0).unwrap(),9,11,vec![metadata(1,&["\\Seen"])]).unwrap();
    let completed = source.store().complete_uid_recovery_success([31;32], &ImapUidRecoveryPage::new(fetch.clone(),page).unwrap()).unwrap();
    reconcile(&source,22,2,&[(1,&["\\Seen"])]);
    source.begin_for(32,9);
    source.store().admit_uid_recovery([32;32], &request("migration-pending",2), UidQueueAdmission::Reserved).unwrap();
    let before = source.store().with_connection(v14_product_rows).unwrap();

    // Project only existing v14 columns into a fresh encrypted v14 fixture.
    // No v15 ID or scope-v4 row is relabelled as historical data.
    let f = Fixture::new();
    f.keyed().with_connection(|c| {
        let tx = c.unchecked_transaction()?;
        tx.execute_batch(SCHEMA_V14)?;
        tx.execute_batch("PRAGMA defer_foreign_keys=ON")?;
        tx.execute("INSERT INTO app_metadata VALUES(1,'mail-hantong',14,1,1)",[])?;
        tx.pragma_update(None,"user_version",14)?;
        for (table, rows) in &before {
            tx.execute(&format!("DELETE FROM {table}"),[])?;
            if let Some(first) = rows.first() {
                let placeholders = std::iter::repeat_n("?",first.len()).collect::<Vec<_>>().join(",");
                let mut insert = tx.prepare(&format!("INSERT INTO {table} VALUES({placeholders})"))?;
                for row in rows { assert_eq!(insert.execute(rusqlite::params_from_iter(row))?,1); }
            }
        }
        validate_schema(&tx,SCHEMA_V14,14)?;
        tx.commit()
    }).unwrap();
    let migrated = f.open().unwrap();
    migrated.with_connection(|c| {
        assert_eq!(before,v14_product_rows(c)?);
        assert_eq!(c.query_row("SELECT count(*) FROM imap_message_identities",[],|r|r.get::<_,i64>(0))?,3);
        assert_eq!(c.query_row("SELECT count(*) FROM imap_uid_epochs",[],|r|r.get::<_,i64>(0))?,2);
        validate_schema(c,SCHEMA,CURRENT_SCHEMA_VERSION)
    }).unwrap();
    assert_eq!(migrated.load_uid_recovery_receipt([9;16], &UidPageStatusRequest::new("acct",[31;32]).unwrap()).unwrap(), completed);
    assert_eq!(migrated.load_uid_recovery_receipt([9;16], &UidPageStatusRequest::new("acct",[32;32]).unwrap()).unwrap().state(),UidPageState::Unresolved);
    assert!(migrated.load_audit([32;32]).unwrap().unwrap().completion().is_none());
}

fn v14_product_rows(c: &rusqlite::Connection) -> rusqlite::Result<V14TableRows> {
    let mut statement = c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT GLOB 'sqlite_*' AND name NOT IN ('app_metadata','imap_message_identities') ORDER BY name")?;
    let names = statement.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    names.into_iter().map(|name| {
        let mut columns = c.prepare(&format!("PRAGMA table_info({name})"))?;
        let columns = columns.query_map([], |r| r.get::<_,String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let rows = read_table_columns(c, &name, &columns)?;
        Ok((name, rows))
    }).collect()
}
