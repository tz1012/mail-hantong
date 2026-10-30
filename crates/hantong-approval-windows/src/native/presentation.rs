use hantong_approval::{Palette, Presentation, RegistrationReview, Rgb, TextKey, text};
use hantong_contracts::{ErrorCode, wire_hex};
use hantong_domain::config::Language;
use windows::{
    UI::ViewManagement::{UIColorType, UISettings},
    Win32::{
        Foundation::{COLORREF, FILETIME, SYSTEMTIME},
        Globalization::GetUserDefaultUILanguage,
        Graphics::Gdi::{
            COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_WINDOW, COLOR_WINDOWTEXT,
            GetSysColor,
        },
        System::Time::FileTimeToSystemTime,
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            WindowsAndMessaging::{
                SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
            },
        },
    },
};

pub(super) fn os_language_is_korean() -> bool {
    // LANGID's primary-language field; all Korean sublanguages use 0x12.
    unsafe { GetUserDefaultUILanguage() & 0x03ff == 0x12 }
}
pub(super) fn os_colors() -> (bool, bool) {
    let dark = UISettings::new()
        .and_then(|settings| settings.GetColorValue(UIColorType::Background))
        .map(|color| {
            (u32::from(color.R) * 299 + u32::from(color.G) * 587 + u32::from(color.B) * 114)
                < 128_000
        })
        .unwrap_or(false);
    let mut contrast = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let high_contrast = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok()
        && contrast.dwFlags.contains(HCF_HIGHCONTRASTON);
    (dark, high_contrast)
}

#[derive(Clone, Copy)]
pub(super) struct Colors {
    pub(super) background: COLORREF,
    pub(super) surface: COLORREF,
    pub(super) text: COLORREF,
    pub(super) muted: COLORREF,
    pub(super) accent: COLORREF,
    pub(super) accent_text: COLORREF,
    pub(super) border: COLORREF,
}
fn color(rgb: Rgb) -> COLORREF {
    COLORREF(u32::from(rgb.r) | (u32::from(rgb.g) << 8) | (u32::from(rgb.b) << 16))
}
impl From<&Palette> for Colors {
    fn from(p: &Palette) -> Self {
        Self {
            background: color(p.background),
            surface: color(p.surface),
            text: color(p.text),
            muted: color(p.muted_text),
            accent: color(p.accent),
            accent_text: color(p.accent_text),
            border: color(p.border),
        }
    }
}
impl Colors {
    pub(super) fn for_presentation(presentation: &Presentation) -> Self {
        if let Some(palette) = presentation.palette() {
            return palette.into();
        }
        // Respect OS user-selected high-contrast colors, not a hard-coded palette.
        unsafe {
            Self {
                background: COLORREF(GetSysColor(COLOR_WINDOW)),
                surface: COLORREF(GetSysColor(COLOR_WINDOW)),
                text: COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                muted: COLORREF(GetSysColor(COLOR_GRAYTEXT)),
                accent: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                accent_text: COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)),
                border: COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
            }
        }
    }
}
pub(super) fn utc_display(value: i64) -> Result<String, ErrorCode> {
    let ticks = u64::try_from(value)
        .ok()
        .and_then(|v| v.checked_mul(10_000))
        .and_then(|v| v.checked_add(116_444_736_000_000_000))
        .ok_or(ErrorCode::InvalidInput)?;
    let filetime = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut system = SYSTEMTIME::default();
    unsafe { FileTimeToSystemTime(&filetime, &mut system) }.map_err(|_| ErrorCode::InvalidInput)?;
    Ok(format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        system.wYear, system.wMonth, system.wDay, system.wHour, system.wMinute, system.wSecond
    ))
}
pub(super) fn review_text(
    review: &RegistrationReview,
    language: Language,
) -> Result<String, ErrorCode> {
    let t = |key| text(language, key);
    let gui = review.client_kind() == hantong_contracts::client::ClientKind::Gui;
    let tools = if gui {
        format!(
            "{}\r\n{}",
            review.scope().tools().join("\r\n"),
            t(TextKey::GuiTools)
        )
    } else {
        format!("{} (mail.accounts.list)", t(TextKey::AccountsListTool))
    };
    let accounts = if review.account_ids().is_empty() {
        t(TextKey::NoAccounts).to_owned()
    } else {
        review.account_ids().join("\r\n")
    };
    Ok(format!(
        "{}: {}\r\n{}: {}\r\n{}: {}\r\n{}: {}\r\n\r\n{}\r\n{}\r\n\r\n{}\r\n{}\r\n\r\n{}\r\n{}\r\n\r\n{}: {}\r\n\r\n{}\r\n{}",
        t(TextKey::Client),
        t(if gui {
            TextKey::GuiClientValue
        } else {
            TextKey::ClientValue
        }),
        t(TextKey::Profile),
        review.profile_name(),
        t(TextKey::Access),
        t(TextKey::ReadOnly),
        t(TextKey::RateLimit),
        t(TextKey::RateLimitValue),
        t(TextKey::Accounts),
        accounts,
        t(TextKey::Tools),
        tools,
        t(TextKey::ServerFingerprint),
        wire_hex::encode(review.server_public_key_hash()),
        t(TextKey::ExpiresAt),
        utc_display(review.expires_at_utc_ms())?,
        t(TextKey::ScopeNotice),
        t(if gui {
            TextKey::GuiVerificationRequired
        } else {
            TextKey::VerificationRequired
        })
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inst04_gui_review_discloses_exact_local_header_scope_in_both_languages() {
        let review = RegistrationReview::new_gui(
            [1; 32],
            1,
            vec!["account_B".into(), "account_A".into()],
            [2; 32],
            1000,
            2000,
        )
        .unwrap();
        for language in [Language::En, Language::Ko] {
            let rendered = review_text(&review, language).unwrap();
            assert!(rendered.contains("account_A\r\naccount_B"));
            assert!(rendered.contains(text(language, TextKey::GuiClientValue)));
            assert!(rendered.contains(text(language, TextKey::GuiTools)));
            assert!(rendered.contains(text(language, TextKey::GuiVerificationRequired)));
            assert!(!rendered.contains(text(language, TextKey::ClientValue)));
            assert!(!rendered.contains("default"));
            for tool in review.scope().tools() {
                assert!(rendered.contains(tool));
            }
        }
    }
    #[test]
    fn inst04_review_text_displays_exact_frozen_scope_and_server_fingerprint() {
        let review = RegistrationReview::new(
            [1; 32],
            1,
            vec!["account_B".into(), "account_A".into()],
            [0xa5; 32],
            1_700_000_000_000,
            1_700_000_300_000,
        )
        .unwrap();
        for language in [Language::En, Language::Ko] {
            let rendered = review_text(&review, language).unwrap();
            assert!(rendered.contains("account_A\r\naccount_B"));
            assert!(rendered.contains(&"a5".repeat(32)));
            assert!(rendered.contains("2023-11-14 22:18:20 UTC"));
            assert!(rendered.contains(text(language, TextKey::ReadOnly)));
            assert!(rendered.contains(text(language, TextKey::ScopeNotice)));
        }
    }
    #[test]
    fn inst04_empty_account_review_is_explicit_and_invalid_time_fails() {
        let review = RegistrationReview::new([1; 32], 1, vec![], [2; 32], 1000, 2000).unwrap();
        assert!(
            review_text(&review, Language::Ko)
                .unwrap()
                .contains(text(Language::Ko, TextKey::NoAccounts))
        );
        assert!(utc_display(-1).is_err());
        assert!(utc_display(i64::MAX).is_err());
    }
}
