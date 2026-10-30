use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

fn label(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    assert!(length >= 0);
    let mut buffer = vec![0u16; length as usize + 1];
    let written = unsafe { GetWindowTextW(hwnd, &mut buffer) };
    String::from_utf16(&buffer[..written as usize]).unwrap()
}
fn child(context: &Context, id: i32) -> HWND {
    context.state.borrow().control(id).unwrap()
}

#[test]
fn ops07_hidden_window_creation_initializes_brand_title_and_retains_owned_context() {
    let _sta = Sta::initialize().unwrap();
    let _dpi = DpiContext::enter();
    let (context, guard) = create(
        ShellOptions::default(),
        Arc::new(|| CoreStatus::Unavailable(ErrorCode::CoreUnavailable)),
        ShellCallbacks::default(),
    )
    .unwrap();
    assert_eq!(label(guard.0), "mail-hantong"); // Reads the actual HWND via GetWindowTextW.
    assert_eq!(Rc::strong_count(&context), 8);
    context
        .state
        .borrow_mut()
        .model
        .set_language(Language::Korean);
    render(guard.0, &context).unwrap();
    assert_eq!(label(guard.0), "mail-hantong");
    let weak = Rc::downgrade(&context);
    drop(context);
    drop(guard);
    assert!(weak.upgrade().is_none());
}

#[test]
fn ops02_failed_window_destroy_retains_userdata_and_child_resources_until_final_destroy() {
    let _sta = Sta::initialize().unwrap();
    let _dpi = DpiContext::enter();
    let (context, guard) = create(
        ShellOptions::default(),
        Arc::new(|| CoreStatus::Unavailable(ErrorCode::CoreUnavailable)),
        ShellCallbacks::default(),
    )
    .unwrap();
    let weak = Rc::downgrade(&context);
    let pointer = Rc::as_ptr(&context);
    let font = context.state.borrow().visual.fonts.body.0;
    assert_eq!(
        Rc::strong_count(&context),
        8,
        "caller, parent HWND, empty list and five combos own their references"
    );

    // Inject the same failure branch used when DestroyWindow returns an error.
    guard.close_with(|_| false);
    assert_eq!(context.error.get(), Some(ErrorCode::InternalError));
    drop(context);
    assert_eq!(
        weak.strong_count(),
        7,
        "the parent and its six subclassed children remain owners"
    );
    assert_eq!(
        unsafe { GetWindowLongPtrW(guard.0, GWLP_USERDATA) },
        pointer as isize
    );
    assert_eq!(unsafe { GetObjectType(HGDIOBJ(font.0)) }, OBJ_FONT.0 as u32);

    // A genuine callback after the caller has released its reference is safe.
    let mut bounds = MINMAXINFO::default();
    unsafe {
        SendMessageW(
            guard.0,
            WM_GETMINMAXINFO,
            None,
            Some(LPARAM((&mut bounds as *mut MINMAXINFO) as isize)),
        );
    }
    assert!(bounds.ptMinTrackSize.x > 0);
    drop(guard); // Actual DestroyWindow reaches WM_NCDESTROY and releases HWND's Rc.
    assert!(weak.upgrade().is_none());
}

#[test]
fn ops02_empty_list_subclass_owns_one_reference_and_releases_on_install_failure_and_destroy() {
    let _sta = Sta::initialize().unwrap();
    let (context, guard) = create(
        ShellOptions::default(),
        Arc::new(|| CoreStatus::Unavailable(ErrorCode::CoreUnavailable)),
        ShellCallbacks::default(),
    )
    .unwrap();
    assert_eq!(Rc::strong_count(&context), 8);
    assert_eq!(
        drawing::install_empty_state(HWND::default(), &context),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(
        Rc::strong_count(&context),
        8,
        "failed installation releases its reference"
    );
    let list = child(&context, MESSAGE_LIST);
    assert_eq!(unsafe { SendMessageW(list, LB_GETCOUNT, None, None) }.0, 0);
    unsafe {
        SendMessageW(list, WM_PAINT, None, None);
        DestroyWindow(list).unwrap();
    }
    assert!(context.error.get().is_none());
    assert_eq!(
        Rc::strong_count(&context),
        7,
        "child NCDESTROY releases only the subclass owner"
    );
    assert_eq!(label(guard.0), "mail-hantong");
    drop(guard);
    assert_eq!(Rc::strong_count(&context), 1);
}

struct DrawingSurface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}
impl DrawingSurface {
    fn new() -> Self {
        unsafe {
            let screen = GetDC(None);
            assert!(!screen.0.is_null());
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, 360, 180);
            ReleaseDC(None, screen);
            assert!(!dc.0.is_null() && !bitmap.0.is_null());
            let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
            Self {
                dc,
                bitmap,
                previous,
            }
        }
    }
}
impl Drop for DrawingSurface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

#[test]
fn ops07_combo_skin_preserves_native_choices_and_releases_its_independent_owner() {
    let _sta = Sta::initialize().unwrap();
    let (context, guard) = create(
        ShellOptions::default(),
        Arc::new(|| CoreStatus::Unavailable(ErrorCode::CoreUnavailable)),
        ShellCallbacks::default(),
    )
    .unwrap();
    assert_eq!(Rc::strong_count(&context), 8);
    assert_eq!(
        drawing::install_combo_skin(HWND::default(), &context),
        Err(ErrorCode::InternalError)
    );
    assert_eq!(Rc::strong_count(&context), 8);
    let language = child(&context, LANGUAGE);
    assert_eq!(
        unsafe { SendMessageW(language, CB_GETCOUNT, None, None) }.0,
        2
    );
    let style = unsafe { GetWindowLongPtrW(language, GWL_STYLE) } as u32;
    assert_ne!(style & WS_TABSTOP.0, 0);
    assert_ne!(style & CBS_HASSTRINGS as u32, 0);
    let surface = DrawingSurface::new();
    for theme in [
        EffectiveTheme::Light,
        EffectiveTheme::Dark,
        EffectiveTheme::HighContrast,
    ] {
        let old = std::mem::replace(
            &mut context.state.borrow_mut().visual,
            Rc::new(Visual::new(theme, 96).unwrap()),
        );
        render(guard.0, &context).unwrap();
        drop(old);
        let colors = context.state.borrow().visual.colors;
        let mut r = RECT::default();
        unsafe {
            GetClientRect(language, &mut r).unwrap();
        }
        unsafe {
            SendMessageW(
                language,
                WM_PRINTCLIENT,
                Some(WPARAM(surface.dc.0 as usize)),
                Some(LPARAM(PRF_CLIENT as isize)),
            );
        }
        assert_eq!(
            unsafe { GetPixel(surface.dc, r.right / 2, 0) },
            colors.border,
            "native combo chrome uses current theme"
        );
        assert_eq!(
            Rc::strong_count(&context),
            8,
            "nested default paint does not leak callback references"
        );
    }
    notify(&context, guard.0, LANGUAGE, CBN_SELCHANGE, Some(1));
    assert_eq!(context.state.borrow().model.language(), Language::Korean);
    assert_eq!(
        unsafe { SendMessageW(language, CB_GETCURSEL, None, None) }.0,
        1
    );
    unsafe {
        DestroyWindow(language).unwrap();
    }
    assert_eq!(Rc::strong_count(&context), 7);
    drop(guard);
    assert_eq!(Rc::strong_count(&context), 1);
}

#[test]
fn ops08_native_selection_motion_is_immediate_in_high_contrast_and_stops_repainting_at_rest() {
    let _sta = Sta::initialize().unwrap();
    let (context, guard) = create(
        ShellOptions::default(),
        Arc::new(|| CoreStatus::Unavailable(ErrorCode::CoreUnavailable)),
        ShellCallbacks::default(),
    )
    .unwrap();
    // Hidden HWNDs have no drawable update region. Keep this native invalidation
    // fixture visible off-screen without activating or changing desktop settings.
    unsafe {
        SetWindowPos(
            guard.0,
            None,
            -20000,
            -20000,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .unwrap();
        let _ = ShowWindow(guard.0, SW_SHOWNOACTIVATE);
    }
    let navigation = child(&context, NAVIGATION);
    context.state.borrow_mut().model.update_os(false, true);
    start_motion(guard.0, &context, NAVIGATION, Some(0), Some(1));
    assert!(
        context
            .state
            .borrow_mut()
            .motion
            .tick(Instant::now())
            .is_none()
    );
    assert!(!motion::enabled(true));
    let now = Instant::now();
    assert!(context.state.borrow_mut().motion.start(
        NAVIGATION,
        Some(0),
        Some(1),
        now - Duration::from_millis(140),
        true
    ));
    unsafe {
        let _ = ValidateRect(Some(navigation), None);
    }
    tick_motion(guard.0, &context);
    assert!(
        unsafe { GetUpdateRect(navigation, None, false) }.as_bool(),
        "final transition frame is painted"
    );
    unsafe {
        let _ = ValidateRect(Some(navigation), None);
    }
    tick_motion(guard.0, &context);
    assert!(
        !unsafe { GetUpdateRect(navigation, None, false) }.as_bool(),
        "idle animation ticks do not repaint"
    );
    assert!(context.state.borrow_mut().motion.tick(now).is_none());
    start_motion(guard.0, &context, NAVIGATION, Some(1), Some(1));
    assert!(context.state.borrow_mut().motion.tick(now).is_none());
    assert!(context.error.get().is_none());
}

#[test]
fn ops07_ops08_native_owner_draw_preserves_semantic_rows_fonts_and_theme_selection() {
    use crate::{MailAccount, MailFolder, MailMessage};
    let _sta = Sta::initialize().unwrap();
    let _dpi = DpiContext::enter();
    let (context, guard) = create(
        ShellOptions {
            language: Some(Language::English),
            theme: Theme::Light,
        },
        Arc::new(|| CoreStatus::Unavailable(ErrorCode::CoreUnavailable)),
        ShellCallbacks::default(),
    )
    .unwrap();
    {
        let mut s = context.state.borrow_mut();
        s.workspace
            .apply(
                &MailAction::Accounts,
                &MailActionResult::Accounts(vec![MailAccount::new("a", "Account").unwrap()]),
            )
            .unwrap();
        s.workspace.select_account("a");
        s.workspace
            .apply(
                &MailAction::Folders {
                    account_id: "a".into(),
                    cursor: None,
                },
                &MailActionResult::Folders {
                    rows: vec![
                        MailFolder::new_with_encoding("a", "Inbox", "494e424f58", "utf8").unwrap(),
                    ],
                    next_cursor: None,
                },
            )
            .unwrap();
        s.workspace.select_folder(0);
        let action = s.workspace.message_action(false).unwrap();
        s.workspace
            .apply(
                &action,
                &MailActionResult::Messages {
                    rows: vec![
                        MailMessage::new(
                            "a",
                            &format!("msg_{}", "a".repeat(64)),
                            "Actual sender",
                            "Actual subject",
                            "2026-10-30",
                            false,
                            false,
                        )
                        .unwrap(),
                    ],
                    next_cursor: None,
                    observed: true,
                },
            )
            .unwrap();
    }
    render(guard.0, &context).unwrap();
    let list = child(&context, MESSAGE_LIST);
    let dpi = context.state.borrow().visual.dpi;
    for (id, height) in [(NAVIGATION, 32), (MESSAGE_LIST, 52)] {
        let control = child(&context, id);
        let style = unsafe { GetWindowLongPtrW(control, GWL_STYLE) } as u32;
        assert_ne!(style & LBS_HASSTRINGS as u32, 0);
        assert_ne!(style & LBS_OWNERDRAWFIXED as u32, 0);
        assert_ne!(style & WS_TABSTOP.0, 0);
        assert_eq!(
            unsafe { SendMessageW(control, LB_GETITEMHEIGHT, Some(WPARAM(0)), None) }.0,
            scale(height, dpi) as isize
        );
    }
    let length = unsafe { SendMessageW(list, LB_GETTEXTLEN, Some(WPARAM(0)), None) }.0;
    assert!(length > 0);
    let mut label = vec![0u16; length as usize + 1];
    assert_eq!(
        unsafe {
            SendMessageW(
                list,
                LB_GETTEXT,
                Some(WPARAM(0)),
                Some(LPARAM(label.as_mut_ptr() as isize)),
            )
        }
        .0,
        length
    );
    let label = String::from_utf16(&label[..length as usize]).unwrap();
    for part in ["Actual sender", "Actual subject", "2026-10-30"] {
        assert!(label.contains(part));
    }
    let surface = DrawingSurface::new();
    let previous_font = unsafe { GetCurrentObject(surface.dc, OBJ_FONT) };
    for theme in [
        EffectiveTheme::Light,
        EffectiveTheme::Dark,
        EffectiveTheme::HighContrast,
    ] {
        let old = std::mem::replace(
            &mut context.state.borrow_mut().visual,
            Rc::new(Visual::new(theme, 96).unwrap()),
        );
        render(guard.0, &context).unwrap();
        drop(old);
        let colors = context.state.borrow().visual.colors;
        for (id, kind, height) in [
            (NAVIGATION, ODT_LISTBOX, 32),
            (MESSAGE_LIST, ODT_LISTBOX, 52),
            (CONNECT, ODT_BUTTON, 34),
        ] {
            let mut draw = DRAWITEMSTRUCT {
                CtlType: kind,
                CtlID: id as u32,
                itemID: 0,
                itemState: ODS_SELECTED,
                hwndItem: child(&context, id),
                hDC: surface.dc,
                rcItem: RECT {
                    left: 0,
                    top: 0,
                    right: 340,
                    bottom: height,
                },
                ..Default::default()
            };
            assert_eq!(
                unsafe {
                    SendMessageW(
                        guard.0,
                        WM_DRAWITEM,
                        Some(WPARAM(id as usize)),
                        Some(LPARAM((&mut draw as *mut DRAWITEMSTRUCT) as isize)),
                    )
                }
                .0,
                1
            );
            assert_eq!(
                unsafe { GetPixel(surface.dc, 170, 4) },
                if id == CONNECT {
                    colors.accent
                } else {
                    colors.selected
                }
            );
            assert_eq!(
                unsafe { GetCurrentObject(surface.dc, OBJ_FONT) },
                previous_font
            );
            if id == MESSAGE_LIST {
                for (top, bottom) in [(6, 24), (26, 48)] {
                    assert!(
                        (top..bottom).any(|y| (20..190)
                            .any(|x| unsafe { GetPixel(surface.dc, x, y) } != colors.selected)),
                        "actual sender and subject are painted on separate lines"
                    );
                }
            }
        }
    }
    // Reassign live control fonts before dropping the replaced visual resources.
    render(guard.0, &context).unwrap();
    assert!(context.error.get().is_none());
}

#[test]
fn ops07_ops08_shell_layout_retains_connected_regions_and_controls_at_all_scales() {
    for dpi in [96, 144, 192] {
        for (width, height) in [(1120, 740), (760, 720), (744, 681), (960, 720)] {
            let layout = Layout::new(scale(width, dpi), scale(height, dpi), dpi);
            let panels: Vec<_> = layout.panels().collect();
            assert_eq!(panels.len(), 3);
            assert_eq!(panels[0].right, panels[1].left);
            assert_eq!(panels[1].right, panels[2].left);
            assert_eq!(panels[1].top, panels[2].top);
            assert_eq!(panels[1].bottom, panels[2].bottom);
            assert!(panels[2].right - panels[2].left > panels[1].right - panels[1].left);
            for id in BRAND..=RELOAD_FOLDER {
                let item = layout.control(id).unwrap();
                if !item.visible {
                    continue;
                }
                let r = item.rect;
                assert!(
                    r.left >= 0 && r.top >= 0 && r.right > r.left && r.bottom > r.top,
                    "{id} at {dpi}"
                );
                assert!(
                    r.right <= scale(width, dpi) && r.bottom <= scale(height, dpi),
                    "{id} at {dpi}"
                );
            }
            for (id, panel) in [(NAVIGATION, 0), (MESSAGE_LIST, 1), (DETAIL_BODY, 2)] {
                let r = layout.control(id).unwrap().rect;
                assert!(
                    r.right + scale(12, dpi) <= panels[panel].right,
                    "reserved themed scroll rail for {id}"
                );
            }
            let nav = layout.control(NAVIGATION).unwrap().rect;
            assert!(nav.bottom - nav.top >= scale(32, dpi) * Navigation::ALL.len() as i32);
            for (first, second) in [
                (FOLDER, FOLDERS_NEXT),
                (FILTER_LABEL, FILTER),
                (FILTER, RELOAD_FOLDER),
                (RELOAD_FOLDER, REFRESH),
            ] {
                assert!(
                    layout.control(first).unwrap().rect.right
                        <= layout.control(second).unwrap().rect.left
                );
            }
            assert!(
                layout.control(RELOAD_FOLDER).unwrap().rect.bottom
                    <= layout.control(SETUP_BODY).unwrap().rect.top
            );
            assert!(
                layout.control(SETUP_BODY).unwrap().rect.bottom
                    <= layout.control(MESSAGE_LIST).unwrap().rect.top
            );
            assert!(
                layout.control(MESSAGE_LIST).unwrap().rect.bottom
                    <= layout.control(NEXT_PAGE).unwrap().rect.top
            );
            assert!(
                layout.control(DETAIL_BODY).unwrap().rect.bottom
                    <= layout.control(ACCESS_NOTICE).unwrap().rect.top
            );
            assert!(
                layout.control(MAIL_STATUS).unwrap().rect.bottom
                    <= layout.control(PREFERENCES).unwrap().rect.top
            );
        }
    }
}

#[test]
fn ops07_ops08_hidden_shell_has_localized_controls_tab_order_and_live_choices() {
    let _sta = Sta::initialize().unwrap();
    let _dpi = DpiContext::enter();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let (context, guard) = create(
        ShellOptions {
            language: Some(Language::English),
            theme: Theme::Light,
        },
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            CoreStatus::Available { mail_ready: true }
        }),
        ShellCallbacks::default(),
    )
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let language = child(&context, LANGUAGE);
    let theme = child(&context, THEME);
    let navigation = child(&context, NAVIGATION);
    let refresh = child(&context, REFRESH);
    assert_eq!(
        unsafe { SendMessageW(language, CB_GETCOUNT, None, None) }.0,
        2
    );
    assert_eq!(unsafe { SendMessageW(theme, CB_GETCOUNT, None, None) }.0, 3);
    assert_eq!(
        unsafe { SendMessageW(navigation, LB_GETCOUNT, None, None) }.0,
        Navigation::ALL.len() as isize
    );
    for control in [language, theme, navigation, refresh] {
        assert!(unsafe { GetWindowLongPtrW(control, GWL_STYLE) } as u32 & WS_TABSTOP.0 != 0);
    }
    assert_eq!(
        unsafe { GetNextDlgTabItem(guard.0, Some(language), false) }.unwrap(),
        theme
    );
    assert_eq!(
        unsafe { GetNextDlgTabItem(guard.0, Some(theme), false) }.unwrap(),
        navigation
    );
    assert_eq!(
        unsafe { GetNextDlgTabItem(guard.0, Some(navigation), false) }.unwrap(),
        child(&context, DETAIL_BODY)
    );
    context
        .state
        .borrow_mut()
        .model
        .set_selected(Navigation::Scheduled);
    unsafe {
        SendMessageW(language, CB_SETCURSEL, Some(WPARAM(1)), None);
        SendMessageW(
            guard.0,
            WM_COMMAND,
            Some(WPARAM(LANGUAGE as usize | ((CBN_SELCHANGE as usize) << 16))),
            Some(LPARAM(language.0 as isize)),
        );
    }
    assert_eq!(context.state.borrow().model.language(), Language::Korean);
    assert_eq!(
        context.state.borrow().model.selected(),
        Navigation::Scheduled
    );
    assert_eq!(label(refresh), text(Language::Korean, "refresh"));
    assert_eq!(
        label(child(&context, SECTION)),
        text(Language::Korean, "nav_scheduled")
    );
    assert!(context.error.get().is_none());
    unsafe {
        SendMessageW(theme, CB_SETCURSEL, Some(WPARAM(1)), None);
        SendMessageW(
            guard.0,
            WM_COMMAND,
            Some(WPARAM(THEME as usize | ((CBN_SELCHANGE as usize) << 16))),
            Some(LPARAM(theme.0 as isize)),
        );
    }
    assert_eq!(context.state.borrow().model.theme(), Theme::Dark);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(context.error.get().is_none());
    drop(guard);
    assert!(context.closed.get());
}

fn notify(context: &Context, hwnd: HWND, id: i32, notification: u32, index: Option<usize>) {
    let control = child(context, id);
    if let Some(index) = index {
        unsafe {
            SendMessageW(
                control,
                if id == MESSAGE_LIST {
                    LB_SETCURSEL
                } else {
                    CB_SETCURSEL
                },
                Some(WPARAM(index)),
                None,
            );
        }
    }
    unsafe {
        SendMessageW(
            hwnd,
            WM_COMMAND,
            Some(WPARAM(id as usize | ((notification as usize) << 16))),
            Some(LPARAM(control.0 as isize)),
        );
    }
    assert!(context.error.get().is_none());
}
fn complete(context: &Context, hwnd: HWND) {
    let start = Instant::now();
    while context.state.borrow().busy() {
        assert!(start.elapsed() < Duration::from_secs(3));
        poll(hwnd, context).unwrap();
        std::thread::yield_now();
    }
    assert!(context.error.get().is_none());
}
#[test]
fn ops07_gui_controls_refresh_select_visible_rows_detail_and_next_page_without_registration() {
    use crate::{MailAccount, MailFolder, MailMessage, PageCursor};
    use hantong_contracts::registration::RegistrationProgress;
    let _sta = Sta::initialize().unwrap();
    let _dpi = DpiContext::enter();
    let actions = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = Arc::clone(&actions);
    let probes = Arc::new(AtomicUsize::new(0));
    let probe_calls = Arc::clone(&probes);
    let (context, guard) = create(
        ShellOptions {
            language: Some(Language::English),
            theme: Theme::Dark,
        },
        Arc::new(move || {
            probe_calls.fetch_add(1, Ordering::SeqCst);
            CoreStatus::Available { mail_ready: true }
        }),
        ShellCallbacks {
            action: Some(Arc::new(move |action| {
                recorded.lock().unwrap().push(action.clone());
                Ok(match action {
                    MailAction::Accounts => MailActionResult::Accounts(vec![
                        MailAccount::new("a", "First").unwrap(),
                        MailAccount::new("b", "Second").unwrap(),
                    ]),
                    MailAction::Folders { account_id, cursor } => {
                        assert_eq!(account_id, "b");
                        assert!(cursor.is_none());
                        MailActionResult::Folders {
                            rows: vec![
                                MailFolder::new_with_encoding(
                                    "b",
                                    "Inbox",
                                    "494e424f58",
                                    "modified_utf7",
                                )
                                .unwrap(),
                            ],
                            next_cursor: None,
                        }
                    }
                    MailAction::Messages {
                        account_id,
                        encoding,
                        cursor,
                        ..
                    } => {
                        assert_eq!(account_id, "b");
                        assert_eq!(encoding, "modified_utf7");
                        let next =
                            PageCursor::new(&format!("m1:{}:1:0:1000", "a".repeat(64))).unwrap();
                        let count = if cursor.is_none() {
                            2
                        } else {
                            assert_eq!(cursor.as_deref(), Some(next.as_str()));
                            1
                        };
                        MailActionResult::Messages {
                            rows: (0..count)
                                .map(|i| {
                                    MailMessage::new(
                                        "b",
                                        &format!("msg_{i:064x}"),
                                        "sender",
                                        &format!("subject-{i}"),
                                        "2026-10-30 00:00:00 UTC",
                                        false,
                                        true,
                                    )
                                    .unwrap()
                                })
                                .collect(),
                            next_cursor: if cursor.is_none() { Some(next) } else { None },
                            observed: true,
                        }
                    }
                    MailAction::Connect => {
                        MailActionResult::Registration(RegistrationProgress::Pending)
                    }
                    MailAction::RegistrationStatus => {
                        MailActionResult::Registration(RegistrationProgress::Ready)
                    }
                })
            })),
        },
    )
    .unwrap();
    paint(guard.0, &context);
    // Ordinary refresh is readiness followed by accounts, never registration status.
    notify(&context, guard.0, REFRESH, BN_CLICKED, None);
    notify(&context, guard.0, CONNECT, BN_CLICKED, None); // same shared in-flight slot rejects it
    complete(&context, guard.0);
    assert_eq!(probes.load(Ordering::SeqCst), 1);
    assert_eq!(*actions.lock().unwrap(), vec![MailAction::Accounts]);
    assert_eq!(
        unsafe { SendMessageW(child(&context, ACCOUNT), CB_GETCOUNT, None, None) }.0,
        2
    );
    assert_eq!(
        unsafe { SendMessageW(child(&context, ACCOUNT), CB_GETCURSEL, None, None) }.0,
        -1
    );
    assert_eq!(
        unsafe { SendMessageW(child(&context, FOLDER), CB_GETCOUNT, None, None) }.0,
        0
    );
    notify(&context, guard.0, ACCOUNT, CBN_SELCHANGE, Some(1));
    complete(&context, guard.0);
    assert_eq!(
        unsafe { SendMessageW(child(&context, FOLDER), CB_GETCURSEL, None, None) }.0,
        -1
    );
    notify(&context, guard.0, FOLDER, CBN_SELCHANGE, Some(0));
    complete(&context, guard.0);
    assert_eq!(
        unsafe { SendMessageW(child(&context, MESSAGE_LIST), LB_GETCOUNT, None, None) }.0,
        2
    );
    assert_eq!(
        label(child(&context, DETAIL_BODY)),
        text(Language::English, "detail_empty")
    );
    notify(&context, guard.0, MESSAGE_LIST, LBN_SELCHANGE, Some(1));
    assert!(label(child(&context, DETAIL_BODY)).contains("subject-1"));
    notify(&context, guard.0, LANGUAGE, CBN_SELCHANGE, Some(1));
    assert!(label(child(&context, DETAIL_BODY)).contains(text(Language::Korean, "column_sender")));
    assert_eq!(context.state.borrow().model.theme(), Theme::Dark);
    notify(&context, guard.0, NEXT_PAGE, BN_CLICKED, None);
    complete(&context, guard.0);
    assert_eq!(
        unsafe { SendMessageW(child(&context, MESSAGE_LIST), LB_GETCOUNT, None, None) }.0,
        1
    );
    assert!(
        context
            .state
            .borrow()
            .workspace
            .selected_message()
            .is_none()
    );
    notify(&context, guard.0, NEXT_PAGE, BN_CLICKED, None); // absent cursor never repeats first page
    assert_eq!(actions.lock().unwrap().len(), 4);
    notify(&context, guard.0, CONNECT, BN_CLICKED, None);
    begin_probe(guard.0, &context).unwrap();
    complete(&context, guard.0);
    assert_eq!(probes.load(Ordering::SeqCst), 1);
    assert_eq!(
        label(child(&context, SETUP_BODY)),
        text(Language::Korean, "registration_pending")
    );
    notify(&context, guard.0, REGISTRATION_STATUS, BN_CLICKED, None);
    complete(&context, guard.0);
    assert_eq!(
        label(child(&context, SETUP_BODY)),
        text(Language::Korean, "registration_ready")
    );
    assert_eq!(
        &actions.lock().unwrap()[4..],
        &[MailAction::Connect, MailAction::RegistrationStatus]
    );
}
#[test]
fn sec03_gui_native_discards_wrong_scope_results_and_refresh_failure_clears_old_data() {
    use crate::{MailAccount, MailFolder};
    let _sta = Sta::initialize().unwrap();
    let _dpi = DpiContext::enter();
    let (context, guard) = create(
        ShellOptions::default(),
        Arc::new(|| CoreStatus::Unavailable(ErrorCode::CoreUnavailable)),
        ShellCallbacks {
            action: Some(Arc::new(|_| {
                Ok(MailActionResult::Folders {
                    rows: vec![
                        MailFolder::new_with_encoding("other", "wrong", "41", "utf8").unwrap(),
                    ],
                    next_cursor: None,
                })
            })),
        },
    )
    .unwrap();
    {
        let mut s = context.state.borrow_mut();
        s.workspace
            .apply(
                &MailAction::Accounts,
                &MailActionResult::Accounts(vec![MailAccount::new("a", "A").unwrap()]),
            )
            .unwrap();
        s.workspace.select_account("a");
    }
    begin_action(
        guard.0,
        &context,
        MailAction::Folders {
            account_id: "a".into(),
            cursor: None,
        },
    )
    .unwrap();
    complete(&context, guard.0);
    assert!(context.state.borrow().workspace.accounts().is_empty());
    assert!(label(child(&context, SETUP_BODY)).contains(ErrorCode::InvalidInput.as_str()));
    {
        context
            .state
            .borrow_mut()
            .workspace
            .apply(
                &MailAction::Accounts,
                &MailActionResult::Accounts(vec![MailAccount::new("a", "A").unwrap()]),
            )
            .unwrap();
    }
    paint(guard.0, &context);
    notify(&context, guard.0, REFRESH, BN_CLICKED, None);
    complete(&context, guard.0);
    assert_eq!(
        unsafe { SendMessageW(child(&context, ACCOUNT), CB_GETCOUNT, None, None) }.0,
        0
    );
    assert!(
        context
            .state
            .borrow()
            .workspace
            .selected_account()
            .is_none()
    );
    assert!(label(child(&context, SETUP_BODY)).contains(ErrorCode::CoreUnavailable.as_str()));
}

#[test]
fn ops08_native_resources_use_system_high_contrast_and_scaled_fonts() {
    for dpi in [96, 144, 192] {
        for theme in [
            EffectiveTheme::Light,
            EffectiveTheme::Dark,
            EffectiveTheme::HighContrast,
        ] {
            let visual = Visual::new(theme, dpi).unwrap();
            assert_eq!(
                unsafe { GetObjectType(HGDIOBJ(visual.fonts.body.0.0)) },
                OBJ_FONT.0 as u32
            );
            assert_eq!(
                unsafe { GetObjectType(HGDIOBJ(visual.surface.0.0)) },
                OBJ_BRUSH.0 as u32
            );
            let mut font = LOGFONTW::default();
            assert_ne!(
                unsafe {
                    GetObjectW(
                        HGDIOBJ(visual.fonts.body.0.0),
                        std::mem::size_of::<LOGFONTW>() as i32,
                        Some((&mut font as *mut LOGFONTW).cast()),
                    )
                },
                0
            );
            assert_eq!(font.lfHeight, -scale(14, dpi));
            if theme == EffectiveTheme::HighContrast {
                assert_eq!(visual.colors.background.0, unsafe {
                    GetSysColor(COLOR_WINDOW)
                });
                assert_eq!(visual.colors.text.0, unsafe {
                    GetSysColor(COLOR_WINDOWTEXT)
                });
            }
        }
    }
}

#[test]
fn ops02_hidden_shell_starts_probe_only_after_paint_and_polls_one_response() {
    let _sta = Sta::initialize().unwrap();
    let _dpi = DpiContext::enter();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let (context, guard) = create(
        ShellOptions::default(),
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            CoreStatus::Available { mail_ready: false }
        }),
        ShellCallbacks::default(),
    )
    .unwrap();
    begin_probe(guard.0, &context).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!context.state.borrow().probe.pending());
    paint(guard.0, &context);
    assert!(context.error.get().is_none());
    assert!(context.state.borrow().painted);
    begin_probe(guard.0, &context).unwrap();
    begin_probe(guard.0, &context).unwrap();
    let started = Instant::now();
    while context.state.borrow().probe.pending() {
        assert!(started.elapsed() < Duration::from_secs(2));
        poll(guard.0, &context).unwrap();
        std::thread::yield_now();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        context.state.borrow().model.status(),
        CoreStatus::Available { mail_ready: false }
    );
    assert!(label(child(&context, ACCESS_NOTICE)).contains(text(
        context.state.borrow().model.language(),
        "cached_notice"
    )));
    drop(guard);
}
#[test]
fn ops07_gui_filter_and_reload_controls_restart_pages_and_keep_selection_explicit() {
    use crate::{MailAccount, MailFolder, MailMessage, PageCursor};
    let _sta = Sta::initialize().unwrap();
    let _dpi = DpiContext::enter();
    let calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = Arc::clone(&calls);
    let (context, guard) = create(
        ShellOptions {
            language: Some(Language::English),
            theme: Theme::Dark,
        },
        Arc::new(|| CoreStatus::Available { mail_ready: true }),
        ShellCallbacks {
            action: Some(Arc::new(move |action| {
                captured.lock().unwrap().push(action.clone());
                match action {
                    MailAction::Messages {
                        account_id,
                        encoding,
                        filter,
                        cursor,
                        ..
                    } => {
                        assert_eq!(account_id, "a");
                        assert_eq!(encoding, "modified_utf7");
                        let suffix = match filter {
                            ImapMetadataFilter::All => "a",
                            ImapMetadataFilter::Unread => "b",
                            ImapMetadataFilter::Favorite => "c",
                        };
                        let next =
                            PageCursor::new(&format!("m1:{}:1:0:1000", suffix.repeat(64))).unwrap();
                        if let Some(value) = &cursor {
                            assert_eq!(value, next.as_str());
                        }
                        Ok(MailActionResult::Messages {
                            rows: vec![
                                MailMessage::new(
                                    "a",
                                    &format!("msg_{}", suffix.repeat(64)),
                                    "sender",
                                    filter.as_str(),
                                    "2026-10-30 00:00:00 UTC",
                                    filter != ImapMetadataFilter::Unread,
                                    filter == ImapMetadataFilter::Favorite,
                                )
                                .unwrap(),
                            ],
                            next_cursor: if cursor.is_none() { Some(next) } else { None },
                            observed: true,
                        })
                    }
                    MailAction::Folders { account_id, cursor } => {
                        assert_eq!(account_id, "b");
                        assert!(cursor.is_none());
                        Ok(MailActionResult::Folders {
                            rows: vec![],
                            next_cursor: None,
                        })
                    }
                    _ => panic!("filter controls must not register or refresh accounts"),
                }
            })),
        },
    )
    .unwrap();
    let enabled=|id|unsafe{GetWindowLongPtrW(child(&context,id),GWL_STYLE)} as u32 & WS_DISABLED.0==0;
    assert!(!enabled(FILTER) && !enabled(RELOAD_FOLDER));
    notify(&context, guard.0, RELOAD_FOLDER, BN_CLICKED, None);
    assert!(calls.lock().unwrap().is_empty());
    {
        let mut state = context.state.borrow_mut();
        state
            .workspace
            .apply(
                &MailAction::Accounts,
                &MailActionResult::Accounts(vec![
                    MailAccount::new("a", "A").unwrap(),
                    MailAccount::new("b", "B").unwrap(),
                ]),
            )
            .unwrap();
        state.workspace.select_account("a");
        state
            .workspace
            .apply(
                &MailAction::Folders {
                    account_id: "a".into(),
                    cursor: None,
                },
                &MailActionResult::Folders {
                    rows: vec![
                        MailFolder::new_with_encoding("a", "Inbox", "494e424f58", "modified_utf7")
                            .unwrap(),
                    ],
                    next_cursor: None,
                },
            )
            .unwrap();
        state.workspace.select_folder(0);
    }
    render(guard.0, &context).unwrap();
    assert!(enabled(FILTER) && enabled(RELOAD_FOLDER));
    assert_eq!(
        unsafe { SendMessageW(child(&context, FILTER), CB_GETCOUNT, None, None) }.0,
        3
    );
    notify(&context, guard.0, RELOAD_FOLDER, BN_CLICKED, None);
    complete(&context, guard.0);
    notify(&context, guard.0, MESSAGE_LIST, LBN_SELCHANGE, Some(0));
    assert!(
        context
            .state
            .borrow()
            .workspace
            .selected_message()
            .is_some()
    );
    notify(&context, guard.0, FILTER, CBN_SELCHANGE, Some(1));
    assert!(!enabled(FILTER) && !enabled(RELOAD_FOLDER) && !enabled(ACCOUNT) && !enabled(FOLDER));
    assert!(context.state.borrow().workspace.messages().is_empty());
    assert!(
        context
            .state
            .borrow()
            .workspace
            .selected_message()
            .is_none()
    );
    notify(&context, guard.0, RELOAD_FOLDER, BN_CLICKED, None); // shared slot rejects a second request
    notify(&context, guard.0, LANGUAGE, CBN_SELCHANGE, Some(1));
    complete(&context, guard.0);
    assert_eq!(
        context.state.borrow().workspace.filter(),
        ImapMetadataFilter::Unread
    );
    assert_eq!(context.state.borrow().model.theme(), Theme::Dark);
    assert_eq!(
        label(child(&context, RELOAD_FOLDER)),
        text(Language::Korean, "reload_folder")
    );
    assert_eq!(calls.lock().unwrap().len(), 2);
    assert!(!context.state.borrow().workspace.messages()[0].is_read);
    notify(&context, guard.0, NEXT_PAGE, BN_CLICKED, None);
    complete(&context, guard.0);
    notify(&context, guard.0, RELOAD_FOLDER, BN_CLICKED, None);
    complete(&context, guard.0);
    notify(&context, guard.0, FILTER, CBN_SELCHANGE, Some(2));
    complete(&context, guard.0);
    assert!(context.state.borrow().workspace.messages()[0].is_favorite);
    assert_eq!(
        unsafe { SendMessageW(child(&context, FILTER), CB_GETCURSEL, None, None) }.0,
        2
    );
    assert!(
        context
            .state
            .borrow()
            .workspace
            .selected_message()
            .is_none()
    );
    let captured = calls.lock().unwrap();
    let requests: Vec<_> = captured
        .iter()
        .map(|value| match value {
            MailAction::Messages { filter, cursor, .. } => (*filter, cursor.is_some()),
            _ => panic!(),
        })
        .collect();
    assert_eq!(
        requests,
        vec![
            (ImapMetadataFilter::All, false),
            (ImapMetadataFilter::Unread, false),
            (ImapMetadataFilter::Unread, true),
            (ImapMetadataFilter::Unread, false),
            (ImapMetadataFilter::Favorite, false)
        ]
    );
    drop(captured);
    notify(&context, guard.0, ACCOUNT, CBN_SELCHANGE, Some(1));
    complete(&context, guard.0);
    assert_eq!(
        context.state.borrow().workspace.filter(),
        ImapMetadataFilter::All
    );
    assert!(!enabled(FILTER) && !enabled(RELOAD_FOLDER));
    assert!(context.state.borrow().workspace.selected_folder().is_none());
}
