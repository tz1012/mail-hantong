use super::*;
fn invoke(args: Vec<&str>, action: impl FnMut(Action) -> Result<ResultValue, ErrorCode>) -> i32 {
    crate::run_with_mail_actions(
        args.into_iter().map(String::from),
        |_| panic!("registration"),
        || panic!("verify"),
        |_| panic!("accounts"),
        |_| panic!("setup"),
        |_| panic!("scope"),
        (|_| panic!("folders"), action),
    )
}
#[test]
fn cli01_uid_invalid_options_fail_before_authentication() {
    let valid = vec![
        "sync",
        "page",
        "--account",
        "acct",
        "--wire-hex",
        "494e424f58",
        "--encoding",
        "modified_utf7",
        "--expected-catalog-generation",
        "1",
        "--expected-cursor-version",
        "0",
        "--operation-id",
        "op",
        "--idempotency-key",
        "key",
    ];
    for index in [3, 5, 7, 9, 11, 13, 15] {
        let mut args = valid.clone();
        args[index] = "";
        assert_eq!(
            invoke(args, |_| panic!("invalid input reached callback")),
            2
        );
    }
    for value in ["+1", "01", "-1", "1.0", "9223372036854775807"] {
        let mut args = valid.clone();
        args[11] = value;
        assert_eq!(
            invoke(args, |_| panic!("invalid version reached callback")),
            2
        );
    }
    for extras in [
        vec!["--account", "other"],
        vec!["--limit", "1"],
        vec!["--include-capabilities"],
        vec!["--host", "private"],
    ] {
        let mut args = valid.clone();
        args.extend(extras);
        assert_eq!(
            invoke(args, |_| panic!("unknown option reached callback")),
            2
        );
    }
}
#[test]
fn cli01_uid_cursor_and_page_each_dispatch_once_with_localized_output() {
    let mailbox = UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap();
    let cursor = UidCursorSnapshot::new("acct", mailbox.clone(), 2, 0, 0, true).unwrap();
    let calls = std::cell::Cell::new(0);
    assert_eq!(
        invoke(
            vec![
                "--lang",
                "ko",
                "--json",
                "--no-input",
                "sync",
                "cursor",
                "--account",
                "acct",
                "--wire-hex",
                "494e424f58",
                "--encoding",
                "modified_utf7"
            ],
            |action| {
                calls.set(calls.get() + 1);
                assert!(matches!(action, Action::Cursor(_)));
                Ok(ResultValue::Cursor(cursor.clone()))
            }
        ),
        0
    );
    assert_eq!(calls.get(), 1);
    assert!(cursor_human(&cursor, Lang::En).contains("Cursor version"));
    assert!(cursor_human(&cursor, Lang::Ko).contains("커서 버전"));
    let receipt = UidPageReceipt::new(
        "acct",
        [1; 32],
        UidPageState::Unresolved,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        invoke(
            vec![
                "sync",
                "page",
                "--account",
                "acct",
                "--wire-hex",
                "494e424f58",
                "--encoding",
                "modified_utf7",
                "--expected-catalog-generation",
                "2",
                "--expected-cursor-version",
                "0",
                "--operation-id",
                "op",
                "--idempotency-key",
                "key"
            ],
            |action| {
                calls.set(calls.get() + 1);
                let Action::Page(request) = action else {
                    panic!("page expected")
                };
                assert_eq!(request.expected_catalog_generation(), 2);
                assert_eq!(request.expected_cursor_version(), 0);
                Ok(ResultValue::Receipt(receipt.clone()))
            }
        ),
        0
    );
    assert_eq!(calls.get(), 2);
    assert!(receipt_human(&receipt, Lang::En).contains("Unresolved"));
    assert!(receipt_human(&receipt, Lang::Ko).contains("미확정"));
}
#[test]
fn cli01_uid_status_binds_execution_and_wrong_result_kind_is_rejected() {
    let execution = "01".repeat(32);
    let args = vec![
        "sync",
        "status",
        "--account",
        "acct",
        "--execution-id",
        &execution,
    ];
    let receipt = UidPageReceipt::new(
        "acct",
        [1; 32],
        UidPageState::Failed,
        None,
        None,
        None,
        Some(ErrorCode::Conflict),
    )
    .unwrap();
    assert_eq!(
        invoke(args, |action| {
            let Action::Status(request) = action else {
                panic!("status expected")
            };
            assert_eq!(request.execution_id(), &[1; 32]);
            Ok(ResultValue::Receipt(receipt.clone()))
        }),
        0
    );
    assert!(receipt_human(&receipt, Lang::En).contains("CONFLICT"));
    assert_eq!(
        invoke(
            vec![
                "sync",
                "cursor",
                "--account",
                "acct",
                "--wire-hex",
                "494e424f58",
                "--encoding",
                "modified_utf7"
            ],
            |_| Ok(ResultValue::Receipt(receipt.clone()))
        ),
        ErrorCode::InternalError.exit_code()
    );
}
