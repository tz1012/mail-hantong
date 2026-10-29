#![cfg(windows)]

use super::*;
use hantong_contracts::ErrorCode;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    store: Option<Store>,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-registration-read-model-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let mut fixture = Self { store: None, root };
        fixture.store =
            Some(Store::open(fixture.root.join("data"), fixture.root.join("keys")).unwrap());
        fixture
    }

    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        drop(self.store.take());
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn scope(accounts: &[&str], tools: &[&str]) -> RequestedScope {
    RequestedScope::new(
        accounts.iter().map(|v| (*v).to_owned()).collect(),
        tools.iter().map(|v| (*v).to_owned()).collect(),
    )
    .unwrap()
}

fn insert_approved_registration(store: &Store, request_id: [u8; 32], registration_id: [u8; 16]) {
    let mut public_key = [0; 72];
    public_key[..4].copy_from_slice(b"ECS1");
    public_key[4..8].copy_from_slice(&32u32.to_le_bytes());
    store
        .with_connection(|c| {
            c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES ('account-a','imap',1,'ready','[]','[]',1)", [])?;
            c.execute(
                "INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES (?1,'cli','desktop','[\"account-a\",\"account-b\"]','[\"mail.read\",\"mail.send\"]','approved','gui',1,1,300001,2)",
                [request_id.as_slice()],
            )?;
            c.execute(
                "INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms,version) VALUES (?1,?2,'cli','desktop',?3,1,'active',1,60,'gui',2,2,NULL,1)",
                rusqlite::params![registration_id.as_slice(), request_id.as_slice(), public_key.as_slice()],
            )?;
            c.execute(
                "INSERT INTO client_registration_accounts(registration_id,account_id) VALUES (?1,'account-a')",
                [registration_id.as_slice()],
            )?;
            c.execute(
                "INSERT INTO client_registration_tools(registration_id,tool_id) VALUES (?1,'mail.read')",
                [registration_id.as_slice()],
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn m102_pending_roundtrip_reopen_ttl_and_duplicate_conflict_at_cap() {
    let mut fixture = Fixture::new();
    let request_id = [1; 32];
    let requested = scope(&["account-a"], &["mail.read"]);
    let created = fixture
        .store()
        .create_registration_request(request_id, ClientKind::Cli, "desktop", &requested)
        .unwrap();
    assert_eq!(
        fixture
            .store()
            .get_registration_request(request_id)
            .unwrap(),
        Some(created.clone())
    );
    assert_eq!(
        fixture.store().create_registration_request(
            request_id,
            ClientKind::Cli,
            "desktop",
            &requested
        ),
        Err(ErrorCode::Conflict)
    );
    drop(fixture.store.take());
    let reopened = Store::open(fixture.root.join("data"), fixture.root.join("keys")).unwrap();
    assert_eq!(
        reopened.get_registration_request(request_id).unwrap(),
        Some(created.clone())
    );
    fixture.store = Some(reopened);
    for index in 2u8..=64 {
        let mut id = [0; 32];
        id[0] = index;
        fixture
            .store()
            .create_registration_request(id, ClientKind::Cli, "desktop", &requested)
            .unwrap();
    }
    let mut overflow = [0; 32];
    overflow[0] = 65;
    assert_eq!(
        fixture.store().create_registration_request(
            overflow,
            ClientKind::Cli,
            "desktop",
            &requested
        ),
        Err(ErrorCode::RateLimited)
    );
    assert_eq!(
        fixture.store().create_registration_request(
            request_id,
            ClientKind::Cli,
            "desktop",
            &requested
        ),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        created.expires_at_utc_ms - created.created_at_utc_ms,
        300_000
    );
}

#[test]
fn m102_expiry_uses_utc_fixture_without_sleep_and_rejects_stale_exhausted_missing() {
    let fixture = Fixture::new();
    let id = [2; 32];
    fixture.store().with_connection(|c| c.execute(
        "INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES (?1,'cli','desktop','[]','[]','pending',NULL,1,1,2,NULL)",
        [id.as_slice()],
    )).unwrap();
    assert_eq!(
        fixture.store().expire_registration_request(id, 99),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        fixture.store().expire_registration_request(id, u64::MAX),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        fixture
            .store()
            .expire_registration_request(id, i64::MAX as u64),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        fixture.store().expire_registration_request([3; 32], 1),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(fixture.store().expire_registration_request(id, 1), Ok(true));
    assert_eq!(
        fixture.store().expire_registration_request(id, 1),
        Err(ErrorCode::Conflict)
    );

    let future = [4; 32];
    let requested = scope(&[], &[]);
    fixture
        .store()
        .create_registration_request(future, ClientKind::Cli, "future", &requested)
        .unwrap();
    assert_eq!(
        fixture.store().expire_registration_request(future, 1),
        Ok(false)
    );

    let cap_fixture = Fixture::new();
    cap_fixture.store().with_connection(|c| {
        c.execute_batch("BEGIN IMMEDIATE")?;
        for index in 0u8..64 {
            let mut pending = [0; 32];
            pending[0] = index;
            c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES (?1,'cli','desktop','[]','[]','pending',NULL,1,1,2,NULL)", [pending.as_slice()])?;
        }
        c.execute_batch("COMMIT")
    }).unwrap();
    assert!(
        cap_fixture
            .store()
            .create_registration_request([5; 32], ClientKind::Cli, "fresh", &requested)
            .is_ok()
    );
}

#[test]
fn m102_retained_cap_counts_1024_rows_without_slow_api_loop() {
    let fixture = Fixture::new();
    fixture.store().with_connection(|c| {
        c.execute_batch("BEGIN IMMEDIATE")?;
        for index in 0u16..1024 {
            let mut id = [0; 32];
            id[..2].copy_from_slice(&index.to_le_bytes());
            c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES (?1,'cli','desktop','[]','[]','approved','gui',1,1000,2000,1500)", [id.as_slice()])?;
        }
        c.execute_batch("COMMIT")
    }).unwrap();
    assert_eq!(
        fixture.store().create_registration_request(
            [9; 32],
            ClientKind::Cli,
            "desktop",
            &scope(&[], &[])
        ),
        Err(ErrorCode::RateLimited)
    );
}

#[test]
fn sec03_granted_scope_comes_from_grants_and_empty_grants_are_empty() {
    let fixture = Fixture::new();
    let request_id = [4; 32];
    let registration_id = [5; 16];
    insert_approved_registration(fixture.store(), request_id, registration_id);
    let record = fixture
        .store()
        .load_registration(registration_id)
        .unwrap()
        .unwrap();
    assert_eq!(record.scope, scope(&["account-a"], &["mail.read"]));
    fixture
        .store()
        .with_connection(|c| {
            c.execute(
                "DELETE FROM client_registration_accounts WHERE registration_id=?1",
                [registration_id.as_slice()],
            )
        })
        .unwrap();
    assert_eq!(
        fixture
            .store()
            .load_registration(registration_id)
            .unwrap()
            .unwrap()
            .scope,
        scope(&[], &["mail.read"])
    );
    fixture
        .store()
        .with_connection(|c| {
            c.execute(
                "DELETE FROM client_registration_tools WHERE registration_id=?1",
                [registration_id.as_slice()],
            )
        })
        .unwrap();
    assert_eq!(
        fixture
            .store()
            .load_registration(registration_id)
            .unwrap()
            .unwrap()
            .scope,
        scope(&[], &[])
    );
}

#[test]
fn sec03_revoked_state_is_returned() {
    let fixture = Fixture::new();
    let id = [6; 16];
    insert_approved_registration(fixture.store(), [7; 32], id);
    fixture.store().with_connection(|c| c.execute("UPDATE client_registrations SET state='revoked',updated_at_utc_ms=1600,revoked_at_utc_ms=1600 WHERE registration_id=?1", [id.as_slice()])).unwrap();
    assert_eq!(
        fixture
            .store()
            .load_registration(id)
            .unwrap()
            .unwrap()
            .state,
        RegistrationState::Revoked
    );
}

#[test]
fn sec03_approval_channel_kind_and_time_mismatches_fail_closed() {
    for sql in [
        "UPDATE client_registrations SET approved_channel='approval_helper'",
        "UPDATE client_registrations SET client_kind='mcp'",
        "UPDATE client_registrations SET approved_at_utc_ms=3,updated_at_utc_ms=3",
    ] {
        let fixture = Fixture::new();
        let id = [8; 16];
        insert_approved_registration(fixture.store(), [9; 32], id);
        fixture
            .store()
            .with_connection(|c| c.execute(sql, []))
            .unwrap();
        assert_eq!(
            fixture.store().load_registration(id),
            Err(ErrorCode::InternalError)
        );
    }
}

#[test]
fn sec03_malformed_key_and_oversized_values_fail_closed_even_with_checks_ignored() {
    let fixture = Fixture::new();
    let id = [10; 16];
    insert_approved_registration(fixture.store(), [11; 32], id);
    fixture.store().with_connection(|c| { c.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE client_registrations SET public_key=x'00' WHERE registration_id=x'0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A0A'; PRAGMA ignore_check_constraints=OFF;") }) .unwrap();
    assert_eq!(
        fixture.store().load_registration(id),
        Err(ErrorCode::InternalError)
    );

    let fixture = Fixture::new();
    let id = [11; 16];
    insert_approved_registration(fixture.store(), [12; 32], id);
    fixture
        .store()
        .with_connection(|c| {
            c.execute_batch("PRAGMA ignore_check_constraints=ON;")?;
            c.execute(
                "UPDATE client_registrations SET display_label=?1 WHERE registration_id=?2",
                rusqlite::params!["x".repeat(129), id.as_slice()],
            )?;
            c.execute_batch("PRAGMA ignore_check_constraints=OFF;")
        })
        .unwrap();
    assert_eq!(
        fixture.store().load_registration(id),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn sec03_129_grants_are_rejected_but_128_maximum_values_roundtrip() {
    let fixture = Fixture::new();
    let id = [12; 16];
    insert_approved_registration(fixture.store(), [13; 32], id);
    fixture.store().with_connection(|c| {
        c.execute_batch("PRAGMA ignore_check_constraints=ON;")?;
        for index in 0u16..129 {
            let account_id = format!("account-{index}");
            c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES (?1,'imap',?2,'ready','[]','[]',1)", rusqlite::params![account_id, i64::from(index) + 2])?;
            c.execute("INSERT INTO client_registration_accounts(registration_id,account_id) VALUES (?1,?2)", rusqlite::params![id.as_slice(), account_id])?;
        }
        c.execute_batch("PRAGMA ignore_check_constraints=OFF;")
    }).unwrap();
    assert_eq!(
        fixture.store().load_registration(id),
        Err(ErrorCode::InternalError)
    );

    let fixture = Fixture::new();
    let id = [14; 16];
    insert_approved_registration(fixture.store(), [15; 32], id);
    let max_accounts: Vec<String> = (0..128)
        .map(|index| format!("{index:03}{}", "a".repeat(125)))
        .collect();
    let max_tools: Vec<String> = (0..128)
        .map(|index| format!("tool{index:03}{}", "a".repeat(89)))
        .collect();
    fixture.store().with_connection(|c| {
        c.execute("DELETE FROM client_registration_accounts WHERE registration_id=?1", [id.as_slice()])?;
        c.execute("DELETE FROM client_registration_tools WHERE registration_id=?1", [id.as_slice()])?;
        c.execute(
            "UPDATE client_registration_requests SET requested_accounts_json=?1,requested_tools_json=?2 WHERE request_id=?3",
            rusqlite::params![serde_json::to_string(&max_accounts).unwrap(), serde_json::to_string(&max_tools).unwrap(), [15u8; 32].as_slice()],
        )?;
        for (index, account_id) in max_accounts.iter().enumerate() {
            c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES (?1,'imap',?2,'ready','[]','[]',1)", rusqlite::params![account_id, i64::try_from(index).unwrap() + 2])?;
            c.execute("INSERT INTO client_registration_accounts(registration_id,account_id) VALUES (?1,?2)", rusqlite::params![id.as_slice(), account_id])?;
        }
        for tool_id in &max_tools {
            c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES (?1,?2)", rusqlite::params![id.as_slice(), tool_id])?;
        }
        Ok(())
    }).unwrap();
    let record = fixture.store().load_registration(id).unwrap().unwrap();
    assert_eq!(record.scope.accounts().len(), 128);
    assert_eq!(record.scope.tools().len(), 128);
}

#[test]
fn m102_maximum_scope_json_roundtrips_through_request_storage() {
    let fixture = Fixture::new();
    let accounts: Vec<String> = (0..128)
        .map(|index| format!("{index:03}{}", "a".repeat(125)))
        .collect();
    let tools: Vec<String> = (0..128)
        .map(|index| format!("tool{index:03}{}", "a".repeat(89)))
        .collect();
    let requested = RequestedScope::new(accounts, tools).unwrap();
    let id = [16; 32];
    let created = fixture
        .store()
        .create_registration_request(id, ClientKind::Gui, "maximum", &requested)
        .unwrap();
    let loaded = fixture
        .store()
        .get_registration_request(id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.scope, requested);
    assert_eq!(loaded, created);
}

#[test]
fn sec03_missing_registration_returns_none() {
    let fixture = Fixture::new();
    assert_eq!(fixture.store().load_registration([0; 16]).unwrap(), None);
}

#[test]
fn m102_registration_labels_reject_control_unicode_overflow_and_empty_without_insert() {
    let fixture = Fixture::new();
    let requested = scope(&[], &[]);
    for label in [
        String::new(),
        " \t\n".into(),
        "label\nsecret".into(),
        "가".repeat(129),
        "x".repeat(513),
    ] {
        assert_eq!(
            fixture.store().create_registration_request(
                [30; 32],
                ClientKind::Cli,
                &label,
                &requested
            ),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            fixture.store().get_registration_request([30; 32]).unwrap(),
            None
        );
    }
    let label = "가".repeat(128);
    let request = fixture
        .store()
        .create_registration_request([30; 32], ClientKind::Gui, &label, &requested)
        .unwrap();
    assert_eq!(
        fixture.store().get_registration_request([30; 32]).unwrap(),
        Some(request.clone())
    );
    assert!(!format!("{request:?}").contains(&label));
}
