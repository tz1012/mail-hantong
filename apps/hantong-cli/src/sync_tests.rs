use super::*;

fn inspection_args() -> Vec<&'static str> {
    vec![
        "sync",
        "inspect",
        "--account",
        "acct",
        "--wire-hex",
        "494e424f58",
        "--encoding",
        "modified_utf7",
    ]
}

#[test]
fn cli01_inspection_dispatches_one_read_and_distinguishes_absent_from_empty() {
    use hantong_contracts::uid_inspection::UidObservationSummary;
    let mailbox = UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap();
    for observation in [
        None,
        Some(UidObservationSummary::new(1, 1, 0, 0, 1).unwrap()),
        Some(UidObservationSummary::new(3, 2, 64, 500, 1767225600000).unwrap()),
    ] {
        let snapshot =
            UidInspectionSnapshot::new("acct", mailbox.clone(), 4, true, observation).unwrap();
        for flags in [vec![], vec!["--lang", "ko", "--no-input", "--json"]] {
            let mut args = inspection_args();
            args.extend(flags);
            let mut calls = 0;
            assert_eq!(
                invoke(args, |action| {
                    calls += 1;
                    let Action::Inspect(request) = action else {
                        panic!("one inspection expected")
                    };
                    assert_eq!(request.account_id(), "acct");
                    assert_eq!(request.mailbox(), &mailbox);
                    Ok(ResultValue::Inspection(snapshot.clone()))
                }),
                0
            );
            assert_eq!(calls, 1);
        }
        let en = inspection_human(&snapshot, Lang::En);
        let ko = inspection_human(&snapshot, Lang::Ko);
        assert!(en.contains("Catalog generation: 4"));
        assert!(ko.contains("폴더 목록 세대: 4"));
        assert!(en.contains("Binding: Current"));
        assert!(ko.contains("현재 설정과 일치"));
        assert!(!en.contains("complete"));
        assert!(!ko.contains("완료"));
        if let Some(value) = snapshot.observation() {
            assert!(en.contains(&format!(
                "Local stored time (UTC milliseconds): {}",
                value.stored_at_utc_ms()
            )));
            assert!(ko.contains(&format!(
                "로컬 저장 시각(UTC 밀리초): {}",
                value.stored_at_utc_ms()
            )));
            assert!(en.contains(&format!(
                "Observed UID ceiling: {}",
                value.observed_uid_ceiling()
            )));
            if value.covered_through_uid() == 0 {
                assert!(en.contains("Stored UID range: empty"));
                assert!(ko.contains("저장된 UID 범위: 비어 있음"));
            } else {
                assert!(en.contains("Stored UID range: 1..64"));
                assert!(ko.contains("저장된 UID 범위: 1..64"));
                assert!(en.contains("Mailbox identity generation: 2"));
                assert!(en.contains("Cursor version: 3"));
            }
        } else {
            assert!(en.contains("No stored UID observation."));
            assert!(ko.contains("저장된 UID 관측 정보가 없습니다."));
            assert!(!en.contains("Local stored time"));
        }
    }
    let wrong = UidPageReceipt::new(
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
        invoke(inspection_args(), |_| Ok(ResultValue::Receipt(
            wrong.clone()
        ))),
        ErrorCode::InternalError.exit_code()
    );
    let stale = UidInspectionSnapshot::new("acct", mailbox, 1, false, None).unwrap();
    assert!(inspection_human(&stale, Lang::En).contains("Binding: Stale"));
    assert!(inspection_human(&stale, Lang::Ko).contains("설정 재확인 필요"));
}
#[test]
fn cli01_inspection_requires_only_explicit_mailbox_identity() {
    let command = vec!["sync".into(), "inspect".into()];
    let options = vec![
        ("--account".into(), "acct".into()),
        ("--wire-hex".into(), "494e424f58".into()),
        ("--encoding".into(), "utf8".into()),
    ];
    assert!(parse(&command, &options).is_ok());
    for missing in 0..3 {
        let mut invalid = options.clone();
        invalid.remove(missing);
        assert!(parse(&command, &invalid).is_err());
    }
    for extra in [
        "--account",
        "--operation-id",
        "--expected-cursor-version",
        "--uid-validity",
    ] {
        let mut invalid = options.clone();
        invalid.push((extra.into(), "1".into()));
        assert!(parse(&command, &invalid).is_err());
    }
}
#[test]
fn cli01_recovery_syntax_is_explicit_and_rejects_zero_cursor() {
    let command = vec!["sync".into(), "recover".into()];
    let mut options = vec![
        ("--account".into(), "acct".into()),
        ("--wire-hex".into(), "494e424f58".into()),
        ("--encoding".into(), "utf8".into()),
        ("--expected-catalog-generation".into(), "1".into()),
        ("--expected-cursor-version".into(), "4".into()),
        ("--operation-id".into(), "op".into()),
        ("--idempotency-key".into(), "key".into()),
    ];
    assert!(parse(&command, &options).is_ok());
    options[4].1 = "0".into();
    assert!(parse(&command, &options).is_err());
    assert!(
        parse(
            &["sync".into(), "recover-status".into()],
            &[
                ("--account".into(), "acct".into()),
                ("--execution-id".into(), "01".repeat(32))
            ]
        )
        .is_ok()
    );
}
#[test]
fn cli01_recovery_dispatches_once_and_localizes_first_page_preservation() {
    let progress = hantong_contracts::uid_recovery::UidRecoveryProgress::new(2, 5, 10, 2).unwrap();
    let receipt = UidRecoveryReceipt::new(
        "acct",
        [1; 32],
        UidPageState::Succeeded,
        Some(progress),
        None,
    )
    .unwrap();
    let mut args = reconcile_args();
    args[1] = "recover";
    args[11] = "4";
    let mut calls = 0;
    assert_eq!(
        invoke(args.clone(), |action| {
            calls += 1;
            let Action::Recover(request) = action else {
                panic!("recovery expected")
            };
            assert_eq!(request.expected_cursor_version(), 4);
            Ok(ResultValue::RecoveryReceipt(receipt.clone()))
        }),
        0
    );
    assert_eq!(calls, 1);
    let old = UidPageReceipt::new(
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
        invoke(args, |_| Ok(ResultValue::Receipt(old.clone()))),
        ErrorCode::InternalError.exit_code()
    );
    let execution = "01".repeat(32);
    assert_eq!(
        invoke(
            vec![
                "--no-input",
                "--json",
                "--lang",
                "ko",
                "sync",
                "recover-status",
                "--account",
                "acct",
                "--execution-id",
                &execution
            ],
            |action| {
                let Action::RecoverStatus(request) = action else {
                    panic!("recovery status expected")
                };
                assert_eq!(request.execution_id(), &[1; 32]);
                Ok(ResultValue::RecoveryReceipt(receipt.clone()))
            }
        ),
        0
    );
    let en = recovery_human(&receipt, Lang::En);
    let ko = recovery_human(&receipt, Lang::Ko);
    assert!(en.contains("First metadata page stored; existing records preserved"));
    assert!(ko.contains("첫 메타데이터 페이지 저장, 기존 기록 보존"));
    assert!(en.contains("Cursor version: 5"));
    assert!(ko.contains("커서 버전: 5"));
}
fn reconcile_args() -> Vec<&'static str> {
    vec![
        "sync",
        "reconcile",
        "--account",
        "acct",
        "--wire-hex",
        "494e424f58",
        "--encoding",
        "modified_utf7",
        "--expected-catalog-generation",
        "2",
        "--expected-cursor-version",
        "3",
        "--operation-id",
        "op",
        "--idempotency-key",
        "key",
    ]
}
#[test]
fn cli01_reconcile_is_one_explicit_action_and_validates_before_authentication() {
    let mut calls = 0;
    let receipt =
        UidReconcileReceipt::new("acct", [1; 32], UidPageState::Unresolved, None, None).unwrap();
    assert_eq!(
        invoke(reconcile_args(), |action| {
            calls += 1;
            let Action::Reconcile(request) = action else {
                panic!("reconcile expected")
            };
            assert_eq!(request.expected_cursor_version(), 3);
            Ok(ResultValue::ReconcileReceipt(receipt.clone()))
        }),
        0
    );
    assert_eq!(calls, 1);
    for invalid in ["0", "01", "-1", "9223372036854775807"] {
        let mut args = reconcile_args();
        args[11] = invalid;
        assert_eq!(invoke(args, |_| panic!("invalid reached callback")), 2);
    }
    for extras in [
        vec!["--account", "other"],
        vec!["--limit", "64"],
        vec!["--host", "private"],
    ] {
        let mut args = reconcile_args();
        args.extend(extras);
        assert_eq!(
            invoke(args, |_| panic!("extra options reached callback")),
            2
        );
    }
    let append = UidPageReceipt::new(
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
        invoke(reconcile_args(), |_| Ok(ResultValue::Receipt(
            append.clone()
        ))),
        ErrorCode::InternalError.exit_code()
    );
}
#[test]
fn cli01_reconcile_status_and_localized_output_describe_only_frozen_pass() {
    let progress = hantong_contracts::uid_reconcile::UidReconcileProgress::new(
        1, 3, 2, 64, 128, 2, 1, 1, 0, true,
    )
    .unwrap();
    let receipt = UidReconcileReceipt::new(
        "acct",
        [1; 32],
        UidPageState::Succeeded,
        Some(progress),
        None,
    )
    .unwrap();
    let execution = "01".repeat(32);
    assert_eq!(
        invoke(
            vec![
                "--lang",
                "ko",
                "--json",
                "--no-input",
                "sync",
                "reconcile-status",
                "--account",
                "acct",
                "--execution-id",
                &execution
            ],
            |action| {
                let Action::ReconcileStatus(request) = action else {
                    panic!("reconcile status expected")
                };
                assert_eq!(request.execution_id(), &[1; 32]);
                Ok(ResultValue::ReconcileReceipt(receipt.clone()))
            }
        ),
        0
    );
    let en = reconcile_human(&receipt, Lang::En);
    let ko = reconcile_human(&receipt, Lang::Ko);
    assert!(en.contains("Frozen pass observations complete"));
    assert!(ko.contains("고정 범위 대조 관측 완료"));
    assert!(en.contains("Remaining source pages: 0"));
    assert!(ko.contains("남은 원본 페이지: 0"));
    assert!(!en.contains("mailbox complete"));
}
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
fn cli01_uid_existing_human_output_stays_stable_after_reconcile_extension() {
    let mailbox = UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap();
    let cursor = UidCursorSnapshot::new("acct", mailbox, 2, 1, 64, true).unwrap();
    assert_eq!(
        cursor_human(&cursor, Lang::En),
        "Local UID cursor\nCatalog generation: 2\nCursor version: 1\nCovered UID: 64\nBinding: Current\n"
    );
    assert_eq!(
        cursor_human(&cursor, Lang::Ko),
        "로컬 UID 커서\n폴더 목록 세대: 2\n커서 버전: 1\n관측한 UID 상한: 64\n설정 결속: 현재 설정과 일치\n"
    );
    let receipt = UidPageReceipt::new(
        "acct",
        [1; 32],
        UidPageState::Succeeded,
        Some(1),
        Some(64),
        Some(2),
        None,
    )
    .unwrap();
    assert_eq!(
        receipt_human(&receipt, Lang::En),
        format!(
            "UID page state: Succeeded\nExecution ID: {}\nCursor version: 1\nCovered UID: 64\nInserted messages: 2\n",
            "01".repeat(32)
        )
    );
    assert_eq!(
        receipt_human(&receipt, Lang::Ko),
        format!(
            "UID 페이지 상태: 성공\n실행 ID: {}\n커서 버전: 1\n관측한 UID 상한: 64\n저장한 메시지 수: 2\n",
            "01".repeat(32)
        )
    );
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
