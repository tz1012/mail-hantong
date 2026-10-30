// Included in the existing isolated SQLCipher migration fixture module.
fn seed_v13_fixture(f: &Fixture) {
    seed_v12_fixture(f);
    let store = Store {
        connection: f.keyed(),
    };
    store
        .migrate_with_backup(12, 13, SCHEMA_V13, |c| {
            c.execute_batch(MIGRATION_V12_TO_V13)
        })
        .unwrap();
    use hantong_contracts::{
        uid_page::{UidMailboxEncoding, UidMailboxIdentity},
        uid_reconcile::UidReconcileRequest,
    };
    let request = UidReconcileRequest::new(
        "v5-account",
        UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap(),
        "v13-reconcile",
        "v13-reconcile-key",
        1,
        1,
        &"00".repeat(32),
    )
    .unwrap();
    let intent = hantong_platform::sha256(&request.intent_bytes()).unwrap();
    store.with_connection(|c|{
        let source_hash:Vec<u8>=c.query_row("SELECT page_hash FROM imap_uid_page_operations WHERE operation_id='v11-page'",[],|r|r.get(0))?;
        let result=serde_json::to_string(&serde_json::json!({"schema_version":1,"generation":1,"head_version":1,"frozen_cursor_version":1,"source_operation_id":"v11-page","source_page_hash":hantong_contracts::wire_hex::encode(&source_hash),"source_cursor_version":1,"after_uid":0,"through_uid":1,"uid_next_at_start":2,"observed_count":1,"flags_changed_count":1,"newly_absent_count":0,"remaining_pages":0,"pass_complete":true,"observations":[{"uid":1,"flags":["\\seen"]}]})).unwrap();
        let result_hash=hantong_platform::sha256(result.as_bytes()).unwrap();
        c.execute("INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,client_kind,display_label,tool_code,started_at_utc_ms) VALUES(?1,?2,1,'cli','v5-fixture','mail.sync.page',14)",rusqlite::params![vec![43u8;32],vec![22u8;16]])?;
        c.execute("INSERT INTO audit_start_accounts VALUES(?1,'v5-account')",[vec![43u8;32]])?;
        c.execute("UPDATE audit_capacity SET retained_executions=retained_executions+1,retained_account_refs=retained_account_refs+1 WHERE singleton=1",[])?;
        c.execute("INSERT INTO audit_terminals VALUES(?1,15,'returned_ok',NULL,NULL)",[vec![43u8;32]])?;
        c.execute("INSERT INTO imap_uid_reconcile_heads VALUES('v5-account','494e424f58','modified_utf7',1,1,1,0,'open',9,2,zeroblob(32),14,NULL)",[])?;
        c.execute("INSERT INTO imap_uid_reconcile_requests(execution_id,operation_id,account_id,idempotency_key,mailbox_key,mailbox_encoding,intent_hash,expected_catalog_generation,expected_cursor_version,bound_account_version,bound_endpoint_version,bound_endpoint_hash,uid_validity,minimum_uid_next,generation,head_version,frozen_cursor_version,source_operation_id,source_cursor_version,source_page_hash,after_uid,through_uid,state,admitted_at_utc_ms,started_at_utc_ms,completed_at_utc_ms,result_json,result_hash,error_code) VALUES(?1,'v13-reconcile','v5-account','v13-reconcile-key','494e424f58','modified_utf7',?2,1,1,2,2,zeroblob(32),9,2,1,1,1,'v11-page',1,?3,0,1,'succeeded',14,14,15,?4,?5,NULL)",rusqlite::params![vec![43u8;32],intent.as_slice(),source_hash,result,result_hash.as_slice()])?;
        c.execute("UPDATE imap_uid_reconcile_heads SET version=2,last_source_cursor_version=1,phase='complete',updated_at_utc_ms=15,last_execution_id=?1",[vec![43u8;32]])?;
        c.execute("INSERT INTO imap_uid_reconcile_messages VALUES('v5-account','494e424f58','modified_utf7',9,1,'present',?1,1,15,?2)",rusqlite::params![serde_json::to_string(&vec!["\\seen"]).unwrap(),vec![43u8;32]])?;
        validate_schema(c,SCHEMA_V13,13)
    }).unwrap();
}
struct TableSnapshot {
    name: &'static str,
    columns: Vec<String>,
    rows: Vec<Vec<rusqlite::types::Value>>,
}
fn read_table_columns(
    c: &rusqlite::Connection,
    name: &str,
    columns: &[String],
) -> rusqlite::Result<Vec<Vec<rusqlite::types::Value>>> {
    let mut q = c.prepare(&format!(
        "SELECT {} FROM {name} ORDER BY rowid",
        columns.join(",")
    ))?;
    let n = q.column_count();
    q.query_map([], |r| {
        (0..n)
            .map(|i| r.get(i))
            .collect::<rusqlite::Result<Vec<_>>>()
    })?
    .collect()
}
fn v13_observation_rows(c: &rusqlite::Connection) -> rusqlite::Result<Vec<TableSnapshot>> {
    [
        "imap_uid_reconcile_heads",
        "imap_uid_reconcile_requests",
        "imap_uid_reconcile_messages",
    ]
    .into_iter()
    .map(|name| {
        let mut q = c.prepare(&format!("PRAGMA table_info({name})"))?;
        let columns = q
            .query_map([], |r| r.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let rows = read_table_columns(c, name, &columns)?;
        Ok(TableSnapshot {
            name,
            columns,
            rows,
        })
    })
    .collect()
}
fn assert_v13_observations(
    c: &rusqlite::Connection,
    snapshots: &[TableSnapshot],
) -> rusqlite::Result<()> {
    for t in snapshots {
        assert_eq!(t.rows, read_table_columns(c, t.name, &t.columns)?);
    }
    Ok(())
}
#[test]
fn ext04_v13_to_v14_preserves_observations_pending_jobs_and_initial_epoch() {
    let f = Fixture::new();
    seed_v13_fixture(&f);
    let (before, observations) = f
        .keyed()
        .with_connection(|c| Ok((v12_rows(c)?, v13_observation_rows(c)?)))
        .unwrap();
    let store = f.open().unwrap();
    assert_eq!(before, store.with_connection(v12_rows).unwrap());
    store
        .with_connection(|c| assert_v13_observations(c, &observations))
        .unwrap();
    let epoch=store.with_connection(|c|c.query_row("SELECT uid_validity,epoch_generation,first_cursor_version,first_operation_id,last_cursor_version,recovery_execution_id FROM imap_uid_epochs",[],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<i64>>(4)?,r.get::<_,Option<Vec<u8>>>(5)?)))).unwrap();
    assert_eq!(epoch, (9, 1, 1, "v11-page".into(), None, None));
    assert!(
        store
            .load_audit([42; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    let receipt = store
        .load_uid_reconcile_receipt(
            [22; 16],
            &hantong_contracts::uid_page::UidPageStatusRequest::new("v5-account", [43; 32])
                .unwrap(),
        )
        .unwrap();
    assert!(receipt.result().unwrap().pass_complete());
    assert_eq!(
        store
            .with_connection(|c| c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)))
            .unwrap(),
        i64::from(CURRENT_SCHEMA_VERSION)
    );
    assert!(f.0.join("data/backup-before-v13.db").is_file());
}
#[test]
fn ext04_v13_to_v14_backup_collision_keeps_original_bytes() {
    let f = Fixture::new();
    seed_v13_fixture(&f);
    let db = f.0.join("data/mail-hantong.db");
    let before = fs::read(&db).unwrap();
    let backup = f.0.join("data/backup-before-v13.db");
    fs::write(&backup, b"preserve-v13").unwrap();
    assert!(matches!(f.open(), Err(ErrorCode::Conflict)));
    assert_eq!(before, fs::read(db).unwrap());
    assert_eq!(fs::read(backup).unwrap(), b"preserve-v13");
}
#[test]
fn ext04_v13_to_v14_backfill_and_ddl_failure_roll_back_original() {
    let f = Fixture::new();
    seed_v13_fixture(&f);
    let (before, observations) = f
        .keyed()
        .with_connection(|c| Ok((v12_rows(c)?, v13_observation_rows(c)?)))
        .unwrap();
    let store = Store {
        connection: f.keyed(),
    };
    assert_eq!(
        store.migrate_with_backup(13, 14, SCHEMA_V14, |c| {
            apply_v14_migration(c)?;
            c.execute_batch("CREATE TABLE unexpected_epoch_schema(value)")
        }),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(before, store.with_connection(v12_rows).unwrap());
    store
        .with_connection(|c| {
            assert_v13_observations(c, &observations)?;
            validate_schema(c, SCHEMA_V13, 13)
        })
        .unwrap();
    assert!(f.0.join("data/backup-before-v13.db").is_file());
}
#[test]
fn ext04_v13_to_v14_rejects_inconsistent_old_namespace_without_rewriting_it() {
    let f = Fixture::new();
    seed_v13_fixture(&f);
    f.keyed()
        .with_connection(|c| {
            c.execute(
                "UPDATE imap_uid_page_operations SET uid_validity=10 WHERE operation_id='v11-page'",
                [],
            )
        })
        .unwrap();
    let before = f.keyed().with_connection(v12_rows).unwrap();
    assert!(matches!(f.open(), Err(ErrorCode::InternalError)));
    f.keyed()
        .with_connection(|c| {
            assert_eq!(before, v12_rows(c)?);
            validate_schema(c, SCHEMA_V13, 13)
        })
        .unwrap();
}
