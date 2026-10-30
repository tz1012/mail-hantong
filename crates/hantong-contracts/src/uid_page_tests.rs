use super::*;
fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}
#[test]
fn ext04_uid_contract_lexical_identity_and_bounds() {
    for wire in ["", "0", "AB", "zz"] {
        assert!(UidMailboxIdentity::new(wire, UidMailboxEncoding::Utf8).is_err());
    }
    assert!(UidMailboxIdentity::new(&"00".repeat(1025), UidMailboxEncoding::Utf8).is_err());
    assert!(UidMailboxIdentity::new("00", UidMailboxEncoding::Utf8).is_ok()); // semantic validation belongs to Core
    assert!(UidMailboxEncoding::parse("UTF8").is_err());
    assert!(UidCursorRequest::new("", mailbox()).is_err());
    assert!(UidPageRequest::new("acct", mailbox(), "op", "key", 0, 0, &"00".repeat(32)).is_err());
    assert!(
        UidPageRequest::new(
            "acct",
            mailbox(),
            "op",
            "key",
            1,
            i64::MAX as u64,
            &"00".repeat(32)
        )
        .is_err()
    );
    assert!(UidPageStatusRequest::new("acct", [0; 32]).is_err());
}
#[test]
fn sync02_uid_intent_covers_every_cas_and_identity_field() {
    let make = |account, mailbox, op, key, generation, cursor| {
        UidPageRequest::new(
            account,
            mailbox,
            op,
            key,
            generation,
            cursor,
            &"00".repeat(32),
        )
        .unwrap()
    };
    let first = make("acct", mailbox(), "op", "key", 1, 0).intent_bytes();
    for changed in [
        make("other", mailbox(), "op", "key", 1, 0),
        make(
            "acct",
            UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap(),
            "op",
            "key",
            1,
            0,
        ),
        make("acct", mailbox(), "op2", "key", 1, 0),
        make("acct", mailbox(), "op", "key2", 1, 0),
        make("acct", mailbox(), "op", "key", 2, 0),
        make("acct", mailbox(), "op", "key", 1, 1),
    ] {
        assert_ne!(first, changed.intent_bytes());
    }
}
#[test]
fn sync02_uid_receipt_and_cursor_reject_impossible_states() {
    assert!(
        UidPageReceipt::new(
            "acct",
            [1; 32],
            UidPageState::Succeeded,
            Some(1),
            Some(0),
            Some(1),
            None
        )
        .is_err()
    );
    assert!(
        UidPageReceipt::new(
            "acct",
            [1; 32],
            UidPageState::Unresolved,
            Some(1),
            None,
            None,
            None
        )
        .is_err()
    );
    assert!(
        UidPageReceipt::new(
            "acct",
            [1; 32],
            UidPageState::Failed,
            None,
            None,
            None,
            None
        )
        .is_err()
    );
    assert!(UidCursorSnapshot::new("acct", mailbox(), 1, 0, 1, true).is_err());
    assert!(UidCursorSnapshot::new("acct", mailbox(), 1, 1, u32::MAX, true).is_err());
    assert!(
        UidPageReceipt::new(
            "acct",
            [1; 32],
            UidPageState::Succeeded,
            Some(1),
            Some(0),
            Some(0),
            None
        )
        .is_ok()
    );
}
#[test]
fn sec08_uid_contract_debug_redacts_identifiers() {
    let request = UidPageRequest::new(
        "private_account",
        mailbox(),
        "private_operation",
        "private_key",
        1,
        0,
        &"ab".repeat(32),
    )
    .unwrap();
    let debug = format!("{request:?}");
    assert!(!debug.contains("private"));
    assert!(!debug.contains("abab"));
}
