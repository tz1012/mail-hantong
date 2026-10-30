use crate::mail_connection::MailMutation;
use crate::uid_reconcile_jobs::tests::{Fixture, append, endpoint, mailbox, metadata};
use hantong_contracts::ErrorCode;
use hantong_domain::imap_sync::{ImapUidPage, ImapUidPageRequest};

fn page_and_mutation() -> (ImapUidPage, MailMutation) {
    let page = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), None, 0).unwrap(),
        7,
        11,
        vec![metadata(1, &["\\Seen"]), metadata(3, &[])],
    )
    .unwrap();
    let mutation = MailMutation::new(
        "base".into(),
        "base".into(),
        0,
        crate::imap_sync::imap_uid_page_payload_hash(2, 1, &endpoint(), &page, 0).unwrap(),
    )
    .unwrap();
    (page, mutation)
}

#[test]
fn mail03_identity_initial_append_creates_one_mapping_per_base_message() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "base");
    assert_eq!(f.count("imap_message_identities"), 2);
}

#[test]
fn mail03_identity_direct_replay_rejects_missing_mapping_without_repair() {
    let f = Fixture::new();
    let (page, mutation) = page_and_mutation();
    f.store()
        .apply_imap_uid_page(2, 1, &endpoint(), &page, &mutation)
        .unwrap();
    f.sql("DELETE FROM imap_message_identities WHERE uid=1");
    assert_eq!(
        f.store()
            .apply_imap_uid_page(2, 1, &endpoint(), &page, &mutation),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(f.count("imap_message_identities"), 1);
}

#[test]
fn mail03_identity_mapping_insert_failure_rolls_back_append() {
    for action in ["ABORT,'synthetic'", "IGNORE"] {
        let f = Fixture::new();
        f.sql(&format!("CREATE TRIGGER fail_identity BEFORE INSERT ON imap_message_identities WHEN NEW.uid=3 BEGIN SELECT RAISE({action}); END"));
        let (page, mutation) = page_and_mutation();
        assert_eq!(
            f.store()
                .apply_imap_uid_page(2, 1, &endpoint(), &page, &mutation),
            Err(ErrorCode::InternalError)
        );
        for table in [
            "imap_uid_page_operations",
            "imap_uid_cursors",
            "imap_uid_epochs",
            "imap_uid_messages",
            "imap_message_identities",
        ] {
            assert_eq!(f.count(table), 0, "{table}");
        }
    }
}

#[test]
fn mail03_identity_startup_rejects_missing_mapping() {
    let mut f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "base");
    f.sql("DELETE FROM imap_message_identities WHERE uid=1");
    assert_eq!(f.try_reopen(), Err(ErrorCode::InternalError));
}

#[test]
fn mail03_identity_hash_vectors_bind_only_canonical_immutable_location() {
    use hantong_domain::mailboxes::{ImapMailboxName, MailboxEncoding};
    for (account, wire, encoding, validity, uid, expected) in [
        (
            "acct",
            b"INBOX".as_slice(),
            MailboxEncoding::ModifiedUtf7,
            7,
            1,
            "msg_83e8ba435b9a9f4c002bff02c9d29253af130b0115cfe71e56ceb9935354c283",
        ),
        (
            "acct",
            b"Archive".as_slice(),
            MailboxEncoding::ModifiedUtf7,
            7,
            1,
            "msg_9c9236f63216bc0b0f2414a3b26f0d405ff77f95836126de1bb9324fdbbe1e82",
        ),
        (
            "acct",
            b"archive".as_slice(),
            MailboxEncoding::ModifiedUtf7,
            7,
            1,
            "msg_3d0c647e17cf6207ad965a99f360844ab95ab97fea4dddd416e9de7a2f614c9c",
        ),
        (
            "other",
            b"INBOX".as_slice(),
            MailboxEncoding::Utf8,
            9,
            u32::MAX - 1,
            "msg_e4b80eac3e8be2937964bfac905ec757810da12f407dd0d8a5a0f700e0c9dac4",
        ),
    ] {
        let name = ImapMailboxName::from_wire(wire, encoding).unwrap();
        assert_eq!(
            super::location_id(account, &name, validity, uid).unwrap(),
            expected
        );
    }
    assert!(super::stored_name("", "modified_utf7").is_err());
    assert!(super::stored_name("696e626f78", "modified_utf7").is_err());
    assert!(super::location_id("acct", &mailbox(), 0, 1).is_err());
}

#[test]
fn mail03_identity_reconcile_preserves_ids_and_never_repairs_mapping() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "base");
    let original = f
        .store()
        .with_connection(|c| {
            let mut s =
                c.prepare("SELECT internal_message_id FROM imap_message_identities ORDER BY uid")?;
            s.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap();
    f.sql("CREATE TRIGGER no_mapping_insert BEFORE INSERT ON imap_message_identities BEGIN SELECT RAISE(ABORT,'read-only'); END; CREATE TRIGGER no_mapping_update BEFORE UPDATE ON imap_message_identities BEGIN SELECT RAISE(ABORT,'read-only'); END;");
    crate::uid_reconcile_jobs::tests::reconcile(&f, 21, 1, &[(1, &["\\Flagged"])]);
    let after = f
        .store()
        .with_connection(|c| {
            let mut s =
                c.prepare("SELECT internal_message_id FROM imap_message_identities ORDER BY uid")?;
            s.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap();
    assert_eq!(original, after);
}

#[test]
fn mail03_identity_append_job_success_receipt_refuses_deleted_mapping() {
    use crate::uid_page_jobs::{UidPageStart, UidQueueAdmission};
    let f = Fixture::new();
    f.begin_for(21, 9);
    let request = crate::uid_reconcile_jobs::tests::append_request("job", "job", 0);
    f.store()
        .admit_uid_page([21; 32], &request, UidQueueAdmission::Reserved)
        .unwrap();
    let UidPageStart::Ready(job) = f.store().start_uid_page([21; 32]).unwrap() else {
        panic!("ready")
    };
    let page =
        ImapUidPage::new(job.fetch_request().clone(), 7, 11, vec![metadata(1, &[])]).unwrap();
    let receipt = f
        .store()
        .complete_uid_page_success([21; 32], &page)
        .unwrap();
    assert!(receipt.error().is_none());
    assert_eq!(f.count("imap_message_identities"), 1);
    f.sql("DELETE FROM imap_message_identities");
    assert_eq!(
        f.store().load_uid_page_receipt(
            [9; 16],
            &hantong_contracts::uid_page::UidPageStatusRequest::new("acct", [21; 32]).unwrap()
        ),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(f.count("imap_message_identities"), 0);
}

#[test]
fn mail03_identity_reconcile_replay_refuses_deleted_mapping() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1, 3], "base");
    crate::uid_reconcile_jobs::tests::reconcile(&f, 21, 1, &[(1, &[])]);
    f.sql("DELETE FROM imap_message_identities WHERE uid=1");
    assert_eq!(
        f.store().load_uid_reconcile_receipt(
            [9; 16],
            &hantong_contracts::uid_page::UidPageStatusRequest::new("acct", [21; 32]).unwrap()
        ),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(f.count("imap_message_identities"), 1);
}

type Rows = Vec<(String, Vec<Vec<rusqlite::types::Value>>)>;
fn rows(f: &Fixture) -> Rows {
    f.store().with_connection(|c|{
        let tables={let mut s=c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;s.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?};
        tables.into_iter().map(|table|{
            let mut s=c.prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))?; let width=s.column_count();
            let values=s.query_map([],|r|(0..width).map(|n|r.get(n)).collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>())?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((table,values))
        }).collect()
    }).unwrap()
}

#[test]
fn mail03_identity_job_and_recovery_mapping_faults_roll_back_every_row() {
    use crate::{
        uid_page_jobs::{UidPageStart, UidQueueAdmission},
        uid_recovery_jobs::UidRecoveryStart,
    };
    use hantong_contracts::uid_recovery::UidRecoveryRequest;
    use hantong_domain::imap_recovery::ImapUidRecoveryPage;
    for recovery in [false, true] {
        for action in [
            "BEFORE INSERT ON imap_message_identities WHEN NEW.uid=3 BEGIN SELECT RAISE(ABORT,'synthetic'); END",
            "BEFORE INSERT ON imap_message_identities WHEN NEW.uid=3 BEGIN SELECT RAISE(IGNORE); END",
            "AFTER INSERT ON imap_message_identities WHEN NEW.uid=3 BEGIN UPDATE imap_message_identities SET internal_message_id='msg_'||printf('%064d',0) WHERE uid=3 AND uid_validity=NEW.uid_validity; END",
        ] {
            let f = Fixture::new();
            let recovery_job = if recovery {
                append(&f, 0, 0, 11, &[1], "base");
                crate::uid_reconcile_jobs::tests::reconcile(&f, 21, 1, &[(1, &["\\Flagged"])]);
                f.begin_for(31, 9);
                let raw = UidRecoveryRequest::new(
                    "acct",
                    crate::uid_reconcile_jobs::tests::identity(b"INBOX"),
                    "recovery",
                    "recovery",
                    1,
                    1,
                    &"0".repeat(64),
                )
                .unwrap();
                let hash = hantong_contracts::wire_hex::encode(
                    &hantong_platform::sha256(&raw.intent_bytes()).unwrap(),
                );
                let r = UidRecoveryRequest::new(
                    "acct",
                    raw.mailbox().clone(),
                    "recovery",
                    "recovery",
                    1,
                    1,
                    &hash,
                )
                .unwrap();
                f.store()
                    .admit_uid_recovery([31; 32], &r, UidQueueAdmission::Reserved)
                    .unwrap();
                let UidRecoveryStart::Ready(job) = f.store().start_uid_recovery([31; 32]).unwrap()
                else {
                    panic!("ready")
                };
                Some(job)
            } else {
                None
            };
            let p = if recovery {
                ImapUidPage::new(
                    ImapUidPageRequest::new("acct", 1, mailbox(), Some(9), 0).unwrap(),
                    9,
                    11,
                    vec![metadata(1, &[]), metadata(3, &[])],
                )
                .unwrap()
            } else {
                f.begin_for(31, 9);
                f.store()
                    .admit_uid_page(
                        [31; 32],
                        &crate::uid_reconcile_jobs::tests::append_request("job", "job", 0),
                        UidQueueAdmission::Reserved,
                    )
                    .unwrap();
                let UidPageStart::Ready(job) = f.store().start_uid_page([31; 32]).unwrap() else {
                    panic!("ready")
                };
                ImapUidPage::new(
                    job.fetch_request().clone(),
                    7,
                    11,
                    vec![metadata(1, &[]), metadata(3, &[])],
                )
                .unwrap()
            };
            f.sql(&format!("CREATE TRIGGER mapping_fault {action}"));
            let before = rows(&f);
            if let Some(job) = &recovery_job {
                assert_eq!(
                    f.store().complete_uid_recovery_success(
                        [31; 32],
                        &ImapUidRecoveryPage::new(job.fetch_request().clone(), p.clone()).unwrap()
                    ),
                    Err(ErrorCode::InternalError)
                );
            } else {
                assert_eq!(
                    f.store().complete_uid_page_success([31; 32], &p),
                    Err(ErrorCode::InternalError)
                );
            }
            assert_eq!(rows(&f), before, "{action}");
            assert!(
                f.store()
                    .load_audit([31; 32])
                    .unwrap()
                    .unwrap()
                    .completion()
                    .is_none()
            );
            f.sql("DROP TRIGGER mapping_fault");
            if let Some(job) = recovery_job {
                assert!(
                    f.store()
                        .complete_uid_recovery_success(
                            [31; 32],
                            &ImapUidRecoveryPage::new(job.fetch_request().clone(), p).unwrap()
                        )
                        .unwrap()
                        .error()
                        .is_none()
                );
            } else {
                assert!(
                    f.store()
                        .complete_uid_page_success([31; 32], &p)
                        .unwrap()
                        .error()
                        .is_none()
                );
            }
        }
    }
}
