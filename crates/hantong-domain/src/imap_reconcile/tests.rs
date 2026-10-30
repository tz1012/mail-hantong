use super::*;
use crate::{
    imap_sync::{ImapEnvelope, ImapEnvelopeInput, ImapMessageMetadata, ImapSyncError},
    mailboxes::{ImapMailboxName, MailboxEncoding},
};

fn mailbox() -> ImapMailboxName {
    ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap()
}
fn request(after: u32, through: u32, minimum: u32) -> ImapUidReconcileRequest {
    ImapUidReconcileRequest::new("acct", 7, mailbox(), 9, after, through, minimum).unwrap()
}
fn message(uid: u32) -> ImapMessageMetadata {
    ImapMessageMetadata::new(
        uid,
        vec!["\\Seen".into()],
        0,
        42,
        ImapEnvelope::new(ImapEnvelopeInput {
            subject: Some(vec![0, 255]),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn ext04_reconcile_request_rejects_empty_overflow_wide_and_unbound_ranges() {
    for (after, through, minimum) in [
        (0, 0, 1),
        (10, 9, 11),
        (0, 65, 66),
        (0, 10, 10),
        (u32::MAX - 1, u32::MAX, u32::MAX),
    ] {
        assert!(
            ImapUidReconcileRequest::new("acct", 7, mailbox(), 9, after, through, minimum).is_err()
        );
    }
    for (account, endpoint, validity) in [("", 7, 9), ("acct", 0, 9), ("acct", 7, 0)] {
        assert!(
            ImapUidReconcileRequest::new(account, endpoint, mailbox(), validity, 0, 10, 11)
                .is_err()
        );
    }
    let edge = request(u32::MAX - 65, u32::MAX - 1, u32::MAX);
    assert_eq!(edge.through_uid() - edge.after_uid(), 64);
}

#[test]
fn sync02_reconcile_page_keeps_tail_fixed_when_uidnext_grows() {
    let req = request(0, 10, 11);
    let page = ImapUidReconcilePage::new(req.clone(), 100, vec![message(10), message(1)]).unwrap();
    assert_eq!(page.request(), &req);
    assert_eq!(page.uid_next_at_start(), 100);
    assert_eq!(
        page.messages().iter().map(|m| m.uid()).collect::<Vec<_>>(),
        [1, 10]
    );
    assert!(ImapUidReconcilePage::new(req.clone(), 100, vec![message(11)]).is_err());
    assert_eq!(
        ImapUidReconcilePage::new(req, 10, vec![]),
        Err(ImapSyncError::CursorInvalid)
    );
}

#[test]
fn sync02_reconcile_page_allows_absence_but_rejects_duplicates_and_outside_uids() {
    assert!(ImapUidReconcilePage::new(request(64, 128, 1000), 1000, vec![]).is_ok());
    assert_eq!(
        ImapUidReconcilePage::new(request(64, 128, 1000), 999, vec![]),
        Err(ImapSyncError::CursorInvalid)
    );
    for messages in [
        vec![message(65), message(65)],
        vec![message(64)],
        vec![message(129)],
        vec![message(65); 65],
    ] {
        assert!(ImapUidReconcilePage::new(request(64, 128, 1000), 1000, messages).is_err());
    }
}

#[test]
fn ext04_reconcile_storage_is_canonical_and_keeps_metadata_codec_bytes() {
    let original = message(1);
    let page = ImapUidReconcilePage::new(request(0, 10, 11), 100, vec![original.clone()]).unwrap();
    let bytes = page.to_storage_json().unwrap();
    assert_eq!(
        ImapUidReconcilePage::from_storage_json(&bytes).unwrap(),
        page
    );
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains(&String::from_utf8(original.to_storage_json().unwrap()).unwrap()));
    for changed in [
        format!(" {text}"),
        text.replacen("\"schema_version\":1", "\"schema_version\":2", 1),
        text.replacen(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
            1,
        ),
        text.replacen(
            "\"schema_version\":1",
            "\"schema_version\":1,\"extra\":true",
            1,
        ),
        text.replace("00ff", "00FF"),
        text.replace("\\\\seen", "\\\\Seen"),
        text.replace("\"through_uid\":10", "\"through_uid\":0"),
    ] {
        assert!(ImapUidReconcilePage::from_storage_json(changed.as_bytes()).is_err());
    }
    assert_eq!(
        ImapUidReconcilePage::from_storage_json(&vec![b' '; 8 * 1024 * 1024 + 1]),
        Err(ImapSyncError::TooLarge)
    );
    assert!(!format!("{page:?}").contains("acct"));
    assert!(!format!("{:?}", page.request()).contains("INBOX"));
}
