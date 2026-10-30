use super::*;
use std::collections::BTreeMap;

#[test]
fn ops07_desktop_first_language_uses_os_unless_explicitly_selected() {
    for os_korean in [false, true] {
        for language in [None, Some(Language::English), Some(Language::Korean)] {
            let options = ShellOptions {
                language,
                ..Default::default()
            };
            let model = Model::new(options, os_korean, false, false);
            let expected = language.unwrap_or(if os_korean {
                Language::Korean
            } else {
                Language::English
            });
            assert_eq!(model.language(), expected);
            assert_eq!(model.theme(), Theme::System);
            assert_eq!(model.selected(), Navigation::Inbox);
            assert_eq!(model.status(), CoreStatus::Checking);
        }
    }
}

#[test]
fn ops08_desktop_explicit_theme_survives_os_changes_and_high_contrast() {
    for (theme, normal) in [
        (Theme::Light, EffectiveTheme::Light),
        (Theme::Dark, EffectiveTheme::Dark),
    ] {
        let mut model = Model::new(
            ShellOptions {
                language: Some(Language::Korean),
                theme,
            },
            false,
            false,
            false,
        );
        model.set_selected(Navigation::Drafts);
        model.set_status(CoreStatus::Available { mail_ready: false });
        for os_dark in [true, false] {
            model.update_os(os_dark, false);
            assert_eq!(model.effective_theme(), normal);
            model.update_os(os_dark, true);
            assert_eq!(model.effective_theme(), EffectiveTheme::HighContrast);
            assert_eq!(palette(model.effective_theme()), None);
            model.update_os(os_dark, false);
            assert_eq!(model.effective_theme(), normal);
            assert_eq!(model.theme(), theme);
            assert_eq!(model.language(), Language::Korean);
            assert_eq!(model.selected(), Navigation::Drafts);
            assert_eq!(model.status(), CoreStatus::Available { mail_ready: false });
        }
    }
}

#[test]
fn ops08_desktop_system_theme_and_user_changes_preserve_working_state() {
    let mut model = Model::new(ShellOptions::default(), false, false, false);
    model.set_selected(Navigation::Scheduled);
    model.set_status(CoreStatus::Unavailable(ErrorCode::CoreUnavailable));
    model.update_os(true, false);
    assert_eq!(model.effective_theme(), EffectiveTheme::Dark);
    model.set_language(Language::Korean);
    model.update_os(false, true);
    model.set_theme(Theme::Dark);
    assert_eq!(model.effective_theme(), EffectiveTheme::HighContrast);
    model.update_os(false, false);
    assert_eq!(model.effective_theme(), EffectiveTheme::Dark);
    model.set_theme(Theme::System);
    assert_eq!(model.effective_theme(), EffectiveTheme::Light);
    assert_eq!(model.language(), Language::Korean);
    assert_eq!(model.selected(), Navigation::Scheduled);
    assert_eq!(
        model.status(),
        CoreStatus::Unavailable(ErrorCode::CoreUnavailable)
    );
}

#[test]
fn ops02_desktop_readiness_is_separate_from_display_choices() {
    let mut model = Model::new(ShellOptions::default(), false, false, false);
    model.set_selected(Navigation::Settings);
    model.set_language(Language::Korean);
    model.set_theme(Theme::Dark);
    for status in [
        CoreStatus::Checking,
        CoreStatus::Available { mail_ready: false },
        CoreStatus::Available { mail_ready: true },
        CoreStatus::Unavailable(ErrorCode::CoreUnavailable),
        CoreStatus::Unavailable(ErrorCode::InvalidInput),
    ] {
        model.set_status(status);
        assert_eq!(model.status(), status);
        assert_eq!(model.selected(), Navigation::Settings);
        assert_eq!(model.language(), Language::Korean);
        assert_eq!(model.theme(), Theme::Dark);
    }
}

fn entries(source: &'static str) -> BTreeMap<&'static str, &'static str> {
    let mut result = BTreeMap::new();
    for line in source.lines() {
        let (key, value) = line.split_once('=').expect("resource entry requires =");
        assert!(!key.is_empty() && !value.is_empty());
        assert!(key.bytes().all(|c| c.is_ascii_lowercase() || c == b'_'));
        assert_eq!(value, value.trim(), "resource value has outer whitespace");
        assert!(!value.chars().any(char::is_control));
        assert!(result.insert(key, value).is_none(), "duplicate key: {key}");
    }
    result
}

#[test]
fn ops07_desktop_both_languages_have_every_required_key_without_fallback() {
    let english = entries(ENGLISH);
    let korean = entries(KOREAN);
    assert_eq!(
        english.keys().collect::<Vec<_>>(),
        korean.keys().collect::<Vec<_>>()
    );
    let mut required = vec![
        "registration_status",
        "folders_next",
        "loading",
        "registration_pending",
        "registration_denied",
        "registration_expired",
        "registration_provisioning",
        "registration_ready",
        "accounts_loaded",
        "folders_loaded",
        "messages_loaded",
        "metadata_unobserved",
        "cached_notice",
        "subject_empty",
        "read_state",
        "read_yes",
        "read_no",
        "favorite_state",
        "yes",
        "no",
        "metadata_only",
        "filter",
        "filter_all",
        "filter_unread",
        "filter_favorite",
        "reload_folder",
        "brand",
        "title",
        "list_title",
        "detail_title",
        "setup_title",
        "setup_body",
        "connection_checking",
        "connection_available",
        "connection_offline",
        "mail_ready",
        "mail_not_ready",
        "gui_not_registered",
        "refresh",
        "connect",
        "account",
        "folder",
        "next_page",
        "language",
        "theme",
        "theme_light",
        "theme_dark",
        "theme_system",
        "language_en",
        "language_ko",
        "session_preferences",
        "column_sender",
        "column_subject",
        "column_date",
        "detail_empty",
        "status_error",
        "startup_error",
        "compact_note",
        "empty_title",
        "empty_hint",
        "empty_matches",
        "navigation_note",
    ];
    required.extend(Navigation::ALL.iter().map(|navigation| navigation.key()));
    required.sort_unstable();
    assert_eq!(english.keys().copied().collect::<Vec<_>>(), required);
    for (language, values) in [(Language::English, english), (Language::Korean, korean)] {
        for (key, value) in values {
            assert_eq!(
                lookup(
                    match language {
                        Language::English => ENGLISH,
                        Language::Korean => KOREAN,
                    },
                    key
                ),
                Some(value)
            );
            assert_eq!(text(language, key), value);
            assert_ne!(value, MISSING_TEXT);
        }
        assert_eq!(text(language, "brand"), "mail-hantong");
        assert_eq!(text(language, "language_en"), "English");
        assert_eq!(text(language, "language_ko"), "한국어");
        assert_eq!(text(language, "__missing_resource_key"), MISSING_TEXT);
    }
}

#[test]
fn ops07_desktop_navigation_has_unique_localized_entries_and_keeps_selection() {
    assert_eq!(Navigation::ALL.len(), 13);
    let mut model = Model::new(ShellOptions::default(), false, false, false);
    for (index, navigation) in Navigation::ALL.iter().copied().enumerate() {
        assert!(!Navigation::ALL[..index].contains(&navigation));
        model.set_selected(navigation);
        for language in [Language::Korean, Language::English] {
            model.set_language(language);
            assert_eq!(model.selected(), navigation);
            assert!(
                lookup(
                    match language {
                        Language::English => ENGLISH,
                        Language::Korean => KOREAN,
                    },
                    navigation.key()
                )
                .is_some()
            );
        }
    }
}

fn luminance(rgb: Rgb) -> f64 {
    let channel = |value: u8| {
        let scaled = f64::from(value) / 255.0;
        if scaled <= 0.04045 {
            scaled / 12.92
        } else {
            ((scaled + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b)
}
fn contrast(a: Rgb, b: Rgb) -> f64 {
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
#[test]
fn ops08_desktop_palette_text_and_focus_meet_contrast_targets() {
    for theme in [EffectiveTheme::Light, EffectiveTheme::Dark] {
        let p = palette(theme).unwrap();
        for background in [p.background, p.surface, p.raised, p.selected] {
            assert!(contrast(p.text, background) >= 4.5, "text {theme:?}");
            assert!(contrast(p.focus, background) >= 3.0, "focus {theme:?}");
        }
        for background in [p.background, p.surface, p.raised] {
            assert!(contrast(p.muted, background) >= 4.5, "muted {theme:?}");
        }
        assert!(contrast(p.accent, p.background) >= 3.0, "accent {theme:?}");
        assert!(
            contrast(p.on_accent, p.accent) >= 4.5,
            "on accent {theme:?}"
        );
        for background in [p.background, p.surface, p.raised] {
            assert!(contrast(p.border, background) >= 3.0, "border {theme:?}");
        }
    }
    assert!(palette(EffectiveTheme::HighContrast).is_none());
}
