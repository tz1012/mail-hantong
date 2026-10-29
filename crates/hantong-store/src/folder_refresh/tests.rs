use super::*;
use crate::{
    account_provisioning::{InitialImapProbeOutcome, imap_account_stage_payload_hash},
    mail_connection::MailMutation,
};
use hantong_contracts::audit::{AuditBegin, AuditTerminal, AuditTool};
use hantong_domain::{
    account_provisioning::NewImapAccountDraft,
    capabilities::ImapCapabilities,
    mail_connection::MailConnectionConfig,
    mailboxes::{ImapMailboxDescriptor, ImapMailboxName, MailboxChildren, MailboxEncoding},
};
use std::{fs, path::PathBuf};

struct Fixture {
    store: Option<Store>,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hantong-folder-job-{}",
            hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let endpoint = endpoint();
        let draft = NewImapAccountDraft::new("acct".into(), None, None).unwrap();
        let m = MailMutation::new(
            "provision".into(),
            "provision".into(),
            0,
            imap_account_stage_payload_hash(&draft, &endpoint).unwrap(),
        )
        .unwrap();
        store.stage_imap_account(&draft, &endpoint, &m).unwrap();
        store
            .finish_initial_imap_probe("acct", 1, 1, 0, &InitialImapProbeOutcome::Ready(vec![]))
            .unwrap();
        let f = Self {
            store: Some(store),
            root,
        };
        f.add_registration(9);
        f
    }
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    fn add_registration(&self, id: u8) {
        self.store().with_connection(|c| {
            let tools="[\"mail.folders.list\",\"mail.folders.refresh\"]";
            c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli','folder-fixture','[\"acct\"]',?2,'approved','gui',1,1,300001,2)",rusqlite::params![vec![id;32],tools])?;
            let mut key=vec![0u8;72];key[..8].copy_from_slice(b"ECS1\x20\0\0\0");key[8]=id;
            c.execute("INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,version) VALUES(?1,?2,'cli','folder-fixture',?3,1,'active',1,60,'gui',2,2,1)",rusqlite::params![vec![id;16],vec![id;32],key])?;
            c.execute("INSERT INTO client_registration_accounts VALUES(?1,'acct')",[vec![id;16]])?;
            for tool in ["mail.folders.list","mail.folders.refresh"] {c.execute("INSERT INTO client_registration_tools VALUES(?1,?2)",rusqlite::params![vec![id;16],tool])?;}
            Ok(())
        }).unwrap();
    }
    fn begin_for(&self, id: u8, owner: u8, tool: AuditTool) {
        self.store()
            .begin_audit(
                &AuditBegin::new([id; 32], [owner; 16], 1, tool, vec!["acct".into()]).unwrap(),
            )
            .unwrap();
    }
    fn begin(&self, id: u8) {
        self.begin_for(id, 9, AuditTool::FoldersRefresh);
    }
    fn admit(
        &self,
        id: u8,
        request: &FolderRefreshRequest,
        queue: FolderQueueAdmission,
    ) -> FolderRefreshAdmission {
        self.begin(id);
        self.store()
            .admit_folder_refresh([id; 32], request, queue)
            .unwrap()
    }
    fn sql(&self, sql: &str) {
        self.store()
            .with_connection(|c| c.execute_batch(sql))
            .unwrap();
    }
    fn count(&self, table: &str) -> i64 {
        self.store()
            .with_connection(|c| {
                c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            })
            .unwrap()
    }
    fn audit_error(&self, id: u8) -> Option<ErrorCode> {
        self.store()
            .load_audit([id; 32])
            .unwrap()
            .unwrap()
            .completion()
            .unwrap()
            .terminal()
            .error()
    }
    fn reopen(&mut self) {
        self.store.take();
        self.store = Some(Store::open(self.root.join("data"), self.root.join("keys")).unwrap());
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.store.take();
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn endpoint() -> ImapEndpointConfig {
    MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":1,"host":"imap.example.test","port":993,"tls":"implicit_tls","login_name":"synthetic","authentication":"app_password"}}"#).unwrap().imap().clone()
}
fn request(op: &str, key: &str, generation: u64) -> FolderRefreshRequest {
    let raw = FolderRefreshRequest::new("acct", op, key, generation, &"0".repeat(64)).unwrap();
    let hash = hantong_contracts::wire_hex::encode(
        &hantong_platform::sha256(&raw.intent_bytes()).unwrap(),
    );
    FolderRefreshRequest::new("acct", op, key, generation, &hash).unwrap()
}
fn snapshot() -> ImapMailboxSnapshot {
    ImapMailboxSnapshot::new(
        "acct",
        1,
        ImapCapabilities::from_features([]),
        vec![
            ImapMailboxDescriptor::new(
                ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap(),
                Some('/'),
                false,
                false,
                MailboxChildren::Unknown,
                vec![],
            )
            .unwrap(),
        ],
    )
    .unwrap()
}
fn status(id: u8) -> FolderRefreshStatusRequest {
    FolderRefreshStatusRequest::new("acct", [id; 32]).unwrap()
}

#[test]
fn ext06_folder_job_success_is_atomic_and_replay_never_restarts() {
    let mut f = Fixture::new();
    let r = request("op", "key", 0);
    assert!(matches!(
        f.admit(1, &r, FolderQueueAdmission::Reserved),
        FolderRefreshAdmission::Admitted(_)
    ));
    assert_eq!(
        f.store()
            .load_folder_refresh_receipt([9; 16], &status(1))
            .unwrap()
            .state(),
        FolderRefreshState::Unresolved
    );
    assert!(
        f.store()
            .load_audit([1; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    let FolderRefreshStart::Ready(job) = f.store().start_folder_refresh([1; 32]).unwrap() else {
        panic!("not admitted")
    };
    assert_eq!(job.request(), &r);
    assert_eq!(job.endpoint(), &endpoint());
    assert!(f.store().start_folder_refresh([1; 32]).is_err());
    let done = f
        .store()
        .complete_folder_refresh_success([1; 32], &snapshot())
        .unwrap();
    assert_eq!(done.state(), FolderRefreshState::Succeeded);
    assert_eq!(done.generation(), Some(1));
    assert_eq!(f.count("imap_mailbox_catalog_operations"), 1);
    assert_eq!(f.audit_error(1), None);
    f.reopen();
    assert!(
        matches!(f.admit(2,&r,FolderQueueAdmission::Full),FolderRefreshAdmission::Existing(v) if v==done)
    );
    assert_eq!(f.count("folder_refresh_requests"), 1);
    assert_eq!(f.audit_error(2), None);
    assert!(
        matches!(f.store().start_folder_refresh([1;32]).unwrap(),FolderRefreshStart::Finished(v) if v==done)
    );
}
#[test]
fn sec03_folder_job_replay_and_status_are_bound_to_owner_account_and_intent() {
    let f = Fixture::new();
    let r = request("op", "key", 0);
    f.admit(1, &r, FolderQueueAdmission::Reserved);
    f.add_registration(10);
    f.begin_for(2, 10, AuditTool::FoldersRefresh);
    assert!(matches!(
        f.store()
            .admit_folder_refresh([2; 32], &r, FolderQueueAdmission::Reserved)
            .unwrap(),
        FolderRefreshAdmission::Rejected(ErrorCode::Conflict)
    ));
    assert_eq!(
        f.store().load_folder_refresh_receipt([10; 16], &status(1)),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        f.store().load_folder_refresh_receipt(
            [9; 16],
            &FolderRefreshStatusRequest::new("other", [1; 32]).unwrap()
        ),
        Err(ErrorCode::NotFound)
    );
    for (id, changed) in [
        (3, request("other", "key", 0)),
        (4, request("op", "other", 0)),
        (5, request("op", "key", 1)),
    ] {
        assert!(matches!(
            f.admit(id, &changed, FolderQueueAdmission::Reserved),
            FolderRefreshAdmission::Rejected(ErrorCode::Conflict)
        ));
        assert_eq!(f.audit_error(id), Some(ErrorCode::Conflict));
    }
    assert_eq!(f.count("folder_refresh_requests"), 1);
}
#[test]
fn ext06_folder_job_full_closed_and_invalid_hash_have_durable_failures_only() {
    let f = Fixture::new();
    for (id, queue, error) in [
        (1, FolderQueueAdmission::Full, ErrorCode::RateLimited),
        (2, FolderQueueAdmission::Closed, ErrorCode::CoreUnavailable),
    ] {
        let name = format!("op{id}");
        let r = request(&name, &name, 0);
        assert!(
            matches!(f.admit(id,&r,queue),FolderRefreshAdmission::Failed(v) if v.error()==Some(error))
        );
        assert_eq!(f.audit_error(id), Some(error));
    }
    let bad = FolderRefreshRequest::new("acct", "bad", "bad", 0, &"0".repeat(64)).unwrap();
    assert!(matches!(
        f.admit(3, &bad, FolderQueueAdmission::Reserved),
        FolderRefreshAdmission::Rejected(ErrorCode::InvalidInput)
    ));
    assert_eq!(f.count("folder_refresh_requests"), 2);
    assert_eq!(f.count("imap_mailbox_catalogs"), 0);
}
#[test]
fn sec03_folder_job_requires_exact_unresolved_audit_and_current_scope() {
    let f = Fixture::new();
    let r = request("op", "key", 0);
    assert_eq!(
        f.store()
            .admit_folder_refresh([1; 32], &r, FolderQueueAdmission::Reserved),
        Err(ErrorCode::NotFound)
    );
    f.begin_for(1, 9, AuditTool::FoldersList);
    assert_eq!(
        f.store()
            .admit_folder_refresh([1; 32], &r, FolderQueueAdmission::Reserved),
        Err(ErrorCode::InvalidInput)
    );
    f.begin(2);
    f.store()
        .finish_audit([2; 32], AuditTerminal::failed(ErrorCode::PolicyBlocked))
        .unwrap();
    assert_eq!(
        f.store()
            .admit_folder_refresh([2; 32], &r, FolderQueueAdmission::Reserved),
        Err(ErrorCode::Conflict)
    );
    f.begin(3);
    f.sql("DELETE FROM client_registration_tools WHERE tool_id='mail.folders.refresh'");
    assert!(matches!(
        f.store()
            .admit_folder_refresh([3; 32], &r, FolderQueueAdmission::Reserved)
            .unwrap(),
        FolderRefreshAdmission::Rejected(ErrorCode::ScopeDenied)
    ));
    assert_eq!(f.count("folder_refresh_requests"), 0);
}
#[test]
fn sec03_folder_job_revalidates_before_network_and_before_catalog_commit() {
    for (sql, error) in [
        (
            "UPDATE client_registrations SET state='suspended'",
            ErrorCode::ClientSuspended,
        ),
        (
            "UPDATE client_registrations SET profile_epoch=2",
            ErrorCode::AuthRequired,
        ),
        (
            "DELETE FROM client_registration_tools WHERE tool_id='mail.folders.refresh'",
            ErrorCode::ScopeDenied,
        ),
        (
            "UPDATE accounts SET settings_version=3",
            ErrorCode::Conflict,
        ),
        (
            "UPDATE accounts SET status='reauth_required'",
            ErrorCode::Conflict,
        ),
        (
            "UPDATE mail_connection_settings SET host='changed.example.test'",
            ErrorCode::Conflict,
        ),
    ] {
        for before_start in [true, false] {
            let f = Fixture::new();
            f.admit(1, &request("op", "key", 0), FolderQueueAdmission::Reserved);
            if !before_start {
                assert!(matches!(
                    f.store().start_folder_refresh([1; 32]).unwrap(),
                    FolderRefreshStart::Ready(_)
                ));
            }
            f.sql(sql);
            let failed = if before_start {
                let FolderRefreshStart::Finished(r) =
                    f.store().start_folder_refresh([1; 32]).unwrap()
                else {
                    panic!("changed binding allowed")
                };
                r
            } else {
                assert_eq!(f.store().check_folder_refresh([1; 32]), Err(error));
                f.store()
                    .complete_folder_refresh_success([1; 32], &snapshot())
                    .unwrap()
            };
            assert_eq!(failed.error(), Some(error));
            assert_eq!(f.audit_error(1), Some(error));
            assert_eq!(f.count("imap_mailbox_catalogs"), 0);
        }
    }
}
#[test]
fn ext06_folder_job_crash_reopen_is_unresolved_and_explicit_duplicate_only_reads() {
    for running in [false, true] {
        let mut f = Fixture::new();
        let r = request("op", "key", 0);
        f.admit(1, &r, FolderQueueAdmission::Reserved);
        if running {
            f.store().start_folder_refresh([1; 32]).unwrap();
        }
        f.reopen();
        assert!(
            matches!(f.admit(2,&r,FolderQueueAdmission::Reserved),FolderRefreshAdmission::Existing(v) if v.state()==FolderRefreshState::Unresolved)
        );
        assert_eq!(f.count("folder_refresh_requests"), 1);
        assert_eq!(f.count("imap_mailbox_catalogs"), 0);
    }
}
#[test]
fn ext06_folder_job_and_direct_catalog_cannot_consume_each_others_operation() {
    for job_first in [true, false] {
        let f = Fixture::new();
        let s = snapshot();
        let e = endpoint();
        let r = request("op", "key", 0);
        let m = MailMutation::new(
            "op".into(),
            "key".into(),
            0,
            crate::imap_catalog::imap_catalog_payload_hash(2, &e, &s, 0).unwrap(),
        )
        .unwrap();
        if job_first {
            f.admit(1, &r, FolderQueueAdmission::Reserved);
            assert_eq!(
                f.store().replace_imap_mailbox_snapshot(2, &e, &s, &m),
                Err(ErrorCode::Conflict)
            );
        } else {
            f.store()
                .replace_imap_mailbox_snapshot(2, &e, &s, &m)
                .unwrap();
            assert!(matches!(
                f.admit(1, &r, FolderQueueAdmission::Reserved),
                FolderRefreshAdmission::Rejected(ErrorCode::Conflict)
            ));
        }
    }
}
#[test]
fn ext06_folder_job_success_failure_rolls_back_catalog_job_and_audit_together() {
    let f = Fixture::new();
    f.admit(1, &request("op", "key", 0), FolderQueueAdmission::Reserved);
    f.store().start_folder_refresh([1; 32]).unwrap();
    f.sql("CREATE TRIGGER fail_job_update BEFORE UPDATE ON folder_refresh_requests WHEN NEW.state='succeeded' BEGIN SELECT RAISE(ABORT,'synthetic'); END");
    assert_eq!(
        f.store()
            .complete_folder_refresh_success([1; 32], &snapshot()),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(f.count("imap_mailbox_catalogs"), 0);
    assert_eq!(f.count("imap_mailbox_catalog_operations"), 0);
    assert_eq!(f.count("audit_terminals"), 0);
    f.sql("DROP TRIGGER fail_job_update");
    assert_eq!(
        f.store()
            .complete_folder_refresh_failure([1; 32], ErrorCode::Offline)
            .unwrap()
            .error(),
        Some(ErrorCode::Offline)
    );
}

#[test]
fn sec03_folder_job_corrupt_binding_does_not_commit_a_normal_failure() {
    for running in [false, true] {
        let f = Fixture::new();
        f.admit(1, &request("op", "key", 0), FolderQueueAdmission::Reserved);
        if running {
            f.store().start_folder_refresh([1; 32]).unwrap();
        }
        f.sql("PRAGMA ignore_check_constraints=ON; UPDATE accounts SET status='corrupt'; PRAGMA ignore_check_constraints=OFF");
        if running {
            assert_eq!(
                f.store()
                    .complete_folder_refresh_success([1; 32], &snapshot()),
                Err(ErrorCode::InternalError)
            );
        } else {
            assert_eq!(
                f.store().start_folder_refresh([1; 32]),
                Err(ErrorCode::InternalError)
            );
        }
        assert_eq!(f.count("audit_terminals"), 0);
    }
}
#[test]
fn sec03_folder_job_success_hash_corruption_is_rejected_before_reopen() {
    for field in ["payload_hash", "snapshot_hash"] {
        for short in [false, true] {
            let mut f = Fixture::new();
            f.admit(1, &request("op", "key", 0), FolderQueueAdmission::Reserved);
            f.store().start_folder_refresh([1; 32]).unwrap();
            f.store()
                .complete_folder_refresh_success([1; 32], &snapshot())
                .unwrap();
            // Keep an old result, so validation cannot rely only on the current catalog.
            f.admit(
                2,
                &request("later", "later", 1),
                FolderQueueAdmission::Reserved,
            );
            f.store().start_folder_refresh([2; 32]).unwrap();
            f.store()
                .complete_folder_refresh_success([2; 32], &snapshot())
                .unwrap();
            let bytes = if short { "x'00'" } else { "zeroblob(32)" };
            f.sql(&format!("PRAGMA ignore_check_constraints=ON; UPDATE imap_mailbox_catalog_operations SET {field}={bytes} WHERE operation_id='op'; PRAGMA ignore_check_constraints=OFF"));
            assert_eq!(
                f.store().load_folder_refresh_receipt([9; 16], &status(1)),
                Err(ErrorCode::InternalError)
            );
            f.store.take();
            assert!(matches!(
                Store::open(f.root.join("data"), f.root.join("keys")),
                Err(ErrorCode::InternalError)
            ));
        }
    }
}
#[test]
fn ext06_folder_job_catalog_quota_is_terminal_without_catalog_change() {
    let f = Fixture::new();
    f.admit(1, &request("op", "key", 0), FolderQueueAdmission::Reserved);
    f.store().start_folder_refresh([1; 32]).unwrap();
    f.sql("WITH RECURSIVE seq(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM seq WHERE n<100000) INSERT INTO imap_mailbox_catalog_operations SELECT 'fill-'||n,'acct','fill-'||n,2,1,zeroblob(32),zeroblob(32),0,1,zeroblob(32),1 FROM seq");
    assert_eq!(
        f.store()
            .complete_folder_refresh_success([1; 32], &snapshot())
            .unwrap()
            .error(),
        Some(ErrorCode::QuotaExceeded)
    );
    assert_eq!(f.audit_error(1), Some(ErrorCode::QuotaExceeded));
    assert_eq!(f.count("imap_mailbox_catalogs"), 0);
}
