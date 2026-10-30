use super::{
    appearance::*,
    probe::{ActionProbe, StatusProbe},
    *,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::OnceLock,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::{
            Dwm::{
                DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
                DwmSetWindowAttribute,
            },
            Gdi::*,
        },
        System::{LibraryLoader::GetModuleHandleW, SystemServices::SS_NOPREFIX},
        UI::{
            Controls::{DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODT_BUTTON},
            HiDpi::*,
            Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus, VK_RETURN},
            WindowsAndMessaging::*,
        },
    },
    core::{HSTRING, w},
};
mod layout;
use layout::Layout;

const CLASS: windows::core::PCWSTR = w!("mail-hantong.desktop-shell.v1");
const BRAND: i32 = 10;
const SUBTITLE: i32 = 11;
const LANGUAGE_LABEL: i32 = 12;
const LANGUAGE: i32 = 13;
const THEME_LABEL: i32 = 14;
const THEME: i32 = 15;
const NAV_LABEL: i32 = 16;
const NAVIGATION: i32 = 17;
const LIST_TITLE: i32 = 18;
const SECTION: i32 = 19;
const SETUP_TITLE: i32 = 20;
const SETUP_BODY: i32 = 21;
const ACCESS_NOTICE: i32 = 22;
const DETAIL_TITLE: i32 = 23;
const DETAIL_BODY: i32 = 24;
const COMPACT: i32 = 25;
const STATUS: i32 = 26;
const MAIL_STATUS: i32 = 27;
const REFRESH: i32 = 28;
const PREFERENCES: i32 = 29;
const CONNECT: i32 = 30;
const ACCOUNT: i32 = 31;
const FOLDER: i32 = 32;
const NEXT_PAGE: i32 = 33;
const REGISTRATION_STATUS: i32 = 34;
const MESSAGE_LIST: i32 = 35;
const ACCOUNT_LABEL: i32 = 36;
const FOLDER_LABEL: i32 = 37;
const FOLDERS_NEXT: i32 = 38;
const TIMER: usize = 1;
const FIRST_PAINT: u32 = WM_APP + 1;
const DEFERRED_VISUAL: u32 = WM_APP + 2;

#[derive(Clone)]
enum Notice {
    Initial,
    Loading,
    Registration(hantong_contracts::registration::RegistrationProgress),
    Accounts(usize),
    Folders(usize),
    Messages(usize),
    Unobserved,
    Error(ErrorCode),
}
impl Notice {
    fn caption(&self, lang: Language) -> String {
        use hantong_contracts::registration::RegistrationProgress::*;
        let key = match self {
            Self::Initial => "setup_body",
            Self::Loading => "loading",
            Self::Registration(Pending) => "registration_pending",
            Self::Registration(Denied) => "registration_denied",
            Self::Registration(Expired) => "registration_expired",
            Self::Registration(Provisioning) => "registration_provisioning",
            Self::Registration(Ready) => "registration_ready",
            Self::Accounts(_) => "accounts_loaded",
            Self::Folders(_) => "folders_loaded",
            Self::Messages(_) => "messages_loaded",
            Self::Unobserved => "metadata_unobserved",
            Self::Error(_) => "status_error",
        };
        match self {
            Self::Accounts(n) | Self::Folders(n) | Self::Messages(n) => {
                format!("{}: {n}", text(lang, key))
            }
            Self::Error(e) => format!("{}: {}", text(lang, key), e.as_str()),
            _ => text(lang, key).into(),
        }
    }
}
fn detail_caption(workspace: &MailWorkspace, lang: Language) -> String {
    let Some(row) = workspace.selected_message() else {
        return text(lang, "detail_empty").into();
    };
    format!(
        "{}: {}\r\n\r\n{}: {}\r\n\r\n{}: {}\r\n\r\n{}: {}\r\n{}: {}\r\n\r\n{}",
        text(lang, "column_sender"),
        row.sender,
        text(lang, "column_subject"),
        row.subject,
        text(lang, "column_date"),
        row.date_utc,
        text(lang, "read_state"),
        text(lang, if row.is_read { "read_yes" } else { "read_no" }),
        text(lang, "favorite_state"),
        text(lang, if row.is_favorite { "yes" } else { "no" }),
        text(lang, "metadata_only")
    )
}

struct Visual {
    colors: Colors,
    background: Brush,
    surface: Brush,
    border: Brush,
    fonts: Fonts,
    dpi: u32,
}
impl Visual {
    fn new(theme: EffectiveTheme, dpi: u32) -> Result<Self, ErrorCode> {
        let colors = Colors::new(theme);
        Ok(Self {
            colors,
            background: Brush::new(colors.background)?,
            surface: Brush::new(colors.surface)?,
            border: Brush::new(colors.border)?,
            fonts: Fonts::new(dpi)?,
            dpi,
        })
    }
    fn font(&self, id: i32) -> HFONT {
        match id {
            BRAND => self.fonts.brand.0,
            LIST_TITLE | DETAIL_TITLE | SETUP_TITLE => self.fonts.heading.0,
            _ => self.fonts.body.0,
        }
    }
}
struct State {
    model: Model,
    visual: Rc<Visual>,
    controls: Vec<(i32, HWND)>,
    probe: StatusProbe,
    action: ActionProbe,
    notice: Notice,
    registration_requested: bool,
    accounts_after_probe: bool,
    workspace: MailWorkspace,
    painted: bool,
    rendered_language: Option<Language>,
    last_theme_poll: Instant,
}
impl State {
    fn new(
        options: ShellOptions,
        probe: Arc<dyn Fn() -> CoreStatus + Send + Sync>,
        callbacks: ShellCallbacks,
        dpi: u32,
    ) -> Result<Self, ErrorCode> {
        let (dark, contrast) = os_colors();
        let model = Model::new(options, os_korean(), dark, contrast);
        let visual = Rc::new(Visual::new(model.effective_theme(), dpi)?);
        Ok(Self {
            model,
            visual,
            controls: Vec::new(),
            probe: StatusProbe::new(probe),
            action: ActionProbe::new(callbacks.action),
            notice: Notice::Initial,
            registration_requested: false,
            accounts_after_probe: false,
            workspace: MailWorkspace::default(),
            painted: false,
            rendered_language: None,
            last_theme_poll: Instant::now(),
        })
    }
    fn control(&self, id: i32) -> Option<HWND> {
        self.controls
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, hwnd)| *hwnd)
    }
    fn busy(&self) -> bool {
        self.probe.pending() || self.action.pending()
    }
}
struct Context {
    state: RefCell<State>,
    applying: Cell<bool>,
    closed: Cell<bool>,
    error: Cell<Option<ErrorCode>>,
    pending_visual: Cell<bool>,
    pending_dpi: Cell<Option<u32>>,
}
impl Context {
    fn new(state: State) -> Self {
        Self {
            state: RefCell::new(state),
            applying: Cell::new(false),
            closed: Cell::new(false),
            error: Cell::new(None),
            pending_visual: Cell::new(false),
            pending_dpi: Cell::new(None),
        }
    }
}
struct Applying<'a>(&'a Cell<bool>);
impl Drop for Applying<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
struct WindowGuard(HWND);
impl WindowGuard {
    fn close_with(&self, destroy: impl FnOnce(HWND) -> bool) {
        if unsafe { IsWindow(Some(self.0)) }.as_bool() && !destroy(self.0) {
            // The HWND retains its own Rc until WM_NCDESTROY, including all
            // fonts still referenced by child controls. A failed destroy must
            // never leave userdata pointing at an allocation owned only by run.
            let pointer = unsafe { GetWindowLongPtrW(self.0, GWLP_USERDATA) } as *const Context;
            if let Some(context) = unsafe { pointer.as_ref() } {
                context.error.set(Some(ErrorCode::InternalError));
            }
        }
    }
}
impl Drop for WindowGuard {
    fn drop(&mut self) {
        self.close_with(|hwnd| unsafe { DestroyWindow(hwnd) }.is_ok());
    }
}
fn module() -> Result<HINSTANCE, ErrorCode> {
    unsafe { GetModuleHandleW(None) }
        .map(|h| HINSTANCE(h.0))
        .map_err(|_| ErrorCode::InternalError)
}
fn register_class() -> Result<(), ErrorCode> {
    static CLASS_READY: OnceLock<Result<(), ErrorCode>> = OnceLock::new();
    *CLASS_READY.get_or_init(|| {
        let c = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: module()?,
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }
                .map_err(|_| ErrorCode::InternalError)?,
            lpszClassName: CLASS,
            ..Default::default()
        };
        if unsafe { RegisterClassW(&c) } == 0 {
            Err(ErrorCode::InternalError)
        } else {
            Ok(())
        }
    })
}
fn create(
    options: ShellOptions,
    probe: Arc<dyn Fn() -> CoreStatus + Send + Sync>,
    callbacks: ShellCallbacks,
) -> Result<(Rc<Context>, WindowGuard), ErrorCode> {
    register_class()?;
    let dpi = unsafe { GetDpiForSystem() }.clamp(96, 768);
    let context = Rc::new(Context::new(State::new(options, probe, callbacks, dpi)?));
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: scale(1120, dpi),
        bottom: scale(740, dpi),
    };
    let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;
    unsafe { AdjustWindowRectExForDpi(&mut rect, style, false, WS_EX_CONTROLPARENT, dpi) }
        .map_err(|_| ErrorCode::InternalError)?;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_CONTROLPARENT,
            CLASS,
            w!("mail-hantong"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(module()?),
            Some(std::ptr::from_ref(&context).cast()),
        )
    }
    .map_err(|_| ErrorCode::InternalError)?;
    let guard = WindowGuard(hwnd);
    // WM_NCCREATE acquired a separate Rc owned by the HWND. It survives even if
    // creation of a child control or a later DestroyWindow call fails.
    create_controls(hwnd, &context)?;
    render(hwnd, &context)?;
    relayout(hwnd, &context)?;
    if unsafe { SetTimer(Some(hwnd), TIMER, 100, None) } == 0 {
        return Err(ErrorCode::InternalError);
    }
    Ok((context, guard))
}
pub(super) fn run(
    options: ShellOptions,
    probe: Arc<dyn Fn() -> CoreStatus + Send + Sync>,
    callbacks: ShellCallbacks,
) -> Result<(), ErrorCode> {
    let _sta = Sta::initialize()?;
    let _dpi = DpiContext::enter();
    let (context, guard) = create(options, probe, callbacks)?;
    let hwnd = guard.0;
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNORMAL);
        let _ = UpdateWindow(hwnd);
    }
    let first = context.state.borrow().control(LANGUAGE);
    if let Some(first) = first {
        unsafe {
            let _ = SetFocus(Some(first));
        }
    }
    let mut message = MSG::default();
    loop {
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
        if result == 0 {
            break;
        }
        if result == -1 {
            context.error.set(Some(ErrorCode::InternalError));
            break;
        }
        if message.message == WM_KEYDOWN && message.wParam.0 == usize::from(VK_RETURN.0) {
            let focus = unsafe { GetFocus() };
            let button = {
                let state = context.state.borrow();
                [
                    REFRESH,
                    CONNECT,
                    REGISTRATION_STATUS,
                    NEXT_PAGE,
                    FOLDERS_NEXT,
                ]
                .iter()
                .any(|id| state.control(*id) == Some(focus))
            };
            if button {
                unsafe {
                    SendMessageW(focus, BM_CLICK, None, None);
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
    drop(guard);
    match context.error.get() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
fn create_controls(hwnd: HWND, context: &Context) -> Result<(), ErrorCode> {
    let mut controls = Vec::new();
    for id in [
        BRAND,
        SUBTITLE,
        LANGUAGE_LABEL,
        LANGUAGE,
        THEME_LABEL,
        THEME,
        NAV_LABEL,
        NAVIGATION,
        SECTION,
        SETUP_TITLE,
        SETUP_BODY,
        ACCOUNT_LABEL,
        ACCOUNT,
        FOLDER_LABEL,
        FOLDER,
        FOLDERS_NEXT,
        LIST_TITLE,
        MESSAGE_LIST,
        NEXT_PAGE,
        DETAIL_TITLE,
        DETAIL_BODY,
        COMPACT,
        ACCESS_NOTICE,
        CONNECT,
        REGISTRATION_STATUS,
        REFRESH,
        STATUS,
        MAIL_STATUS,
        PREFERENCES,
    ] {
        let (class, style) = match id {
            LANGUAGE | THEME | ACCOUNT | FOLDER => (
                w!("COMBOBOX"),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
            ),
            NAVIGATION | MESSAGE_LIST => (
                w!("LISTBOX"),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE((LBS_NOTIFY | LBS_NOINTEGRALHEIGHT) as u32),
            ),
            REFRESH => (
                w!("BUTTON"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            ),
            CONNECT | NEXT_PAGE | REGISTRATION_STATUS | FOLDERS_NEXT => (
                w!("BUTTON"),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_OWNERDRAW as u32),
            ),
            DETAIL_BODY => (
                w!("EDIT"),
                WS_CHILD
                    | WS_VISIBLE
                    | WS_TABSTOP
                    | WS_VSCROLL
                    | WINDOW_STYLE((ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32),
            ),
            _ => (
                w!("STATIC"),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(SS_NOPREFIX.0),
            ),
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
        controls.push((id, child));
    }
    context.state.borrow_mut().controls = controls;
    Ok(())
}
fn caption(model: &Model, id: i32) -> String {
    let lang = model.language();
    let key = match id {
        BRAND => "brand",
        SUBTITLE | NAV_LABEL => "title",
        LANGUAGE_LABEL => "language",
        THEME_LABEL => "theme",
        LIST_TITLE => "list_title",
        SECTION => model.selected().key(),
        SETUP_TITLE => "setup_title",
        SETUP_BODY => "setup_body",
        ACCESS_NOTICE => "gui_not_registered",
        DETAIL_TITLE => "detail_title",
        DETAIL_BODY => "detail_empty",
        COMPACT => "compact_note",
        REFRESH => "refresh",
        CONNECT => "connect",
        ACCOUNT => "account",
        FOLDER => "folder",
        NEXT_PAGE => "next_page",
        REGISTRATION_STATUS => "registration_status",
        FOLDERS_NEXT => "folders_next",
        ACCOUNT_LABEL => "account",
        FOLDER_LABEL => "folder",
        PREFERENCES => "session_preferences",
        STATUS => match model.status() {
            CoreStatus::Checking => "connection_checking",
            CoreStatus::Available { .. } => "connection_available",
            CoreStatus::Unavailable(_) => "connection_offline",
        },
        MAIL_STATUS => match model.status() {
            CoreStatus::Available { mail_ready: true } => "mail_ready",
            CoreStatus::Available { mail_ready: false } => "mail_not_ready",
            CoreStatus::Unavailable(_) => "status_error",
            CoreStatus::Checking => return String::new(),
        },
        _ => return String::new(),
    };
    let value = text(lang, key);
    if id == MAIL_STATUS
        && let CoreStatus::Unavailable(error) = model.status()
    {
        format!("{value}: {}", error.as_str())
    } else {
        value.into()
    }
}
fn render(hwnd: HWND, context: &Context) -> Result<(), ErrorCode> {
    if context.closed.get() {
        return Ok(());
    }
    if context.applying.replace(true) {
        return Ok(());
    }
    let _applying = Applying(&context.applying);
    let (model, visual, controls, rebuild, pending, notice, workspace, requested, available) = {
        let mut state = context.state.borrow_mut();
        if state.controls.is_empty() {
            return Ok(());
        }
        let rebuild = state.rendered_language != Some(state.model.language());
        state.rendered_language = Some(state.model.language());
        (
            state.model.clone(),
            Rc::clone(&state.visual),
            state.controls.clone(),
            rebuild,
            state.busy(),
            state.notice.clone(),
            state.workspace.clone(),
            state.registration_requested,
            state.action.available(),
        )
    };
    // No RefCell borrow is held across synchronous control messages.
    for (id, child) in &controls {
        unsafe {
            SendMessageW(
                *child,
                WM_SETFONT,
                Some(WPARAM(visual.font(*id).0 as usize)),
                Some(LPARAM(0)),
            );
        }
        if !matches!(
            *id,
            LANGUAGE | THEME | NAVIGATION | MESSAGE_LIST | ACCOUNT | FOLDER
        ) {
            let value = match *id {
                SETUP_TITLE => String::new(),
                SETUP_BODY => notice.caption(model.language()),
                ACCESS_NOTICE => text(model.language(), "cached_notice").into(),
                DETAIL_BODY => detail_caption(&workspace, model.language()),
                _ => caption(&model, *id),
            };
            unsafe { SetWindowTextW(*child, &HSTRING::from(value)) }
                .map_err(|_| ErrorCode::InternalError)?;
        }
    }
    for (id, child) in &controls {
        if *id == LANGUAGE || *id == THEME {
            let (keys, index): (&[&str], usize) = if *id == LANGUAGE {
                (
                    &["language_en", "language_ko"],
                    usize::from(model.language() == Language::Korean),
                )
            } else {
                (
                    &["theme_light", "theme_dark", "theme_system"],
                    match model.theme() {
                        Theme::Light => 0,
                        Theme::Dark => 1,
                        Theme::System => 2,
                    },
                )
            };
            if rebuild {
                unsafe {
                    SendMessageW(*child, CB_RESETCONTENT, None, None);
                }
                for key in keys {
                    let value = HSTRING::from(text(model.language(), key));
                    let inserted = unsafe {
                        SendMessageW(
                            *child,
                            CB_ADDSTRING,
                            None,
                            Some(LPARAM(value.as_ptr() as isize)),
                        )
                    }
                    .0;
                    if inserted < 0 {
                        return Err(ErrorCode::InternalError);
                    }
                }
            }
            if unsafe { SendMessageW(*child, CB_SETCURSEL, Some(WPARAM(index)), None) }.0 < 0 {
                return Err(ErrorCode::InternalError);
            }
        }
        if *id == NAVIGATION {
            if rebuild {
                unsafe {
                    SendMessageW(*child, LB_RESETCONTENT, None, None);
                }
                for item in Navigation::ALL {
                    let value = HSTRING::from(text(model.language(), item.key()));
                    if unsafe {
                        SendMessageW(
                            *child,
                            LB_ADDSTRING,
                            None,
                            Some(LPARAM(value.as_ptr() as isize)),
                        )
                    }
                    .0 < 0
                    {
                        return Err(ErrorCode::InternalError);
                    }
                }
            }
            let index = Navigation::ALL
                .iter()
                .position(|item| *item == model.selected())
                .ok_or(ErrorCode::InternalError)?;
            if unsafe { SendMessageW(*child, LB_SETCURSEL, Some(WPARAM(index)), None) }.0 < 0 {
                return Err(ErrorCode::InternalError);
            }
        }
        if matches!(*id, ACCOUNT | FOLDER) {
            unsafe {
                SendMessageW(*child, CB_RESETCONTENT, None, None);
            }
            let values: Vec<String> = if *id == ACCOUNT {
                workspace
                    .accounts()
                    .iter()
                    .map(|v| format!("{} ({})", v.label, v.account_id))
                    .collect()
            } else {
                workspace
                    .folders()
                    .iter()
                    .map(|v| v.display_name.clone())
                    .collect()
            };
            for value in values {
                let value = HSTRING::from(value);
                if unsafe {
                    SendMessageW(
                        *child,
                        CB_ADDSTRING,
                        None,
                        Some(LPARAM(value.as_ptr() as isize)),
                    )
                }
                .0 < 0
                {
                    return Err(ErrorCode::InternalError);
                }
            }
            let selected = if *id == ACCOUNT {
                workspace
                    .accounts()
                    .iter()
                    .position(|a| Some(a.account_id.as_str()) == workspace.selected_account())
            } else {
                workspace.folder_index()
            };
            unsafe {
                SendMessageW(
                    *child,
                    CB_SETCURSEL,
                    Some(WPARAM(selected.unwrap_or(usize::MAX))),
                    None,
                );
            }
        }
        if *id == MESSAGE_LIST {
            unsafe {
                SendMessageW(*child, LB_RESETCONTENT, None, None);
            }
            for row in workspace.messages() {
                let value = HSTRING::from(format!(
                    "{} | {}",
                    row.sender,
                    if row.subject.is_empty() {
                        text(model.language(), "subject_empty")
                    } else {
                        &row.subject
                    }
                ));
                if unsafe {
                    SendMessageW(
                        *child,
                        LB_ADDSTRING,
                        None,
                        Some(LPARAM(value.as_ptr() as isize)),
                    )
                }
                .0 < 0
                {
                    return Err(ErrorCode::InternalError);
                }
            }
            unsafe {
                SendMessageW(
                    *child,
                    LB_SETCURSEL,
                    Some(WPARAM(workspace.message_index().unwrap_or(usize::MAX))),
                    None,
                );
            }
        }
        if matches!(
            *id,
            REFRESH
                | CONNECT
                | NEXT_PAGE
                | REGISTRATION_STATUS
                | ACCOUNT
                | FOLDER
                | FOLDERS_NEXT
                | MESSAGE_LIST
        ) {
            let enabled = !pending
                && match *id {
                    REFRESH => true,
                    CONNECT => available,
                    REGISTRATION_STATUS => available && requested,
                    ACCOUNT => available && !workspace.accounts().is_empty(),
                    FOLDER => available && !workspace.folders().is_empty(),
                    FOLDERS_NEXT => available && workspace.folder_cursor().is_some(),
                    MESSAGE_LIST => !workspace.messages().is_empty(),
                    NEXT_PAGE => available && workspace.next_cursor().is_some(),
                    _ => false,
                };
            unsafe {
                let _ = EnableWindow(*child, enabled);
            }
        }
    }
    let dark = i32::from(model.effective_theme() == EffectiveTheme::Dark);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark as *const i32).cast(),
            std::mem::size_of::<i32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            (&visual.colors.background as *const _ as *const u32).cast(),
            4,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            (&visual.colors.text as *const _ as *const u32).cast(),
            4,
        );
    }
    invalidate(hwnd);
    drop(_applying);
    if context.pending_visual.get() {
        unsafe { PostMessageW(Some(hwnd), DEFERRED_VISUAL, WPARAM(0), LPARAM(0)) }
            .map_err(|_| ErrorCode::InternalError)?;
    }
    Ok(())
}
fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN,
        );
    }
}
fn relayout(hwnd: HWND, context: &Context) -> Result<(), ErrorCode> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect) }.map_err(|_| ErrorCode::InternalError)?;
    let (dpi, controls) = {
        let s = context.state.borrow();
        (s.visual.dpi, s.controls.clone())
    };
    let layout = Layout::new(rect.right, rect.bottom, dpi);
    for (id, child) in controls {
        if let Some(p) = layout.control(id) {
            unsafe {
                let _ = ShowWindow(child, if p.visible { SW_SHOW } else { SW_HIDE });
                MoveWindow(
                    child,
                    p.rect.left,
                    p.rect.top,
                    p.rect.right - p.rect.left,
                    if matches!(id, LANGUAGE | THEME | ACCOUNT | FOLDER) {
                        scale(200, dpi)
                    } else {
                        p.rect.bottom - p.rect.top
                    },
                    false,
                )
            }
            .map_err(|_| ErrorCode::InternalError)?;
        }
    }
    invalidate(hwnd);
    Ok(())
}
fn update_visual(
    hwnd: HWND,
    context: &Context,
    dpi: Option<u32>,
    force: bool,
) -> Result<(), ErrorCode> {
    if context.applying.get() {
        context.pending_visual.set(true);
        if dpi.is_some() {
            context.pending_dpi.set(dpi);
        }
        return Ok(());
    }
    let (dark, high) = os_colors();
    let (model, old_dpi, old_colors) = {
        let mut state = context.state.borrow_mut();
        state.model.update_os(dark, high);
        (state.model.clone(), state.visual.dpi, state.visual.colors)
    };
    let dpi = dpi.unwrap_or(old_dpi).clamp(96, 768);
    if !force && dpi == old_dpi && Colors::new(model.effective_theme()) == old_colors {
        return Ok(());
    }
    let next = Rc::new(Visual::new(model.effective_theme(), dpi)?);
    let old = {
        let mut s = context.state.borrow_mut();
        std::mem::replace(&mut s.visual, next)
    };
    let result = render(hwnd, context).and_then(|_| relayout(hwnd, context));
    drop(old);
    result
}
fn begin_probe(hwnd: HWND, context: &Context) -> Result<(), ErrorCode> {
    if context.closed.get() || context.error.get().is_some() {
        return Ok(());
    }
    let started = {
        let mut s = context.state.borrow_mut();
        if !s.painted || s.busy() {
            return Ok(());
        }
        let started = s.probe.begin()?;
        if started {
            s.model.set_status(CoreStatus::Checking);
        }
        started
    };
    if started {
        render(hwnd, context)?;
    }
    Ok(())
}
fn begin_action(hwnd: HWND, context: &Context, action: MailAction) -> Result<(), ErrorCode> {
    let started = {
        let mut s = context.state.borrow_mut();
        if s.busy() || context.closed.get() {
            return Ok(());
        }
        let started = s.action.begin(action)?;
        if started {
            s.notice = Notice::Loading;
        }
        started
    };
    if started {
        render(hwnd, context)?;
    }
    Ok(())
}
fn poll(hwnd: HWND, context: &Context) -> Result<(), ErrorCode> {
    if context.closed.get() {
        return Ok(());
    }
    let (changed, theme) = {
        let mut s = context.state.borrow_mut();
        let mut changed = false;
        if let Some(status) = s.probe.poll() {
            s.model.set_status(status);
            changed = true;
            if s.accounts_after_probe {
                s.accounts_after_probe = false;
                if matches!(status, CoreStatus::Available { .. }) {
                    if !s.action.begin(MailAction::Accounts)? {
                        s.notice = Notice::Initial;
                    }
                } else if let CoreStatus::Unavailable(error) = status {
                    s.notice = Notice::Error(error);
                }
            }
        }
        if let Some((action, result)) = s.action.poll() {
            let result = result.and_then(|value| {
                s.workspace.apply(&action, &value)?;
                Ok(value)
            });
            match result {
                Ok(MailActionResult::Registration(progress)) => {
                    s.registration_requested = true;
                    s.notice = Notice::Registration(progress);
                }
                Ok(MailActionResult::Accounts(rows)) => s.notice = Notice::Accounts(rows.len()),
                Ok(MailActionResult::Folders { rows, .. }) => {
                    s.notice = Notice::Folders(rows.len())
                }
                Ok(MailActionResult::Messages { rows, observed, .. }) => {
                    s.notice = if observed {
                        Notice::Messages(rows.len())
                    } else {
                        Notice::Unobserved
                    }
                }
                Err(error) => {
                    s.workspace.clear();
                    s.notice = Notice::Error(error);
                }
            }
            changed = true;
        }
        let theme = s.last_theme_poll.elapsed() >= Duration::from_secs(1);
        if theme {
            s.last_theme_poll = Instant::now();
        }
        (changed, theme)
    };
    if changed {
        render(hwnd, context)?;
    }
    if theme {
        update_visual(hwnd, context, None, false)?;
    }
    Ok(())
}
fn command(hwnd: HWND, context: &Context, wparam: WPARAM, lparam: LPARAM) -> Result<(), ErrorCode> {
    if context.applying.get() || context.closed.get() {
        return Ok(());
    }
    let id = (wparam.0 & 0xffff) as i32;
    let notification = (wparam.0 >> 16) as u16;
    let child = HWND(lparam.0 as *mut _);
    if context.state.borrow().control(id) != Some(child) {
        return Ok(());
    }
    if matches!(
        id,
        ACCOUNT
            | FOLDER
            | REFRESH
            | CONNECT
            | NEXT_PAGE
            | REGISTRATION_STATUS
            | FOLDERS_NEXT
            | MESSAGE_LIST
    ) && context.state.borrow().busy()
    {
        return Ok(());
    }
    if matches!(id, ACCOUNT | FOLDER) && notification == CBN_SELCHANGE as u16 {
        let selected = unsafe { SendMessageW(child, CB_GETCURSEL, None, None) }.0;
        let Ok(index) = usize::try_from(selected) else {
            return Ok(());
        };
        let action = {
            let mut s = context.state.borrow_mut();
            if id == ACCOUNT {
                let Some(account) = s
                    .workspace
                    .accounts()
                    .get(index)
                    .map(|a| a.account_id.clone())
                else {
                    return Ok(());
                };
                s.workspace.select_account(&account);
                MailAction::Folders {
                    account_id: account,
                    cursor: None,
                }
            } else {
                if !s.workspace.select_folder(index) {
                    return Ok(());
                }
                let Some(action) = s.workspace.message_action(false) else {
                    return Ok(());
                };
                action
            }
        };
        return begin_action(hwnd, context, action);
    }
    if id == MESSAGE_LIST && notification == LBN_SELCHANGE as u16 {
        let selected = unsafe { SendMessageW(child, LB_GETCURSEL, None, None) }.0;
        if let Ok(index) = usize::try_from(selected) {
            context.state.borrow_mut().workspace.select_message(index);
        }
        return render(hwnd, context);
    }
    if matches!(
        id,
        REFRESH | CONNECT | REGISTRATION_STATUS | NEXT_PAGE | FOLDERS_NEXT
    ) && notification == BN_CLICKED as u16
    {
        if id == REFRESH {
            {
                let mut s = context.state.borrow_mut();
                s.workspace.clear();
                s.notice = Notice::Loading;
                s.accounts_after_probe = true;
            }
            return begin_probe(hwnd, context);
        }
        let action = match id {
            CONNECT => MailAction::Connect,
            REGISTRATION_STATUS => {
                if !context.state.borrow().registration_requested {
                    return Ok(());
                }
                MailAction::RegistrationStatus
            }
            FOLDERS_NEXT => {
                let s = context.state.borrow();
                let (Some(account), Some(cursor)) =
                    (s.workspace.selected_account(), s.workspace.folder_cursor())
                else {
                    return Ok(());
                };
                MailAction::Folders {
                    account_id: account.into(),
                    cursor: Some(cursor.into()),
                }
            }
            NEXT_PAGE => {
                let Some(action) = context.state.borrow().workspace.message_action(true) else {
                    return Ok(());
                };
                action
            }
            _ => return Ok(()),
        };
        return begin_action(hwnd, context, action);
    }
    if matches!(id, LANGUAGE | THEME) && notification == CBN_SELCHANGE as u16 {
        let selected = unsafe { SendMessageW(child, CB_GETCURSEL, None, None) }.0;
        {
            let mut s = context.state.borrow_mut();
            match (id, selected) {
                (LANGUAGE, 0) => s.model.set_language(Language::English),
                (LANGUAGE, 1) => s.model.set_language(Language::Korean),
                (THEME, 0) => s.model.set_theme(Theme::Light),
                (THEME, 1) => s.model.set_theme(Theme::Dark),
                (THEME, 2) => s.model.set_theme(Theme::System),
                _ => return Err(ErrorCode::InternalError),
            }
        }
        if id == THEME {
            update_visual(hwnd, context, None, true)?;
        } else {
            render(hwnd, context)?;
        }
        return Ok(());
    }
    if id == NAVIGATION && notification == LBN_SELCHANGE as u16 {
        let selected = unsafe { SendMessageW(child, LB_GETCURSEL, None, None) }.0;
        let nav = usize::try_from(selected)
            .ok()
            .and_then(|i| Navigation::ALL.get(i))
            .ok_or(ErrorCode::InternalError)?;
        context.state.borrow_mut().model.set_selected(*nav);
        render(hwnd, context)?;
    }
    Ok(())
}
fn fail(hwnd: HWND, context: &Context, error: ErrorCode) {
    context.error.set(Some(error));
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}
unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle(hwnd, message, wparam, lparam)
    })) {
        Ok(result) => result,
        Err(_) => {
            let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Context;
            if let Some(context) = unsafe { p.as_ref() } {
                fail(hwnd, context, ErrorCode::InternalError);
            }
            LRESULT(0)
        }
    }
}
fn handle(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_NCCREATE {
        if lparam.0 == 0 {
            return LRESULT(0);
        }
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        if create.lpCreateParams.is_null() {
            return LRESULT(0);
        }
        // CreateWindowExW is synchronous; its parameter borrows the caller's Rc.
        // Publish only an owned pointer produced by Rc::into_raw, paired exactly
        // once with from_raw on publication failure or WM_NCDESTROY.
        let owner = unsafe { &*create.lpCreateParams.cast::<Rc<Context>>() };
        let pointer = Rc::into_raw(Rc::clone(owner));
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, pointer as isize);
        }
        if unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } != pointer as isize {
            unsafe {
                drop(Rc::from_raw(pointer));
            }
            return LRESULT(0);
        }
        // Default non-client creation also initializes the caption supplied to
        // CreateWindowExW. Owning userdata does not replace that initialization.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    if message == WM_NCDESTROY {
        let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Context;
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        let cleared = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } == 0;
        let result = unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        // Clear before releasing: a nested callback cannot retrieve this Rc.
        // If clearing fails, retaining the reference is safer than a dangling
        // userdata pointer; that exceptional allocation lasts until process exit.
        if cleared && !pointer.is_null() {
            unsafe {
                drop(Rc::from_raw(pointer));
            }
        }
        return result;
    }
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Context;
    let Some(context) = (unsafe { pointer.as_ref() }) else {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    if message == WM_DESTROY {
        context.closed.set(true);
        {
            let mut state = context.state.borrow_mut();
            state.probe.close();
            state.action.close();
        }
        unsafe {
            let _ = KillTimer(Some(hwnd), TIMER);
            PostQuitMessage(0);
        }
        return LRESULT(0);
    }
    if message == WM_PAINT {
        return paint(hwnd, context);
    }
    if matches!(
        message,
        WM_CTLCOLORSTATIC | WM_CTLCOLORLISTBOX | WM_CTLCOLOREDIT | WM_CTLCOLORBTN
    ) {
        let (visual, id) = {
            let s = context.state.borrow();
            (
                Rc::clone(&s.visual),
                s.controls
                    .iter()
                    .find(|(_, child)| child.0 as isize == lparam.0)
                    .map(|(id, _)| *id),
            )
        };
        let surface = matches!(
            id,
            Some(
                NAV_LABEL
                    | NAVIGATION
                    | LIST_TITLE
                    | SECTION
                    | SETUP_TITLE
                    | SETUP_BODY
                    | ACCESS_NOTICE
                    | DETAIL_TITLE
                    | DETAIL_BODY
                    | COMPACT
                    | LANGUAGE
                    | THEME
                    | ACCOUNT
                    | FOLDER
                    | MESSAGE_LIST
            )
        );
        let dc = HDC(wparam.0 as *mut _);
        unsafe {
            SetTextColor(
                dc,
                if matches!(
                    id,
                    Some(
                        SETUP_BODY
                            | ACCESS_NOTICE
                            | DETAIL_BODY
                            | PREFERENCES
                            | SUBTITLE
                            | MAIL_STATUS
                    )
                ) {
                    visual.colors.muted
                } else {
                    visual.colors.text
                },
            );
            SetBkColor(
                dc,
                if surface {
                    visual.colors.surface
                } else {
                    visual.colors.background
                },
            );
        }
        return LRESULT(if surface {
            visual.surface.0.0
        } else {
            visual.background.0.0
        } as isize);
    }
    if message == WM_DRAWITEM && lparam.0 != 0 {
        let draw = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
        if draw.CtlType == ODT_BUTTON
            && [
                REFRESH,
                CONNECT,
                REGISTRATION_STATUS,
                NEXT_PAGE,
                FOLDERS_NEXT,
            ]
            .iter()
            .any(|id| draw.CtlID == *id as u32)
        {
            draw_button(context, draw);
            return LRESULT(1);
        }
    }
    let result = match message {
        WM_COMMAND => command(hwnd, context, wparam, lparam),
        WM_TIMER if wparam.0 == TIMER => {
            if context.applying.get() {
                Ok(())
            } else {
                poll(hwnd, context)
            }
        }
        FIRST_PAINT => begin_probe(hwnd, context),
        DEFERRED_VISUAL => {
            context.pending_visual.set(false);
            update_visual(hwnd, context, context.pending_dpi.take(), false)
        }
        WM_SIZE => relayout(hwnd, context),
        WM_SETTINGCHANGE | WM_THEMECHANGED | WM_SYSCOLORCHANGE => {
            update_visual(hwnd, context, None, false)
        }
        WM_GETMINMAXINFO => {
            if lparam.0 != 0 {
                let dpi = context.state.borrow().visual.dpi;
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                info.ptMinTrackSize.x = scale(760, dpi);
                info.ptMinTrackSize.y = scale(720, dpi);
            }
            Ok(())
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xffff) as u32;
            let result = update_visual(hwnd, context, Some(dpi), true);
            if result.is_ok() && lparam.0 != 0 {
                let r = unsafe { &*(lparam.0 as *const RECT) };
                unsafe {
                    SetWindowPos(
                        hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOACTIVATE | SWP_NOZORDER,
                    )
                }
                .map_err(|_| ErrorCode::InternalError)
            } else {
                result
            }
        }
        WM_CLOSE => {
            context.closed.set(true);
            unsafe { DestroyWindow(hwnd) }.map_err(|_| ErrorCode::InternalError)
        }
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    };
    if let Err(error) = result {
        fail(hwnd, context, error);
    }
    LRESULT(0)
}
fn paint(hwnd: HWND, context: &Context) -> LRESULT {
    let visual = Rc::clone(&context.state.borrow().visual);
    let mut p = PAINTSTRUCT::default();
    let dc = unsafe { BeginPaint(hwnd, &mut p) };
    let mut rect = RECT::default();
    let valid = !dc.0.is_null() && unsafe { GetClientRect(hwnd, &mut rect) }.is_ok();
    let layout = Layout::new(rect.right, rect.bottom, visual.dpi);
    let filled = if valid {
        unsafe {
            let background = FillRect(dc, &p.rcPaint, visual.background.0) != 0;
            for panel in layout.panels() {
                FillRect(dc, &panel, visual.surface.0);
                FrameRect(dc, &panel, visual.border.0);
            }
            background
        }
    } else {
        false
    };
    let ended = unsafe { EndPaint(hwnd, &p) }.as_bool();
    if !filled || !ended {
        fail(hwnd, context, ErrorCode::InternalError);
        return LRESULT(0);
    }
    let first = {
        let mut s = context.state.borrow_mut();
        let first = !s.painted && !s.controls.is_empty();
        s.painted |= first;
        first
    };
    if first {
        // Finish the initial controls before queuing external readiness work.
        // The first-paint flag is already set, so nested WM_PAINT cannot repeat it.
        let complete = unsafe {
            RedrawWindow(
                Some(hwnd),
                None,
                None,
                RDW_INVALIDATE | RDW_ALLCHILDREN | RDW_UPDATENOW,
            )
        }
        .as_bool();
        if !complete {
            fail(hwnd, context, ErrorCode::InternalError);
            return LRESULT(0);
        }
        unsafe {
            if PostMessageW(Some(hwnd), FIRST_PAINT, WPARAM(0), LPARAM(0)).is_err() {
                fail(hwnd, context, ErrorCode::InternalError);
            }
        }
    }
    LRESULT(0)
}
fn draw_button(context: &Context, draw: &DRAWITEMSTRUCT) {
    let (visual, caption) = {
        let s = context.state.borrow();
        (Rc::clone(&s.visual), caption(&s.model, draw.CtlID as i32))
    };
    let mut caption: Vec<u16> = caption.encode_utf16().collect();
    let mut rect = draw.rcItem;
    unsafe {
        FillRect(draw.hDC, &rect, visual.surface.0);
        FrameRect(draw.hDC, &rect, visual.border.0);
        let old = SelectObject(draw.hDC, HGDIOBJ(visual.fonts.body.0.0));
        SetTextColor(
            draw.hDC,
            if draw.itemState.0 & ODS_DISABLED.0 != 0 {
                visual.colors.muted
            } else {
                visual.colors.text
            },
        );
        SetBkColor(draw.hDC, visual.colors.surface);
        DrawTextW(
            draw.hDC,
            &mut caption,
            &mut rect,
            DT_SINGLELINE | DT_CENTER | DT_VCENTER | DT_NOPREFIX,
        );
        SelectObject(draw.hDC, old);
        if draw.itemState.0 & ODS_FOCUS.0 != 0 {
            rect.left += 3;
            rect.top += 3;
            rect.right -= 3;
            rect.bottom -= 3;
            SetTextColor(draw.hDC, visual.colors.focus);
            let _ = DrawFocusRect(draw.hDC, &rect);
        }
    }
}

#[cfg(test)]
mod tests;
