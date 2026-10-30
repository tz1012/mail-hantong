use super::*;
use crate::uid_page::UidMailboxEncoding;
fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}
fn mutation() -> InitialSyncMutation {
    InitialSyncMutation::new("op", "key", &"0".repeat(64)).unwrap()
}
#[test]
fn sync01_initial_requests_bound_fields_intents_and_redaction() {
    let start = InitialSyncRequest::new_start("acct", mailbox(), mutation(), 1, 0, 2).unwrap();
    let resume =
        InitialSyncRequest::new_resume("acct", mailbox(), mutation(), [1; 32], 1, 1).unwrap();
    let pause = InitialSyncRequest::new_pause("acct", mailbox(), mutation(), [1; 32], 1).unwrap();
    assert_ne!(start.intent_bytes(), resume.intent_bytes());
    assert_ne!(resume.intent_bytes(), pause.intent_bytes());
    assert!(!format!("{start:?}").contains("acct"));
    assert!(InitialSyncRequest::new_start("acct", mailbox(), mutation(), 0, 0, 1).is_err());
    assert!(InitialSyncRequest::new_start("acct", mailbox(), mutation(), 1, 0, 9).is_err());
    assert!(InitialSyncRequest::new_resume("acct", mailbox(), mutation(), [0; 32], 1, 1).is_err());
    assert!(InitialSyncPlanRequest::new("", mailbox(), [1; 32]).is_err());
}
#[test]
fn sync01_progress_rejects_unreachable_model_versions() {
    for state in [
        InitialSyncState::Running,
        InitialSyncState::Pausing,
        InitialSyncState::PausedUser,
        InitialSyncState::Interrupted,
        InitialSyncState::Failed,
        InitialSyncState::StaleBinding,
        InitialSyncState::NeedsRecovery,
    ] {
        assert!(InitialSyncProgress::new(1, 200, 2, 0, state, 1).is_err());
    }
    assert!(InitialSyncProgress::new(1, 1, 1, 0, InitialSyncState::Complete, 2).is_err());
    assert!(InitialSyncProgress::new(1, 200, 2, 0, InitialSyncState::PausedUser, 3).is_err());
    assert!(InitialSyncProgress::new(1, 200, 2, 2, InitialSyncState::Complete, 4).is_err());
    let p = InitialSyncProgress::new(1, 200, 2, 1, InitialSyncState::PausedBudget, 3).unwrap();
    assert_eq!(p.selected_lower_uid(), Some(129));
    assert_eq!(p.remaining_uid_span(), 64);
    assert_eq!(p.not_selected_before_uid(), Some(128));
}
#[test]
fn sync01_receipt_requires_terminal_shape_and_account_binding() {
    let p = InitialSyncPlanSnapshot::new(
        InitialSyncPlanIdentity::new([1; 32], "acct", mailbox(), 1).unwrap(),
        1,
        InitialSyncProgress::new(5, 1, 1, 0, InitialSyncState::Complete, 1).unwrap(),
        true,
        true,
        1,
    )
    .unwrap();
    assert!(
        InitialSyncReceipt::new(
            "acct",
            [1; 32],
            InitialSyncAction::Start,
            InitialSyncJobState::Succeeded,
            Some(p.clone()),
            None
        )
        .is_ok()
    );
    assert!(
        InitialSyncReceipt::new(
            "other",
            [1; 32],
            InitialSyncAction::Start,
            InitialSyncJobState::Succeeded,
            Some(p.clone()),
            None
        )
        .is_err()
    );
    assert!(
        InitialSyncReceipt::new(
            "acct",
            [1; 32],
            InitialSyncAction::Start,
            InitialSyncJobState::Running,
            Some(p),
            None
        )
        .is_err()
    );
    assert!(
        InitialSyncReceipt::new(
            "acct",
            [1; 32],
            InitialSyncAction::Start,
            InitialSyncJobState::Failed,
            None,
            None
        )
        .is_err()
    );
}
