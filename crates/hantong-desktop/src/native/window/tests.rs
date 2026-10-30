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
    assert_eq!(Rc::strong_count(&context), 2);
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
        2,
        "caller and HWND each own one reference"
    );

    // Inject the same failure branch used when DestroyWindow returns an error.
    guard.close_with(|_| false);
    assert_eq!(context.error.get(), Some(ErrorCode::InternalError));
    drop(context);
    assert_eq!(weak.strong_count(), 1, "the live HWND remains the owner");
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
fn ops07_ops08_shell_layout_retains_regions_at_100_150_200_percent() {
    for dpi in [96, 144, 192] {
        for (width, height, compact) in [(1120, 740, false), (760, 720, true)] {
            let layout = Layout::new(scale(width, dpi), scale(height, dpi), dpi);
            assert_eq!(layout.panels().count(), if compact { 2 } else { 3 });
            assert!(layout.control(DETAIL_BODY).unwrap().visible);
            assert!(!layout.control(COMPACT).unwrap().visible);
            for id in BRAND..=FOLDERS_NEXT {
                let p = layout.control(id).unwrap();
                if !p.visible {
                    continue;
                }
                assert!(p.rect.left >= 0 && p.rect.top >= 0);
                assert!(
                    p.rect.right <= scale(width, dpi) && p.rect.bottom <= scale(height, dpi),
                    "{id} at {dpi}"
                );
                assert!(p.rect.right > p.rect.left && p.rect.bottom > p.rect.top);
            }
            let list = layout.control(MESSAGE_LIST).unwrap().rect;
            assert!(layout.control(SETUP_BODY).unwrap().rect.bottom <= list.top);
            assert!(list.bottom <= layout.control(NEXT_PAGE).unwrap().rect.top);
            if compact {
                assert!(
                    layout.control(NEXT_PAGE).unwrap().rect.bottom
                        <= layout.control(DETAIL_TITLE).unwrap().rect.top
                );
            }
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
            assert_eq!(font.lfHeight, -scale(16, dpi));
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
