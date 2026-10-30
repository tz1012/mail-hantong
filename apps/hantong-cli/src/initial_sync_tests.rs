use super::*;
use std::cell::RefCell;

fn options(action: &str) -> Vec<(String, String)> {
    if action == "initial-status" {
        return vec![
            ("--account".into(), "acct".into()),
            ("--execution-id".into(), "ab".repeat(32)),
        ];
    }
    let mut result = vec![
        ("--account".into(), "acct".into()),
        ("--wire-hex".into(), "494e424f58".into()),
        ("--encoding".into(), "modified_utf7".into()),
    ];
    if action != "initial-start" {
        result.push(("--plan-id".into(), "ab".repeat(32)));
    }
    if action != "initial-plan" {
        result.extend([
            ("--operation-id".into(), "op".into()),
            ("--idempotency-key".into(), "key".into()),
        ]);
        if action != "initial-pause" {
            result.push(("--expected-catalog-generation".into(), "1".into()));
        }
        if action == "initial-start" {
            result.extend([
                ("--expected-preview-generation".into(), "0".into()),
                ("--recent-pages".into(), "8".into()),
            ]);
        } else {
            result.push(("--expected-plan-version".into(), "1".into()));
        }
    }
    result
}
fn args(action: &str, options: &[(String, String)]) -> Vec<String> {
    let mut args = vec!["sync".into(), action.into()];
    args.extend(options.iter().flat_map(|(k, v)| [k.clone(), v.clone()]));
    args
}
fn run(args: Vec<String>, events: &RefCell<Vec<&'static str>>) -> i32 {
    crate::run_with_mail_actions(
        args,
        |_| panic!("no registration"),
        || panic!("no bootstrap verification"),
        |_| panic!("no account list"),
        |_| panic!("no setup"),
        |_| panic!("no scope refresh"),
        (
            |_| panic!("no folders"),
            |_| {
                events.borrow_mut().push("action");
                Err(ErrorCode::Offline)
            },
            || {
                events.borrow_mut().push("language");
                Ok(
                    hantong_contracts::display_settings::CliDisplaySettings::new(Some(
                        hantong_contracts::display_settings::CliDisplayLanguage::Ko,
                    )),
                )
            },
        ),
    )
}
const ACTIONS: [&str; 5] = [
    "initial-start",
    "initial-resume",
    "initial-pause",
    "initial-status",
    "initial-plan",
];

#[test]
fn sync01_initial_cli_parses_all_explicit_actions_before_one_optional_language_read() {
    for action in ACTIONS {
        let opts = options(action);
        assert!(parse(&["sync".into(), action.into()], &opts).is_ok());
        for extra in [
            vec![],
            vec!["--json"],
            vec!["--lang", "en"],
            vec!["--lang", "ko", "--no-input"],
        ] {
            let mut input = args(action, &opts);
            input.extend(extra.iter().map(|v| v.to_string()));
            let events = RefCell::new(vec![]);
            assert_eq!(run(input, &events), ErrorCode::Offline.exit_code());
            assert_eq!(
                *events.borrow(),
                if extra.is_empty() {
                    vec!["language", "action"]
                } else {
                    vec!["action"]
                }
            );
        }
    }
}

#[test]
fn sec03_initial_cli_missing_duplicate_unknown_and_invalid_options_have_no_effects() {
    for action in ACTIONS {
        let opts = options(action);
        let mut bad = vec![];
        for i in 0..opts.len() {
            let mut missing = opts.clone();
            missing.remove(i);
            bad.push(missing);
        }
        let mut duplicate = opts.clone();
        duplicate.push(opts[0].clone());
        bad.push(duplicate);
        let mut unknown = opts.clone();
        unknown.push(("--filter".into(), "all".into()));
        bad.push(unknown);
        let mut account = opts.clone();
        account[0].1 = "bad/account".into();
        bad.push(account);
        for options in bad {
            let events = RefCell::new(vec![]);
            assert_eq!(
                run(args(action, &options), &events),
                ErrorCode::InvalidInput.exit_code()
            );
            assert!(events.borrow().is_empty());
        }
    }
    for (action, key, text) in [
        ("initial-start", "--recent-pages", "0"),
        ("initial-start", "--recent-pages", "9"),
        ("initial-start", "--recent-pages", "256"),
        ("initial-start", "--expected-preview-generation", "01"),
        (
            "initial-start",
            "--expected-preview-generation",
            "9223372036854775807",
        ),
        ("initial-start", "--expected-catalog-generation", "0"),
        (
            "initial-resume",
            "--expected-plan-version",
            "18446744073709551616",
        ),
        ("initial-pause", "--expected-plan-version", "-1"),
        ("initial-plan", "--plan-id", "00"),
        ("initial-status", "--execution-id", "00"),
    ] {
        let mut opts = options(action);
        opts.iter_mut().find(|(k, _)| k == key).unwrap().1 = text.into();
        let events = RefCell::new(vec![]);
        assert_eq!(
            run(args(action, &opts), &events),
            ErrorCode::InvalidInput.exit_code()
        );
        assert!(events.borrow().is_empty());
    }
}

#[test]
fn sync01_initial_cli_human_progress_is_localized_and_does_not_claim_message_counts_or_live_workers()
 {
    let snapshot = InitialSyncPlanSnapshot::new(
        InitialSyncPlanIdentity::new(
            [0xab; 32],
            "acct",
            UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap(),
            1,
        )
        .unwrap(),
        1,
        InitialSyncProgress::new(7, u32::MAX, 8, 0, InitialSyncState::Ready, 1).unwrap(),
        true,
        false,
        1000,
    )
    .unwrap();
    for (lang, state_label, span_label, note) in [
        (
            Lang::En,
            "Stored plan state",
            "Remaining UID span",
            "not message counts",
        ),
        (
            Lang::Ko,
            "저장된 계획 상태",
            "남은 UID 범위 크기",
            "메일 수가 아닙니다",
        ),
    ] {
        let text = plan_human(&snapshot, lang);
        assert!(text.contains(state_label));
        assert!(text.contains(&format!("{span_label}: 510")));
        assert!(text.contains(note));
        assert!(text.contains(resource(lang, "sync_stale")));
        let pending = InitialSyncReceipt::new(
            "acct",
            [1; 32],
            InitialSyncAction::Start,
            InitialSyncJobState::Running,
            None,
            None,
        )
        .unwrap();
        let text = receipt_human(&pending, lang);
        assert!(text.contains(resource(lang, "initial_job_state")));
        assert!(text.contains(resource(lang, "initial_unobserved")));
    }
    let machine = hantong_ipc_client::initial_sync::plan_json(&snapshot);
    assert!(machine.contains("\"remaining_uid_span\":510"));
    assert!(!machine.contains("Stored plan state"));
    assert!(!machine.contains("저장된 계획 상태"));
}
