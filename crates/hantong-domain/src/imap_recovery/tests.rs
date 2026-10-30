use super::*;
use crate::{
    imap_sync::{
        ImapEnvelope, ImapEnvelopeInput, ImapMessageMetadata, ImapUidPage, ImapUidPageRequest,
    },
    mailboxes::{ImapMailboxName, MailboxEncoding},
};
fn mailbox() -> ImapMailboxName {
    ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap()
}
fn request() -> ImapUidRecoveryRequest {
    ImapUidRecoveryRequest::new("acct", 7, mailbox(), 9).unwrap()
}
fn page(
    account: &str,
    endpoint: u64,
    mailbox: ImapMailboxName,
    expected: Option<u32>,
    after: u32,
    validity: u32,
) -> ImapUidPage {
    ImapUidPage::new(
        ImapUidPageRequest::new(account, endpoint, mailbox, expected, after).unwrap(),
        validity,
        100,
        vec![],
    )
    .unwrap()
}
#[test]
fn ext04_recovery_request_rejects_invalid_identity_and_redacts() {
    for (account, endpoint, validity) in [
        ("", 7, 9),
        ("a/b", 7, 9),
        ("acct", 0, 9),
        ("acct", u64::MAX, 9),
        ("acct", 7, 0),
    ] {
        assert!(ImapUidRecoveryRequest::new(account, endpoint, mailbox(), validity).is_err());
    }
    let request = request();
    assert_eq!(request.previous_uid_validity(), 9);
    assert!(!format!("{request:?}").contains("acct"));
    assert!(!format!("{request:?}").contains("INBOX"));
}
#[test]
fn sync02_recovery_wrapper_requires_new_epoch_and_exact_first_page_binding() {
    for p in [
        page("acct", 7, mailbox(), Some(9), 0, 9),
        page("other", 7, mailbox(), Some(10), 0, 10),
        page("acct", 8, mailbox(), Some(10), 0, 10),
        page(
            "acct",
            7,
            ImapMailboxName::from_wire(b"Other", MailboxEncoding::ModifiedUtf7).unwrap(),
            Some(10),
            0,
            10,
        ),
        page(
            "acct",
            7,
            ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::Utf8).unwrap(),
            Some(10),
            0,
            10,
        ),
        page("acct", 7, mailbox(), None, 0, 10),
        page("acct", 7, mailbox(), Some(10), 1, 10),
    ] {
        assert!(ImapUidRecoveryPage::new(request(), p).is_err());
    }
}
#[test]
fn sync02_recovery_new_namespace_retains_existing_codec_and_allows_reused_uid_number() {
    let old = ImapMessageMetadata::new(
        1,
        vec![],
        0,
        1,
        ImapEnvelope::new(ImapEnvelopeInput {
            subject: Some(b"old".to_vec()),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    let new = ImapMessageMetadata::new(
        1,
        vec![],
        0,
        2,
        ImapEnvelope::new(ImapEnvelopeInput {
            subject: Some(b"new".to_vec()),
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(old.uid(), new.uid());
    assert_ne!(old, new);
    let p = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 7, mailbox(), Some(10), 0).unwrap(),
        10,
        2,
        vec![new],
    )
    .unwrap();
    let bytes = p.to_storage_json().unwrap();
    let recovery = ImapUidRecoveryPage::new(request(), p).unwrap();
    assert_eq!(recovery.page().to_storage_json().unwrap(), bytes);
    assert_eq!(recovery.page().covered_through_uid(), 1);
    assert_eq!(recovery.request(), &request());
    assert!(!format!("{recovery:?}").contains("new"));
}
#[test]
fn sync02_recovery_empty_new_namespace_is_valid() {
    let p = ImapUidPage::new(
        ImapUidPageRequest::new("acct", 7, mailbox(), Some(1), 0).unwrap(),
        1,
        1,
        vec![],
    )
    .unwrap();
    let recovery = ImapUidRecoveryPage::new(request(), p).unwrap();
    assert_eq!(recovery.page().covered_through_uid(), 0);
    assert!(recovery.page().messages().is_empty());
}
