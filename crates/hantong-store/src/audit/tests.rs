#![cfg(windows)]

use super::*;
use hantong_contracts::{
    ErrorCode,
    audit::{AuditBegin, AuditRecord, AuditTerminal, AuditTool},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    store: Option<Store>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "hantong-audit-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let fixture = Self {
            root,
            store: Some(store),
        };
        fixture.seed_registration();
        fixture
    }
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    fn seed_registration(&self) {
        self.store().with_connection(|c| {
            c.execute("INSERT INTO accounts(account_id,provider,account_number,status,capabilities,granted_scopes,settings_version) VALUES('account-a','imap',1,'ready','[]','[]',1)", [])?;
            c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli','fixture','[\"account-a\"]','[\"mail.accounts.list\"]','approved','gui',1,1,300001,2)", [vec![8u8;32]])?;
            let mut key = vec![0u8;72]; key[..4].copy_from_slice(b"ECS1"); key[4..8].copy_from_slice(&32u32.to_le_bytes());
            c.execute("INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,revoked_at_utc_ms,version) VALUES(?1,?2,'cli','fixture',?3,1,'active',1,60,'gui',2,2,NULL,1)", rusqlite::params![vec![9u8;16],vec![8u8;32],key])?;
            c.execute("INSERT INTO client_registration_accounts(registration_id,account_id) VALUES(?1,'account-a')", [vec![9u8;16]])?;
            c.execute("INSERT INTO client_registration_tools(registration_id,tool_id) VALUES(?1,'mail.accounts.list')", [vec![9u8;16]])?;
            Ok(())
        }).unwrap();
    }
    fn begin(&self, id: u8) -> AuditBegin {
        AuditBegin::new(
            [id; 32],
            [9; 16],
            1,
            AuditTool::AccountsList,
            vec!["account-a".into()],
        )
        .unwrap()
    }
    fn sql(&self, query: &str) {
        self.store()
            .with_connection(|c| c.execute_batch(query))
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.store.take();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn ai10_requester_snapshot_survives_registration_metadata_change() {
    let mut fixture = Fixture::new();
    fixture.store().begin_audit(&fixture.begin(80)).unwrap();
    let actor: (String, String) = fixture
        .store()
        .with_connection(|c| {
            c.query_row(
                "SELECT client_kind,display_label FROM audit_starts WHERE execution_id=?1",
                [vec![80u8; 32]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!(actor, ("cli".into(), "fixture".into()));
    fixture.sql("UPDATE client_registrations SET client_kind='mcp',display_label='changed'");
    // The registration no longer matches its approved request. History must still
    // describe the original actor, without deriving it from today's registration.
    drop(fixture.store.take());
    fixture.store =
        Some(Store::open(fixture.root.join("data"), fixture.root.join("keys")).unwrap());
    let actor: (String, String) = fixture
        .store()
        .with_connection(|c| {
            c.query_row(
                "SELECT client_kind,display_label FROM audit_starts WHERE execution_id=?1",
                [vec![80u8; 32]],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .unwrap();
    assert_eq!(actor, ("cli".into(), "fixture".into()));
    let record = fixture.store().load_audit([80; 32]).unwrap().unwrap();
    assert_eq!(record.actor().kind(), crate::ClientKind::Cli);
    assert_eq!(record.actor().display_label(), "fixture");
    assert_eq!(
        fixture.store().list_audit_page(None, 1).unwrap()[0].actor(),
        record.actor()
    );
}

#[test]
fn m108_begin_persists_unresolved_and_survives_reopen() {
    let mut fixture = Fixture::new();
    let seq = fixture.store().begin_audit(&fixture.begin(1)).unwrap();
    let record = fixture.store().load_audit([1; 32]).unwrap().unwrap();
    assert_eq!(record.sequence(), seq);
    assert!(record.completion().is_none());
    assert_eq!(fixture.store().list_audit_page(None, 100).unwrap().len(), 1);
    drop(fixture.store.take());
    fixture.store =
        Some(Store::open(fixture.root.join("data"), fixture.root.join("keys")).unwrap());
    let reopened = fixture.store().load_audit([1; 32]).unwrap().unwrap();
    assert_eq!(reopened.sequence(), seq);
    assert!(reopened.completion().is_none());
}

#[test]
fn m108_finish_success_count_and_duplicate_conflict() {
    let fixture = Fixture::new();
    fixture.store().begin_audit(&fixture.begin(2)).unwrap();
    fixture
        .store()
        .finish_audit([2; 32], AuditTerminal::succeeded(Some(3)).unwrap())
        .unwrap();
    let record = fixture.store().load_audit([2; 32]).unwrap().unwrap();
    assert_eq!(
        record.completion().unwrap().terminal().result_count(),
        Some(3)
    );
    assert_eq!(
        fixture
            .store()
            .finish_audit([2; 32], AuditTerminal::failed(ErrorCode::SendUnknown)),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn m108_unknown_registration_state_and_epoch_do_not_insert() {
    let fixture = Fixture::new();
    let mut unknown = fixture.begin(3); // change only the opaque registration through a new ID
    unknown = AuditBegin::new(
        *unknown.execution_id(),
        [7; 16],
        1,
        AuditTool::AccountsList,
        vec!["account-a".into()],
    )
    .unwrap();
    assert_eq!(
        fixture.store().begin_audit(&unknown),
        Err(ErrorCode::ClientNotRegistered)
    );
    fixture.sql("UPDATE client_registrations SET state='suspended'");
    assert_eq!(
        fixture.store().begin_audit(&fixture.begin(4)),
        Err(ErrorCode::ClientSuspended)
    );
    fixture.sql("UPDATE client_registrations SET state='active', profile_epoch=2");
    assert_eq!(
        fixture.store().begin_audit(&fixture.begin(5)),
        Err(ErrorCode::AuthRequired)
    );
}

#[test]
fn m108_duplicate_execution_is_conflict_without_capacity_change() {
    let fixture = Fixture::new();
    fixture.store().begin_audit(&fixture.begin(6)).unwrap();
    let before: (i64,i64) = fixture.store().with_connection(|c| c.query_row("SELECT retained_executions,retained_account_refs FROM audit_capacity WHERE singleton=1", [], |r| Ok((r.get(0)?,r.get(1)?)))).unwrap();
    assert_eq!(
        fixture.store().begin_audit(&fixture.begin(6)),
        Err(ErrorCode::Conflict)
    );
    let after: (i64,i64) = fixture.store().with_connection(|c| c.query_row("SELECT retained_executions,retained_account_refs FROM audit_capacity WHERE singleton=1", [], |r| Ok((r.get(0)?,r.get(1)?)))).unwrap();
    assert_eq!(before, after);
}

#[test]
fn m108_scope_and_pagination_are_bounded_descending() {
    let fixture = Fixture::new();
    for id in 10..13 {
        fixture.store().begin_audit(&fixture.begin(id)).unwrap();
    }
    let rows = fixture.store().list_audit_page(None, 100).unwrap();
    assert_eq!(
        rows.iter().map(AuditRecord::sequence).collect::<Vec<_>>(),
        vec![3, 2, 1]
    );
    assert_eq!(
        fixture.store().list_audit_page(Some(2), 100).unwrap().len(),
        1
    );
    assert_eq!(
        fixture.store().list_audit_page(None, 0),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        fixture.store().list_audit_page(None, 101),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        fixture.store().list_audit_page(Some(0), 1),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        fixture.store().list_audit_page(Some(u64::MAX), 1),
        Err(ErrorCode::InvalidInput)
    );
    let bad = AuditBegin::new(
        [14; 32],
        [9; 16],
        1,
        AuditTool::AccountsList,
        vec!["missing".into()],
    )
    .unwrap();
    assert_eq!(
        fixture.store().begin_audit(&bad),
        Err(ErrorCode::ScopeDenied)
    );
}

#[test]
fn m108_send_unknown_is_terminal_failure_and_missing_is_unresolved() {
    let fixture = Fixture::new();
    fixture.store().begin_audit(&fixture.begin(15)).unwrap();
    assert!(
        fixture
            .store()
            .load_audit([15; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    fixture
        .store()
        .finish_audit([15; 32], AuditTerminal::failed(ErrorCode::SendUnknown))
        .unwrap();
    assert_eq!(
        fixture
            .store()
            .load_audit([15; 32])
            .unwrap()
            .unwrap()
            .completion()
            .unwrap()
            .terminal()
            .error(),
        Some(ErrorCode::SendUnknown)
    );
}

#[test]
fn m108_start_trigger_failure_rolls_back_rows_and_capacity() {
    let fixture = Fixture::new();
    fixture.sql("CREATE TRIGGER audit_account_abort AFTER INSERT ON audit_start_accounts BEGIN SELECT RAISE(ABORT,'fixture'); END");
    assert_eq!(
        fixture.store().begin_audit(&fixture.begin(16)),
        Err(ErrorCode::InternalError)
    );
    let counts: (i64,i64,i64) = fixture.store().with_connection(|c| c.query_row("SELECT (SELECT count(*) FROM audit_starts),(SELECT count(*) FROM audit_start_accounts),(SELECT retained_executions FROM audit_capacity WHERE singleton=1)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))).unwrap();
    assert_eq!(counts, (0, 0, 0));
}

#[test]
fn m108_finish_trigger_failure_preserves_unresolved_start() {
    let fixture = Fixture::new();
    fixture.store().begin_audit(&fixture.begin(17)).unwrap();
    fixture.sql("CREATE TRIGGER audit_terminal_abort BEFORE INSERT ON audit_terminals BEGIN SELECT RAISE(ABORT,'fixture'); END");
    assert_eq!(
        fixture
            .store()
            .finish_audit([17; 32], AuditTerminal::succeeded(None).unwrap()),
        Err(ErrorCode::InternalError)
    );
    assert!(
        fixture
            .store()
            .load_audit([17; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
}

#[test]
fn m108_quota_rejection_is_atomic_and_existing_terminal_is_allowed() {
    let fixture = Fixture::new();
    fixture.store().begin_audit(&fixture.begin(18)).unwrap();
    fixture.sql("UPDATE audit_capacity SET retained_executions=100000");
    assert_eq!(
        fixture.store().begin_audit(&fixture.begin(19)),
        Err(ErrorCode::QuotaExceeded)
    );
    fixture
        .store()
        .finish_audit([18; 32], AuditTerminal::succeeded(None).unwrap())
        .unwrap();
    fixture.sql("UPDATE audit_capacity SET retained_executions=1, retained_account_refs=1000000");
    assert_eq!(
        fixture.store().begin_audit(&fixture.begin(20)),
        Err(ErrorCode::QuotaExceeded)
    );
}

#[test]
fn m108_revoked_after_start_keeps_historic_load_and_finish() {
    let fixture = Fixture::new();
    fixture.store().begin_audit(&fixture.begin(21)).unwrap();
    fixture.sql(
        "UPDATE client_registrations SET state='revoked', updated_at_utc_ms=3, revoked_at_utc_ms=3",
    );
    assert!(fixture.store().load_audit([21; 32]).unwrap().is_some());
    fixture
        .store()
        .finish_audit([21; 32], AuditTerminal::failed(ErrorCode::SendUnknown))
        .unwrap();
}

#[test]
fn m108_missing_start_finish_is_not_found() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture
            .store()
            .finish_audit([22; 32], AuditTerminal::failed(ErrorCode::NotFound)),
        Err(ErrorCode::NotFound)
    );
}

#[test]
fn m108_reader_corruption_is_internal_error() {
    for mutation in [
        "UPDATE audit_starts SET tool_code='bad'",
        "UPDATE audit_starts SET started_at_utc_ms=-1",
        "UPDATE audit_starts SET client_kind='bad'",
        "UPDATE audit_starts SET display_label=' ' ",
        "UPDATE audit_starts SET display_label=char(10)",
        "UPDATE audit_starts SET display_label=replace(hex(zeroblob(129)),'00','a')",
        "UPDATE audit_start_accounts SET account_id='x' || replace(hex(zeroblob(128)),'00','a')",
    ] {
        let fixture = Fixture::new();
        fixture.store().begin_audit(&fixture.begin(23)).unwrap();
        fixture.sql("PRAGMA ignore_check_constraints=ON");
        fixture.sql(mutation);
        assert_eq!(
            fixture.store().load_audit([23; 32]),
            Err(ErrorCode::InternalError)
        );
    }
    let fixture = Fixture::new();
    fixture.store().begin_audit(&fixture.begin(24)).unwrap();
    fixture
        .store()
        .finish_audit([24; 32], AuditTerminal::succeeded(Some(1)).unwrap())
        .unwrap();
    fixture.sql("PRAGMA ignore_check_constraints=ON; UPDATE audit_terminals SET result_count=-1");
    assert_eq!(
        fixture.store().load_audit([24; 32]),
        Err(ErrorCode::InternalError)
    );
}
