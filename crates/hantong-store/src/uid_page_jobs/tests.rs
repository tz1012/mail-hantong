use super::*;
use crate::{
    account_provisioning::{InitialImapProbeOutcome, imap_account_stage_payload_hash},
    imap_catalog::imap_catalog_payload_hash,
    imap_sync::imap_uid_page_payload_hash,
    mail_connection::MailMutation,
};
use hantong_contracts::{
    audit::{AuditBegin, AuditTool},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity, UidPageState},
};
use hantong_domain::{
    account_provisioning::NewImapAccountDraft,
    capabilities::ImapCapabilities,
    imap_sync::{ImapEnvelope, ImapEnvelopeInput, ImapMessageMetadata},
    mail_connection::MailConnectionConfig,
    mailboxes::{
        ImapMailboxDescriptor, ImapMailboxName, ImapMailboxSnapshot, MailboxChildren,
        MailboxEncoding,
    },
};
use std::{fs, path::PathBuf};

struct Fixture {
    store: Option<Store>,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "hantong-uid-job-{}",
            hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
        ));
        fs::create_dir(&root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let ep = endpoint();
        let draft = NewImapAccountDraft::new("acct".into(), None, None).unwrap();
        let mutation = MailMutation::new(
            "provision".into(),
            "provision".into(),
            0,
            imap_account_stage_payload_hash(&draft, &ep).unwrap(),
        )
        .unwrap();
        store.stage_imap_account(&draft, &ep, &mutation).unwrap();
        store
            .finish_initial_imap_probe("acct", 1, 1, 0, &InitialImapProbeOutcome::Ready(vec![]))
            .unwrap();
        let f = Self {
            store: Some(store),
            root,
        };
        f.catalog(0);
        f.add_registration(9);
        f
    }
    fn store(&self) -> &Store {
        self.store.as_ref().unwrap()
    }
    fn catalog(&self, previous: u64) {
        let snapshot = ImapMailboxSnapshot::new(
            "acct",
            1,
            ImapCapabilities::from_features([]),
            vec![
                ImapMailboxDescriptor::new(
                    mailbox(),
                    Some('/'),
                    false,
                    false,
                    MailboxChildren::Unknown,
                    vec![],
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let id = format!("catalog-{previous}");
        let mutation = MailMutation::new(
            id.clone(),
            id,
            previous,
            imap_catalog_payload_hash(2, &endpoint(), &snapshot, previous).unwrap(),
        )
        .unwrap();
        self.store()
            .replace_imap_mailbox_snapshot(2, &endpoint(), &snapshot, &mutation)
            .unwrap();
    }
    fn add_registration(&self, id: u8) {
        self.store().with_connection(|c| {
            c.execute("INSERT INTO client_registration_requests(request_id,client_kind,display_label,requested_accounts_json,requested_tools_json,state,decision_channel,version,created_at_utc_ms,expires_at_utc_ms,decided_at_utc_ms) VALUES(?1,'cli','uid-fixture','[\"acct\"]','[\"mail.sync.page\"]','approved','gui',1,1,300001,2)",[vec![id;32]])?;
            let mut key=vec![0u8;72];key[..8].copy_from_slice(b"ECS1\x20\0\0\0");key[8]=id;
            c.execute("INSERT INTO client_registrations(registration_id,request_id,client_kind,display_label,public_key,profile_epoch,state,read_only,requests_per_minute,approved_channel,approved_at_utc_ms,updated_at_utc_ms,version) VALUES(?1,?2,'cli','uid-fixture',?3,1,'active',1,60,'gui',2,2,1)",rusqlite::params![vec![id;16],vec![id;32],key])?;
            c.execute("INSERT INTO client_registration_accounts VALUES(?1,'acct')",[vec![id;16]])?;
            c.execute("INSERT INTO client_registration_tools VALUES(?1,'mail.sync.page')",[vec![id;16]])?;
            Ok(())
        }).unwrap();
    }
    fn begin_for(&self, id: u8, owner: u8) {
        self.store()
            .begin_audit(
                &AuditBegin::new(
                    [id; 32],
                    [owner; 16],
                    1,
                    AuditTool::SyncPage,
                    vec!["acct".into()],
                )
                .unwrap(),
            )
            .unwrap();
    }
    fn admit(
        &self,
        id: u8,
        request: &UidPageRequest,
        queue: UidQueueAdmission,
    ) -> UidPageAdmission {
        self.begin_for(id, 9);
        self.store()
            .admit_uid_page([id; 32], request, queue)
            .unwrap()
    }
    fn start(&self, id: u8) -> UidPageJob {
        let UidPageStart::Ready(job) = self.store().start_uid_page([id; 32]).unwrap() else {
            panic!("expected job")
        };
        *job
    }
    fn sql(&self, s: &str) {
        self.store()
            .with_connection(|c| c.execute_batch(s))
            .unwrap()
    }
    fn count(&self, table: &str) -> i64 {
        self.store()
            .with_connection(|c| {
                c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            })
            .unwrap()
    }
    fn receipt(&self, id: u8) -> UidPageReceipt {
        self.store()
            .load_uid_page_receipt(
                [9; 16],
                &UidPageStatusRequest::new("acct", [id; 32]).unwrap(),
            )
            .unwrap()
    }
    fn error(&self, id: u8) -> Option<ErrorCode> {
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
        self.store = Some(Store::open(self.root.join("data"), self.root.join("keys")).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.store.take();
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn endpoint() -> ImapEndpointConfig {
    MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":1,"host":"imap.fixture.invalid","port":993,"tls":"implicit_tls","login_name":"synthetic-uid","authentication":"app_password"}}"#).unwrap().imap().clone()
}
fn mailbox() -> ImapMailboxName {
    ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap()
}
fn identity(wire: &[u8]) -> UidMailboxIdentity {
    UidMailboxIdentity::new(
        &hantong_contracts::wire_hex::encode(wire),
        UidMailboxEncoding::ModifiedUtf7,
    )
    .unwrap()
}
fn request_with(op: &str, key: &str, cg: u64, cv: u64, wire: &[u8]) -> UidPageRequest {
    let raw =
        UidPageRequest::new("acct", identity(wire), op, key, cg, cv, &"0".repeat(64)).unwrap();
    let hash = hantong_contracts::wire_hex::encode(
        &hantong_platform::sha256(&raw.intent_bytes()).unwrap(),
    );
    UidPageRequest::new("acct", identity(wire), op, key, cg, cv, &hash).unwrap()
}
fn request(op: &str, cv: u64) -> UidPageRequest {
    request_with(op, op, 1, cv, b"INBOX")
}
fn page(job: &UidPageJob, next: u32, uid: Option<u32>) -> ImapUidPage {
    ImapUidPage::new(
        job.fetch_request().clone(),
        7,
        next,
        uid.into_iter()
            .map(|uid| {
                ImapMessageMetadata::new(
                    uid,
                    vec!["\\Seen".into()],
                    1767225600000,
                    12,
                    ImapEnvelope::new(ImapEnvelopeInput::default()).unwrap(),
                )
                .unwrap()
            })
            .collect(),
    )
    .unwrap()
}
fn mutation(page: &ImapUidPage, cv: u64, id: &str) -> MailMutation {
    MailMutation::new(
        id.into(),
        id.into(),
        cv,
        imap_uid_page_payload_hash(2, 1, &endpoint(), page, cv).unwrap(),
    )
    .unwrap()
}

#[test]
fn sync02_uid_job_success_replay_keeps_historical_receipt_after_later_page() {
    let mut f = Fixture::new();
    let r = request("first", 0);
    assert!(matches!(
        f.admit(1, &r, UidQueueAdmission::Reserved),
        UidPageAdmission::Admitted(_)
    ));
    let job = f.start(1);
    assert_eq!(job.request(), &r);
    assert_eq!(job.endpoint(), &endpoint());
    let p = page(&job, 101, Some(2));
    let first = f.store().complete_uid_page_success([1; 32], &p).unwrap();
    assert_eq!(first.state(), UidPageState::Succeeded);
    assert_eq!(first.cursor_version(), Some(1));
    assert_eq!(first.covered_through_uid(), Some(64));
    assert_eq!(first.inserted_count(), Some(1));
    assert_eq!(f.error(1), None);
    assert!(matches!(
        f.admit(2, &request("second", 1), UidQueueAdmission::Reserved),
        UidPageAdmission::Admitted(_)
    ));
    let second = f.start(2);
    f.store()
        .complete_uid_page_success([2; 32], &page(&second, 101, Some(90)))
        .unwrap();
    assert_eq!(
        f.admit(3, &r, UidQueueAdmission::Full),
        UidPageAdmission::Existing(first.clone())
    );
    assert_eq!(f.count("imap_uid_page_requests"), 2);
    assert_eq!(f.count("imap_uid_page_operations"), 2);
    assert_eq!(
        f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .version(),
        2
    );
    f.reopen();
    assert_eq!(f.receipt(1), first);
    assert_eq!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 7, 2)
            .unwrap()
            .unwrap()
            .uid(),
        2
    );
}

#[test]
fn sec03_uid_job_queue_failure_and_owner_status_are_durable() {
    for (queue, error) in [
        (UidQueueAdmission::Full, ErrorCode::RateLimited),
        (UidQueueAdmission::Closed, ErrorCode::CoreUnavailable),
    ] {
        let mut f = Fixture::new();
        let UidPageAdmission::Failed(receipt) = f.admit(1, &request("full", 0), queue) else {
            panic!("failed receipt")
        };
        assert_eq!(receipt.error(), Some(error));
        assert_eq!(f.error(1), Some(error));
        assert_eq!(f.count("imap_uid_page_operations"), 0);
        assert_eq!(
            f.store().start_uid_page([1; 32]).unwrap(),
            UidPageStart::Finished(receipt.clone())
        );
        assert_eq!(
            f.store().load_uid_page_receipt(
                [8; 16],
                &UidPageStatusRequest::new("acct", [1; 32]).unwrap()
            ),
            Err(ErrorCode::NotFound)
        );
        assert_eq!(
            f.store().load_uid_page_receipt(
                [9; 16],
                &UidPageStatusRequest::new("other", [1; 32]).unwrap()
            ),
            Err(ErrorCode::NotFound)
        );
        f.reopen();
        assert_eq!(f.receipt(1), receipt);
    }
}

#[test]
fn sync02_uid_job_bad_hash_stale_cas_and_noncanonical_mailbox_only_end_attempt() {
    let f = Fixture::new();
    let cases = [
        (
            UidPageRequest::new(
                "acct",
                identity(b"INBOX"),
                "hash",
                "hash",
                1,
                0,
                &"0".repeat(64),
            )
            .unwrap(),
            ErrorCode::InvalidInput,
        ),
        (
            request_with("catalog", "catalog", 2, 0, b"INBOX"),
            ErrorCode::Conflict,
        ),
        (request("cursor", 1), ErrorCode::Conflict),
        (
            request_with("case", "case", 1, 0, b"inbox"),
            ErrorCode::InvalidInput,
        ),
        (
            request_with("missing", "missing", 1, 0, b"Missing"),
            ErrorCode::NotFound,
        ),
    ];
    for (n, (r, error)) in cases.iter().enumerate() {
        let id = n as u8 + 1;
        assert_eq!(
            f.admit(id, r, UidQueueAdmission::Reserved),
            UidPageAdmission::Rejected(*error)
        );
        assert_eq!(f.error(id), Some(*error));
    }
    assert_eq!(f.count("imap_uid_page_requests"), 0);
    assert_eq!(f.count("imap_uid_page_operations"), 0);
}

#[test]
fn sec03_uid_job_collision_owner_and_tool_revocation_fail_closed() {
    let f = Fixture::new();
    let r = request("one", 0);
    f.admit(1, &r, UidQueueAdmission::Reserved);
    assert_eq!(
        f.admit(
            2,
            &request_with("one", "different", 1, 0, b"INBOX"),
            UidQueueAdmission::Reserved
        ),
        UidPageAdmission::Rejected(ErrorCode::Conflict)
    );
    assert_eq!(
        f.admit(
            3,
            &request_with("different", "one", 1, 0, b"INBOX"),
            UidQueueAdmission::Reserved
        ),
        UidPageAdmission::Rejected(ErrorCode::Conflict)
    );
    f.add_registration(8);
    f.begin_for(4, 8);
    assert_eq!(
        f.store()
            .admit_uid_page([4; 32], &r, UidQueueAdmission::Reserved)
            .unwrap(),
        UidPageAdmission::Rejected(ErrorCode::Conflict)
    );
    f.start(1);
    f.sql("DELETE FROM client_registration_tools WHERE tool_id='mail.sync.page'");
    assert_eq!(
        f.store().check_uid_page([1; 32]),
        Err(ErrorCode::ScopeDenied)
    );
    let receipt = f
        .store()
        .complete_uid_page_failure([1; 32], ErrorCode::ScopeDenied)
        .unwrap();
    assert_eq!(receipt.error(), Some(ErrorCode::ScopeDenied));
    assert_eq!(
        f.store()
            .complete_uid_page_failure([1; 32], ErrorCode::Offline),
        Err(ErrorCode::Conflict)
    );
}

#[test]
fn sync02_uid_job_direct_apply_and_job_keys_cannot_consume_each_other() {
    let f = Fixture::new();
    let r = request("reserved", 0);
    f.admit(1, &r, UidQueueAdmission::Reserved);
    let job = f.start(1);
    let p = page(&job, 3, Some(2));
    assert_eq!(
        f.store()
            .apply_imap_uid_page(2, 1, &endpoint(), &p, &mutation(&p, 0, "reserved")),
        Err(ErrorCode::Conflict)
    );
    f.store().complete_uid_page_success([1; 32], &p).unwrap();
    assert_eq!(
        f.store()
            .apply_imap_uid_page(2, 1, &endpoint(), &p, &mutation(&p, 0, "reserved")),
        Err(ErrorCode::Conflict)
    );
    let f = Fixture::new();
    let p = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), None, 0).unwrap(),
        7,
        1,
        vec![],
    )
    .unwrap();
    f.store()
        .apply_imap_uid_page(2, 1, &endpoint(), &p, &mutation(&p, 0, "direct"))
        .unwrap();
    assert_eq!(
        f.admit(1, &request("direct", 1), UidQueueAdmission::Reserved),
        UidPageAdmission::Rejected(ErrorCode::Conflict)
    );
}

#[test]
fn sync02_uid_job_every_success_write_rolls_back_with_original_audit_unresolved() {
    for (event, table) in [
        ("INSERT", "imap_uid_page_operations"),
        ("INSERT", "imap_uid_cursors"),
        ("INSERT", "imap_uid_messages"),
        ("UPDATE", "imap_uid_page_requests"),
        ("INSERT", "audit_terminals"),
    ] {
        let f = Fixture::new();
        f.admit(1, &request("rollback", 0), UidQueueAdmission::Reserved);
        let job = f.start(1);
        f.sql(&format!("CREATE TEMP TRIGGER reject_uid_job BEFORE {event} ON {table} BEGIN SELECT RAISE(ABORT,'fixture'); END;"));
        assert_eq!(
            f.store()
                .complete_uid_page_success([1; 32], &page(&job, 3, Some(2))),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(f.count("imap_uid_page_operations"), 0);
        assert_eq!(f.count("imap_uid_cursors"), 0);
        assert_eq!(f.count("imap_uid_messages"), 0);
        assert_eq!(f.receipt(1).state(), UidPageState::Unresolved);
        assert!(
            f.store()
                .load_audit([1; 32])
                .unwrap()
                .unwrap()
                .completion()
                .is_none()
        );
    }
}

#[test]
fn sync02_uid_job_restart_does_not_restart_admitted_or_running() {
    let mut f = Fixture::new();
    let r = request("once", 0);
    f.admit(1, &r, UidQueueAdmission::Reserved);
    f.reopen();
    assert_eq!(f.receipt(1).state(), UidPageState::Unresolved);
    f.start(1);
    f.reopen();
    assert_eq!(f.receipt(1).state(), UidPageState::Unresolved);
    assert_eq!(f.store().start_uid_page([1; 32]), Err(ErrorCode::Conflict));
    assert!(matches!(
        f.admit(2, &r, UidQueueAdmission::Reserved),
        UidPageAdmission::Existing(_)
    ));
    assert_eq!(f.count("imap_uid_page_operations"), 0);
}

#[test]
fn sync02_uid_job_catalog_and_cursor_changes_cancel_bound_work() {
    let f = Fixture::new();
    f.admit(1, &request("stale-catalog", 0), UidQueueAdmission::Reserved);
    let job = f.start(1);
    f.catalog(1);
    assert_eq!(f.store().check_uid_page([1; 32]), Err(ErrorCode::Conflict));
    assert_eq!(
        f.store()
            .complete_uid_page_success([1; 32], &page(&job, 2, Some(1)))
            .unwrap()
            .error(),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(f.count("imap_uid_page_operations"), 0);
    let f = Fixture::new();
    f.admit(1, &request("stale-cursor", 0), UidQueueAdmission::Reserved);
    let job = f.start(1);
    let p = page(&job, 2, Some(1));
    f.store()
        .apply_imap_uid_page(2, 1, &endpoint(), &p, &mutation(&p, 0, "other"))
        .unwrap();
    assert_eq!(f.store().check_uid_page([1; 32]), Err(ErrorCode::Conflict));
    assert_eq!(
        f.store()
            .complete_uid_page_success([1; 32], &p)
            .unwrap()
            .error(),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(f.count("imap_uid_page_operations"), 1);
}

#[test]
fn sec03_uid_job_epoch_change_before_start_fails_without_fetch_request() {
    let f = Fixture::new();
    f.admit(1, &request("epoch", 0), UidQueueAdmission::Reserved);
    f.sql("UPDATE client_registrations SET profile_epoch=2,version=version+1");
    let UidPageStart::Finished(receipt) = f.store().start_uid_page([1; 32]).unwrap() else {
        panic!("must finish")
    };
    assert_eq!(receipt.error(), Some(ErrorCode::AuthRequired));
    assert_eq!(f.count("imap_uid_page_operations"), 0);
}

#[test]
fn sync02_uid_job_rejects_wrong_result_request_and_corrupt_success_children() {
    let f = Fixture::new();
    f.admit(1, &request("wrong-page", 0), UidQueueAdmission::Reserved);
    f.start(1);
    let wrong = ImapUidPage::new(
        ImapUidPageRequest::new(
            "acct",
            1,
            ImapMailboxName::from_wire(b"Archive", MailboxEncoding::ModifiedUtf7).unwrap(),
            None,
            0,
        )
        .unwrap(),
        7,
        1,
        vec![],
    )
    .unwrap();
    assert_eq!(
        f.store()
            .complete_uid_page_success([1; 32], &wrong)
            .unwrap()
            .error(),
        Some(ErrorCode::InvalidInput)
    );
    assert_eq!(f.count("imap_uid_cursors"), 0);
    let f = Fixture::new();
    f.admit(1, &request("corrupt", 0), UidQueueAdmission::Reserved);
    let job = f.start(1);
    f.store()
        .complete_uid_page_success([1; 32], &page(&job, 3, Some(2)))
        .unwrap();
    f.sql("DELETE FROM imap_uid_messages");
    assert_eq!(
        f.store().load_uid_page_receipt(
            [9; 16],
            &UidPageStatusRequest::new("acct", [1; 32]).unwrap()
        ),
        Err(ErrorCode::InternalError)
    );
}

#[test]
fn sync02_uid_job_reusing_original_audit_never_finishes_live_job() {
    let f = Fixture::new();
    let request = request("same-audit", 0);
    f.admit(1, &request, UidQueueAdmission::Reserved);
    assert_eq!(
        f.store()
            .admit_uid_page([1; 32], &request, UidQueueAdmission::Reserved),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(f.receipt(1).state(), UidPageState::Unresolved);
    assert!(
        f.store()
            .load_audit([1; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
}

#[test]
fn sync02_uid_job_ignored_cursor_update_rolls_back_new_operation() {
    let f = Fixture::new();
    f.admit(1, &request("first", 0), UidQueueAdmission::Reserved);
    let first = f.start(1);
    f.store()
        .complete_uid_page_success([1; 32], &page(&first, 100, Some(1)))
        .unwrap();
    f.admit(2, &request("second", 1), UidQueueAdmission::Reserved);
    let second = f.start(2);
    f.sql("CREATE TEMP TRIGGER ignore_cursor BEFORE UPDATE ON imap_uid_cursors BEGIN SELECT RAISE(IGNORE); END;");
    assert_eq!(
        f.store()
            .complete_uid_page_success([2; 32], &page(&second, 100, Some(90))),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(f.count("imap_uid_page_operations"), 1);
    assert_eq!(f.count("imap_uid_messages"), 1);
    assert_eq!(f.receipt(2).state(), UidPageState::Unresolved);
    assert_eq!(
        f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .version(),
        1
    );
}

#[test]
fn sync02_uid_job_failed_terminal_is_atomic() {
    let f = Fixture::new();
    f.admit(1, &request("failure", 0), UidQueueAdmission::Reserved);
    f.start(1);
    f.sql("CREATE TEMP TRIGGER reject_terminal BEFORE INSERT ON audit_terminals BEGIN SELECT RAISE(ABORT,'fixture'); END;");
    assert_eq!(
        f.store()
            .complete_uid_page_failure([1; 32], ErrorCode::Offline),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(f.receipt(1).state(), UidPageState::Unresolved);
    assert!(
        f.store()
            .load_audit([1; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
}

#[test]
fn sync02_uid_job_empty_cursor_does_not_reset_after_endpoint_change() {
    let f = Fixture::new();
    f.admit(1, &request("empty", 0), UidQueueAdmission::Reserved);
    let empty = f.start(1);
    f.store()
        .complete_uid_page_success([1; 32], &page(&empty, 1, None))
        .unwrap();
    let current = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert_eq!(current.covered_through_uid(), 0);
    f.admit(2, &request("bound-empty", 1), UidQueueAdmission::Reserved);
    f.start(2);
    f.sql("UPDATE mail_connection_settings SET host='changed.fixture.invalid' WHERE account_id='acct' AND endpoint_kind='imap'");
    assert_eq!(f.store().check_uid_page([2; 32]), Err(ErrorCode::Conflict));
    assert_eq!(
        f.admit(3, &request("not-reset", 1), UidQueueAdmission::Reserved),
        UidPageAdmission::Rejected(ErrorCode::Conflict)
    );
    assert_eq!(f.count("imap_uid_page_operations"), 1);
    assert_eq!(f.receipt(1).cursor_version(), Some(1));
}

#[test]
fn sec03_uid_job_missing_grant_and_suspended_registration_do_not_admit() {
    let f = Fixture::new();
    f.sql("DELETE FROM client_registration_tools WHERE tool_id='mail.sync.page'");
    assert_eq!(
        f.admit(1, &request("missing-grant", 0), UidQueueAdmission::Reserved),
        UidPageAdmission::Rejected(ErrorCode::ScopeDenied)
    );
    let f = Fixture::new();
    f.begin_for(1, 9);
    f.sql("UPDATE client_registrations SET state='suspended',version=version+1");
    assert_eq!(
        f.store()
            .admit_uid_page(
                [1; 32],
                &request("suspended", 0),
                UidQueueAdmission::Reserved
            )
            .unwrap(),
        UidPageAdmission::Rejected(ErrorCode::ClientSuspended)
    );
    assert_eq!(f.count("imap_uid_page_requests"), 0);
}

#[test]
fn sync02_uid_job_tampered_intent_result_hash_and_clock_are_rejected() {
    for sql in [
        "UPDATE imap_uid_page_requests SET intent_hash=zeroblob(32)",
        "UPDATE imap_uid_page_requests SET result_page_hash=zeroblob(32)",
        "UPDATE imap_uid_page_requests SET result_payload_hash=zeroblob(32)",
    ] {
        let f = Fixture::new();
        f.admit(1, &request("tamper", 0), UidQueueAdmission::Reserved);
        let job = f.start(1);
        f.store()
            .complete_uid_page_success([1; 32], &page(&job, 3, Some(2)))
            .unwrap();
        f.sql(sql);
        assert_eq!(
            f.store().load_uid_page_receipt(
                [9; 16],
                &UidPageStatusRequest::new("acct", [1; 32]).unwrap()
            ),
            Err(ErrorCode::InternalError)
        );
    }
    let f = Fixture::new();
    f.admit(1, &request("clock", 0), UidQueueAdmission::Reserved);
    f.start(1);
    f.sql("UPDATE imap_uid_page_requests SET started_at_utc_ms=9000000000000000");
    assert_eq!(
        f.store()
            .complete_uid_page_failure([1; 32], ErrorCode::Offline),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(f.receipt(1).state(), UidPageState::Unresolved);
}

#[test]
fn sync02_uid_job_operation_quota_is_terminal_without_advancing_cursor() {
    let f = Fixture::new();
    let first = request("first", 0);
    f.admit(1, &first, UidQueueAdmission::Reserved);
    let job = f.start(1);
    let receipt = f
        .store()
        .complete_uid_page_success([1; 32], &page(&job, 100, Some(1)))
        .unwrap();
    f.admit(2, &request("second", 1), UidQueueAdmission::Reserved);
    let second = f.start(2);
    // Unrelated unread synthetic operations exercise the count preflight only.
    f.sql("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<99999) INSERT INTO imap_uid_page_operations SELECT 'fill-'||x,account_id,'fill-'||x,printf('%016x',x),mailbox_encoding,expected_cursor_version,result_cursor_version,account_version,catalog_generation,endpoint_version,endpoint_hash,page_json,page_hash,payload_hash,uid_validity,uid_next_at_start,after_uid,covered_through_uid,inserted_count,applied_at_utc_ms FROM imap_uid_page_operations CROSS JOIN n WHERE operation_id='first'");
    assert_eq!(
        f.store()
            .complete_uid_page_success([2; 32], &page(&second, 100, Some(90)))
            .unwrap()
            .error(),
        Some(ErrorCode::QuotaExceeded)
    );
    assert_eq!(f.error(2), Some(ErrorCode::QuotaExceeded));
    assert_eq!(f.count("imap_uid_page_operations"), 100000);
    assert_eq!(f.count("imap_uid_messages"), 1);
    assert_eq!(
        f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .version(),
        1
    );
    assert_eq!(
        f.admit(3, &first, UidQueueAdmission::Full),
        UidPageAdmission::Existing(receipt)
    );
}
