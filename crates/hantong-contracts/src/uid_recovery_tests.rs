use super::*;
use crate::{uid_page::UidMailboxEncoding, uid_reconcile::UidReconcileRequest};
fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}
#[test]
fn sync02_recovery_request_has_distinct_intent_and_requires_existing_cursor() {
    let hash = "01".repeat(32);
    let request = UidRecoveryRequest::new("acct", mailbox(), "op", "key", 1, 4, &hash).unwrap();
    let append = UidPageRequest::new("acct", mailbox(), "op", "key", 1, 4, &hash).unwrap();
    let reconcile = UidReconcileRequest::new("acct", mailbox(), "op", "key", 1, 4, &hash).unwrap();
    assert!(
        request
            .intent_bytes()
            .starts_with(b"mail-hantong.imap-uid-recovery-intent.v1\0")
    );
    assert_ne!(request.intent_bytes(), append.intent_bytes());
    assert_ne!(request.intent_bytes(), reconcile.intent_bytes());
    assert_eq!(request.account_ids(), ["acct"]);
    for version in [0, i64::MAX as u64, u64::MAX] {
        assert!(
            UidRecoveryRequest::new("acct", mailbox(), "op", "key", 1, version, &hash).is_err()
        );
    }
    assert!(!format!("{request:?}").contains("acct"));
}
#[test]
fn sync02_recovery_progress_validates_epoch_cursor_and_first_page_bounds() {
    for (generation, cursor, coverage, count) in [
        (2, 2, 64, 64),
        (2, 100, 0, 0),
        (i64::MAX as u64, i64::MAX as u64, 10, 0),
    ] {
        assert!(UidRecoveryProgress::new(generation, cursor, coverage, count).is_ok());
    }
    for (generation, cursor, coverage, count) in [
        (0, 2, 64, 1),
        (1, 2, 64, 1),
        (3, 2, 64, 1),
        (2, u64::MAX, 64, 1),
        (2, 2, 65, 1),
        (2, 2, 0, 1),
        (2, 2, 2, 3),
    ] {
        assert!(UidRecoveryProgress::new(generation, cursor, coverage, count).is_err());
    }
}
#[test]
fn sec08_recovery_receipt_nullness_identity_and_debug_are_bounded() {
    let result = UidRecoveryProgress::new(2, 5, 0, 0).unwrap();
    for (state, value, error) in [
        (UidPageState::Unresolved, None, None),
        (UidPageState::Succeeded, Some(result.clone()), None),
        (UidPageState::Failed, None, Some(ErrorCode::Conflict)),
    ] {
        let receipt = UidRecoveryReceipt::new("acct", [1; 32], state, value, error).unwrap();
        assert!(!format!("{receipt:?}").contains("acct"));
    }
    for (state, value, error) in [
        (UidPageState::Unresolved, Some(result.clone()), None),
        (UidPageState::Succeeded, None, None),
        (
            UidPageState::Succeeded,
            Some(result),
            Some(ErrorCode::Conflict),
        ),
        (UidPageState::Failed, None, None),
    ] {
        assert!(UidRecoveryReceipt::new("acct", [1; 32], state, value, error).is_err());
    }
    assert!(
        UidRecoveryReceipt::new("acct", [0; 32], UidPageState::Unresolved, None, None).is_err()
    );
}
