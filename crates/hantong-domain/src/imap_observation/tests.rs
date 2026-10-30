use super::*;
use crate::mailboxes::MailboxEncoding;

fn mailbox() -> ImapMailboxName {
    ImapMailboxName::from_wire(b"INBOX", MailboxEncoding::ModifiedUtf7).unwrap()
}

#[test]
fn sync01_observation_binds_account_endpoint_mailbox_and_nonzero_uid_state() {
    let request = ImapUidObservationRequest::new("acct", 7, mailbox()).unwrap();
    assert_eq!(request.account_id(), "acct");
    assert_eq!(request.endpoint_version(), 7);
    assert_eq!(request.mailbox(), &mailbox());
    for (validity, next) in [(1, 1), (u32::MAX, u32::MAX), (7, 194)] {
        let observed = ImapUidObservation::new(request.clone(), validity, next).unwrap();
        assert_eq!(observed.request(), &request);
        assert_eq!(observed.uid_validity(), validity);
        assert_eq!(observed.uid_next(), next);
    }
    for (validity, next) in [(0, 1), (1, 0), (0, 0)] {
        assert_eq!(
            ImapUidObservation::new(request.clone(), validity, next),
            Err(ImapSyncError::InvalidInput)
        );
    }
}

#[test]
fn sec03_observation_rejects_invalid_selectors_and_redacts_debug() {
    for (account, version) in [
        ("", 1),
        ("a/b", 1),
        ("a\n", 1),
        ("acct", 0),
        ("acct", u64::MAX),
    ] {
        assert!(ImapUidObservationRequest::new(account, version, mailbox()).is_err());
    }
    let request = ImapUidObservationRequest::new("private_account", 7, mailbox()).unwrap();
    assert_eq!(
        format!("{request:?}"),
        "ImapUidObservationRequest(<redacted>)"
    );
    let observed = ImapUidObservation::new(request, 7, 100).unwrap();
    assert_eq!(format!("{observed:?}"), "ImapUidObservation(<redacted>)");
}
