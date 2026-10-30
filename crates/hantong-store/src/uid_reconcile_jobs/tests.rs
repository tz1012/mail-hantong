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
    imap_sync::{
        ImapEnvelope, ImapEnvelopeInput, ImapMessageMetadata, ImapUidPage, ImapUidPageRequest,
    },
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
            "hantong-reconcile-job-{}",
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
        request: &UidReconcileRequest,
        queue: UidQueueAdmission,
    ) -> UidReconcileAdmission {
        self.begin_for(id, 9);
        self.store()
            .admit_uid_reconcile([id; 32], request, queue)
            .unwrap()
    }
    fn start(&self, id: u8) -> UidReconcileJob {
        let UidReconcileStart::Ready(job) = self.store().start_uid_reconcile([id; 32]).unwrap()
        else {
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
    fn receipt(&self, id: u8) -> UidReconcileReceipt {
        self.store()
            .load_uid_reconcile_receipt(
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

fn request(op: &str, cv: u64) -> UidReconcileRequest {
    let raw = UidReconcileRequest::new("acct", identity(b"INBOX"), op, op, 1, cv, &"0".repeat(64))
        .unwrap();
    let digest = hantong_contracts::wire_hex::encode(
        &hantong_platform::sha256(&raw.intent_bytes()).unwrap(),
    );
    UidReconcileRequest::new("acct", identity(b"INBOX"), op, op, 1, cv, &digest).unwrap()
}
fn metadata(uid: u32, flags: &[&str]) -> ImapMessageMetadata {
    ImapMessageMetadata::new(
        uid,
        flags.iter().map(|v| v.to_string()).collect(),
        1767225600000,
        12,
        ImapEnvelope::new(ImapEnvelopeInput::default()).unwrap(),
    )
    .unwrap()
}
fn append(f: &Fixture, cv: u64, after: u32, next: u32, uids: &[u32], op: &str) {
    let page = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), (cv > 0).then_some(7), after).unwrap(),
        7,
        next,
        uids.iter().map(|u| metadata(*u, &["\\Seen"])).collect(),
    )
    .unwrap();
    let mutation = MailMutation::new(
        op.into(),
        op.into(),
        cv,
        imap_uid_page_payload_hash(2, 1, &endpoint(), &page, cv).unwrap(),
    )
    .unwrap();
    f.store()
        .apply_imap_uid_page(2, 1, &endpoint(), &page, &mutation)
        .unwrap();
}
fn reconcile(f: &Fixture, id: u8, cv: u64, uids: &[(u32, &[&str])]) -> UidReconcileReceipt {
    assert!(matches!(
        f.admit(
            id,
            &request(&format!("reconcile-{id}"), cv),
            UidQueueAdmission::Reserved
        ),
        UidReconcileAdmission::Admitted(_)
    ));
    let job = f.start(id);
    let page = ImapUidReconcilePage::new(
        job.fetch_request().clone(),
        job.fetch_request().minimum_uid_next(),
        uids.iter().map(|(u, flags)| metadata(*u, flags)).collect(),
    )
    .unwrap();
    f.store()
        .complete_uid_reconcile_success([id; 32], &page)
        .unwrap()
}
#[test]
fn m204_reconcile_flags_and_absence_preserve_immutable_base_and_cursor() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "append-1");
    let receipt = reconcile(&f, 21, 1, &[(1, &["\\Flagged"])]);
    let r = receipt.result().unwrap();
    assert!(r.pass_complete());
    assert_eq!(r.flags_changed_count(), 1);
    assert_eq!(r.newly_absent_count(), 1);
    assert_eq!(
        f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .version(),
        1
    );
    assert_eq!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 7, 1)
            .unwrap()
            .unwrap()
            .flags(),
        ["\\seen"]
    );
    assert_eq!(
        f.store()
            .load_imap_uid_observation("acct", &mailbox(), 7, 1)
            .unwrap()
            .unwrap()
            .flags()
            .unwrap(),
        ["\\flagged"]
    );
    assert!(
        f.store()
            .load_imap_uid_observation("acct", &mailbox(), 7, 3)
            .unwrap()
            .unwrap()
            .remote_absent()
    );
    assert!(
        f.store()
            .load_imap_uid_observation("acct", &mailbox(), 7, 2)
            .unwrap()
            .is_none()
    );
}
#[test]
fn m204_reconcile_pass_ceiling_stays_frozen_across_new_append() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1], "append-1");
    append(&f, 1, 64, 200, &[65], "append-2");
    let first = reconcile(&f, 21, 2, &[(1, &["\\Seen"])]);
    assert_eq!(first.result().unwrap().remaining_pages(), 1);
    append(&f, 2, 128, 200, &[129], "append-3");
    let second = reconcile(&f, 22, 3, &[(65, &["\\Seen"])]);
    assert_eq!(second.result().unwrap().frozen_cursor_version(), 2);
    assert!(second.result().unwrap().pass_complete());
    let third = reconcile(&f, 23, 3, &[(1, &["\\Seen"])]);
    assert_eq!(third.result().unwrap().generation(), 2);
    assert_eq!(third.result().unwrap().frozen_cursor_version(), 3);
}
#[test]
fn m204_reconcile_no_work_skips_empty_pages_before_queue_failure_without_head() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[], "empty");
    assert_eq!(
        f.admit(21, &request("none", 1), UidQueueAdmission::Full),
        UidReconcileAdmission::Rejected(ErrorCode::NotFound)
    );
    assert_eq!(f.count("imap_uid_reconcile_heads"), 0);
    assert_eq!(f.count("imap_uid_reconcile_requests"), 0);
    assert_eq!(f.error(21), Some(ErrorCode::NotFound));
}
#[test]
fn m204_reconcile_replay_does_not_restore_earlier_flags() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "append-1");
    let first = reconcile(&f, 21, 1, &[(1, &["\\Flagged"])]);
    reconcile(&f, 22, 1, &[(1, &["\\Answered"])]);
    assert_eq!(
        f.admit(23, &request("reconcile-21", 1), UidQueueAdmission::Full),
        UidReconcileAdmission::Existing(first)
    );
    assert_eq!(
        f.store()
            .load_imap_uid_observation("acct", &mailbox(), 7, 1)
            .unwrap()
            .unwrap()
            .flags()
            .unwrap(),
        ["\\answered"]
    );
}
#[test]
fn m204_reconcile_rejects_unknown_uid_changed_metadata_and_resurrection() {
    for case in 0..3 {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "append-1");
        if case == 2 {
            reconcile(&f, 20, 1, &[]);
        }
        f.admit(21, &request("bad", 1), UidQueueAdmission::Reserved);
        let job = f.start(21);
        let m = if case == 0 {
            metadata(2, &[])
        } else if case == 1 {
            ImapMessageMetadata::new(
                1,
                vec![],
                1767225600000,
                13,
                ImapEnvelope::new(ImapEnvelopeInput::default()).unwrap(),
            )
            .unwrap()
        } else {
            metadata(1, &[])
        };
        let page = ImapUidReconcilePage::new(job.fetch_request().clone(), 11, vec![m]).unwrap();
        let r = f
            .store()
            .complete_uid_reconcile_success([21; 32], &page)
            .unwrap();
        assert_eq!(r.error(), Some(ErrorCode::Conflict));
        assert_eq!(
            f.count("imap_uid_reconcile_messages"),
            if case == 2 { 1 } else { 0 }
        );
    }
}
#[test]
fn m204_reconcile_restart_retains_unresolved_without_advance() {
    let mut f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "append-1");
    f.admit(21, &request("pending", 1), UidQueueAdmission::Reserved);
    f.start(21);
    f.reopen();
    assert_eq!(f.receipt(21).state(), UidPageState::Unresolved);
    assert_eq!(
        f.store().start_uid_reconcile([21; 32]),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(f.count("imap_uid_reconcile_messages"), 0);
}
#[test]
fn m204_reconcile_decodes_only_selected_source_and_rejects_corruption_when_selected() {
    let f = Fixture::new();
    for (cv, after, uid) in [(0, 0, 1), (1, 64, 65), (2, 128, 129), (3, 192, 193)] {
        append(&f, cv, after, 300, &[uid], &format!("append-{cv}"));
    }
    f.sql("UPDATE imap_uid_page_operations SET page_json='{}' WHERE operation_id='append-1'");
    let receipt = reconcile(&f, 21, 4, &[(1, &["\\Seen"])]);
    assert_eq!(receipt.result().unwrap().remaining_pages(), 3);
    f.begin_for(22, 9);
    assert_eq!(
        f.store()
            .admit_uid_reconcile([22; 32], &request("next", 4), UidQueueAdmission::Reserved),
        Err(ErrorCode::InternalError)
    );
}
#[test]
fn m204_reconcile_success_rolls_back_each_write_on_abort_and_ignore() {
    for action in ["ABORT,'synthetic'", "IGNORE"] {
        for (table, event) in [
            ("imap_uid_reconcile_messages", "INSERT"),
            ("imap_uid_reconcile_heads", "UPDATE"),
            ("imap_uid_reconcile_requests", "UPDATE"),
            ("audit_terminals", "INSERT"),
        ] {
            let f = Fixture::new();
            append(&f, 0, 0, 11, &[1, 3], "base");
            f.admit(21, &request("change", 1), UidQueueAdmission::Reserved);
            let job = f.start(21);
            let page = ImapUidReconcilePage::new(
                job.fetch_request().clone(),
                11,
                vec![metadata(1, &["\\Flagged"])],
            )
            .unwrap();
            f.sql(&format!("CREATE TRIGGER fail_write BEFORE {event} ON {table} BEGIN SELECT RAISE({action}); END"));
            assert_eq!(
                f.store().complete_uid_reconcile_success([21; 32], &page),
                Err(ErrorCode::InternalError),
                "{table} {action}"
            );
            assert_eq!(f.count("imap_uid_reconcile_messages"), 0);
            assert_eq!(f.receipt(21).state(), UidPageState::Unresolved);
            assert!(
                f.store()
                    .load_audit([21; 32])
                    .unwrap()
                    .unwrap()
                    .completion()
                    .is_none()
            );
            f.sql("DROP TRIGGER fail_write");
            assert!(
                f.store()
                    .complete_uid_reconcile_success([21; 32], &page)
                    .unwrap()
                    .result()
                    .unwrap()
                    .pass_complete()
            );
        }
    }
}
#[test]
fn m204_reconcile_admission_and_failure_are_atomic() {
    for (table, event) in [
        ("imap_uid_reconcile_heads", "INSERT"),
        ("imap_uid_reconcile_requests", "INSERT"),
        ("audit_terminals", "INSERT"),
    ] {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "base");
        f.begin_for(21, 9);
        f.sql(&format!("CREATE TRIGGER fail_write BEFORE {event} ON {table} BEGIN SELECT RAISE(ABORT,'synthetic'); END"));
        assert!(
            f.store()
                .admit_uid_reconcile([21; 32], &request("change", 1), UidQueueAdmission::Full)
                .is_err()
        );
        assert_eq!(f.count("imap_uid_reconcile_heads"), 0);
        assert_eq!(f.count("imap_uid_reconcile_requests"), 0);
        assert!(
            f.store()
                .load_audit([21; 32])
                .unwrap()
                .unwrap()
                .completion()
                .is_none()
        );
    }
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    f.admit(21, &request("change", 1), UidQueueAdmission::Reserved);
    f.start(21);
    f.sql("CREATE TRIGGER fail_write BEFORE INSERT ON audit_terminals BEGIN SELECT RAISE(ABORT,'synthetic'); END");
    assert_eq!(
        f.store()
            .complete_uid_reconcile_failure([21; 32], ErrorCode::Offline),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(f.receipt(21).state(), UidPageState::Unresolved);
}
#[test]
fn m204_reconcile_missing_or_corrupt_latest_overlay_never_falls_back_to_base() {
    for corruption in [
        "DELETE FROM imap_uid_reconcile_messages WHERE uid=1",
        "UPDATE imap_uid_reconcile_messages SET flags_json='[]' WHERE uid=1",
        "UPDATE imap_uid_reconcile_requests SET result_hash=zeroblob(32) WHERE state='succeeded'",
        "UPDATE imap_uid_reconcile_messages SET generation=2 WHERE uid=1",
    ] {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1, 3], "base");
        reconcile(&f, 21, 1, &[(1, &["\\Flagged"])]);
        f.sql(corruption);
        assert_eq!(
            f.store()
                .load_imap_uid_observation("acct", &mailbox(), 7, 1),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            f.store().load_uid_reconcile_receipt(
                [9; 16],
                &UidPageStatusRequest::new("acct", [21; 32]).unwrap()
            ),
            Err(ErrorCode::InternalError)
        );
        f.begin_for(22, 9);
        assert_eq!(
            f.store().admit_uid_reconcile(
                [22; 32],
                &request("next", 1),
                UidQueueAdmission::Reserved
            ),
            Err(ErrorCode::InternalError)
        );
    }
}
#[test]
fn m204_reconcile_current_catalog_cursor_epoch_and_account_are_rechecked() {
    for change in 0..4 {
        let f = Fixture::new();
        append(&f, 0, 0, 200, &[1], "base");
        f.admit(21, &request("change", 1), UidQueueAdmission::Reserved);
        let job = f.start(21);
        match change {
            0 => f.catalog(1),
            1 => append(&f, 1, 64, 200, &[65], "next-base"),
            2 => f.sql("UPDATE client_registrations SET profile_epoch=2"),
            _ => f.sql(
                "UPDATE accounts SET status='reauth_required',settings_version=settings_version+1",
            ),
        }
        let expected = if change == 2 {
            ErrorCode::AuthRequired
        } else {
            ErrorCode::Conflict
        };
        assert_eq!(
            f.store().check_uid_reconcile([21; 32]),
            Err(expected),
            "case {change}"
        );
        let page =
            ImapUidReconcilePage::new(job.fetch_request().clone(), 200, vec![metadata(1, &[])])
                .unwrap();
        assert_eq!(
            f.store()
                .complete_uid_reconcile_success([21; 32], &page)
                .unwrap()
                .error(),
            Some(expected)
        );
        assert_eq!(f.count("imap_uid_reconcile_messages"), 0);
    }
}
fn append_request(op: &str, key: &str, cv: u64) -> hantong_contracts::uid_page::UidPageRequest {
    use hantong_contracts::uid_page::UidPageRequest;
    let r =
        UidPageRequest::new("acct", identity(b"INBOX"), op, key, 1, cv, &"0".repeat(64)).unwrap();
    let hash =
        hantong_contracts::wire_hex::encode(&hantong_platform::sha256(&r.intent_bytes()).unwrap());
    UidPageRequest::new("acct", identity(b"INBOX"), op, key, 1, cv, &hash).unwrap()
}
fn reconcile_request(op: &str, key: &str) -> UidReconcileRequest {
    let r = UidReconcileRequest::new("acct", identity(b"INBOX"), op, key, 1, 1, &"0".repeat(64))
        .unwrap();
    let hash =
        hantong_contracts::wire_hex::encode(&hantong_platform::sha256(&r.intent_bytes()).unwrap());
    UidReconcileRequest::new("acct", identity(b"INBOX"), op, key, 1, 1, &hash).unwrap()
}
#[test]
fn m204_reconcile_reservations_span_direct_append_and_append_jobs_both_directions() {
    use crate::uid_page_jobs::UidPageAdmission;
    for same_key in [false, true] {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "base");
        let (op, key) = if same_key {
            ("other", "base")
        } else {
            ("base", "other")
        };
        assert_eq!(
            f.admit(21, &reconcile_request(op, key), UidQueueAdmission::Reserved),
            UidReconcileAdmission::Rejected(ErrorCode::Conflict)
        );
        f.begin_for(22, 9);
        assert!(matches!(
            f.store()
                .admit_uid_page(
                    [22; 32],
                    &append_request("append-pending", "append-pending", 1),
                    UidQueueAdmission::Reserved
                )
                .unwrap(),
            UidPageAdmission::Admitted(_)
        ));
        let (op, key) = if same_key {
            ("other", "append-pending")
        } else {
            ("append-pending", "other")
        };
        assert_eq!(
            f.admit(23, &reconcile_request(op, key), UidQueueAdmission::Reserved),
            UidReconcileAdmission::Rejected(ErrorCode::Conflict)
        );
        f.admit(
            24,
            &request("reconcile-pending", 1),
            UidQueueAdmission::Reserved,
        );
        let (op, key) = if same_key {
            ("other", "reconcile-pending")
        } else {
            ("reconcile-pending", "other")
        };
        f.begin_for(25, 9);
        assert_eq!(
            f.store()
                .admit_uid_page(
                    [25; 32],
                    &append_request(op, key, 1),
                    UidQueueAdmission::Reserved
                )
                .unwrap(),
            UidPageAdmission::Rejected(ErrorCode::Conflict)
        );
        let page = ImapUidPage::new(
            ImapUidPageRequest::new("acct", 1, mailbox(), Some(7), 10).unwrap(),
            7,
            20,
            vec![metadata(12, &[])],
        )
        .unwrap();
        let m = MailMutation::new(
            op.into(),
            key.into(),
            1,
            imap_uid_page_payload_hash(2, 1, &endpoint(), &page, 1).unwrap(),
        )
        .unwrap();
        assert_eq!(
            f.store().apply_imap_uid_page(2, 1, &endpoint(), &page, &m),
            Err(ErrorCode::Conflict)
        );
    }
}
#[test]
fn m204_reconcile_cannot_consume_original_or_other_job_audit() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    f.admit(21, &request("pending", 1), UidQueueAdmission::Reserved);
    assert_eq!(
        f.store().admit_uid_reconcile(
            [21; 32],
            &request("different", 1),
            UidQueueAdmission::Reserved
        ),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().admit_uid_page(
            [21; 32],
            &append_request("different", "different", 1),
            UidQueueAdmission::Reserved
        ),
        Err(ErrorCode::Conflict)
    );
    assert!(
        f.store()
            .load_audit([21; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
    f.begin_for(22, 9);
    f.store()
        .admit_uid_page(
            [22; 32],
            &append_request("append", "append", 1),
            UidQueueAdmission::Reserved,
        )
        .unwrap();
    assert_eq!(
        f.store().admit_uid_reconcile(
            [22; 32],
            &request("different", 1),
            UidQueueAdmission::Reserved
        ),
        Err(ErrorCode::Conflict)
    );
    assert!(
        f.store()
            .load_audit([22; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
}
#[test]
fn m204_reconcile_owner_and_queue_failure_receipts_remain_bound() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    f.add_registration(10);
    let UidReconcileAdmission::Failed(first) =
        f.admit(21, &request("full", 1), UidQueueAdmission::Full)
    else {
        panic!("failed receipt")
    };
    assert_eq!(first.error(), Some(ErrorCode::RateLimited));
    assert_eq!(
        f.admit(22, &request("full", 1), UidQueueAdmission::Reserved),
        UidReconcileAdmission::Existing(first)
    );
    f.begin_for(23, 10);
    assert_eq!(
        f.store()
            .admit_uid_reconcile([23; 32], &request("full", 1), UidQueueAdmission::Reserved)
            .unwrap(),
        UidReconcileAdmission::Rejected(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().load_uid_reconcile_receipt(
            [10; 16],
            &UidPageStatusRequest::new("acct", [21; 32]).unwrap()
        ),
        Err(ErrorCode::NotFound)
    );
    let UidReconcileAdmission::Failed(closed) =
        f.admit(24, &request("closed", 1), UidQueueAdmission::Closed)
    else {
        panic!("failed receipt")
    };
    assert_eq!(closed.error(), Some(ErrorCode::CoreUnavailable));
    assert_eq!(
        reconcile(&f, 25, 1, &[(1, &["\\Seen"])])
            .result()
            .unwrap()
            .generation(),
        1
    );
}
#[test]
fn m204_reconcile_replay_checks_cross_ledger_collisions_before_returning_history() {
    use crate::uid_page_jobs::UidPageAdmission;
    for direction in 0..3 {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "base");
        f.admit(21, &request("reconcile", 1), UidQueueAdmission::Reserved);
        if direction == 0 {
            f.sql("UPDATE imap_uid_page_operations SET idempotency_key='reconcile' WHERE operation_id='base'");
            assert_eq!(
                f.admit(23, &request("reconcile", 1), UidQueueAdmission::Reserved),
                UidReconcileAdmission::Rejected(ErrorCode::Conflict)
            );
        } else {
            f.begin_for(22, 9);
            f.store()
                .admit_uid_page(
                    [22; 32],
                    &append_request("append", "append", 1),
                    UidQueueAdmission::Reserved,
                )
                .unwrap();
            if direction == 1 {
                f.sql("UPDATE imap_uid_page_requests SET idempotency_key='reconcile' WHERE operation_id='append'");
                assert_eq!(
                    f.admit(23, &request("reconcile", 1), UidQueueAdmission::Reserved),
                    UidReconcileAdmission::Rejected(ErrorCode::Conflict)
                );
            } else {
                f.sql("UPDATE imap_uid_reconcile_requests SET idempotency_key='append' WHERE operation_id='reconcile'");
                f.begin_for(23, 9);
                assert_eq!(
                    f.store()
                        .admit_uid_page(
                            [23; 32],
                            &append_request("append", "append", 1),
                            UidQueueAdmission::Reserved
                        )
                        .unwrap(),
                    UidPageAdmission::Rejected(ErrorCode::Conflict)
                );
            }
        }
    }
}
#[test]
fn m204_reconcile_admission_quota_keeps_existing_job_and_does_not_create_another() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    f.admit(21, &request("pending", 1), UidQueueAdmission::Reserved);
    // Unread synthetic rows exercise only the documented startup/count boundary.
    f.sql("CREATE TEMP TABLE quota_ids AS WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<99999) SELECT x,randomblob(32) AS id FROM n; INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,client_kind,display_label,tool_code,started_at_utc_ms) SELECT q.id,a.registration_id,a.profile_epoch,a.client_kind,a.display_label,a.tool_code,a.started_at_utc_ms FROM audit_starts a CROSS JOIN quota_ids q WHERE a.execution_id=(SELECT execution_id FROM imap_uid_reconcile_requests WHERE operation_id='pending'); INSERT INTO audit_start_accounts SELECT id,'acct' FROM quota_ids;");
    f.store().with_connection(|c|{
        let columns={let mut s=c.prepare("PRAGMA table_info(imap_uid_reconcile_requests)")?;s.query_map([],|r|r.get::<_,String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?};
        let expressions=columns.iter().map(|name|match name.as_str(){"execution_id"=>"q.id".to_owned(),"operation_id"|"idempotency_key"=>"'fill-'||q.x".to_owned(),_=>format!("j.{name}")}).collect::<Vec<_>>().join(",");
        c.execute(&format!("INSERT INTO imap_uid_reconcile_requests SELECT {expressions} FROM imap_uid_reconcile_requests j CROSS JOIN quota_ids q WHERE j.operation_id='pending'"),[])?;
        Ok(())
    }).unwrap();
    assert_eq!(f.count("imap_uid_reconcile_requests"), 100_000);
    assert_eq!(
        f.admit(22, &request("over-quota", 1), UidQueueAdmission::Reserved),
        UidReconcileAdmission::Rejected(ErrorCode::QuotaExceeded)
    );
    assert_eq!(f.count("imap_uid_reconcile_requests"), 100_000);
    assert_eq!(f.receipt(21).state(), UidPageState::Unresolved);
    assert_eq!(f.count("imap_uid_reconcile_messages"), 0);
}
#[test]
fn m204_reconcile_clock_regression_and_wrong_fetch_are_non_mutating() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    f.admit(21, &request("wrong", 1), UidQueueAdmission::Reserved);
    let job = f.start(21);
    let other = ImapUidReconcileRequest::new("acct", 1, mailbox(), 7, 1, 10, 11).unwrap();
    let page = ImapUidReconcilePage::new(other, 11, vec![metadata(3, &[])]).unwrap();
    assert_eq!(
        f.store()
            .complete_uid_reconcile_success([21; 32], &page)
            .unwrap()
            .error(),
        Some(ErrorCode::InvalidInput)
    );
    f.admit(22, &request("clock", 1), UidQueueAdmission::Reserved);
    f.start(22);
    f.sql("UPDATE imap_uid_reconcile_requests SET started_at_utc_ms=started_at_utc_ms+60000 WHERE operation_id='clock'");
    assert_eq!(
        f.store()
            .complete_uid_reconcile_failure([22; 32], ErrorCode::Offline),
        Err(ErrorCode::InternalError)
    );
    let page =
        ImapUidReconcilePage::new(job.fetch_request().clone(), 11, vec![metadata(1, &[])]).unwrap();
    assert_eq!(
        f.store().complete_uid_reconcile_success([22; 32], &page),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(f.count("imap_uid_reconcile_messages"), 0);
}
#[test]
fn m204_reconcile_source_counts_cannot_hide_nonempty_or_missing_children() {
    for corruption in [
        "UPDATE imap_uid_page_operations SET inserted_count=0 WHERE operation_id='append-1'",
        "DELETE FROM imap_uid_messages WHERE operation_id='append-1'",
        "UPDATE imap_uid_page_operations SET after_uid=covered_through_uid,uid_next_at_start=covered_through_uid+1,inserted_count=0 WHERE operation_id='append-1'",
    ] {
        let f = Fixture::new();
        for (cv, after, uid) in [(0, 0, 1), (1, 64, 65), (2, 128, 129), (3, 192, 193)] {
            append(&f, cv, after, 300, &[uid], &format!("append-{cv}"));
        }
        f.sql(corruption);
        f.begin_for(21, 9);
        assert_eq!(
            f.store().admit_uid_reconcile(
                [21; 32],
                &request("first", 4),
                UidQueueAdmission::Reserved
            ),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(f.count("imap_uid_reconcile_heads"), 0);
        assert_eq!(f.count("imap_uid_reconcile_requests"), 0);
        assert!(
            f.store()
                .load_audit([21; 32])
                .unwrap()
                .unwrap()
                .completion()
                .is_none()
        );
    }
}
