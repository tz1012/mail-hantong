//! Data and UI intents only; neither type proves user presence or approval.
use hantong_contracts::{ErrorCode, auth::RequestedScope, client::ClientKind};
use hantong_domain::config::{Language, Theme};
use std::{fmt, time::Duration};

const MAX_REVIEW_LIFETIME_MS: i64 = 300_000;
const MAX_REVIEW_ELAPSED: Duration = Duration::from_secs(300);
const REVIEW_DOMAIN: &[u8] = b"mail-hantong/registration-review/v1\0";

#[derive(Clone, Eq, PartialEq)]
pub struct RegistrationReview {
    client_kind: ClientKind,
    request_id: [u8; 32],
    request_version: u64,
    scope: RequestedScope,
    server_public_key_hash: [u8; 32],
    created_at_utc_ms: i64,
    expires_at_utc_ms: i64,
}
impl fmt::Debug for RegistrationReview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RegistrationReview { <redacted> }")
    }
}
impl RegistrationReview {
    pub fn new(
        request_id: [u8; 32],
        request_version: u64,
        account_ids: Vec<String>,
        server_public_key_hash: [u8; 32],
        created_at_utc_ms: i64,
        expires_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        let lifetime = expires_at_utc_ms.checked_sub(created_at_utc_ms);
        if request_id == [0; 32]
            || server_public_key_hash == [0; 32]
            || request_version == 0
            || request_version > i64::MAX as u64
            || created_at_utc_ms <= 0
            || !lifetime.is_some_and(|value| (1..=MAX_REVIEW_LIFETIME_MS).contains(&value))
        {
            return Err(ErrorCode::InvalidInput);
        }
        let scope = RequestedScope::new(account_ids, vec!["mail.accounts.list".into()])?;
        Ok(Self {
            client_kind: ClientKind::Cli,
            request_id,
            request_version,
            scope,
            server_public_key_hash,
            created_at_utc_ms,
            expires_at_utc_ms,
        })
    }
    pub fn new_gui(
        request_id: [u8; 32],
        request_version: u64,
        account_ids: Vec<String>,
        server_public_key_hash: [u8; 32],
        created_at_utc_ms: i64,
        expires_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        let mut review = Self::new(
            request_id,
            request_version,
            account_ids,
            server_public_key_hash,
            created_at_utc_ms,
            expires_at_utc_ms,
        )?;
        review.client_kind = ClientKind::Gui;
        review.scope = RequestedScope::new(
            review.account_ids().to_vec(),
            hantong_contracts::auth::gui_read_tools(),
        )?;
        Ok(review)
    }
    pub fn new_mcp(
        request_id: [u8; 32],
        request_version: u64,
        account_ids: Vec<String>,
        server_public_key_hash: [u8; 32],
        created_at_utc_ms: i64,
        expires_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        let mut review = Self::new(
            request_id,
            request_version,
            account_ids,
            server_public_key_hash,
            created_at_utc_ms,
            expires_at_utc_ms,
        )?;
        review.client_kind = ClientKind::Mcp;
        review.scope = RequestedScope::new(
            review.account_ids().to_vec(),
            hantong_contracts::auth::mcp_read_tools(),
        )?;
        Ok(review)
    }
    pub fn new_mcp_mail(
        request_id: [u8; 32],
        request_version: u64,
        account_ids: Vec<String>,
        server_public_key_hash: [u8; 32],
        created_at_utc_ms: i64,
        expires_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        let mut review = Self::new_mcp(
            request_id,
            request_version,
            account_ids,
            server_public_key_hash,
            created_at_utc_ms,
            expires_at_utc_ms,
        )?;
        review.scope = RequestedScope::new(
            review.account_ids().to_vec(),
            hantong_contracts::auth::mcp_mail_tools(),
        )?;
        Ok(review)
    }
    pub fn request_id(&self) -> &[u8; 32] {
        &self.request_id
    }
    pub fn request_version(&self) -> u64 {
        self.request_version
    }
    pub fn scope(&self) -> &RequestedScope {
        &self.scope
    }
    pub fn account_ids(&self) -> &[String] {
        self.scope.accounts()
    }
    pub fn server_public_key_hash(&self) -> &[u8; 32] {
        &self.server_public_key_hash
    }
    pub fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
    pub fn client_kind(&self) -> ClientKind {
        self.client_kind
    }
    pub fn profile_name(&self) -> &'static str {
        if self.client_kind == ClientKind::Mcp {
            "mcp"
        } else if self.client_kind == ClientKind::Gui {
            "gui"
        } else {
            "default"
        }
    }
    pub fn read_only(&self) -> bool {
        self.scope.tools() != hantong_contracts::auth::mcp_mail_tools()
    }
    pub fn requests_per_minute(&self) -> u32 {
        60
    }
    /// Stable comparison/hash input. These bytes are data, never presence evidence.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let accounts = self.scope.accounts();
        let account_bytes: usize = accounts.iter().map(|value| 4 + value.len()).sum();
        let mut output = Vec::with_capacity(REVIEW_DOMAIN.len() + 92 + account_bytes);
        if self.client_kind == ClientKind::Mcp {
            if self.scope.tools() == hantong_contracts::auth::mcp_mail_tools() {
                output.extend_from_slice(b"mail-hantong/mcp-mail-registration-review/v2\0");
            } else {
                output.extend_from_slice(b"mail-hantong/mcp-registration-review/v1\0");
            }
            output.extend_from_slice(b"mcp\0mcp\0");
            output.extend_from_slice(&(self.scope.tools().len() as u32).to_be_bytes());
            for tool in self.scope.tools() {
                output.extend_from_slice(&(tool.len() as u32).to_be_bytes());
                output.extend_from_slice(tool.as_bytes());
            }
        } else if self.client_kind == ClientKind::Gui {
            output.extend_from_slice(b"mail-hantong/gui-registration-review/v1\0");
            output.extend_from_slice(b"gui\0gui\0");
            output.extend_from_slice(&(self.scope.tools().len() as u32).to_be_bytes());
            for tool in self.scope.tools() {
                output.extend_from_slice(&(tool.len() as u32).to_be_bytes());
                output.extend_from_slice(tool.as_bytes());
            }
        } else {
            output.extend_from_slice(REVIEW_DOMAIN);
        }
        output.extend_from_slice(&self.request_id);
        output.extend_from_slice(&self.request_version.to_be_bytes());
        output.extend_from_slice(&self.server_public_key_hash);
        output.extend_from_slice(&self.created_at_utc_ms.to_be_bytes());
        output.extend_from_slice(&self.expires_at_utc_ms.to_be_bytes());
        output.extend_from_slice(&(accounts.len() as u32).to_be_bytes());
        for account in accounts {
            output.extend_from_slice(&(account.len() as u32).to_be_bytes());
            output.extend_from_slice(account.as_bytes());
        }
        output
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveTheme {
    Light,
    Dark,
    HighContrast,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Palette {
    pub background: Rgb,
    pub surface: Rgb,
    pub text: Rgb,
    pub muted_text: Rgb,
    pub border: Rgb,
    pub accent: Rgb,
    pub accent_text: Rgb,
    pub danger: Rgb,
    pub focus: Rgb,
}
const fn rgb(value: u32) -> Rgb {
    Rgb {
        r: (value >> 16) as u8,
        g: (value >> 8) as u8,
        b: value as u8,
    }
}
static LIGHT_PALETTE: Palette = Palette {
    background: rgb(0xf4f6f9),
    surface: rgb(0xffffff),
    text: rgb(0x18202b),
    muted_text: rgb(0x475569),
    border: rgb(0x8292a6),
    accent: rgb(0x1d4ed8),
    accent_text: rgb(0xffffff),
    danger: rgb(0xb91c1c),
    focus: rgb(0x1d4ed8),
};
static DARK_PALETTE: Palette = Palette {
    background: rgb(0x111827),
    surface: rgb(0x1f2937),
    text: rgb(0xf3f4f6),
    muted_text: rgb(0xcbd5e1),
    border: rgb(0x64748b),
    accent: rgb(0x93c5fd),
    accent_text: rgb(0x111827),
    danger: rgb(0xfca5a5),
    focus: rgb(0xbfdbfe),
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Presentation {
    language: Language,
    theme: Theme,
    os_dark: bool,
    high_contrast: bool,
}
impl Presentation {
    pub fn new(
        language: Option<Language>,
        theme: Theme,
        os_is_korean: bool,
        os_dark: bool,
        high_contrast: bool,
    ) -> Self {
        Self {
            language: language.unwrap_or(if os_is_korean {
                Language::Ko
            } else {
                Language::En
            }),
            theme,
            os_dark,
            high_contrast,
        }
    }
    pub fn defaults(os_is_korean: bool, os_dark: bool, high_contrast: bool) -> Self {
        Self::new(None, Theme::System, os_is_korean, os_dark, high_contrast)
    }
    pub fn language(&self) -> Language {
        self.language
    }
    pub fn theme(&self) -> Theme {
        self.theme
    }
    pub fn effective_theme(&self) -> EffectiveTheme {
        if self.high_contrast {
            return EffectiveTheme::HighContrast;
        }
        match self.theme {
            Theme::Light => EffectiveTheme::Light,
            Theme::Dark => EffectiveTheme::Dark,
            Theme::System if self.os_dark => EffectiveTheme::Dark,
            Theme::System => EffectiveTheme::Light,
        }
    }
    /// None requires the native renderer to resolve current OS high-contrast colors.
    pub fn palette(&self) -> Option<&'static Palette> {
        match self.effective_theme() {
            EffectiveTheme::Light => Some(&LIGHT_PALETTE),
            EffectiveTheme::Dark => Some(&DARK_PALETTE),
            EffectiveTheme::HighContrast => None,
        }
    }
    pub fn set_language(&mut self, language: Language) {
        self.language = language;
    }
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }
    pub fn system_theme_changed(&mut self, os_dark: bool, high_contrast: bool) {
        self.os_dark = os_dark;
        self.high_contrast = high_contrast;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewState {
    AwaitingPaint,
    Reviewing,
    Verifying,
    Closed,
    Expired,
    Unavailable,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// UI effects only. Even RequestVerification is not verified user presence.
pub enum ReviewIntent {
    None,
    RequestVerification,
    Deny,
    Cancel,
    Expired,
    Unavailable,
}
/// Immutable, validated display data. This trait carries no approval authority.
pub trait ReviewSnapshot: Clone + Eq + fmt::Debug {
    fn created_at_utc_ms(&self) -> i64;
    fn expires_at_utc_ms(&self) -> i64;
}
impl ReviewSnapshot for RegistrationReview {
    fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms()
    }
    fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms()
    }
}
impl ReviewSnapshot for crate::AccountSetupReview {
    fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms()
    }
    fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms()
    }
}

#[derive(Debug)]
pub struct ReviewModel<R: ReviewSnapshot = RegistrationReview> {
    review: R,
    presentation: Presentation,
    state: ReviewState,
    last_utc_ms: i64,
    last_elapsed: Duration,
}
impl<R: ReviewSnapshot> ReviewModel<R> {
    pub fn new(review: R, presentation: Presentation) -> Self {
        let last_utc_ms = review.created_at_utc_ms();
        Self {
            review,
            presentation,
            state: ReviewState::AwaitingPaint,
            last_utc_ms,
            last_elapsed: Duration::ZERO,
        }
    }
    pub fn review(&self) -> &R {
        &self.review
    }
    pub fn presentation(&self) -> &Presentation {
        &self.presentation
    }
    pub fn state(&self) -> ReviewState {
        self.state
    }
    /// The native renderer calls this after rendering the matching immutable data.
    /// Matching data alone does not prove that a human saw it or approved it.
    pub fn paint_completed(&mut self, displayed: &R) -> bool {
        if self.state != ReviewState::AwaitingPaint || displayed != &self.review {
            return false;
        }
        self.state = ReviewState::Reviewing;
        true
    }
    /// Emits only an instruction to start OS verification; it is not a grant.
    pub fn begin_verification(&mut self, now_utc_ms: i64, elapsed: Duration) -> ReviewIntent {
        let expiry = self.check_expiry(now_utc_ms, elapsed);
        if expiry != ReviewIntent::None {
            return expiry;
        }
        if self.state != ReviewState::Reviewing {
            return ReviewIntent::None;
        }
        self.state = ReviewState::Verifying;
        ReviewIntent::RequestVerification
    }
    /// The host supplies UTC and elapsed time from its own clocks, never from IPC.
    pub fn check_expiry(&mut self, now_utc_ms: i64, elapsed: Duration) -> ReviewIntent {
        if !self.is_live() {
            return ReviewIntent::None;
        }
        if now_utc_ms < self.last_utc_ms
            || now_utc_ms >= self.review.expires_at_utc_ms()
            || elapsed < self.last_elapsed
            || elapsed >= MAX_REVIEW_ELAPSED
        {
            self.state = ReviewState::Expired;
            return ReviewIntent::Expired;
        }
        self.last_utc_ms = now_utc_ms;
        self.last_elapsed = elapsed;
        ReviewIntent::None
    }
    pub fn cancel(&mut self) -> ReviewIntent {
        self.finish(ReviewState::Closed, ReviewIntent::Cancel)
    }
    pub fn deny(&mut self) -> ReviewIntent {
        self.finish(ReviewState::Closed, ReviewIntent::Deny)
    }
    pub fn fail(&mut self) -> ReviewIntent {
        self.finish(ReviewState::Unavailable, ReviewIntent::Unavailable)
    }
    fn is_live(&self) -> bool {
        matches!(
            self.state,
            ReviewState::AwaitingPaint | ReviewState::Reviewing | ReviewState::Verifying
        )
    }
    fn finish(&mut self, state: ReviewState, intent: ReviewIntent) -> ReviewIntent {
        if !self.is_live() {
            return ReviewIntent::None;
        }
        self.state = state;
        intent
    }
    pub fn set_language(&mut self, language: Language) {
        self.presentation.set_language(language);
    }
    pub fn set_theme(&mut self, theme: Theme) {
        self.presentation.set_theme(theme);
    }
    pub fn system_theme_changed(&mut self, os_dark: bool, high_contrast: bool) {
        self.presentation
            .system_theme_changed(os_dark, high_contrast);
    }
}

#[cfg(test)]
mod mcp_binding_tests {
    #[test]
    fn ai09_mcp_full_mail_proof_has_distinct_domain_and_writable_disclosure() {
        let old =
            super::RegistrationReview::new_mcp([1; 32], 1, vec!["a".into()], [2; 32], 1, 1000)
                .unwrap();
        let new =
            super::RegistrationReview::new_mcp_mail([1; 32], 1, vec!["a".into()], [2; 32], 1, 1000)
                .unwrap();
        assert!(old.read_only());
        assert!(!new.read_only());
        assert!(
            old.canonical_bytes()
                .starts_with(b"mail-hantong/mcp-registration-review/v1\0")
        );
        assert!(
            new.canonical_bytes()
                .starts_with(b"mail-hantong/mcp-mail-registration-review/v2\0")
        );
        assert_ne!(old.canonical_bytes(), new.canonical_bytes());
    }
    use super::*;
    #[test]
    fn mcp03_registration_proof_domain_and_kind_are_not_gui_or_cli() {
        let args = vec!["A".into()];
        let mcp = RegistrationReview::new_mcp([1; 32], 1, args.clone(), [2; 32], 1, 1000).unwrap();
        let gui = RegistrationReview::new_gui([1; 32], 1, args.clone(), [2; 32], 1, 1000).unwrap();
        let cli = RegistrationReview::new([1; 32], 1, args, [2; 32], 1, 1000).unwrap();
        assert_eq!(mcp.client_kind(), ClientKind::Mcp);
        assert_eq!(mcp.profile_name(), "mcp");
        assert_ne!(mcp.canonical_bytes(), gui.canonical_bytes());
        assert_ne!(mcp.canonical_bytes(), cli.canonical_bytes());
        assert_eq!(
            mcp.scope().tools(),
            hantong_contracts::auth::mcp_read_tools()
        );
    }
}
