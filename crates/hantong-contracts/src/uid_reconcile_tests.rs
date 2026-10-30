use super::*;
fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new(
        "494e424f58",
        crate::uid_page::UidMailboxEncoding::ModifiedUtf7,
    )
    .unwrap()
}
fn progress() -> UidReconcileProgress {
    UidReconcileProgress::new(1, 3, 2, 64, 128, 2, 1, 1, 0, true).unwrap()
}
#[test]
fn ext04_reconcile_request_has_distinct_intent_and_positive_cursor() {
    let request =
        UidReconcileRequest::new("acct", mailbox(), "op", "key", 1, 3, &"01".repeat(32)).unwrap();
    let append =
        UidPageRequest::new("acct", mailbox(), "op", "key", 1, 3, &"01".repeat(32)).unwrap();
    assert!(
        request
            .intent_bytes()
            .starts_with(b"mail-hantong.imap-uid-reconcile-intent.v1\0")
    );
    assert_ne!(request.intent_bytes(), append.intent_bytes());
    assert_eq!(request.account_ids(), &["acct"]);
    for cursor in [0, i64::MAX as u64, u64::MAX] {
        assert!(
            UidReconcileRequest::new("acct", mailbox(), "op", "key", 1, cursor, &"01".repeat(32))
                .is_err()
        );
    }
    assert!(!format!("{request:?}").contains("acct"));
}
#[test]
fn sync02_reconcile_progress_enforces_range_counts_and_pass_boundary() {
    assert_eq!(progress().source_cursor_version(), 2);
    for args in [
        (0, 3, 2, 64, 128, 2, 1, 1, 0, true),
        (1, 1, 2, 64, 128, 2, 1, 1, 0, true),
        (1, 3, 2, 64, 129, 2, 1, 1, 0, true),
        (1, 3, 2, 64, 64, 2, 1, 1, 0, true),
        (1, 3, 2, u32::MAX - 1, u32::MAX, 1, 0, 0, 0, true),
        (1, 3, 2, 64, 128, 0, 0, 0, 0, true),
        (1, 3, 2, 64, 65, 2, 0, 0, 0, true),
        (1, 3, 2, 64, 128, 2, u32::MAX, 1, 0, true),
        (1, 3, 2, 64, 128, 2, 2, 1, 0, true),
        (1, 3, 2, 64, 128, 2, 1, 1, 1, true),
        (1, 3, 2, 64, 128, 2, 1, 1, 100001, false),
    ] {
        let (a, b, c, d, e, f, g, h, i, j) = args;
        assert!(UidReconcileProgress::new(a, b, c, d, e, f, g, h, i, j).is_err());
    }
}
#[test]
fn sec08_reconcile_receipt_requires_exact_state_nullness_and_redacts() {
    for (state, result, error) in [
        (UidPageState::Unresolved, None, None),
        (UidPageState::Succeeded, Some(progress()), None),
        (UidPageState::Failed, None, Some(ErrorCode::Conflict)),
    ] {
        let r = UidReconcileReceipt::new("acct", [1; 32], state, result, error).unwrap();
        assert!(!format!("{r:?}").contains("acct"));
    }
    for (state, result, error) in [
        (UidPageState::Unresolved, Some(progress()), None),
        (UidPageState::Succeeded, None, None),
        (
            UidPageState::Succeeded,
            Some(progress()),
            Some(ErrorCode::Conflict),
        ),
        (
            UidPageState::Failed,
            Some(progress()),
            Some(ErrorCode::Conflict),
        ),
        (UidPageState::Failed, None, None),
    ] {
        assert!(UidReconcileReceipt::new("acct", [1; 32], state, result, error).is_err());
    }
    assert!(
        UidReconcileReceipt::new("acct", [0; 32], UidPageState::Unresolved, None, None).is_err()
    );
}
