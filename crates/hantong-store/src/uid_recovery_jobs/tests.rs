use super::*;
use crate::uid_reconcile_jobs::tests::{
    Fixture, append, endpoint, identity, mailbox, metadata, reconcile,
};
use hantong_contracts::uid_page::UidPageState;
use hantong_domain::imap_sync::{ImapUidPage, ImapUidPageRequest};

include!("atomic_tests.rs");

fn request(id: &str, cv: u64, cg: u64) -> UidRecoveryRequest {
    request_with_key(id, id, cv, cg)
}
fn request_with_key(id: &str, key: &str, cv: u64, cg: u64) -> UidRecoveryRequest {
    let r = UidRecoveryRequest::new("acct", identity(b"INBOX"), id, key, cg, cv, &"0".repeat(64))
        .unwrap();
    let digest =
        hantong_contracts::wire_hex::encode(&hantong_platform::sha256(&r.intent_bytes()).unwrap());
    UidRecoveryRequest::new("acct", identity(b"INBOX"), id, key, cg, cv, &digest).unwrap()
}
fn admit(f: &Fixture, id: u8, cv: u64) -> UidRecoveryReceipt {
    f.begin_for(id, 9);
    let UidRecoveryAdmission::Admitted(r) = f
        .store()
        .admit_uid_recovery(
            [id; 32],
            &request(&format!("recover-{id}"), cv, 1),
            UidQueueAdmission::Reserved,
        )
        .unwrap()
    else {
        panic!("expected admission")
    };
    r
}
fn start(f: &Fixture, id: u8) -> UidRecoveryJob {
    let UidRecoveryStart::Ready(job) = f.store().start_uid_recovery([id; 32]).unwrap() else {
        panic!("expected start")
    };
    *job
}
fn page(job: &UidRecoveryJob, validity: u32, next: u32, uid: Option<u32>) -> ImapUidRecoveryPage {
    let r = job.fetch_request();
    let page = ImapUidPage::new(
        ImapUidPageRequest::new(
            r.account_id(),
            r.endpoint_version(),
            r.mailbox().clone(),
            Some(validity),
            0,
        )
        .unwrap(),
        validity,
        next,
        uid.into_iter()
            .map(|u| metadata(u, &["\\Flagged"]))
            .collect(),
    )
    .unwrap();
    ImapUidRecoveryPage::new(r.clone(), page).unwrap()
}
fn recover(f: &Fixture, id: u8, cv: u64, validity: u32) -> UidRecoveryReceipt {
    admit(f, id, cv);
    let job = start(f, id);
    f.store()
        .complete_uid_recovery_success([id; 32], &page(&job, validity, 11, Some(1)))
        .unwrap()
}
#[test]
fn sync02_initial_append_creates_epoch_and_recovery_keeps_global_cursor_and_old_metadata() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1], "base");
    append(&f, 1, 64, 200, &[65], "second");
    assert_eq!(f.count("imap_uid_epochs"), 1);
    let receipt = recover(&f, 31, 2, 9);
    let r = receipt.result().unwrap();
    assert_eq!(r.epoch_generation(), 2);
    assert_eq!(r.cursor_version(), 3);
    assert_eq!(r.covered_through_uid(), 10);
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
            .load_imap_uid_metadata("acct", &mailbox(), 9, 1)
            .unwrap()
            .unwrap()
            .flags(),
        ["\\flagged"]
    );
    assert!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 7, 2)
            .unwrap()
            .is_none()
    );
    let cursor = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert_eq!(cursor.version(), 3);
    assert_eq!(cursor.uid_validity(), 9);
    assert_eq!(f.count("imap_uid_epochs"), 2);
    assert_eq!(f.count("imap_uid_messages"), 3);
}
#[test]
fn sync02_recovery_preserves_old_reconcile_receipt_overlay_and_starts_new_epoch_pass() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "base");
    let old = reconcile(&f, 21, 1, &[(1, &["\\Answered"])]);
    recover(&f, 31, 1, 9);
    assert_eq!(f.receipt(21), old);
    assert_eq!(
        f.store()
            .load_imap_uid_observation("acct", &mailbox(), 7, 1)
            .unwrap()
            .unwrap()
            .flags()
            .unwrap(),
        ["\\answered"]
    );
    assert!(
        f.store()
            .load_imap_uid_observation("acct", &mailbox(), 7, 3)
            .unwrap()
            .unwrap()
            .remote_absent()
    );
    let new = reconcile(&f, 22, 2, &[(1, &["\\Flagged"])]);
    assert_eq!(new.result().unwrap().generation(), 1);
    assert_eq!(new.result().unwrap().source_cursor_version(), 2);
    assert!(new.result().unwrap().pass_complete());
    assert_eq!(f.count("imap_uid_reconcile_heads"), 2);
}
#[test]
fn sync02_recovery_replay_does_not_restore_retired_namespace() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    let first = recover(&f, 31, 1, 9);
    recover(&f, 32, 2, 11);
    f.begin_for(33, 9);
    assert_eq!(
        f.store()
            .admit_uid_recovery(
                [33; 32],
                &request("recover-31", 1, 1),
                UidQueueAdmission::Full
            )
            .unwrap(),
        UidRecoveryAdmission::Existing(first)
    );
    assert_eq!(
        f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .uid_validity(),
        11
    );
}
#[test]
fn sync02_recovery_rejects_retired_validity_reuse_and_allows_new_empty_epoch() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1], "base");
    recover(&f, 31, 1, 9);
    admit(&f, 32, 2);
    let job = start(&f, 32);
    assert_eq!(
        f.store()
            .complete_uid_recovery_success([32; 32], &page(&job, 7, 11, Some(1)))
            .unwrap()
            .error(),
        Some(ErrorCode::Conflict)
    );
    admit(&f, 33, 2);
    let job = start(&f, 33);
    let r = f
        .store()
        .complete_uid_recovery_success([33; 32], &page(&job, 11, 1, None))
        .unwrap();
    assert_eq!(r.result().unwrap().covered_through_uid(), 0);
    assert_eq!(r.result().unwrap().cursor_version(), 3);
    assert_eq!(f.count("imap_uid_messages"), 2);
}
#[test]
fn sync02_recovery_new_epoch_is_not_an_ordinary_append_validity_bypass() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    let p = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), Some(9), 0).unwrap(),
        9,
        11,
        vec![metadata(1, &[])],
    )
    .unwrap();
    let m = crate::mail_connection::MailMutation::new(
        "bypass".into(),
        "bypass".into(),
        1,
        crate::imap_sync::imap_uid_page_payload_hash(2, 1, &endpoint(), &p, 1).unwrap(),
    )
    .unwrap();
    assert_eq!(
        f.store().apply_imap_uid_page(2, 1, &endpoint(), &p, &m),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(f.count("imap_uid_epochs"), 1);
}
#[test]
fn sync02_recovery_restart_keeps_job_unresolved_without_network_or_transition() {
    let mut f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    admit(&f, 31, 1);
    start(&f, 31);
    f.reopen();
    assert_eq!(
        f.store()
            .load_uid_recovery_receipt(
                [9; 16],
                &UidPageStatusRequest::new("acct", [31; 32]).unwrap()
            )
            .unwrap()
            .state(),
        UidPageState::Unresolved
    );
    assert_eq!(
        f.store().start_uid_recovery([31; 32]),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(f.count("imap_uid_epochs"), 1);
}
#[test]
fn sec03_recovery_review_original_append_audit_cannot_be_reused_with_different_keys() {
    use crate::uid_reconcile_jobs::tests::append_request;
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    f.begin_for(21, 9);
    f.store()
        .admit_uid_page(
            [21; 32],
            &append_request("pending", "pending", 1),
            UidQueueAdmission::Reserved,
        )
        .unwrap();
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
}
#[test]
fn sync02_recovery_review_unknown_epoch_does_not_hide_existing_metadata() {
    let f = Fixture::new();
    for (cv, after, uid) in [(0, 0, 1), (1, 64, 65), (2, 128, 129), (3, 192, 193)] {
        append(&f, cv, after, 300, &[uid], &format!("append-{cv}"));
    }
    f.sql("PRAGMA foreign_keys=OFF; UPDATE imap_uid_messages SET uid_validity=999 WHERE operation_id='append-1'; PRAGMA foreign_keys=ON;");
    assert_eq!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 999, 65),
        Err(ErrorCode::InternalError)
    );
}
fn original_page() -> ImapUidPage {
    ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), None, 0).unwrap(),
        7,
        11,
        vec![metadata(1, &["\\Seen"])],
    )
    .unwrap()
}
#[test]
fn sync02_recovery_review_historical_receipts_validate_the_retired_ancestor_epoch() {
    use crate::uid_reconcile_jobs::tests::append_request;
    for kind in 0..3 {
        let f = Fixture::new();
        if kind == 1 {
            f.begin_for(20, 9);
            f.store()
                .admit_uid_page(
                    [20; 32],
                    &append_request("base", "base", 0),
                    UidQueueAdmission::Reserved,
                )
                .unwrap();
            f.store().start_uid_page([20; 32]).unwrap();
            f.store()
                .complete_uid_page_success([20; 32], &original_page())
                .unwrap();
        } else {
            append(&f, 0, 0, 11, &[1], "base");
        }
        if kind == 2 {
            reconcile(&f, 21, 1, &[(1, &["\\Seen"])]);
        }
        recover(&f, 31, 1, 9);
        recover(&f, 32, 2, 11);
        f.sql(
            "UPDATE imap_uid_epochs SET retired_at_utc_ms=retired_at_utc_ms+1 WHERE uid_validity=7",
        );
        match kind {
            0 => {
                let p = original_page();
                let m = crate::mail_connection::MailMutation::new(
                    "base".into(),
                    "base".into(),
                    0,
                    crate::imap_sync::imap_uid_page_payload_hash(2, 1, &endpoint(), &p, 0).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    f.store().apply_imap_uid_page(2, 1, &endpoint(), &p, &m),
                    Err(ErrorCode::InternalError)
                );
            }
            1 => assert_eq!(
                f.store().load_uid_page_receipt(
                    [9; 16],
                    &UidPageStatusRequest::new("acct", [20; 32]).unwrap()
                ),
                Err(ErrorCode::InternalError)
            ),
            _ => assert_eq!(
                f.store().load_uid_reconcile_receipt(
                    [9; 16],
                    &UidPageStatusRequest::new("acct", [21; 32]).unwrap()
                ),
                Err(ErrorCode::InternalError)
            ),
        }
    }
}
#[test]
fn sync02_recovery_review_orphan_epoch_fails_startup() {
    let mut f = Fixture::new();
    for (cv, after, uid) in [(0, 0, 1), (1, 64, 65), (2, 128, 129), (3, 192, 193)] {
        append(&f, cv, after, 300, &[uid], &format!("append-{cv}"));
    }
    f.sql("INSERT INTO imap_uid_epochs(account_id,mailbox_key,mailbox_encoding,uid_validity,epoch_generation,first_cursor_version,first_operation_id,endpoint_version,endpoint_hash,created_at_utc_ms) SELECT account_id,'4f727068616e',mailbox_encoding,uid_validity,1,1,operation_id,endpoint_version,endpoint_hash,applied_at_utc_ms FROM imap_uid_page_operations WHERE operation_id='append-1'");
    assert_eq!(f.try_reopen(), Err(ErrorCode::InternalError));
}
#[test]
fn sync02_recovery_review_transition_provenance_names_the_actual_previous_epoch() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    recover(&f, 31, 1, 9);
    recover(&f, 32, 2, 11);
    f.sql("UPDATE imap_uid_recovery_requests SET previous_uid_validity=7 WHERE operation_id='recover-32'");
    assert_eq!(
        f.store().load_imap_uid_cursor("acct", &mailbox()),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        f.store().load_imap_uid_metadata("acct", &mailbox(), 11, 1),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        f.store().load_uid_recovery_receipt(
            [9; 16],
            &UidPageStatusRequest::new("acct", [32; 32]).unwrap()
        ),
        Err(ErrorCode::InternalError)
    );
}

fn refresh_catalog(f: &Fixture, av: u64, ep: &ImapEndpointConfig) {
    let old = f
        .store()
        .load_imap_mailbox_snapshot("acct")
        .unwrap()
        .unwrap();
    let snapshot = hantong_domain::mailboxes::ImapMailboxSnapshot::new(
        "acct",
        ep.settings_version(),
        *old.snapshot().capabilities(),
        old.snapshot().mailboxes().to_vec(),
    )
    .unwrap();
    let mutation = crate::mail_connection::MailMutation::new(
        "refresh-current".into(),
        "refresh-current".into(),
        old.generation(),
        crate::imap_catalog::imap_catalog_payload_hash(av, ep, &snapshot, old.generation())
            .unwrap(),
    )
    .unwrap();
    f.store()
        .replace_imap_mailbox_snapshot(av, ep, &snapshot, &mutation)
        .unwrap();
}
#[test]
fn sync02_recovery_accepts_reauthenticated_ready_account_with_current_catalog() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    f.sql("UPDATE accounts SET status='reauth_required',settings_version=3; UPDATE accounts SET status='ready',settings_version=4;");
    refresh_catalog(&f, 4, &endpoint());
    assert!(
        !f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .binding_current()
    );
    f.begin_for(31, 9);
    assert!(matches!(
        f.store()
            .admit_uid_recovery(
                [31; 32],
                &request("fresh-account", 1, 2),
                UidQueueAdmission::Reserved
            )
            .unwrap(),
        UidRecoveryAdmission::Admitted(_)
    ));
    let job = start(&f, 31);
    f.store()
        .complete_uid_recovery_success([31; 32], &page(&job, 9, 11, Some(1)))
        .unwrap();
    let current = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert!(current.binding_current());
    assert_eq!(current.version(), 2);
    assert_eq!(current.uid_validity(), 9);
}
#[test]
fn sync02_recovery_rejects_changed_endpoint_even_with_fresh_catalog() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    let config=hantong_domain::mail_connection::MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":2,"host":"changed.fixture.invalid","port":993,"tls":"implicit_tls","login_name":"synthetic-uid","authentication":"app_password"}}"#).unwrap();
    let ep = config.imap();
    let mutation = crate::mail_connection::MailMutation::new(
        "changed-endpoint".into(),
        "changed-endpoint".into(),
        1,
        crate::mail_connection::imap_payload_hash("acct", ep, 1).unwrap(),
    )
    .unwrap();
    f.store()
        .put_imap_connection("acct", ep, &mutation)
        .unwrap();
    refresh_catalog(&f, 2, ep);
    f.begin_for(31, 9);
    assert_eq!(
        f.store()
            .admit_uid_recovery(
                [31; 32],
                &request("changed-endpoint-recovery", 1, 2),
                UidQueueAdmission::Reserved
            )
            .unwrap(),
        UidRecoveryAdmission::Rejected(ErrorCode::Conflict)
    );
    assert_eq!(f.count("imap_uid_epochs"), 1);
}
#[test]
fn sync02_recovery_rechecks_current_account_catalog_cursor_and_grants() {
    for change in 0..5 {
        let f = Fixture::new();
        append(&f, 0, 0, 200, &[1], "base");
        admit(&f, 31, 1);
        let job = start(&f, 31);
        match change {
            0 => f.catalog(1),
            1 => append(&f, 1, 64, 200, &[65], "next-base"),
            2 => f.sql("UPDATE client_registrations SET profile_epoch=2"),
            3 => f.sql(
                "UPDATE accounts SET status='reauth_required',settings_version=settings_version+1",
            ),
            _ => f.sql("UPDATE accounts SET settings_version=settings_version+1"),
        }
        let expected = if change == 2 {
            ErrorCode::AuthRequired
        } else {
            ErrorCode::Conflict
        };
        assert_eq!(
            f.store().check_uid_recovery([31; 32]),
            Err(expected),
            "case {change}"
        );
        assert_eq!(
            f.store()
                .complete_uid_recovery_success([31; 32], &page(&job, 9, 11, Some(1)))
                .unwrap()
                .error(),
            Some(expected)
        );
        assert_eq!(f.count("imap_uid_epochs"), 1);
    }
}
#[test]
fn sync02_recovery_preserves_different_metadata_for_reused_uid_and_allows_later_append() {
    use hantong_domain::imap_sync::{ImapEnvelope, ImapEnvelopeInput, ImapMessageMetadata};
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    admit(&f, 31, 1);
    let job = start(&f, 31);
    let new = ImapMessageMetadata::new(
        1,
        vec![],
        1767225601000,
        999,
        ImapEnvelope::new(ImapEnvelopeInput {
            subject: Some("Different namespace".into()),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    let newpage = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), Some(9), 0).unwrap(),
        9,
        11,
        vec![new.clone()],
    )
    .unwrap();
    f.store()
        .complete_uid_recovery_success(
            [31; 32],
            &ImapUidRecoveryPage::new(job.fetch_request().clone(), newpage).unwrap(),
        )
        .unwrap();
    assert_eq!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 7, 1)
            .unwrap()
            .unwrap()
            .size(),
        12
    );
    assert_eq!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 9, 1)
            .unwrap()
            .unwrap(),
        new
    );
    let after = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), Some(9), 10).unwrap(),
        9,
        20,
        vec![metadata(11, &[])],
    )
    .unwrap();
    let mutation = crate::mail_connection::MailMutation::new(
        "later-append".into(),
        "later-append".into(),
        2,
        crate::imap_sync::imap_uid_page_payload_hash(2, 1, &endpoint(), &after, 2).unwrap(),
    )
    .unwrap();
    assert_eq!(
        f.store()
            .apply_imap_uid_page(2, 1, &endpoint(), &after, &mutation)
            .unwrap()
            .cursor_version(),
        3
    );
    assert_eq!(f.count("imap_uid_epochs"), 2);
}
#[test]
fn sync02_recovery_blocks_all_old_running_jobs_after_namespace_transition() {
    use crate::uid_reconcile_jobs::tests::{append_request, reconcile_request};
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    f.begin_for(21, 9);
    f.store()
        .admit_uid_page(
            [21; 32],
            &append_request("old-append", "old-append", 1),
            UidQueueAdmission::Reserved,
        )
        .unwrap();
    f.store().start_uid_page([21; 32]).unwrap();
    f.admit(
        22,
        &reconcile_request("old-reconcile", "old-reconcile"),
        UidQueueAdmission::Reserved,
    );
    let reconcile_job = f.start(22);
    admit(&f, 23, 1);
    let recovery_job = start(&f, 23);
    recover(&f, 31, 1, 9);
    assert_eq!(f.store().check_uid_page([21; 32]), Err(ErrorCode::Conflict));
    assert_eq!(
        f.store().check_uid_reconcile([22; 32]),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store().check_uid_recovery([23; 32]),
        Err(ErrorCode::Conflict)
    );
    let append_page = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), Some(7), 10).unwrap(),
        7,
        20,
        vec![metadata(11, &[])],
    )
    .unwrap();
    let reconcile_page = hantong_domain::imap_reconcile::ImapUidReconcilePage::new(
        reconcile_job.fetch_request().clone(),
        11,
        vec![metadata(1, &[])],
    )
    .unwrap();
    assert_eq!(
        f.store()
            .complete_uid_page_success([21; 32], &append_page)
            .unwrap()
            .error(),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store()
            .complete_uid_reconcile_success([22; 32], &reconcile_page)
            .unwrap()
            .error(),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store()
            .complete_uid_recovery_success([23; 32], &page(&recovery_job, 11, 11, Some(1)))
            .unwrap()
            .error(),
        Some(ErrorCode::Conflict)
    );
    assert_eq!(
        f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .version(),
        2
    );
    assert_eq!(f.count("imap_uid_epochs"), 2);
}

#[test]
fn sync02_recovery_review_retired_epoch_validates_its_outgoing_transition() {
    use crate::uid_reconcile_jobs::tests::append_request;
    for kind in 0..3 {
        let f = Fixture::new();
        if kind == 1 {
            f.begin_for(20, 9);
            f.store()
                .admit_uid_page(
                    [20; 32],
                    &append_request("base", "base", 0),
                    UidQueueAdmission::Reserved,
                )
                .unwrap();
            f.store().start_uid_page([20; 32]).unwrap();
            f.store()
                .complete_uid_page_success([20; 32], &original_page())
                .unwrap();
        } else {
            append(&f, 0, 0, 11, &[1], "base");
        }
        if kind == 2 {
            reconcile(&f, 21, 1, &[(1, &["\\Seen"])]);
        }
        recover(&f, 31, 1, 9);
        recover(&f, 32, 2, 11);
        f.sql("UPDATE imap_uid_recovery_requests SET previous_uid_validity=11 WHERE operation_id='recover-31'");
        match kind {
            0 => {
                let p = original_page();
                let m = crate::mail_connection::MailMutation::new(
                    "base".into(),
                    "base".into(),
                    0,
                    crate::imap_sync::imap_uid_page_payload_hash(2, 1, &endpoint(), &p, 0).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    f.store().apply_imap_uid_page(2, 1, &endpoint(), &p, &m),
                    Err(ErrorCode::InternalError)
                );
            }
            1 => assert_eq!(
                f.store().load_uid_page_receipt(
                    [9; 16],
                    &UidPageStatusRequest::new("acct", [20; 32]).unwrap()
                ),
                Err(ErrorCode::InternalError)
            ),
            _ => assert_eq!(
                f.store().load_uid_reconcile_receipt(
                    [9; 16],
                    &UidPageStatusRequest::new("acct", [21; 32]).unwrap()
                ),
                Err(ErrorCode::InternalError)
            ),
        }
    }
}

#[test]
fn sec03_recovery_reserves_operation_and_account_key_in_every_direction() {
    use crate::uid_reconcile_jobs::tests::{append_request, reconcile_request};
    use crate::{uid_page_jobs::UidPageAdmission, uid_reconcile_jobs::UidReconcileAdmission};
    for origin in 0..4 {
        for destination in 0..4 {
            if origin != 3 && destination != 3 {
                continue;
            }
            for same_key in [false, true] {
                let f = Fixture::new();
                append(&f, 0, 0, 11, &[1], "base");
                let (reserved_op, reserved_key) = if origin == 0 {
                    ("base", "base")
                } else {
                    ("reserved-op", "reserved-key")
                };
                if origin > 0 {
                    f.begin_for(21, 9);
                }
                match origin {
                    1 => {
                        f.store()
                            .admit_uid_page(
                                [21; 32],
                                &append_request(reserved_op, reserved_key, 1),
                                UidQueueAdmission::Reserved,
                            )
                            .unwrap();
                    }
                    2 => {
                        f.store()
                            .admit_uid_reconcile(
                                [21; 32],
                                &reconcile_request(reserved_op, reserved_key),
                                UidQueueAdmission::Reserved,
                            )
                            .unwrap();
                    }
                    3 => {
                        f.store()
                            .admit_uid_recovery(
                                [21; 32],
                                &request_with_key(reserved_op, reserved_key, 1, 1),
                                UidQueueAdmission::Reserved,
                            )
                            .unwrap();
                    }
                    _ => {}
                }
                let (op, key) = if same_key {
                    ("different", reserved_key)
                } else {
                    (reserved_op, "different")
                };
                if destination > 0 {
                    f.begin_for(22, 9);
                }
                match destination {
                    0 => {
                        let p = ImapUidPage::new(
                            ImapUidPageRequest::new("acct", 1, mailbox(), Some(7), 10).unwrap(),
                            7,
                            20,
                            vec![metadata(11, &[])],
                        )
                        .unwrap();
                        let m = crate::mail_connection::MailMutation::new(
                            op.into(),
                            key.into(),
                            1,
                            crate::imap_sync::imap_uid_page_payload_hash(2, 1, &endpoint(), &p, 1)
                                .unwrap(),
                        )
                        .unwrap();
                        assert_eq!(
                            f.store().apply_imap_uid_page(2, 1, &endpoint(), &p, &m),
                            Err(ErrorCode::Conflict)
                        );
                    }
                    1 => assert_eq!(
                        f.store()
                            .admit_uid_page(
                                [22; 32],
                                &append_request(op, key, 1),
                                UidQueueAdmission::Reserved
                            )
                            .unwrap(),
                        UidPageAdmission::Rejected(ErrorCode::Conflict)
                    ),
                    2 => assert_eq!(
                        f.store()
                            .admit_uid_reconcile(
                                [22; 32],
                                &reconcile_request(op, key),
                                UidQueueAdmission::Reserved
                            )
                            .unwrap(),
                        UidReconcileAdmission::Rejected(ErrorCode::Conflict)
                    ),
                    _ => assert_eq!(
                        f.store()
                            .admit_uid_recovery(
                                [22; 32],
                                &request_with_key(op, key, 1, 1),
                                UidQueueAdmission::Reserved
                            )
                            .unwrap(),
                        UidRecoveryAdmission::Rejected(ErrorCode::Conflict)
                    ),
                }
                if origin > 0 {
                    assert!(
                        f.store()
                            .load_audit([21; 32])
                            .unwrap()
                            .unwrap()
                            .completion()
                            .is_none()
                    );
                }
                assert_eq!(f.count("imap_uid_page_operations"), 1);
                assert_eq!(f.count("imap_uid_epochs"), 1);
            }
        }
    }
}
#[test]
fn sec03_recovery_cannot_consume_any_original_unresolved_job_audit() {
    use crate::uid_reconcile_jobs::tests::{append_request, reconcile_request};
    for origin in 1..4 {
        for destination in 1..4 {
            if origin != 3 && destination != 3 {
                continue;
            }
            let f = Fixture::new();
            append(&f, 0, 0, 11, &[1], "base");
            f.begin_for(21, 9);
            match origin {
                1 => {
                    f.store()
                        .admit_uid_page(
                            [21; 32],
                            &append_request("pending", "pending", 1),
                            UidQueueAdmission::Reserved,
                        )
                        .unwrap();
                }
                2 => {
                    f.store()
                        .admit_uid_reconcile(
                            [21; 32],
                            &reconcile_request("pending", "pending"),
                            UidQueueAdmission::Reserved,
                        )
                        .unwrap();
                }
                _ => {
                    f.store()
                        .admit_uid_recovery(
                            [21; 32],
                            &request("pending", 1, 1),
                            UidQueueAdmission::Reserved,
                        )
                        .unwrap();
                }
            }
            match destination {
                1 => assert_eq!(
                    f.store().admit_uid_page(
                        [21; 32],
                        &append_request("other", "other", 1),
                        UidQueueAdmission::Reserved
                    ),
                    Err(ErrorCode::Conflict)
                ),
                2 => assert_eq!(
                    f.store().admit_uid_reconcile(
                        [21; 32],
                        &reconcile_request("other", "other"),
                        UidQueueAdmission::Reserved
                    ),
                    Err(ErrorCode::Conflict)
                ),
                _ => assert_eq!(
                    f.store().admit_uid_recovery(
                        [21; 32],
                        &request("other", 1, 1),
                        UidQueueAdmission::Reserved
                    ),
                    Err(ErrorCode::Conflict)
                ),
            }
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
}
#[test]
fn sec03_recovery_cross_ledger_collision_precedes_each_exact_replay() {
    use crate::uid_reconcile_jobs::tests::{append_request, reconcile_request};
    use crate::{uid_page_jobs::UidPageAdmission, uid_reconcile_jobs::UidReconcileAdmission};
    for origin in 0..4 {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "base");
        let op = if origin == 0 { "base" } else { "reserved" };
        if origin > 0 {
            f.begin_for(21, 9);
        }
        match origin {
            1 => {
                f.store()
                    .admit_uid_page(
                        [21; 32],
                        &append_request(op, op, 1),
                        UidQueueAdmission::Reserved,
                    )
                    .unwrap();
            }
            2 => {
                f.store()
                    .admit_uid_reconcile(
                        [21; 32],
                        &reconcile_request(op, op),
                        UidQueueAdmission::Reserved,
                    )
                    .unwrap();
            }
            3 => {
                f.store()
                    .admit_uid_recovery([21; 32], &request(op, 1, 1), UidQueueAdmission::Reserved)
                    .unwrap();
            }
            _ => {}
        }
        f.begin_for(22, 9);
        if origin == 3 {
            f.store()
                .admit_uid_page(
                    [22; 32],
                    &append_request("other", "other", 1),
                    UidQueueAdmission::Reserved,
                )
                .unwrap();
            let collision = append_request(op, op, 1);
            let intent = hantong_platform::sha256(&collision.intent_bytes()).unwrap();
            f.store().with_connection(|c|c.execute("UPDATE imap_uid_page_requests SET operation_id=?1,idempotency_key=?1,intent_hash=?2 WHERE operation_id='other'",rusqlite::params![op,intent.as_slice()]).map(|_|())).unwrap();
        } else {
            f.store()
                .admit_uid_recovery(
                    [22; 32],
                    &request("other", 1, 1),
                    UidQueueAdmission::Reserved,
                )
                .unwrap();
            let collision = request(op, 1, 1);
            let intent = hantong_platform::sha256(&collision.intent_bytes()).unwrap();
            f.store().with_connection(|c|c.execute("UPDATE imap_uid_recovery_requests SET operation_id=?1,idempotency_key=?1,intent_hash=?2 WHERE operation_id='other'",rusqlite::params![op,intent.as_slice()]).map(|_|())).unwrap();
        }
        f.begin_for(23, 9);
        match origin {
            0 => {
                let p = original_page();
                let m = crate::mail_connection::MailMutation::new(
                    "base".into(),
                    "base".into(),
                    0,
                    crate::imap_sync::imap_uid_page_payload_hash(2, 1, &endpoint(), &p, 0).unwrap(),
                )
                .unwrap();
                assert_eq!(
                    f.store().apply_imap_uid_page(2, 1, &endpoint(), &p, &m),
                    Err(ErrorCode::Conflict)
                );
            }
            1 => assert_eq!(
                f.store()
                    .admit_uid_page(
                        [23; 32],
                        &append_request(op, op, 1),
                        UidQueueAdmission::Full
                    )
                    .unwrap(),
                UidPageAdmission::Rejected(ErrorCode::Conflict)
            ),
            2 => assert_eq!(
                f.store()
                    .admit_uid_reconcile(
                        [23; 32],
                        &reconcile_request(op, op),
                        UidQueueAdmission::Full
                    )
                    .unwrap(),
                UidReconcileAdmission::Rejected(ErrorCode::Conflict)
            ),
            _ => assert_eq!(
                f.store()
                    .admit_uid_recovery([23; 32], &request(op, 1, 1), UidQueueAdmission::Full)
                    .unwrap(),
                UidRecoveryAdmission::Rejected(ErrorCode::Conflict)
            ),
        }
    }
}

#[test]
fn sync02_recovery_epoch_quota_keeps_the_old_namespace_and_records_failure() {
    let f = Fixture::new();
    append(&f, 0, 0, 1, &[], "base");
    admit(&f, 31, 1);
    let job = start(&f, 31);
    // Unread bounded payload copies exercise scalar quota limits, not history semantics.
    f.sql("CREATE TEMP TABLE epoch_quota_ids AS WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<99999) SELECT x,lower(hex('quota-'||x)) AS mailbox_key FROM n;");
    f.store().with_connection(|c|{
        let columns={let mut s=c.prepare("PRAGMA table_info(imap_uid_page_operations)")?;s.query_map([],|r|r.get::<_,String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?};
        let expressions=columns.iter().map(|name|match name.as_str(){"operation_id"|"idempotency_key"=>"'quota-'||q.x".to_owned(),"mailbox_key"=>"q.mailbox_key".to_owned(),_=>format!("p.{name}")}).collect::<Vec<_>>().join(",");
        c.execute(&format!("INSERT INTO imap_uid_page_operations SELECT {expressions} FROM imap_uid_page_operations p CROSS JOIN epoch_quota_ids q WHERE p.operation_id='base'"),[])?;
        c.execute("INSERT INTO imap_uid_epochs(account_id,mailbox_key,mailbox_encoding,uid_validity,epoch_generation,first_cursor_version,first_operation_id,endpoint_version,endpoint_hash,created_at_utc_ms) SELECT account_id,mailbox_key,mailbox_encoding,uid_validity,1,1,operation_id,endpoint_version,endpoint_hash,applied_at_utc_ms FROM imap_uid_page_operations WHERE operation_id<>'base'",[])?;
        c.execute("INSERT INTO imap_uid_cursors SELECT p.account_id,p.mailbox_key,p.mailbox_encoding,1,p.account_version,p.catalog_generation,p.endpoint_version,p.endpoint_hash,p.uid_validity,p.uid_next_at_start,p.covered_through_uid,p.applied_at_utc_ms,p.operation_id FROM imap_uid_page_operations p WHERE p.operation_id<>'base'",[])?;
        Ok(())
    }).unwrap();
    assert_eq!(f.count("imap_uid_epochs"), 100_000);
    assert_eq!(
        f.store()
            .with_connection(|c| Ok(crate::uid_epochs::check_quota(c)))
            .unwrap(),
        Err(ErrorCode::QuotaExceeded)
    );
    assert_eq!(
        f.store()
            .complete_uid_recovery_success([31; 32], &page(&job, 9, 11, Some(1)))
            .unwrap()
            .error(),
        Some(ErrorCode::QuotaExceeded)
    );
    assert_eq!(f.count("imap_uid_epochs"), 100_000);
    assert_eq!(f.count("imap_uid_page_operations"), 100_000);
    let cursor = f
        .store()
        .load_imap_uid_cursor("acct", &mailbox())
        .unwrap()
        .unwrap();
    assert_eq!((cursor.uid_validity(), cursor.version()), (7, 1));
}
#[test]
fn sec03_recovery_admission_quota_keeps_existing_job_and_exact_replay() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    let original = admit(&f, 31, 1);
    // Unread synthetic jobs exercise the documented count limit; the original stays strict.
    f.sql("CREATE TEMP TABLE recovery_quota_ids AS WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<99999) SELECT x,randomblob(32) AS id FROM n; INSERT INTO audit_starts(execution_id,registration_id,profile_epoch,client_kind,display_label,tool_code,started_at_utc_ms) SELECT q.id,a.registration_id,a.profile_epoch,a.client_kind,a.display_label,a.tool_code,a.started_at_utc_ms FROM audit_starts a CROSS JOIN recovery_quota_ids q WHERE a.execution_id=(SELECT execution_id FROM imap_uid_recovery_requests WHERE operation_id='recover-31'); INSERT INTO audit_start_accounts SELECT id,'acct' FROM recovery_quota_ids;");
    f.store().with_connection(|c|{
        let columns={let mut s=c.prepare("PRAGMA table_info(imap_uid_recovery_requests)")?;s.query_map([],|r|r.get::<_,String>(1))?.collect::<rusqlite::Result<Vec<_>>>()?};
        let expressions=columns.iter().map(|name|match name.as_str(){"execution_id"=>"q.id".to_owned(),"operation_id"|"idempotency_key"=>"'fill-'||q.x".to_owned(),_=>format!("j.{name}")}).collect::<Vec<_>>().join(",");
        c.execute(&format!("INSERT INTO imap_uid_recovery_requests SELECT {expressions} FROM imap_uid_recovery_requests j CROSS JOIN recovery_quota_ids q WHERE j.operation_id='recover-31'"),[])?;Ok(())
    }).unwrap();
    f.begin_for(32, 9);
    assert_eq!(
        f.store()
            .admit_uid_recovery(
                [32; 32],
                &request("over-quota", 1, 1),
                UidQueueAdmission::Reserved
            )
            .unwrap(),
        UidRecoveryAdmission::Rejected(ErrorCode::QuotaExceeded)
    );
    f.begin_for(33, 9);
    assert_eq!(
        f.store()
            .admit_uid_recovery(
                [33; 32],
                &request("recover-31", 1, 1),
                UidQueueAdmission::Full
            )
            .unwrap(),
        UidRecoveryAdmission::Existing(original)
    );
    assert_eq!(f.count("imap_uid_recovery_requests"), 100_000);
    assert_eq!(f.count("imap_uid_epochs"), 1);
    assert!(
        f.store()
            .load_audit([31; 32])
            .unwrap()
            .unwrap()
            .completion()
            .is_none()
    );
}
