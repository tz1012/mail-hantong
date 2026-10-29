use super::{
    consent::{ConsentOperation, non_verified_error},
    dialog::{DialogBinding, DialogReview, DialogVerified},
    presentation::{Colors, os_colors, os_language_is_korean},
};
use crate::{
    VerifiedImapSetup, VerifiedRegistration,
    binding::{CapturedReview, utc_ms},
    setup_binding::{capture_before_verification, input_error, validate_pending, validate_plan},
};
use hantong_approval::{
    AccountSetupReview, EffectiveTheme, Presentation, RegistrationReview, ReviewIntent,
    ReviewModel, ReviewState, SetupTextKey, TextKey, setup_text, text,
};
use hantong_contracts::ErrorCode;
use hantong_domain::config::{Language, Theme};
use hantong_secrets::{ImapPasswordCapturePlan, PasswordInput, PendingImapCredential};
use std::{
    cell::RefCell,
    sync::{
        OnceLock,
        mpsc::{Receiver, TryRecvError},
    },
    time::{Duration, Instant},
};
use windows::{
    Security::Credentials::UI::UserConsentVerificationResult,
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
                DwmSetWindowAttribute,
            },
            Gdi::*,
        },
        System::{
            LibraryLoader::GetModuleHandleW,
            WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize},
        },
        UI::{
            Controls::{DRAWITEMSTRUCT, EM_SETLIMITTEXT, ODS_DISABLED, ODS_FOCUS, ODT_BUTTON},
            HiDpi::*,
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus, VK_ESCAPE, VK_RETURN},
            WindowsAndMessaging::*,
        },
    },
    core::{HSTRING, w},
};

const CLASS: windows::core::PCWSTR = w!("mail-hantong.registration-review.v1");
const HEADING: i32 = 10;
const INTRO: i32 = 11;
const LANGUAGE_LABEL: i32 = 12;
const ENGLISH: i32 = 13;
const KOREAN: i32 = 14;
const THEME_LABEL: i32 = 15;
const LIGHT: i32 = 16;
const DARK: i32 = 17;
const SYSTEM: i32 = 18;
const REVIEW: i32 = 19;
const STATUS: i32 = 20;
const FOOTER: i32 = 21;
const VERIFY: i32 = 22;
const DENY: i32 = 23;
const CLOSE: i32 = 24;
const PASSWORD_LABEL: i32 = 25;
const PASSWORD_INPUT: u16 = 26;
const TIMER: usize = 1;
const BUTTONS: [i32; 8] = [ENGLISH, KOREAN, LIGHT, DARK, SYSTEM, VERIFY, DENY, CLOSE];

struct Sta;
impl Sta {
    fn initialize() -> Result<Self, ErrorCode> {
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }
            .map_err(|_| ErrorCode::CapabilityUnsupported)?;
        Ok(Self)
    }
}
impl Drop for Sta {
    fn drop(&mut self) {
        unsafe {
            RoUninitialize();
        }
    }
}
struct DpiContext(DPI_AWARENESS_CONTEXT);
impl DpiContext {
    fn enter() -> Self {
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}
impl Drop for DpiContext {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            unsafe {
                SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
}
struct OwnedBrush(HBRUSH);
impl OwnedBrush {
    fn new(color: COLORREF) -> Result<Self, ErrorCode> {
        let brush = unsafe { CreateSolidBrush(color) };
        if brush.0.is_null() {
            Err(ErrorCode::InternalError)
        } else {
            Ok(Self(brush))
        }
    }
}
impl Drop for OwnedBrush {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}
struct OwnedFont(HFONT);
impl OwnedFont {
    fn new(dpi: u32) -> Result<Self, ErrorCode> {
        let font = unsafe {
            CreateFontW(
                -scale(16, dpi),
                0,
                0,
                0,
                FW_NORMAL.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PITCH.0 as u32,
                w!("Segoe UI"),
            )
        };
        if font.0.is_null() {
            Err(ErrorCode::InternalError)
        } else {
            Ok(Self(font))
        }
    }
}
impl Drop for OwnedFont {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}
struct WindowGuard(HWND);
impl Drop for WindowGuard {
    fn drop(&mut self) {
        unsafe {
            if IsWindow(Some(self.0)).as_bool() {
                let _ = DestroyWindow(self.0);
            }
        }
    }
}

struct State {
    model: ReviewModel<DialogReview>,
    binding: Option<DialogBinding>,
    password_plan: Option<ImapPasswordCapturePlan>,
    password_input: Option<PasswordInput>,
    pending: Option<PendingImapCredential>,
    cancellation: Receiver<()>,
    operation: Option<ConsentOperation>,
    cleanup: Option<ConsentOperation>,
    outcome: Option<Result<DialogVerified, ErrorCode>>,
    controls: Vec<(i32, HWND)>,
    colors: Colors,
    background: OwnedBrush,
    surface: OwnedBrush,
    font: OwnedFont,
    dpi: u32,
    first_paint_started: bool,
    last_theme_poll: Instant,
}
impl State {
    fn new(
        binding: DialogBinding,
        password_plan: Option<ImapPasswordCapturePlan>,
        language: Option<Language>,
        theme: Theme,
        cancellation: Receiver<()>,
        dpi: u32,
    ) -> Result<Self, ErrorCode> {
        let (dark, contrast) = os_colors();
        let presentation =
            Presentation::new(language, theme, os_language_is_korean(), dark, contrast);
        let colors = Colors::for_presentation(&presentation);
        Ok(Self {
            model: ReviewModel::new(binding.review(), presentation),
            binding: Some(binding),
            password_plan,
            password_input: None,
            pending: None,
            cancellation,
            operation: None,
            cleanup: None,
            outcome: None,
            controls: Vec::new(),
            colors,
            background: OwnedBrush::new(colors.background)?,
            surface: OwnedBrush::new(colors.surface)?,
            font: OwnedFont::new(dpi)?,
            dpi,
            first_paint_started: false,
            last_theme_poll: Instant::now(),
        })
    }
    fn control(&self, id: i32) -> Option<HWND> {
        self.controls
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, hwnd)| *hwnd)
    }
    fn finish(&mut self, result: Result<DialogVerified, ErrorCode>) {
        // Mark the review dead before dropping/canceling an active OS operation.
        self.model.cancel();
        self.pending.take();
        self.password_plan.take();
        if let Some(input) = self.password_input.take() {
            let _ = input.cancel();
        }
        if let Some(operation) = self.operation.take() {
            operation.cancel();
            self.cleanup = Some(operation);
        }
        if self.outcome.is_none() {
            self.outcome = Some(result);
        }
    }
    fn poll(&mut self, hwnd: HWND) -> Result<bool, ErrorCode> {
        if !matches!(self.cancellation.try_recv(), Err(TryRecvError::Empty)) {
            self.finish(Err(ErrorCode::ApprovalRequired));
            return Ok(true);
        }
        let binding = self.binding.as_ref().ok_or(ErrorCode::InternalError)?;
        let now = utc_ms()?;
        if self.model.check_expiry(now, binding.started().elapsed()) == ReviewIntent::Expired {
            self.finish(Err(ErrorCode::ApprovalExpired));
            return Ok(true);
        }
        if self.last_theme_poll.elapsed() >= Duration::from_secs(1) {
            self.last_theme_poll = Instant::now();
            self.update_system_theme(hwnd)?;
        }
        if let Some(result) = self.operation.as_ref().and_then(ConsentOperation::poll) {
            match result {
                Ok(UserConsentVerificationResult::Verified)
                    if self.model.state() == ReviewState::Verifying =>
                {
                    self.validate_display(hwnd)?;
                    let binding = self.binding.take().ok_or(ErrorCode::InternalError)?;
                    binding.validate(self.model.review())?;
                    // This is the only product issuance branch. Clicks/intents
                    // never enter it: only the private OS completion channel does.
                    let result = binding.into_verified(self.pending.take());
                    self.finish(result);
                }
                Ok(other) => self.finish(Err(non_verified_error(other))),
                Err(error) => self.finish(Err(error)),
            }
            return Ok(true);
        }
        Ok(false)
    }
    fn command(&mut self, hwnd: HWND, id: i32) -> Result<bool, ErrorCode> {
        match id {
            CLOSE => {
                self.finish(Err(ErrorCode::ApprovalRequired));
                return Ok(true);
            }
            DENY => {
                self.model.deny();
                self.finish(Err(ErrorCode::ApprovalDenied));
                return Ok(true);
            }
            ENGLISH => self.model.set_language(Language::En),
            KOREAN => self.model.set_language(Language::Ko),
            LIGHT => self.model.set_theme(Theme::Light),
            DARK => self.model.set_theme(Theme::Dark),
            SYSTEM => self.model.set_theme(Theme::System),
            VERIFY => {
                self.validate_display(hwnd)?;
                let binding = self.binding.as_ref().ok_or(ErrorCode::InternalError)?;
                binding.validate(self.model.review())?;
                match self
                    .model
                    .begin_verification(utc_ms()?, binding.started().elapsed())
                {
                    ReviewIntent::RequestVerification => {
                        self.start_verification(hwnd)?;
                        if self
                            .operation
                            .as_ref()
                            .is_some_and(ConsentOperation::setup_failed)
                        {
                            self.finish(Err(ErrorCode::InternalError));
                            return Ok(true);
                        }
                    }
                    ReviewIntent::Expired => {
                        self.finish(Err(ErrorCode::ApprovalExpired));
                        return Ok(true);
                    }
                    _ => {}
                }
            }
            _ => return Ok(false),
        }
        self.refresh(hwnd)?;
        Ok(false)
    }
    fn start_verification(&mut self, hwnd: HWND) -> Result<(), ErrorCode> {
        let language = self.model.presentation().language();
        match self.model.review() {
            DialogReview::Registration(_) => {
                self.operation = Some(ConsentOperation::start(
                    hwnd,
                    text(language, TextKey::VerificationRequired),
                )?);
            }
            DialogReview::Imap(review) => {
                let input = self.password_input.take().ok_or(ErrorCode::InternalError)?;
                let (pending, operation) = capture_before_verification(
                    || {
                        let pending = input.capture().map_err(input_error)?;
                        validate_pending(review, &pending)?;
                        self.binding
                            .as_ref()
                            .ok_or(ErrorCode::InternalError)?
                            .validate(self.model.review())?;
                        Ok(pending)
                    },
                    || {
                        ConsentOperation::start(
                            hwnd,
                            setup_text(
                                language,
                                if review.is_resume() {
                                    SetupTextKey::ResumeVerificationRequired
                                } else {
                                    SetupTextKey::VerificationRequired
                                },
                            ),
                        )
                    },
                )?;
                self.pending = Some(pending);
                self.operation = Some(operation);
            }
        }
        Ok(())
    }
    fn validate_display(&self, hwnd: HWND) -> Result<(), ErrorCode> {
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() || unsafe { IsIconic(hwnd) }.as_bool() {
            return Err(ErrorCode::ApprovalRequired);
        }
        let body = self.control(REVIEW).ok_or(ErrorCode::InternalError)?;
        let expected: Vec<u16> = self
            .model
            .review()
            .summary(self.model.presentation().language())?
            .encode_utf16()
            .collect();
        let length = unsafe { GetWindowTextLengthW(body) };
        if length < 0 || length as usize != expected.len() || expected.len() > 32 * 1024 {
            return Err(ErrorCode::InternalError);
        }
        let mut actual = vec![0; expected.len() + 1];
        let copied = unsafe { GetWindowTextW(body, &mut actual) };
        if copied as usize != expected.len() || actual[..expected.len()] != expected {
            return Err(ErrorCode::InternalError);
        }
        Ok(())
    }
    fn update_system_theme(&mut self, hwnd: HWND) -> Result<(), ErrorCode> {
        let old = *self.model.presentation();
        let (dark, contrast) = os_colors();
        self.model.system_theme_changed(dark, contrast);
        if old != *self.model.presentation() {
            self.refresh(hwnd)?;
        }
        Ok(())
    }
    fn refresh(&mut self, hwnd: HWND) -> Result<(), ErrorCode> {
        let language = self.model.presentation().language();
        self.colors = Colors::for_presentation(self.model.presentation());
        self.background = OwnedBrush::new(self.colors.background)?;
        self.surface = OwnedBrush::new(self.colors.surface)?;
        for (id, child) in &self.controls {
            let caption = if *id == REVIEW {
                self.model.review().summary(language)?
            } else {
                self.caption(*id)
            };
            unsafe { SetWindowTextW(*child, &HSTRING::from(caption)) }
                .map_err(|_| ErrorCode::InternalError)?;
            unsafe {
                SendMessageW(
                    *child,
                    WM_SETFONT,
                    Some(WPARAM(self.font.0.0 as usize)),
                    Some(LPARAM(0)),
                );
            }
        }
        if let Some(input) = &self.password_input {
            // Only Secrets may access this control's text. It is never in controls.
            input
                .set_font(self.font.0.0 as isize)
                .map_err(input_error)?;
            input
                .set_enabled(self.model.state() == ReviewState::Reviewing)
                .map_err(input_error)?;
        }
        unsafe {
            SetWindowTextW(
                hwnd,
                &HSTRING::from(format!("mail-hantong — {}", self.caption(HEADING))),
            )
            .map_err(|_| ErrorCode::InternalError)?;
            if let Some(verify) = self.control(VERIFY) {
                let _ = EnableWindow(verify, self.model.state() == ReviewState::Reviewing);
            }
            let dark =
                i32::from(self.model.presentation().effective_theme() == EffectiveTheme::Dark);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                (&dark as *const i32).cast(),
                4,
            );
            let (caption, text) =
                if self.model.presentation().effective_theme() == EffectiveTheme::HighContrast {
                    (COLORREF(u32::MAX), COLORREF(u32::MAX))
                } else {
                    (self.colors.background, self.colors.text)
                };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_CAPTION_COLOR,
                (&caption as *const COLORREF).cast(),
                4,
            );
            let _ =
                DwmSetWindowAttribute(hwnd, DWMWA_TEXT_COLOR, (&text as *const COLORREF).cast(), 4);
            // Queue all control paints after the mutable State borrow ends.
            // No RDW_UPDATENOW: synchronous color/owner-draw callbacks must be
            // able to borrow the same State immutably.
            let _ = RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE,
            );
        }
        Ok(())
    }
    fn selected(&self, id: i32) -> bool {
        match id {
            ENGLISH => self.model.presentation().language() == Language::En,
            KOREAN => self.model.presentation().language() == Language::Ko,
            LIGHT => self.model.presentation().theme() == Theme::Light,
            DARK => self.model.presentation().theme() == Theme::Dark,
            SYSTEM => self.model.presentation().theme() == Theme::System,
            VERIFY => true,
            _ => false,
        }
    }
    fn caption(&self, id: i32) -> String {
        if self.model.review().is_setup() {
            let resume = self.model.review().is_resume();
            let setup_key = match id {
                HEADING => Some(if resume {
                    SetupTextKey::ResumeTitle
                } else {
                    SetupTextKey::Title
                }),
                INTRO => Some(if resume {
                    SetupTextKey::ResumeIntroduction
                } else {
                    SetupTextKey::Introduction
                }),
                PASSWORD_LABEL => Some(SetupTextKey::Password),
                VERIFY => Some(if resume {
                    SetupTextKey::ResumeVerifyConnect
                } else {
                    SetupTextKey::VerifyConnect
                }),
                DENY => Some(if resume {
                    SetupTextKey::ResumeDeny
                } else {
                    SetupTextKey::Deny
                }),
                FOOTER => Some(if resume {
                    SetupTextKey::ResumeFooter
                } else {
                    SetupTextKey::Footer
                }),
                STATUS => match self.model.state() {
                    ReviewState::Reviewing
                        if self.model.presentation().effective_theme()
                            != EffectiveTheme::HighContrast =>
                    {
                        Some(if resume {
                            SetupTextKey::ResumeReviewing
                        } else {
                            SetupTextKey::Reviewing
                        })
                    }
                    ReviewState::Expired => Some(if resume {
                        SetupTextKey::ResumeExpired
                    } else {
                        SetupTextKey::Expired
                    }),
                    ReviewState::Unavailable => Some(if resume {
                        SetupTextKey::ResumeUnavailable
                    } else {
                        SetupTextKey::Unavailable
                    }),
                    _ => None,
                },
                _ => None,
            };
            if let Some(key) = setup_key {
                return setup_text(self.model.presentation().language(), key).to_owned();
            }
        }
        let key = match id {
            HEADING => TextKey::Title,
            INTRO => TextKey::Introduction,
            LANGUAGE_LABEL => TextKey::Language,
            ENGLISH => TextKey::English,
            KOREAN => TextKey::Korean,
            THEME_LABEL => TextKey::Theme,
            LIGHT => TextKey::Light,
            DARK => TextKey::Dark,
            SYSTEM => TextKey::System,
            VERIFY => TextKey::Verify,
            DENY => TextKey::Deny,
            CLOSE => TextKey::Cancel,
            FOOTER => TextKey::Footer,
            STATUS => match self.model.state() {
                ReviewState::AwaitingPaint => TextKey::AwaitingPaint,
                ReviewState::Reviewing => {
                    if self.model.presentation().effective_theme() == EffectiveTheme::HighContrast {
                        TextKey::HighContrast
                    } else {
                        TextKey::Reviewing
                    }
                }
                ReviewState::Verifying => TextKey::Verifying,
                ReviewState::Closed => TextKey::Closed,
                ReviewState::Expired => TextKey::Expired,
                ReviewState::Unavailable => TextKey::Unavailable,
            },
            _ => TextKey::Unavailable,
        };
        let caption = text(self.model.presentation().language(), key);
        if matches!(id, ENGLISH | KOREAN | LIGHT | DARK | SYSTEM) && self.selected(id) {
            format!("✓ {caption}")
        } else {
            caption.to_owned()
        }
    }
}

fn scale(value: i32, dpi: u32) -> i32 {
    value.saturating_mul(dpi.clamp(96, 768) as i32) / 96
}
fn module() -> Result<HINSTANCE, ErrorCode> {
    unsafe { GetModuleHandleW(None) }
        .map(|handle| HINSTANCE(handle.0))
        .map_err(|_| ErrorCode::InternalError)
}
fn register_class() -> Result<(), ErrorCode> {
    static CLASS_READY: OnceLock<Result<(), ErrorCode>> = OnceLock::new();
    *CLASS_READY.get_or_init(|| {
        let class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: module()?,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }
                .map_err(|_| ErrorCode::InternalError)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&class) } == 0 {
            Err(ErrorCode::InternalError)
        } else {
            Ok(())
        }
    })
}
fn create_controls(hwnd: HWND, state: &mut State) -> Result<(), ErrorCode> {
    let password_label = state.model.review().is_setup().then_some(PASSWORD_LABEL);
    for id in (HEADING..=REVIEW)
        .chain(password_label)
        .chain(STATUS..=CLOSE)
    {
        let (class, style) = if id == REVIEW {
            (
                w!("EDIT"),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
            )
        } else if BUTTONS.contains(&id) {
            (
                w!("BUTTON"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            )
        } else {
            (w!("STATIC"), WS_CHILD | WS_VISIBLE)
        };
        let child = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!(""),
                style,
                0,
                0,
                0,
                0,
                Some(hwnd),
                Some(HMENU(id as usize as *mut _)),
                Some(module()?),
                None,
            )
        }
        .map_err(|_| ErrorCode::InternalError)?;
        state.controls.push((id, child));
        if id == PASSWORD_LABEL {
            let plan = state.password_plan.take().ok_or(ErrorCode::InternalError)?;
            let DialogReview::Imap(review) = state.model.review() else {
                return Err(ErrorCode::InternalError);
            };
            validate_plan(review, &plan)?;
            state.password_input = Some(
                plan.attach_to_parent(hwnd.0 as isize, PASSWORD_INPUT)
                    .map_err(input_error)?,
            );
        }
    }
    if let Some(body) = state.control(REVIEW) {
        unsafe {
            SendMessageW(body, EM_SETLIMITTEXT, Some(WPARAM(32 * 1024)), None);
        }
    }
    state.refresh(hwnd)?;
    layout(hwnd, state)
}
fn layout(hwnd: HWND, state: &State) -> Result<(), ErrorCode> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.map_err(|_| ErrorCode::InternalError)?;
    let p = |n| scale(n, state.dpi);
    let width = rect.right.max(p(600));
    let height = rect.bottom.max(p(470));
    let full = width - p(40);
    let setup = state.model.review().is_setup();
    for (id, child) in &state.controls {
        let (x, y, w, h) = match *id {
            HEADING => (p(20), p(16), full, p(28)),
            INTRO => (p(20), p(48), full, p(42)),
            LANGUAGE_LABEL => (p(20), p(100), p(100), p(28)),
            ENGLISH => (p(124), p(94), p(126), p(36)),
            KOREAN => (p(258), p(94), p(126), p(36)),
            THEME_LABEL => (p(20), p(146), p(100), p(28)),
            LIGHT => (p(124), p(140), p(112), p(36)),
            DARK => (p(244), p(140), p(112), p(36)),
            SYSTEM => (p(364), p(140), p(208), p(36)),
            REVIEW => (
                p(20),
                p(190),
                full,
                (height - p(if setup { 430 } else { 335 })).max(p(100)),
            ),
            PASSWORD_LABEL => (p(20), height - p(230), full, p(28)),
            STATUS => (p(20), height - p(130), full, p(30)),
            FOOTER => (p(20), height - p(98), full, p(32)),
            VERIFY => (p(20), height - p(56), p(270), p(38)),
            DENY => (width - p(324), height - p(56), p(178), p(38)),
            CLOSE => (width - p(138), height - p(56), p(118), p(38)),
            _ => continue,
        };
        // MoveWindow(true) paints synchronously while our caller holds RefMut,
        // preventing nested color/owner-draw callbacks from borrowing State.
        unsafe { MoveWindow(*child, x, y, w, h, false) }.map_err(|_| ErrorCode::InternalError)?;
    }
    if let Some(input) = &state.password_input {
        input
            .set_bounds(p(20), height - p(198), full, p(36))
            .map_err(input_error)?;
    }
    Ok(())
}
pub(super) fn run(
    review: RegistrationReview,
    language: Option<Language>,
    theme: Theme,
    cancellation: Receiver<()>,
) -> Result<VerifiedRegistration, ErrorCode> {
    match run_dialog(
        DialogReview::Registration(review),
        None,
        language,
        theme,
        cancellation,
    )? {
        DialogVerified::Registration(verified) => Ok(*verified),
        DialogVerified::Imap(_) => Err(ErrorCode::InternalError),
    }
}

pub(super) fn run_setup(
    review: AccountSetupReview,
    plan: ImapPasswordCapturePlan,
    language: Option<Language>,
    theme: Theme,
    cancellation: Receiver<()>,
) -> Result<VerifiedImapSetup, ErrorCode> {
    validate_plan(&review, &plan)?;
    match run_dialog(
        DialogReview::Imap(review),
        Some(plan),
        language,
        theme,
        cancellation,
    )? {
        DialogVerified::Imap(verified) => Ok(*verified),
        DialogVerified::Registration(_) => Err(ErrorCode::InternalError),
    }
}

fn run_dialog(
    review: DialogReview,
    plan: Option<ImapPasswordCapturePlan>,
    language: Option<Language>,
    theme: Theme,
    cancellation: Receiver<()>,
) -> Result<DialogVerified, ErrorCode> {
    let _sta = Sta::initialize()?;
    let _dpi = DpiContext::enter();
    register_class()?;
    if !matches!(cancellation.try_recv(), Err(TryRecvError::Empty)) {
        return Err(ErrorCode::ApprovalRequired);
    }
    let binding = match review {
        DialogReview::Registration(review) => {
            DialogBinding::Registration(CapturedReview::capture(review)?)
        }
        DialogReview::Imap(review) => DialogBinding::Imap(CapturedReview::capture(review)?),
    };
    let dpi = unsafe { GetDpiForSystem() }.clamp(96, 768);
    // The allocation outlives CreateWindow, all synchronous/reentrant callbacks,
    // DestroyWindow and WM_NCDESTROY. The WndProc never frees it.
    let state = Box::new(RefCell::new(State::new(
        binding,
        plan,
        language,
        theme,
        cancellation,
        dpi,
    )?));
    let state_ptr = std::ptr::from_ref(state.as_ref());
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            CLASS,
            w!("mail-hantong"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            scale(760, dpi),
            scale(760, dpi),
            None,
            None,
            Some(module()?),
            Some(state_ptr.cast()),
        )
    }
    .map_err(|_| ErrorCode::InternalError)?;
    let guard = WindowGuard(hwnd);
    create_controls(hwnd, &mut state.borrow_mut())?;
    if unsafe { SetTimer(Some(hwnd), TIMER, 100, None) } == 0 {
        return Err(ErrorCode::InternalError);
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = UpdateWindow(hwnd);
    }
    if let Some(input) = &state.borrow().password_input {
        input.focus().map_err(input_error)?;
    } else if let Some(body) = state.borrow().control(REVIEW) {
        unsafe {
            let _ = SetFocus(Some(body));
        }
    }
    let mut message = MSG::default();
    loop {
        let status = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if status == 0 {
            break;
        }
        if status == -1 {
            state.borrow_mut().finish(Err(ErrorCode::InternalError));
            break;
        }
        if message.message == WM_KEYDOWN {
            if message.wParam.0 == usize::from(VK_ESCAPE.0) {
                unsafe {
                    SendMessageW(hwnd, WM_CLOSE, None, None);
                }
                continue;
            }
            if message.wParam.0 == usize::from(VK_RETURN.0) {
                let focus = unsafe { GetFocus() };
                let id = state
                    .borrow()
                    .controls
                    .iter()
                    .find(|(id, child)| *child == focus && BUTTONS.contains(id))
                    .map(|(id, _)| *id);
                if let Some(id) = id {
                    unsafe {
                        SendMessageW(
                            hwnd,
                            WM_COMMAND,
                            Some(WPARAM(id as usize)),
                            Some(LPARAM(focus.0 as isize)),
                        );
                    }
                }
                continue;
            }
        }
        if !unsafe { IsDialogMessageW(hwnd, &message) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
    if state.borrow().outcome.is_none() {
        state.borrow_mut().finish(Err(ErrorCode::ApprovalRequired));
    }
    drop(guard); // clears GWLP_USERDATA before the owning Box can be released
    let cleanup = state.borrow_mut().cleanup.take();
    if let Some(operation) = cleanup {
        cleanup_operation(operation);
    }
    state
        .borrow_mut()
        .outcome
        .take()
        .unwrap_or(Err(ErrorCode::ApprovalRequired))
}

fn redraw_controls(hwnd: HWND) {
    unsafe {
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_ERASE,
        );
    }
}

fn cleanup_operation(operation: ConsentOperation) {
    // This existing STA remains available for WinRT completion while the closed
    // review cannot issue a token. Bound cleanup to five seconds so an OS that
    // never completes cancellation cannot strand the application/window gate.
    // On that limit we release the reference without Close on Started. A
    // registered callback can still close later; failed handler registration
    // has no guaranteed terminal observer after this bounded fallback ends.
    let started = Instant::now();
    let mut message = MSG::default();
    while !operation.cleanup_finished(started.elapsed()) {
        unsafe {
            let _ = MsgWaitForMultipleObjectsEx(None, 50, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            // Bound each drain so a flood of messages cannot defeat the limit.
            for _ in 0..64 {
                if !PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    break;
                }
                if message.message != WM_QUIT {
                    let _ = TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_message(hwnd, message, wparam, lparam)
    })) {
        Ok(result) => result,
        Err(_) => {
            // Do not unwind through Win32. A panic can only close this review;
            // it is never translated into a successful verification result.
            let pointer =
                unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const RefCell<State>;
            if let Some(cell) = unsafe { pointer.as_ref() }
                && let Ok(mut state) = cell.try_borrow_mut()
            {
                state.finish(Err(ErrorCode::InternalError));
            }
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
    }
}
fn handle_message(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        if lparam.0 == 0 {
            return LRESULT(0);
        }
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    }
    if message == WM_NCDESTROY {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    if message == WM_DESTROY {
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER);
            PostQuitMessage(0);
        }
        return LRESULT(0);
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const RefCell<State>;
    let Some(cell) = (unsafe { pointer.as_ref() }) else {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    if message == WM_PAINT {
        return paint(hwnd, cell);
    }
    if matches!(
        message,
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORBTN
    ) && let Ok(state) = cell.try_borrow()
    {
        let hdc = HDC(wparam.0 as *mut _);
        let is_body = state.control(REVIEW) == Some(HWND(lparam.0 as *mut _))
            || state
                .password_input
                .as_ref()
                .is_some_and(|input| input.matches_child_hwnd(lparam.0));
        unsafe {
            SetTextColor(hdc, state.colors.text);
            SetBkColor(
                hdc,
                if is_body {
                    state.colors.surface
                } else {
                    state.colors.background
                },
            );
        }
        return LRESULT(if is_body {
            state.surface.0.0
        } else {
            state.background.0.0
        } as isize);
    }
    if message == WM_DRAWITEM
        && lparam.0 != 0
        && let Ok(state) = cell.try_borrow()
    {
        let draw = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
        if draw.CtlType == ODT_BUTTON && state.control(draw.CtlID as i32) == Some(draw.hwndItem) {
            draw_button(&state, draw);
            return LRESULT(1);
        }
    }
    let Ok(mut state) = cell.try_borrow_mut() else {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    let result: Result<bool, ErrorCode> = match message {
        WM_COMMAND if (wparam.0 >> 16) as u16 == BN_CLICKED as u16 => {
            let id = (wparam.0 & 0xffff) as i32;
            if BUTTONS.contains(&id) && state.control(id) == Some(HWND(lparam.0 as *mut _)) {
                state.command(hwnd, id)
            } else {
                Ok(false)
            }
        }
        WM_TIMER if wparam.0 == TIMER => state.poll(hwnd),
        WM_CLOSE => {
            state.finish(Err(ErrorCode::ApprovalRequired));
            Ok(true)
        }
        WM_SIZE => layout(hwnd, &state).map(|_| false),
        WM_SETTINGCHANGE | WM_THEMECHANGED => state.update_system_theme(hwnd).map(|_| false),
        WM_SYSCOLORCHANGE => state
            .update_system_theme(hwnd)
            .and_then(|_| state.refresh(hwnd))
            .map(|_| false),
        WM_GETMINMAXINFO => {
            if lparam.0 != 0 {
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                info.ptMinTrackSize.x = scale(700, state.dpi);
                info.ptMinTrackSize.y = scale(600, state.dpi);
            }
            Ok(false)
        }
        WM_DPICHANGED => {
            state.dpi = (wparam.0 & 0xffff) as u32;
            match OwnedFont::new(state.dpi) {
                Ok(font) => {
                    let old = std::mem::replace(&mut state.font, font);
                    let refresh = state.refresh(hwnd);
                    drop(old);
                    if lparam.0 != 0 {
                        let rect = unsafe { &*(lparam.0 as *const RECT) };
                        unsafe {
                            let _ = SetWindowPos(
                                hwnd,
                                None,
                                rect.left,
                                rect.top,
                                rect.right - rect.left,
                                rect.bottom - rect.top,
                                SWP_NOACTIVATE | SWP_NOZORDER,
                            );
                        }
                    }
                    refresh.and_then(|_| layout(hwnd, &state)).map(|_| false)
                }
                Err(error) => Err(error),
            }
        }
        _ => {
            drop(state);
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
    };
    let close = match result {
        Ok(close) => close,
        Err(error) => {
            state.finish(Err(error));
            true
        }
    };
    drop(state); // DestroyWindow synchronously re-enters the WndProc.
    if close {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    } else if matches!(message, WM_SIZE | WM_DPICHANGED) {
        redraw_controls(hwnd);
    }
    LRESULT(0)
}
fn paint(hwnd: HWND, cell: &RefCell<State>) -> LRESULT {
    let Ok(state) = cell.try_borrow() else {
        return unsafe { DefWindowProcW(hwnd, WM_PAINT, WPARAM(0), LPARAM(0)) };
    };
    let brush = state.background.0;
    drop(state);
    let mut paint = PAINTSTRUCT::default();
    let painted = unsafe {
        let hdc = BeginPaint(hwnd, &mut paint);
        let filled = !hdc.0.is_null() && FillRect(hdc, &paint.rcPaint, brush) != 0;
        EndPaint(hwnd, &paint).as_bool() && filled
    };
    if !painted {
        fail_paint(hwnd, cell);
        return LRESULT(0);
    }
    let first = if let Ok(mut state) = cell.try_borrow_mut() {
        let first = !state.first_paint_started && state.control(REVIEW).is_some();
        state.first_paint_started |= first;
        first
    } else {
        false
    };
    if first {
        // Complete child rendering while no RefCell borrow is held. This permits
        // synchronous WM_CTLCOLOR callbacks to use the chosen first-frame theme.
        let children_painted = unsafe {
            RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW,
            )
            .as_bool()
        };
        if !children_painted {
            fail_paint(hwnd, cell);
            return LRESULT(0);
        }
        if let Ok(mut state) = cell.try_borrow_mut() {
            let review = state.model.review().clone();
            let displayed = state.validate_display(hwnd);
            if displayed.is_err()
                || (state.model.paint_completed(&review) && state.refresh(hwnd).is_err())
            {
                state.finish(Err(ErrorCode::InternalError));
                drop(state);
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        }
    }
    LRESULT(0)
}
fn fail_paint(hwnd: HWND, cell: &RefCell<State>) {
    if let Ok(mut state) = cell.try_borrow_mut() {
        state.finish(Err(ErrorCode::InternalError));
    }
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}
fn draw_button(state: &State, draw: &DRAWITEMSTRUCT) {
    let enabled = draw.itemState.0 & ODS_DISABLED.0 == 0;
    let selected = state.selected(draw.CtlID as i32) && enabled;
    let fill = if selected {
        state.colors.accent
    } else {
        state.colors.surface
    };
    let foreground = if !enabled {
        state.colors.muted
    } else if selected {
        state.colors.accent_text
    } else {
        state.colors.text
    };
    let Ok(brush) = OwnedBrush::new(fill) else {
        return;
    };
    let Ok(border) = OwnedBrush::new(state.colors.border) else {
        return;
    };
    let mut caption: Vec<u16> = state.caption(draw.CtlID as i32).encode_utf16().collect();
    let mut rect = draw.rcItem;
    unsafe {
        FillRect(draw.hDC, &rect, brush.0);
        FrameRect(draw.hDC, &rect, border.0);
        let previous = SelectObject(draw.hDC, HGDIOBJ(state.font.0.0));
        SetTextColor(draw.hDC, foreground);
        SetBkColor(draw.hDC, fill);
        DrawTextW(
            draw.hDC,
            &mut caption,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(draw.hDC, previous);
        if draw.itemState.0 & ODS_FOCUS.0 != 0 {
            rect.left += 3;
            rect.right -= 3;
            rect.top += 3;
            rect.bottom -= 3;
            let _ = DrawFocusRect(draw.hDC, &rect);
        }
    }
}
