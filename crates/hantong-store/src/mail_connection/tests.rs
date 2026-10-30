#![cfg(windows)]
use super::*;
use crate::Store;
use hantong_domain::mail_connection::MailConnectionConfig;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    store: Store,
    root: Cleanup,
}
struct Cleanup(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "mail-hantong-connection-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        store.with_connection(|connection| {
            connection.execute(
                "INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('acct_1','imap',1,'ready','[]','[]',1)",
                [],
            )?;
            Ok(())
        }).unwrap();
        Self {
            store,
            root: Cleanup(root),
        }
    }
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("fixture handles closed before cleanup");
    }
}

fn endpoints(
    imap_version: u64,
    smtp_version: Option<u64>,
) -> (
    hantong_domain::mail_connection::ImapEndpointConfig,
    Option<hantong_domain::mail_connection::SmtpEndpointConfig>,
) {
    let smtp = smtp_version.map(|version| format!(
        r#","smtp":{{"settings_version":{version},"host":"smtp.example","port":465,"tls":"implicit_tls","login_name":"out","authentication":"oauth2"}}"#
    )).unwrap_or_default();
    let raw = format!(
        r#"{{"schema_version":1,"account_id":"acct_1","imap":{{"settings_version":{imap_version},"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"in","authentication":"app_password"}}{smtp}}}"#
    );
    let config = MailConnectionConfig::from_json(raw.as_bytes()).unwrap();
    (config.imap().clone(), config.smtp().cloned())
}

fn mutation(operation: &str, key: &str, expected: u64, hash: [u8; 32]) -> MailMutation {
    MailMutation::new(operation.into(), key.into(), expected, hash).unwrap()
}

#[cfg(windows)]
#[test]
fn ext01_imap_smtp_put_load_reopen_and_hash_vector() {
    let fixture = Fixture::new();
    let (imap, smtp) = endpoints(1, Some(1));
    let known = MailConnectionConfig::from_json(
        br#"{"schema_version":1,"account_id":"a","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"u","authentication":"app_password"}}"#,
    ).unwrap();
    assert_eq!(
        imap_payload_hash("a", known.imap(), 0).unwrap(),
        [
            0xf6, 0x9c, 0x05, 0x2d, 0x94, 0x91, 0xaf, 0x66, 0x62, 0x70, 0x8f, 0x14, 0x4b, 0xd2,
            0x4f, 0xc0, 0xb1, 0x80, 0x1a, 0xc5, 0xef, 0x24, 0x21, 0xbd, 0x81, 0x24, 0xc4, 0x24,
            0x07, 0x5e, 0x41, 0x3a,
        ]
    );
    let ih = imap_payload_hash("acct_1", &imap, 0).unwrap();
    fixture
        .store
        .put_imap_connection("acct_1", &imap, &mutation("op-imap", "key-imap", 0, ih))
        .unwrap();
    let smtp = smtp.unwrap();
    let sh = smtp_payload_hash("acct_1", &smtp, 0).unwrap();
    fixture
        .store
        .put_smtp_connection("acct_1", &smtp, &mutation("op-smtp", "key-smtp", 0, sh))
        .unwrap();
    let loaded = fixture
        .store
        .load_mail_connection("acct_1")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.imap().host(), "imap.example");
    assert_eq!(loaded.smtp().unwrap().host(), "smtp.example");
    let Fixture { store, root } = fixture;
    drop(store);
    let store = Store::open(root.0.join("data"), root.0.join("keys")).unwrap();
    assert_eq!(
        store.load_mail_connection("acct_1").unwrap().unwrap(),
        loaded
    );
}

#[cfg(windows)]
#[test]
fn ext01_exact_replay_does_not_revert_later_settings_and_conflicts_on_payload() {
    let fixture = Fixture::new();
    let (v1, _) = endpoints(1, None);
    let h1 = imap_payload_hash("acct_1", &v1, 0).unwrap();
    let m1 = mutation("op-1", "key-1", 0, h1);
    assert_eq!(fixture.store.put_imap_connection("acct_1", &v1, &m1), Ok(1));
    let (v2, _) = endpoints(2, None);
    let h2 = imap_payload_hash("acct_1", &v2, 1).unwrap();
    fixture
        .store
        .put_imap_connection("acct_1", &v2, &mutation("op-2", "key-2", 1, h2))
        .unwrap();
    assert_eq!(fixture.store.put_imap_connection("acct_1", &v1, &m1), Ok(1));
    assert_eq!(
        fixture
            .store
            .load_mail_connection("acct_1")
            .unwrap()
            .unwrap()
            .imap()
            .settings_version(),
        2
    );
    assert_eq!(
        fixture
            .store
            .put_imap_connection("acct_1", &v2, &mutation("op-1", "key-1", 1, h2)),
        Err(hantong_contracts::ErrorCode::Conflict)
    );
}

#[cfg(windows)]
#[test]
fn ext01_observation_cas_settings_binding_and_endpoint_isolation() {
    let fixture = Fixture::new();
    let (imap, smtp) = endpoints(1, Some(1));
    let ih = imap_payload_hash("acct_1", &imap, 0).unwrap();
    fixture
        .store
        .put_imap_connection("acct_1", &imap, &mutation("op-i", "key-i", 0, ih))
        .unwrap();
    let smtp = smtp.unwrap();
    let sh = smtp_payload_hash("acct_1", &smtp, 0).unwrap();
    fixture
        .store
        .put_smtp_connection("acct_1", &smtp, &mutation("op-s", "key-s", 0, sh))
        .unwrap();
    assert_eq!(
        fixture.store.record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            1,
            0,
            MailConnectionState::Ready
        ),
        Ok(1)
    );
    assert_eq!(
        fixture.store.record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            1,
            0,
            MailConnectionState::Offline
        ),
        Err(hantong_contracts::ErrorCode::Conflict)
    );
    assert_eq!(
        fixture.store.record_mail_observation(
            "acct_1",
            MailEndpointKind::Smtp,
            1,
            0,
            MailConnectionState::TlsFailed
        ),
        Ok(1)
    );
    assert_eq!(
        fixture
            .store
            .load_mail_observation("acct_1", MailEndpointKind::Imap)
            .unwrap()
            .unwrap()
            .state,
        MailConnectionState::Ready
    );
    assert_eq!(
        fixture
            .store
            .load_mail_observation("acct_1", MailEndpointKind::Smtp)
            .unwrap()
            .unwrap()
            .state,
        MailConnectionState::TlsFailed
    );
    assert_eq!(
        fixture
            .store
            .load_mail_observation("missing", MailEndpointKind::Imap),
        Err(hantong_contracts::ErrorCode::NotFound)
    );
}

#[cfg(windows)]
#[test]
fn ext03_atomic_failure_and_persisted_corruption_fail_closed() {
    let fixture = Fixture::new();
    let (imap, _) = endpoints(1, None);
    let hash = imap_payload_hash("acct_1", &imap, 0).unwrap();
    fixture.store.with_connection(|connection| {
        connection.execute_batch("CREATE TRIGGER fail_mail_operation AFTER INSERT ON mail_connection_operations BEGIN SELECT RAISE(FAIL, 'fixture'); END")?;
        Ok(())
    }).unwrap();
    assert_eq!(
        fixture.store.put_imap_connection(
            "acct_1",
            &imap,
            &mutation("op-fail", "key-fail", 0, hash)
        ),
        Err(hantong_contracts::ErrorCode::InternalError)
    );
    fixture
        .store
        .with_connection(|connection| {
            connection.execute("DROP TRIGGER fail_mail_operation", [])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(fixture.store.load_mail_connection("acct_1").unwrap(), None);
    fixture
        .store
        .put_imap_connection("acct_1", &imap, &mutation("op-ok", "key-ok", 0, hash))
        .unwrap();
    fixture
        .store
        .with_connection(|connection| {
            connection.execute(
                "UPDATE mail_connection_settings SET host='IMAP.EXAMPLE' WHERE account_id='acct_1'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        fixture.store.load_mail_connection("acct_1"),
        Err(hantong_contracts::ErrorCode::InternalError)
    );
}

#[cfg(windows)]
#[test]
fn ext03_missing_provider_and_mutation_bounds_are_stable() {
    assert_eq!(
        MailMutation::new("".into(), "k".into(), 0, [0; 32]),
        Err(hantong_contracts::ErrorCode::InvalidInput)
    );
    assert_eq!(
        MailMutation::new("op".into(), "key".into(), i64::MAX as u64, [0; 32]),
        Err(hantong_contracts::ErrorCode::InvalidInput)
    );
    let fixture = Fixture::new();
    let (_, smtp) = endpoints(1, Some(1));
    assert_eq!(
        fixture.store.load_mail_connection("missing"),
        Err(hantong_contracts::ErrorCode::NotFound)
    );
    let smtp = smtp.unwrap();
    let hash = smtp_payload_hash("acct_1", &smtp, 0).unwrap();
    assert_eq!(
        fixture
            .store
            .put_smtp_connection("acct_1", &smtp, &mutation("op", "key", 0, hash)),
        Err(hantong_contracts::ErrorCode::Conflict)
    );
}

fn create_imap(s: &Store) -> (ImapEndpointConfig, MailMutation) {
    let (e, _) = endpoints(1, None);
    let m = mutation("i1", "ik1", 0, imap_payload_hash("acct_1", &e, 0).unwrap());
    assert_eq!(s.put_imap_connection("acct_1", &e, &m), Ok(1));
    (e, m)
}
fn operation_count(s: &Store) -> i64 {
    s.with_connection(|c| {
        c.query_row("SELECT count(*) FROM mail_connection_operations", [], |r| {
            r.get(0)
        })
    })
    .unwrap()
}

#[test]
fn ext01_hash_input_bounds_and_identity_collisions_do_not_mutate() {
    let f = Fixture::new();
    let s = &f.store;
    let (e, m) = create_imap(s);
    for id in ["".to_string(), "x".repeat(129), "../a".into()] {
        assert_eq!(imap_payload_hash(&id, &e, 0), Err(ErrorCode::InvalidInput));
    }
    assert_eq!(
        imap_payload_hash("acct_1", &e, 1),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        imap_payload_hash("acct_1", &e, u64::MAX),
        Err(ErrorCode::InvalidInput)
    );
    for (id, key) in [("other", "ik1"), ("i1", "other"), ("stale", "stale")] {
        assert_eq!(
            s.put_imap_connection("acct_1", &e, &mutation(id, key, 0, *m.payload_hash())),
            Err(ErrorCode::Conflict)
        );
    }
    assert_eq!(
        s.put_imap_connection("acct_1", &e, &mutation("bad", "bad", 0, [0; 32])),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(operation_count(s), 1);
}

#[test]
fn ext01_reopen_replay_preserves_newer_settings_and_observations_without_writes() {
    let f = Fixture::new();
    let (e, m) = create_imap(&f.store);
    let (v2, _) = endpoints(2, None);
    let m2 = mutation("i2", "ik2", 1, imap_payload_hash("acct_1", &v2, 1).unwrap());
    f.store.put_imap_connection("acct_1", &v2, &m2).unwrap();
    f.store
        .record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            2,
            0,
            MailConnectionState::Ready,
        )
        .unwrap();
    let before = f
        .store
        .load_mail_observation("acct_1", MailEndpointKind::Imap)
        .unwrap();
    let Fixture { store, root } = f;
    drop(store);
    let s = Store::open(root.0.join("data"), root.0.join("keys")).unwrap();
    let changes = s.with_connection(|c| Ok(c.total_changes())).unwrap();
    assert_eq!(s.put_imap_connection("acct_1", &e, &m), Ok(1));
    assert_eq!(
        s.with_connection(|c| Ok(c.total_changes())).unwrap(),
        changes
    );
    assert_eq!(
        s.load_mail_connection("acct_1").unwrap().unwrap().imap(),
        &v2
    );
    assert_eq!(
        s.load_mail_observation("acct_1", MailEndpointKind::Imap)
            .unwrap(),
        before
    );
    assert_eq!(
        s.put_imap_connection("acct_1", &e, &mutation("i1", "ik2", 0, *m.payload_hash())),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn ext01_missing_stale_exhausted_observation_and_setting_change_invalidation() {
    let f = Fixture::new();
    let s = &f.store;
    assert_eq!(
        s.record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            1,
            0,
            MailConnectionState::Ready
        ),
        Err(ErrorCode::NotFound)
    );
    create_imap(s);
    assert_eq!(
        s.record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            1,
            1,
            MailConnectionState::Ready
        ),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        s.record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            2,
            0,
            MailConnectionState::Ready
        ),
        Err(ErrorCode::Conflict)
    );
    s.record_mail_observation(
        "acct_1",
        MailEndpointKind::Imap,
        1,
        0,
        MailConnectionState::Ready,
    )
    .unwrap();
    s.with_connection(|c| {
        c.execute(
            "UPDATE mail_connection_observations SET observation_version=?1",
            [i64::MAX],
        )
    })
    .unwrap();
    assert_eq!(
        s.record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            1,
            i64::MAX as u64,
            MailConnectionState::Ready
        ),
        Err(ErrorCode::Conflict)
    );
    let (v2, _) = endpoints(2, None);
    s.put_imap_connection(
        "acct_1",
        &v2,
        &mutation("i2", "ik2", 1, imap_payload_hash("acct_1", &v2, 1).unwrap()),
    )
    .unwrap();
    assert_eq!(
        s.load_mail_observation("acct_1", MailEndpointKind::Imap),
        Ok(None)
    );
    assert_eq!(
        s.record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            1,
            0,
            MailConnectionState::Ready
        ),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        s.record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            2,
            0,
            MailConnectionState::Ready
        ),
        Ok(1)
    );
}

#[test]
fn ext01_clock_rollback_and_corrupt_observation_are_preserved() {
    let f = Fixture::new();
    let s = &f.store;
    create_imap(s);
    s.record_mail_observation(
        "acct_1",
        MailEndpointKind::Imap,
        1,
        0,
        MailConnectionState::Ready,
    )
    .unwrap();
    s.with_connection(|c| {
        c.execute(
            "UPDATE mail_connection_observations SET observed_at_utc_ms=?1",
            [i64::MAX],
        )
    })
    .unwrap();
    let before = s
        .load_mail_observation("acct_1", MailEndpointKind::Imap)
        .unwrap();
    assert_eq!(
        s.record_mail_observation(
            "acct_1",
            MailEndpointKind::Imap,
            1,
            1,
            MailConnectionState::Offline
        ),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        s.load_mail_observation("acct_1", MailEndpointKind::Imap)
            .unwrap(),
        before
    );
    for sql in [
        "UPDATE mail_connection_observations SET observed_settings_version=2",
        "UPDATE mail_connection_observations SET observed_settings_version=1,state='unknown'",
        "UPDATE mail_connection_observations SET state='ready',observed_at_utc_ms=0",
    ] {
        s.with_connection(|c| {
            c.pragma_update(None, "ignore_check_constraints", true)?;
            c.execute(sql, [])?;
            c.pragma_update(None, "ignore_check_constraints", false)
        })
        .unwrap();
        assert_eq!(
            s.load_mail_observation("acct_1", MailEndpointKind::Imap),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            s.record_mail_observation(
                "acct_1",
                MailEndpointKind::Imap,
                1,
                1,
                MailConnectionState::Offline
            ),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn ext03_failed_update_rolls_back_settings_observation_and_operation_together() {
    let f = Fixture::new();
    let s = &f.store;
    let (e, _) = create_imap(s);
    s.record_mail_observation(
        "acct_1",
        MailEndpointKind::Imap,
        1,
        0,
        MailConnectionState::Ready,
    )
    .unwrap();
    let before = s
        .load_mail_observation("acct_1", MailEndpointKind::Imap)
        .unwrap();
    s.with_connection(|c|c.execute_batch("CREATE TRIGGER fail_update AFTER INSERT ON mail_connection_operations BEGIN SELECT RAISE(FAIL,'fixture'); END")).unwrap();
    let (v2, _) = endpoints(2, None);
    assert_eq!(
        s.put_imap_connection(
            "acct_1",
            &v2,
            &mutation("i2", "ik2", 1, imap_payload_hash("acct_1", &v2, 1).unwrap())
        ),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(operation_count(s), 1);
    assert_eq!(
        s.load_mail_connection("acct_1").unwrap().unwrap().imap(),
        &e
    );
    assert_eq!(
        s.load_mail_observation("acct_1", MailEndpointKind::Imap)
            .unwrap(),
        before
    );
}

#[test]
fn ext01_capacity_blocks_new_operations_but_not_exact_replay() {
    let f = Fixture::new();
    let s = &f.store;
    let (e, m) = create_imap(s);
    s.with_connection(|c|c.execute_batch("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<99999) INSERT INTO mail_connection_operations(operation_id,account_id,idempotency_key,endpoint_kind,payload_hash,expected_version,result_version,completed_at_utc_ms) SELECT 'fixture-'||x,'acct_1','fixture-'||x,'imap',zeroblob(32),0,1,1 FROM n")).unwrap();
    assert_eq!(operation_count(s), 100000);
    assert_eq!(s.put_imap_connection("acct_1", &e, &m), Ok(1));
    let (v2, _) = endpoints(2, None);
    assert_eq!(
        s.put_imap_connection(
            "acct_1",
            &v2,
            &mutation("i2", "ik2", 1, imap_payload_hash("acct_1", &v2, 1).unwrap())
        ),
        Err(ErrorCode::QuotaExceeded)
    );
    assert_eq!(
        s.load_mail_connection("acct_1").unwrap().unwrap().imap(),
        &e
    );
}

#[test]
fn ext03_bad_operation_and_setting_metadata_rejected_without_repair() {
    let f = Fixture::new();
    let s = &f.store;
    let (e, m) = create_imap(s);
    s.with_connection(|c| {
        c.pragma_update(None, "ignore_check_constraints", true)?;
        c.execute(
            "UPDATE mail_connection_operations SET completed_at_utc_ms=0",
            [],
        )?;
        c.pragma_update(None, "ignore_check_constraints", false)
    })
    .unwrap();
    assert_eq!(
        s.put_imap_connection("acct_1", &e, &m),
        Err(ErrorCode::InternalError)
    );
    s.with_connection(|c| {
        c.pragma_update(None, "ignore_check_constraints", true)?;
        c.execute(
            "UPDATE mail_connection_settings SET updated_at_utc_ms=0",
            [],
        )?;
        c.pragma_update(None, "ignore_check_constraints", false)
    })
    .unwrap();
    assert_eq!(
        s.load_mail_connection("acct_1"),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        s.load_mail_observation("", MailEndpointKind::Imap),
        Err(ErrorCode::InvalidInput)
    );
}
#[test]
fn acc03_initial_smtp_is_bound_to_account_version_and_cannot_overwrite() {
    let f = Fixture::new();
    let (imap, smtp) = endpoints(1, Some(1));
    let smtp = smtp.unwrap();
    f.store
        .put_imap_connection(
            "acct_1",
            &imap,
            &mutation(
                "imap-initial",
                "imap-initial",
                0,
                imap_payload_hash("acct_1", &imap, 0).unwrap(),
            ),
        )
        .unwrap();
    let m = mutation(
        "smtp-initial",
        "smtp-initial",
        0,
        smtp_payload_hash("acct_1", &smtp, 0).unwrap(),
    );
    assert_eq!(
        f.store.put_initial_smtp_connection("acct_1", &smtp, &m, 2),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store.put_initial_smtp_connection("acct_1", &smtp, &m, 1),
        Ok(1)
    );
    let replacement = mutation(
        "smtp-other",
        "smtp-other",
        0,
        smtp_payload_hash("acct_1", &smtp, 0).unwrap(),
    );
    assert_eq!(
        f.store
            .put_initial_smtp_connection("acct_1", &smtp, &replacement, 1),
        Err(ErrorCode::Conflict)
    );
    f.store
        .with_connection(|c| {
            c.execute(
                "UPDATE accounts SET settings_version=2 WHERE account_id='acct_1'",
                [],
            )
        })
        .unwrap();
    assert_eq!(
        f.store.put_initial_smtp_connection("acct_1", &smtp, &m, 1),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store
            .load_mail_connection("acct_1")
            .unwrap()
            .unwrap()
            .smtp(),
        Some(&smtp)
    );
}
