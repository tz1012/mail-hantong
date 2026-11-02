fn seed_v21_fixture(f: &Fixture) {
    seed_v20_fixture(f);
    Store { connection: f.keyed() }.migrate_v20_to_v21().unwrap();
}

fn migration_google_public_key() -> [u8; 72] {
    let mut key = [0; 72];
    key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
    key[8..40].copy_from_slice(
        &hantong_contracts::wire_hex::decode_fixed::<32>(
            "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        )
        .unwrap(),
    );
    key[40..].copy_from_slice(
        &hantong_contracts::wire_hex::decode_fixed::<32>(
            "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        )
        .unwrap(),
    );
    key
}

fn seed_v22_google_body(f: &Fixture) -> ([u8; 32], String) {
    seed_v21_fixture(f);
    let store = Store { connection: f.keyed() };
    store.migrate_v21_to_v22().unwrap();
    let public_key = migration_google_public_key();
    let server_hash = hantong_platform::sha256(&public_key).unwrap();
    let account = format!("google_{}", "1a".repeat(32));
    store.with_connection(|c| {
        c.execute("UPDATE server_identity SET key_id=?1,state='active',public_key=?2,key_epoch=1,version=version+1,updated_at_utc_ms=9999999999999 WHERE singleton=1", rusqlite::params![[9_u8; 32].as_slice(), public_key.as_slice()])?;
        c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES(?1,'gmail',9001,'ready','[]','[]',1)", [&account])?;
        Ok(())
    }).unwrap();
    let request = store.create_registration_request(
        [1; 32],
        ClientKind::Cli,
        DEFAULT_CLI_LABEL,
        &hantong_contracts::auth::RequestedScope::new(
            vec![account.clone()],
            hantong_contracts::auth::default_cli_legacy_tools(),
        ).unwrap(),
    ).unwrap();
    let registration_id = hantong_platform::registration_id_for_request(request.request_id).unwrap();
    let stage = CliRegistrationStage::new_google_oauth(request, registration_id, public_key, server_hash).unwrap();
    let record = store.stage_google_oauth_cli_registration(&stage).unwrap();
    store.activate_cli_registration(registration_id, record.version, server_hash).unwrap();
    let body = hantong_contracts::auth::RequestedScope::new(
        vec![account.clone()], hantong_contracts::auth::default_cli_body_tools(),
    ).unwrap();
    let refresh = store.create_google_oauth_cli_scope_refresh([2; 32], body, server_hash).unwrap();
    let staged = store.stage_cli_scope_refresh(&refresh).unwrap();
    store.activate_cli_scope_refresh(&staged).unwrap();
    (server_hash, account)
}
fn v21_product_rows(c: &rusqlite::Connection) -> rusqlite::Result<V14TableRows> {
    let template = rusqlite::Connection::open_in_memory()?;
    template.execute_batch(SCHEMA_V21)?;
    frozen_product_table_names(SCHEMA_V21)?.into_iter()
        .filter(|name| name != "app_metadata")
        .map(|name| {
            let mut query = template.prepare(&format!("PRAGMA table_info({name})"))?;
            let columns = query.query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((name.clone(), read_table_columns(c, &name, &columns)?))
        }).collect()
}
#[test]
fn acc02_v22_migration_preserves_all_legacy_rows_hashes_foreign_keys_and_sequence() {
    let fixture = Fixture::new();
    seed_v21_fixture(&fixture);
    let before = fixture.keyed().with_connection(v21_product_rows).unwrap();
    let sequence = fixture.keyed().with_connection(|c| c.query_row("SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='audit_starts'),-1)", [], |r| r.get::<_,i64>(0))).unwrap();
    let store = Store { connection: fixture.keyed() };
    store.migrate_v21_to_v22().unwrap();
    store.with_connection(|c| {
        assert_eq!(before, v21_product_rows(c)?);
        assert_eq!(sequence, c.query_row("SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='audit_starts'),-1)", [], |r| r.get::<_,i64>(0))?);
        assert_eq!(c.query_row("SELECT count(*) FROM client_scope_refreshes WHERE decision_channel<>'approval_helper'", [], |r| r.get::<_, i64>(0))?, 0);
        assert_eq!(c.query_row("SELECT count(*) FROM client_registrations WHERE approved_channel='google_oauth'", [], |r| r.get::<_, i64>(0))?, 0);
        assert!(c.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_none());
        validate_schema(c, SCHEMA_V22, 22)
    }).unwrap();
    assert!(fixture.0.join("data/backup-before-v21.db").is_file());
}
#[test]
fn acc02_v22_migration_collision_and_validation_failure_preserve_v21() {
    let fixture = Fixture::new();
    seed_v21_fixture(&fixture);
    let database = fixture.0.join("data/mail-hantong.db");
    let before = fs::read(&database).unwrap();
    let backup = fixture.0.join("data/backup-before-v21.db");
    fs::write(&backup, b"synthetic reserved backup").unwrap();
    assert!(matches!(fixture.open(), Err(ErrorCode::Conflict)));
    assert_eq!(fs::read(database).unwrap(), before);
    assert_eq!(fs::read(backup).unwrap(), b"synthetic reserved backup");
    let other = Fixture::new();
    seed_v21_fixture(&other);
    let store = Store { connection: other.keyed() };
    let before = store.with_connection(v21_product_rows).unwrap();
    assert_eq!(store.migrate_with_backup(21,22,SCHEMA,|c| {
        c.execute_batch(MIGRATION_V21_TO_V22)?;
        c.execute_batch("CREATE TABLE unexpected_google_schema(value)")
    }), Err(ErrorCode::InternalError));
    store.with_connection(|c| {
        assert_eq!(before, v21_product_rows(c)?);
        validate_schema(c, SCHEMA_V21, 21)
    }).unwrap();
}

#[test]
fn acc02_v23_migration_preserves_active_google_v8_and_allows_v9_upgrade() {
    let fixture = Fixture::new();
    let (server_hash, account) = seed_v22_google_body(&fixture);
    let store = Store { connection: fixture.keyed() };
    store.migrate_v22_to_v23().unwrap();
    let body = store.load_cli_scope_refresh([2; 32]).unwrap().unwrap();
    assert_eq!(body.decision_channel(), "google_oauth");
    assert_eq!(body.target_profile_epoch(), 2);
    let send = hantong_contracts::auth::RequestedScope::new(
        vec![account], hantong_contracts::auth::default_cli_send_tools(),
    ).unwrap();
    let upgrade = store.create_google_oauth_cli_send_upgrade([4; 32], send, server_hash).unwrap();
    let staged = store.stage_cli_scope_refresh(&upgrade).unwrap();
    store.activate_cli_scope_refresh(&staged).unwrap();
    store.with_connection(|c| {
        assert_eq!(c.query_row("SELECT hash_version FROM client_scope_refreshes WHERE refresh_id=?1", [[2_u8; 32].as_slice()], |r| r.get::<_, i64>(0))?, 8);
        assert_eq!(c.query_row("SELECT hash_version FROM client_scope_refreshes WHERE refresh_id=?1", [[4_u8; 32].as_slice()], |r| r.get::<_, i64>(0))?, 9);
        Ok(())
    }).unwrap();
}

#[test]
fn acc02_v23_migration_failure_rolls_back_schema22_and_google_v8_rows() {
    let fixture = Fixture::new();
    let _ = seed_v22_google_body(&fixture);
    let store = Store { connection: fixture.keyed() };
    assert_eq!(store.migrate_with_backup(22, 23, SCHEMA, |c| {
        c.execute_batch(MIGRATION_V22_TO_V23)?;
        c.execute_batch("CREATE TABLE unexpected_v23_failure(value)")
    }), Err(ErrorCode::InternalError));
    store.with_connection(|c| {
        validate_schema(c, SCHEMA_V22, 22)?;
        assert_eq!(c.query_row("SELECT hash_version FROM client_scope_refreshes WHERE refresh_id=?1", [[2_u8; 32].as_slice()], |r| r.get::<_, i64>(0))?, 8);
        assert_eq!(c.query_row("SELECT approved_channel FROM client_registrations WHERE display_label=?1", [DEFAULT_CLI_LABEL], |r| r.get::<_, String>(0))?, "google_oauth");
        Ok(())
    }).unwrap();
}

fn v23_product_rows(c: &rusqlite::Connection) -> rusqlite::Result<V14TableRows> {
    let template = rusqlite::Connection::open_in_memory()?;
    template.execute_batch(SCHEMA_V23)?;
    frozen_product_table_names(SCHEMA_V23)?.into_iter()
        .filter(|name| name != "app_metadata")
        .map(|name| {
            let mut query = template.prepare(&format!("PRAGMA table_info({name})"))?;
            let columns = query.query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((name.clone(), read_table_columns(c, &name, &columns)?))
        }).collect()
}

#[test]
fn acc02_v24_migration_preserves_all_v23_rows_fk_sequence_and_google_history() {
    let f = Fixture::new();
    let (pin, account) = seed_v22_google_body(&f);
    let store = Store { connection:f.keyed() };
    store.migrate_v22_to_v23().unwrap();
    store.with_connection(|c| {
        c.execute("INSERT INTO sqlite_sequence(name,seq) SELECT 'audit_starts',9000 WHERE NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='audit_starts')",[])?;
        c.execute("UPDATE sqlite_sequence SET seq=9000 WHERE name='audit_starts'",[])?;
        validate_schema(c,SCHEMA_V23,23)
    }).unwrap();
    let before = store.with_connection(v23_product_rows).unwrap();
    store.migrate_v23_to_v24().unwrap();
    store.with_connection(|c| {
        assert_eq!(before,v23_product_rows(c)?);
        assert_eq!(c.query_row("SELECT seq FROM sqlite_sequence WHERE name='audit_starts'",[],|r|r.get::<_,i64>(0))?,9000);
        assert!(c.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_none());
        validate_schema(c,SCHEMA,24)
    }).unwrap();
    let next = store.create_terminal_cli_scope_refresh([31;32],
        hantong_contracts::auth::RequestedScope::new(vec![account],hantong_contracts::auth::default_cli_send_tools()).unwrap(),pin).unwrap();
    assert_eq!(next.decision_channel(),"terminal_cli");
    let staged = store.stage_cli_scope_refresh(&next).unwrap();
    store.activate_cli_scope_refresh(&staged).unwrap();
    assert_eq!(store.load_cli_scope_refresh([2;32]).unwrap().unwrap().decision_channel(),"google_oauth");
    drop(store);
    let reopened = f.open().unwrap();
    assert_eq!(reopened.load_cli_scope_refresh([31;32]).unwrap().unwrap().decision_channel(),"terminal_cli");
    assert!(f.0.join("data/backup-before-v23.db").is_file());
}

#[test]
fn acc02_v24_migration_failure_rolls_back_all_v23_rows_and_schema() {
    let f = Fixture::new();
    seed_v22_google_body(&f);
    let store = Store { connection:f.keyed() };
    store.migrate_v22_to_v23().unwrap();
    let before = store.with_connection(v23_product_rows).unwrap();
    assert_eq!(store.migrate_with_backup(23,24,SCHEMA,|c| {
        c.execute_batch(MIGRATION_V23_TO_V24)?;
        c.execute_batch("CREATE TABLE unexpected_terminal_failure(value)")
    }),Err(ErrorCode::InternalError));
    store.with_connection(|c| {
        assert_eq!(before,v23_product_rows(c)?);
        assert!(c.prepare("PRAGMA foreign_key_check")?.query([])?.next()?.is_none());
        validate_schema(c,SCHEMA_V23,23)
    }).unwrap();
}
