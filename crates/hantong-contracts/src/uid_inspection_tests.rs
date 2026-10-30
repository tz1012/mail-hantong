use super::*;
use crate::uid_page::UidMailboxEncoding;

fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}

#[test]
fn sync02_inspection_summary_validates_epoch_uid_and_local_time_bounds() {
    for (cursor, epoch, covered, ceiling, stored) in [
        (1, 1, 0, 0, 1),
        (1, 1, 64, 500, 1767225600000),
        (9, 2, 128, 128, i64::MAX),
        (
            i64::MAX as u64,
            i64::MAX as u64,
            u32::MAX - 1,
            u32::MAX - 1,
            1,
        ),
    ] {
        let summary = UidObservationSummary::new(cursor, epoch, covered, ceiling, stored).unwrap();
        assert_eq!(summary.cursor_version(), cursor);
        assert_eq!(summary.epoch_generation(), epoch);
        assert_eq!(summary.covered_through_uid(), covered);
        assert_eq!(summary.observed_uid_ceiling(), ceiling);
        assert_eq!(summary.stored_at_utc_ms(), stored);
        assert_eq!(summary.coverage_at_observed_ceiling(), covered == ceiling);
    }
    for (cursor, epoch, covered, ceiling, stored) in [
        (0, 1, 0, 0, 1),
        (1, 0, 0, 0, 1),
        (1, 2, 0, 0, 1),
        (i64::MAX as u64 + 1, 1, 0, 0, 1),
        (1, 1, 2, 1, 1),
        (1, 1, 0, u32::MAX, 1),
        (1, 1, 0, 0, 0),
        (1, 1, 0, 0, -1),
    ] {
        assert_eq!(
            UidObservationSummary::new(cursor, epoch, covered, ceiling, stored),
            Err(ErrorCode::InvalidInput)
        );
    }
}

#[test]
fn sync02_inspection_distinguishes_unobserved_from_empty_observation() {
    let absent = UidInspectionSnapshot::new("acct", mailbox(), 1, false, None).unwrap();
    let empty = UidInspectionSnapshot::new(
        "acct",
        mailbox(),
        2,
        true,
        Some(UidObservationSummary::new(1, 1, 0, 0, 1).unwrap()),
    )
    .unwrap();
    assert!(absent.observation().is_none());
    assert_eq!(empty.account_id(), "acct");
    assert_eq!(empty.mailbox(), &mailbox());
    assert_eq!(empty.catalog_generation(), 2);
    assert!(empty.binding_current());
    assert!(empty.observation().unwrap().coverage_at_observed_ceiling());
    assert_ne!(absent, empty);
    for account in ["", "a b", "../acct"] {
        assert!(UidInspectionSnapshot::new(account, mailbox(), 1, true, None).is_err());
    }
    for generation in [0, i64::MAX as u64 + 1, u64::MAX] {
        assert!(UidInspectionSnapshot::new("acct", mailbox(), generation, true, None).is_err());
    }
}

#[test]
fn sec08_inspection_debug_redacts_identity_and_observation() {
    let summary = UidObservationSummary::new(73, 2, 64, 123456, 1767225600000).unwrap();
    let snapshot =
        UidInspectionSnapshot::new("private-account", mailbox(), 3, true, Some(summary.clone()))
            .unwrap();
    assert_eq!(
        format!("{summary:?}"),
        "UidObservationSummary { <redacted> }"
    );
    assert_eq!(
        format!("{snapshot:?}"),
        "UidInspectionSnapshot { <redacted> }"
    );
}
