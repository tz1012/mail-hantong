use super::*;
use crate::uid_page_jobs::UidQueueAdmission;
use crate::uid_reconcile_jobs::tests::{
    Fixture, append, append_request, endpoint, identity, mailbox, metadata, reconcile,
};
use hantong_contracts::{uid_inspection::UidObservationSummary, uid_recovery::UidRecoveryRequest};
use hantong_domain::{
    imap_recovery::ImapUidRecoveryPage,
    imap_sync::{ImapUidPage, ImapUidPageRequest},
};

fn request() -> UidCursorRequest {
    UidCursorRequest::new("acct", identity(b"INBOX")).unwrap()
}
type StoredRows = Vec<(String, Vec<Vec<rusqlite::types::Value>>)>;
fn rows(f: &Fixture) -> StoredRows {
    f.store().with_connection(|c| {
        let tables={let mut s=c.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;s.query_map([],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?};
        tables.into_iter().map(|table| {
            let mut s=c.prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))?;
            let width=s.column_count();
            let values=s.query_map([],|r|(0..width).map(|n|r.get(n)).collect::<rusqlite::Result<Vec<rusqlite::types::Value>>>())?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((table,values))
        }).collect()
    }).unwrap()
}
fn inspect(f: &Fixture, r: &UidCursorRequest) -> Result<UidInspectionSnapshot, ErrorCode> {
    let before = rows(f);
    let result = f.store().inspect_imap_uid(r);
    assert_eq!(rows(f), before, "inspection changed stored data");
    result
}
fn summary(snapshot: &UidInspectionSnapshot) -> &UidObservationSummary {
    snapshot.observation().expect("observed cursor")
}

#[test]
fn sync02_inspection_distinguishes_unobserved_pending_job_and_observed_empty_mailbox() {
    let f = Fixture::new();
    let unobserved = inspect(&f, &request()).unwrap();
    assert_eq!(unobserved.account_id(), "acct");
    assert_eq!(unobserved.mailbox(), request().mailbox());
    assert_eq!(unobserved.catalog_generation(), 1);
    assert!(unobserved.binding_current());
    assert!(unobserved.observation().is_none());
    f.begin_for(21, 9);
    f.store()
        .admit_uid_page(
            [21; 32],
            &append_request("pending", "pending", 0),
            UidQueueAdmission::Reserved,
        )
        .unwrap();
    assert_eq!(inspect(&f, &request()).unwrap(), unobserved);
    append(&f, 0, 0, 1, &[], "empty");
    let observed = inspect(&f, &request()).unwrap();
    let s = summary(&observed);
    assert_eq!((s.cursor_version(), s.epoch_generation()), (1, 1));
    assert_eq!((s.covered_through_uid(), s.observed_uid_ceiling()), (0, 0));
    assert!(s.coverage_at_observed_ceiling());
    assert!(s.stored_at_utc_ms() > 0);
}
#[test]
fn sync02_inspection_reports_sparse_uid_range_and_newly_observed_ceiling() {
    let f = Fixture::new();
    append(&f, 0, 0, 65, &[1, 64], "first");
    let first = inspect(&f, &request()).unwrap();
    let s = summary(&first);
    assert_eq!(
        (s.covered_through_uid(), s.observed_uid_ceiling()),
        (64, 64)
    );
    assert!(s.coverage_at_observed_ceiling());
    append(&f, 1, 64, 201, &[65, 128], "second");
    let second = inspect(&f, &request()).unwrap();
    let s = summary(&second);
    assert_eq!((s.cursor_version(), s.epoch_generation()), (2, 1));
    assert_eq!(
        (s.covered_through_uid(), s.observed_uid_ceiling()),
        (128, 200)
    );
    assert!(!s.coverage_at_observed_ceiling());
    assert_eq!(
        s.stored_at_utc_ms(),
        f.store()
            .load_imap_uid_cursor("acct", &mailbox())
            .unwrap()
            .unwrap()
            .stored_at_utc_ms()
    );
    assert_eq!(f.count("imap_uid_messages"), 4);
}
#[test]
fn sync02_inspection_reconcile_does_not_advance_page_observation() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1, 3], "base");
    let before = inspect(&f, &request()).unwrap();
    reconcile(&f, 21, 1, &[(1, &["\\Answered"])]);
    assert_eq!(inspect(&f, &request()).unwrap(), before);
}
#[test]
fn sync02_inspection_uses_recovered_epoch_without_removing_old_metadata() {
    let f = Fixture::new();
    append(&f, 0, 0, 200, &[1], "base");
    f.begin_for(31, 9);
    let initial = UidRecoveryRequest::new(
        "acct",
        identity(b"INBOX"),
        "recover",
        "recover",
        1,
        1,
        &"0".repeat(64),
    )
    .unwrap();
    let hash = hantong_contracts::wire_hex::encode(
        &hantong_platform::sha256(&initial.intent_bytes()).unwrap(),
    );
    let recovery = UidRecoveryRequest::new(
        "acct",
        identity(b"INBOX"),
        "recover",
        "recover",
        1,
        1,
        &hash,
    )
    .unwrap();
    f.store()
        .admit_uid_recovery([31; 32], &recovery, UidQueueAdmission::Reserved)
        .unwrap();
    let crate::uid_recovery_jobs::UidRecoveryStart::Ready(job) =
        f.store().start_uid_recovery([31; 32]).unwrap()
    else {
        panic!("expected recovery")
    };
    let p = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 1, mailbox(), Some(9), 0).unwrap(),
        9,
        11,
        vec![metadata(1, &["\\Flagged"])],
    )
    .unwrap();
    f.store()
        .complete_uid_recovery_success(
            [31; 32],
            &ImapUidRecoveryPage::new(job.fetch_request().clone(), p).unwrap(),
        )
        .unwrap();
    let result = inspect(&f, &request()).unwrap();
    let s = summary(&result);
    assert_eq!((s.cursor_version(), s.epoch_generation()), (2, 2));
    assert_eq!(
        (s.covered_through_uid(), s.observed_uid_ceiling()),
        (10, 10)
    );
    assert!(s.coverage_at_observed_ceiling());
    assert_eq!(
        f.store()
            .load_imap_uid_metadata("acct", &mailbox(), 7, 1)
            .unwrap()
            .unwrap()
            .flags(),
        ["\\seen"]
    );
    assert_eq!(f.count("imap_uid_epochs"), 2);
}
#[test]
fn sync02_inspection_current_catalog_generation_does_not_change_cursor_binding() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    let before = inspect(&f, &request()).unwrap();
    f.catalog(1);
    let after = inspect(&f, &request()).unwrap();
    assert_eq!(after.catalog_generation(), 2);
    assert!(after.binding_current());
    assert_eq!(after.observation(), before.observation());
}

#[test]
fn sync02_inspection_fresh_catalog_keeps_historical_cursor_binding_false() {
    let f = Fixture::new();
    append(&f, 0, 0, 11, &[1], "base");
    let before = inspect(&f, &request()).unwrap();
    f.sql("UPDATE accounts SET settings_version=3");
    let snapshot = f
        .store()
        .load_imap_mailbox_snapshot("acct")
        .unwrap()
        .unwrap()
        .snapshot()
        .clone();
    let mutation = crate::mail_connection::MailMutation::new(
        "fresh-catalog".into(),
        "fresh-catalog".into(),
        1,
        crate::imap_catalog::imap_catalog_payload_hash(3, &endpoint(), &snapshot, 1).unwrap(),
    )
    .unwrap();
    f.store()
        .replace_imap_mailbox_snapshot(3, &endpoint(), &snapshot, &mutation)
        .unwrap();
    assert!(
        f.store()
            .load_imap_mailbox_snapshot("acct")
            .unwrap()
            .unwrap()
            .binding_current()
    );
    let result = inspect(&f, &request()).unwrap();
    assert_eq!(result.catalog_generation(), 2);
    assert!(!result.binding_current());
    assert_eq!(result.observation(), before.observation());
}
#[test]
fn sync02_inspection_stale_account_or_endpoint_reports_false_without_repair() {
    for observed in [false, true] {
        for endpoint_change in [false, true] {
            let f = Fixture::new();
            if observed {
                append(&f, 0, 0, 11, &[1], "base");
            }
            if endpoint_change {
                let config=hantong_domain::mail_connection::MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"acct","imap":{"settings_version":2,"host":"changed.fixture.invalid","port":993,"tls":"implicit_tls","login_name":"synthetic-uid","authentication":"app_password"}}"#).unwrap();
                let m = crate::mail_connection::MailMutation::new(
                    "changed".into(),
                    "changed".into(),
                    1,
                    crate::mail_connection::imap_payload_hash("acct", config.imap(), 1).unwrap(),
                )
                .unwrap();
                f.store()
                    .put_imap_connection("acct", config.imap(), &m)
                    .unwrap();
            } else {
                f.sql("UPDATE accounts SET settings_version=settings_version+1,status='reauth_required'");
            }
            let result = inspect(&f, &request()).unwrap();
            assert!(!result.binding_current());
            assert_eq!(result.observation().is_some(), observed);
        }
    }
}
#[test]
fn sync02_inspection_requires_exact_selectable_catalog_mailbox_and_canonical_input() {
    let f = Fixture::new();
    assert_eq!(
        inspect(
            &f,
            &UidCursorRequest::new("missing", identity(b"INBOX")).unwrap()
        ),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        inspect(
            &f,
            &UidCursorRequest::new("acct", identity(b"Archive")).unwrap()
        ),
        Err(ErrorCode::NotFound)
    );
    assert_eq!(
        inspect(
            &f,
            &UidCursorRequest::new("acct", identity(b"inbox")).unwrap()
        ),
        Err(ErrorCode::InvalidInput)
    );
    let utf8 = hantong_contracts::uid_page::UidMailboxIdentity::new(
        "494e424f58",
        hantong_contracts::uid_page::UidMailboxEncoding::Utf8,
    )
    .unwrap();
    assert_eq!(
        inspect(&f, &UidCursorRequest::new("acct", utf8).unwrap()),
        Err(ErrorCode::NotFound)
    );
    let snapshot = hantong_domain::mailboxes::ImapMailboxSnapshot::new(
        "acct",
        1,
        hantong_domain::capabilities::ImapCapabilities::from_features([]),
        vec![
            hantong_domain::mailboxes::ImapMailboxDescriptor::new(
                mailbox(),
                Some('/'),
                true,
                false,
                hantong_domain::mailboxes::MailboxChildren::Unknown,
                vec![],
            )
            .unwrap(),
        ],
    )
    .unwrap();
    let m = crate::mail_connection::MailMutation::new(
        "nonselectable".into(),
        "nonselectable".into(),
        1,
        crate::imap_catalog::imap_catalog_payload_hash(2, &endpoint(), &snapshot, 1).unwrap(),
    )
    .unwrap();
    f.store()
        .replace_imap_mailbox_snapshot(2, &endpoint(), &snapshot, &m)
        .unwrap();
    assert_eq!(inspect(&f, &request()), Err(ErrorCode::NotFound));
    f.sql("DELETE FROM imap_mailbox_catalogs");
    assert_eq!(inspect(&f, &request()), Err(ErrorCode::NotFound));
}
#[test]
fn sync02_inspection_rejects_corrupt_catalog_cursor_epoch_and_operation() {
    for corruption in [
        "UPDATE imap_mailbox_catalogs SET snapshot_hash=zeroblob(32)",
        "UPDATE imap_uid_cursors SET covered_through_uid=covered_through_uid-1",
        "DELETE FROM imap_uid_epochs",
        "UPDATE imap_uid_page_operations SET page_hash=zeroblob(32)",
    ] {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "base");
        f.sql(corruption);
        assert_eq!(
            inspect(&f, &request()),
            Err(ErrorCode::InternalError),
            "{corruption}"
        );
    }
}
#[test]
fn sync02_inspection_missing_cursor_cannot_hide_any_persisted_uid_history() {
    for remaining in [
        "imap_uid_epochs",
        "imap_uid_page_operations",
        "imap_uid_messages",
    ] {
        let f = Fixture::new();
        append(&f, 0, 0, 11, &[1], "base");
        f.sql("PRAGMA foreign_keys=OFF; DELETE FROM imap_uid_cursors;");
        for table in [
            "imap_uid_epochs",
            "imap_uid_page_operations",
            "imap_uid_messages",
        ] {
            if table != remaining {
                f.sql(&format!("DELETE FROM {table}"));
            }
        }
        f.sql("PRAGMA foreign_keys=ON;");
        assert_eq!(
            inspect(&f, &request()),
            Err(ErrorCode::InternalError),
            "{remaining}"
        );
    }
}
