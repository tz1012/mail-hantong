use hantong_approval::{
    EffectiveTheme, Presentation, RegistrationReview, ReviewIntent, ReviewModel, ReviewState, Rgb,
    TextKey, text,
};
use hantong_contracts::{ErrorCode, client::ClientKind};
use hantong_domain::config::{Language, Theme};
use std::{collections::BTreeSet, time::Duration};

fn review() -> RegistrationReview {
    RegistrationReview::new(
        [17; 32],
        9,
        vec!["b".into(), "a".into()],
        [34; 32],
        1_000,
        301_000,
    )
    .unwrap()
}
fn model() -> ReviewModel {
    ReviewModel::new(review(), Presentation::defaults(false, false, false))
}

#[test]
fn sec03_gui_review_is_distinct_and_binds_all_three_local_read_tools() {
    let gui = RegistrationReview::new_gui(
        [17; 32],
        9,
        vec!["b".into(), "a".into()],
        [34; 32],
        1000,
        301000,
    )
    .unwrap();
    assert_eq!(gui.client_kind(), ClientKind::Gui);
    assert_eq!(gui.profile_name(), "gui");
    assert_eq!(
        gui.scope().tools(),
        hantong_contracts::auth::gui_read_tools()
    );
    assert_ne!(gui.canonical_bytes(), review().canonical_bytes());
    let bytes = gui.canonical_bytes();
    assert!(bytes.starts_with(b"mail-hantong/gui-registration-review/v1\0gui\0gui\0"));
    for tool in gui.scope().tools() {
        assert!(
            bytes
                .windows(tool.len())
                .any(|part| part == tool.as_bytes())
        );
    }
    assert!(!gui.scope().permits_account("other", "mail.messages.search"));
    assert!(!gui.scope().permits_account("a", "mail.sync.page"));
    let empty = RegistrationReview::new_gui([1; 32], 1, vec![], [2; 32], 1, 2).unwrap();
    assert!(!empty.scope().permits_account("a", "mail.accounts.list"));
}

#[test]
fn cli07_review_fixes_read_only_default_scope_and_preserves_empty_as_zero() {
    let review = review();
    assert_eq!(review.client_kind(), ClientKind::Cli);
    assert_eq!(review.profile_name(), "default");
    assert!(review.read_only());
    assert_eq!(review.requests_per_minute(), 60);
    assert_eq!(review.account_ids(), &["a", "b"]);
    assert_eq!(review.scope().tools(), &["mail.accounts.list"]);
    let empty = RegistrationReview::new([1; 32], 1, vec![], [2; 32], 1, 2).unwrap();
    assert!(empty.account_ids().is_empty());
    assert!(
        !empty
            .scope()
            .permits_account("new_account", "mail.accounts.list")
    );
}

#[test]
fn apr02_canonical_review_matches_v1_layout_and_sorted_scope() {
    let value = review();
    let mut expected = b"mail-hantong/registration-review/v1\0".to_vec();
    expected.extend_from_slice(&[17; 32]);
    expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 9]);
    expected.extend_from_slice(&[34; 32]);
    expected.extend_from_slice(&[0, 0, 0, 0, 0, 0, 3, 232]);
    expected.extend_from_slice(&[0, 0, 0, 0, 0, 4, 151, 200]);
    expected.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 1, b'a', 0, 0, 0, 1, b'b']);
    assert_eq!(value.canonical_bytes(), expected);
    assert_eq!(
        value,
        RegistrationReview::new(
            [17; 32],
            9,
            vec!["a".into(), "b".into()],
            [34; 32],
            1000,
            301000
        )
        .unwrap()
    );
}

#[test]
fn apr02_every_review_binding_field_changes_canonical_bytes() {
    let base = review();
    let changes = [
        RegistrationReview::new(
            [18; 32],
            9,
            vec!["a".into(), "b".into()],
            [34; 32],
            1000,
            301000,
        ),
        RegistrationReview::new(
            [17; 32],
            10,
            vec!["a".into(), "b".into()],
            [34; 32],
            1000,
            301000,
        ),
        RegistrationReview::new(
            [17; 32],
            9,
            vec!["a".into(), "c".into()],
            [34; 32],
            1000,
            301000,
        ),
        RegistrationReview::new(
            [17; 32],
            9,
            vec!["a".into(), "b".into()],
            [35; 32],
            1000,
            301000,
        ),
        RegistrationReview::new(
            [17; 32],
            9,
            vec!["a".into(), "b".into()],
            [34; 32],
            1001,
            301000,
        ),
        RegistrationReview::new(
            [17; 32],
            9,
            vec!["a".into(), "b".into()],
            [34; 32],
            1000,
            300999,
        ),
    ];
    let mut bytes = BTreeSet::from([base.canonical_bytes()]);
    for changed in changes {
        assert!(bytes.insert(changed.unwrap().canonical_bytes()));
    }
}

#[test]
fn apr02_canonical_account_lengths_prevent_concatenation_ambiguity() {
    let make = |accounts| RegistrationReview::new([1; 32], 1, accounts, [2; 32], 1, 2).unwrap();
    let first = make(vec!["ab".into(), "c".into()]);
    let second = make(vec!["a".into(), "bc".into()]);
    assert_eq!(first.account_ids().concat(), second.account_ids().concat());
    assert_ne!(first.canonical_bytes(), second.canonical_bytes());
}

#[test]
fn cli07_review_rejects_invalid_ids_versions_times_and_scopes() {
    let invalid = [
        RegistrationReview::new([0; 32], 1, vec![], [2; 32], 1, 2),
        RegistrationReview::new([1; 32], 1, vec![], [0; 32], 1, 2),
        RegistrationReview::new([1; 32], 0, vec![], [2; 32], 1, 2),
        RegistrationReview::new([1; 32], i64::MAX as u64 + 1, vec![], [2; 32], 1, 2),
        RegistrationReview::new([1; 32], 1, vec![], [2; 32], 0, 2),
        RegistrationReview::new([1; 32], 1, vec![], [2; 32], 2, 2),
        RegistrationReview::new([1; 32], 1, vec![], [2; 32], 3, 2),
        RegistrationReview::new([1; 32], 1, vec![], [2; 32], 1, 300002),
        RegistrationReview::new([1; 32], 1, vec![], [2; 32], i64::MIN, i64::MAX),
        RegistrationReview::new([1; 32], 1, vec!["dup".into(), "dup".into()], [2; 32], 1, 2),
        RegistrationReview::new([1; 32], 1, vec!["".into()], [2; 32], 1, 2),
        RegistrationReview::new([1; 32], 1, vec!["한글".into()], [2; 32], 1, 2),
        RegistrationReview::new([1; 32], 1, vec!["a\n".into()], [2; 32], 1, 2),
        RegistrationReview::new([1; 32], 1, vec!["a".repeat(129)], [2; 32], 1, 2),
        RegistrationReview::new(
            [1; 32],
            1,
            (0..129).map(|i| format!("a{i}")).collect(),
            [2; 32],
            1,
            2,
        ),
    ];
    for value in invalid {
        assert_eq!(value, Err(ErrorCode::InvalidInput));
    }
    let max = RegistrationReview::new(
        [1; 32],
        i64::MAX as u64,
        (0..128)
            .map(|i| format!("{i:03}{}", "a".repeat(125)))
            .collect(),
        [2; 32],
        i64::MAX - 300000,
        i64::MAX,
    )
    .unwrap();
    assert!(max.canonical_bytes().len() < 32768);
}

#[test]
fn sec08_review_and_model_debug_redact_binding_data() {
    let value = RegistrationReview::new(
        [0xab; 32],
        1,
        vec!["private_account_marker".into()],
        [0xcd; 32],
        1,
        2,
    )
    .unwrap();
    let debug = format!(
        "{value:?} {:?}",
        ReviewModel::new(value.clone(), Presentation::defaults(false, false, false))
    );
    for marker in ["private_account_marker", "171", "205", "abab", "cdcd"] {
        assert!(!debug.contains(marker));
    }
}

#[test]
fn inst04_matching_first_paint_is_required_before_verification_intent() {
    let mut value = model();
    assert_eq!(value.state(), ReviewState::AwaitingPaint);
    assert_eq!(
        value.begin_verification(1000, Duration::ZERO),
        ReviewIntent::None
    );
    let wrong = RegistrationReview::new(
        [18; 32],
        9,
        vec!["a".into(), "b".into()],
        [34; 32],
        1000,
        301000,
    )
    .unwrap();
    assert!(!value.paint_completed(&wrong));
    assert_eq!(value.state(), ReviewState::AwaitingPaint);
    assert!(value.paint_completed(&review()));
    assert_eq!(value.state(), ReviewState::Reviewing);
    assert_eq!(
        value.begin_verification(1000, Duration::ZERO),
        ReviewIntent::RequestVerification
    );
    assert_eq!(value.state(), ReviewState::Verifying);
    assert_eq!(
        value.begin_verification(1001, Duration::from_millis(1)),
        ReviewIntent::None
    );
    assert!(!value.paint_completed(&review()));
}

#[test]
fn apr02_utc_and_monotonic_expiry_close_review_without_authority() {
    for (now, elapsed) in [
        (999, Duration::ZERO),
        (301000, Duration::ZERO),
        (1000, Duration::from_secs(300)),
        (i64::MAX, Duration::MAX),
    ] {
        let mut value = model();
        assert!(value.paint_completed(&review()));
        assert_eq!(
            value.begin_verification(now, elapsed),
            ReviewIntent::Expired
        );
        assert_eq!(value.state(), ReviewState::Expired);
        assert_eq!(
            value.begin_verification(1000, Duration::ZERO),
            ReviewIntent::None
        );
    }
    let mut waiting = model();
    assert_eq!(
        waiting.check_expiry(301000, Duration::ZERO),
        ReviewIntent::Expired
    );
    assert!(!waiting.paint_completed(&review()));
}

#[test]
fn apr02_clock_rollback_and_elapsed_rollback_do_not_extend_review() {
    for (now, elapsed) in [
        (1999, Duration::from_secs(2)),
        (2000, Duration::from_millis(999)),
    ] {
        let mut value = model();
        value.paint_completed(&review());
        assert_eq!(
            value.check_expiry(2000, Duration::from_secs(1)),
            ReviewIntent::None
        );
        assert_eq!(
            value.begin_verification(now, elapsed),
            ReviewIntent::Expired
        );
    }
}

#[test]
fn inst04_terminal_ui_intents_cannot_restart_or_become_approval() {
    for (intent, state) in [
        (ReviewIntent::Cancel, ReviewState::Closed),
        (ReviewIntent::Deny, ReviewState::Closed),
        (ReviewIntent::Unavailable, ReviewState::Unavailable),
    ] {
        let mut value = model();
        value.paint_completed(&review());
        assert_eq!(
            value.begin_verification(1000, Duration::ZERO),
            ReviewIntent::RequestVerification
        );
        assert_eq!(
            match intent {
                ReviewIntent::Cancel => value.cancel(),
                ReviewIntent::Deny => value.deny(),
                _ => value.fail(),
            },
            intent
        );
        assert_eq!(value.state(), state);
        assert!(!value.paint_completed(&review()));
        assert_eq!(
            value.begin_verification(1001, Duration::from_millis(1)),
            ReviewIntent::None
        );
        assert_eq!(value.cancel(), ReviewIntent::None);
        assert_eq!(value.deny(), ReviewIntent::None);
        assert_eq!(value.fail(), ReviewIntent::None);
        assert_eq!(
            value.check_expiry(301000, Duration::MAX),
            ReviewIntent::None
        );
        assert_eq!(value.state(), state);
    }
}

#[test]
fn ops07_initial_language_uses_saved_choice_then_os_fallback() {
    assert_eq!(
        Presentation::defaults(true, false, false).language(),
        Language::Ko
    );
    assert_eq!(
        Presentation::defaults(false, false, false).language(),
        Language::En
    );
    assert_eq!(
        Presentation::new(Some(Language::En), Theme::System, true, false, false).language(),
        Language::En
    );
    assert_eq!(
        Presentation::new(Some(Language::Ko), Theme::System, false, false, false).language(),
        Language::Ko
    );
}

#[test]
fn ops08_system_explicit_and_high_contrast_modes_preserve_choice() {
    let mut value = Presentation::defaults(false, false, false);
    assert_eq!(value.theme(), Theme::System);
    assert_eq!(value.effective_theme(), EffectiveTheme::Light);
    value.system_theme_changed(true, false);
    assert_eq!(value.effective_theme(), EffectiveTheme::Dark);
    value.set_theme(Theme::Light);
    assert_eq!(value.effective_theme(), EffectiveTheme::Light);
    value.system_theme_changed(true, true);
    assert_eq!(value.effective_theme(), EffectiveTheme::HighContrast);
    assert!(value.palette().is_none());
    assert_eq!(value.theme(), Theme::Light);
    value.system_theme_changed(true, false);
    assert_eq!(value.effective_theme(), EffectiveTheme::Light);
    value.set_theme(Theme::Dark);
    value.system_theme_changed(false, false);
    assert_eq!(value.effective_theme(), EffectiveTheme::Dark);
}

#[test]
fn ops07_ops08_language_and_theme_changes_preserve_snapshot_and_verification_state() {
    let mut value = model();
    value.paint_completed(&review());
    value.begin_verification(2000, Duration::from_secs(1));
    let before = value.review().canonical_bytes();
    value.set_language(Language::Ko);
    value.set_theme(Theme::Dark);
    value.system_theme_changed(false, true);
    assert_eq!(value.review().canonical_bytes(), before);
    assert_eq!(value.state(), ReviewState::Verifying);
    assert_eq!(value.presentation().language(), Language::Ko);
    assert_eq!(
        value.presentation().effective_theme(),
        EffectiveTheme::HighContrast
    );
    assert_eq!(
        value.check_expiry(1999, Duration::from_secs(2)),
        ReviewIntent::Expired
    );
}

fn luminance(value: Rgb) -> f64 {
    let convert = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * convert(value.r) + 0.7152 * convert(value.g) + 0.0722 * convert(value.b)
}
fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
#[test]
fn ops08_standard_palettes_keep_readable_text_and_focus_contrast() {
    for theme in [Theme::Light, Theme::Dark] {
        let value = Presentation::new(Some(Language::En), theme, false, false, false);
        let p = value.palette().expect("standard palette");
        for (text, background) in [
            (p.text, p.background),
            (p.text, p.surface),
            (p.muted_text, p.surface),
            (p.accent_text, p.accent),
            (p.danger, p.surface),
        ] {
            assert!(contrast(text, background) >= 4.5);
        }
        assert!(contrast(p.focus, p.surface) >= 3.0);
    }
}

#[test]
fn ops07_resource_keys_match_both_languages_and_have_safe_fixed_text() {
    let expected: BTreeSet<_> = TextKey::ALL.iter().map(|key| key.as_str()).collect();
    assert_eq!(expected.len(), TextKey::ALL.len());
    for (language, source) in [
        (Language::En, include_str!("../resources/en.txt")),
        (Language::Ko, include_str!("../resources/ko.txt")),
    ] {
        let mut seen = BTreeSet::new();
        for line in source.lines() {
            let (key, value) = line.split_once('=').expect("key=value");
            assert!(seen.insert(key), "duplicate key");
            assert!(!value.trim().is_empty());
            assert!(!value.chars().any(char::is_control));
            assert!(
                !value.contains('{') && !value.contains('}'),
                "values are rendered separately"
            );
        }
        assert_eq!(seen, expected);
        for key in TextKey::ALL {
            assert!(!text(language, *key).is_empty());
        }
    }
    assert_eq!(text(Language::En, TextKey::English), "English");
    assert_eq!(text(Language::Ko, TextKey::Korean), "한국어");
    assert_ne!(
        text(Language::En, TextKey::Title),
        text(Language::Ko, TextKey::Title)
    );
}
