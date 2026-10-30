use super::*;
use windows::Win32::Foundation::COLORREF;

// Every drawing operation restores the caller's DC, including its selected font.
struct SavedDc(HDC, i32);
impl SavedDc {
    fn new(dc: HDC) -> Option<Self> {
        let saved = unsafe { SaveDC(dc) };
        (saved != 0).then_some(Self(dc, saved))
    }
}
impl Drop for SavedDc {
    fn drop(&mut self) {
        if self.1 != 0 {
            unsafe {
                let _ = RestoreDC(self.0, self.1);
            }
        }
    }
}
fn inset(mut r: RECT, n: i32) -> RECT {
    r.left += n;
    r.top += n;
    r.right -= n;
    r.bottom -= n;
    r
}
fn rounded(dc: HDC, r: RECT, radius: i32, fill: COLORREF, edge: COLORREF) {
    let Some(_saved) = SavedDc::new(dc) else {
        return;
    };
    unsafe {
        SelectObject(dc, GetStockObject(DC_BRUSH));
        SelectObject(dc, GetStockObject(DC_PEN));
        SetDCBrushColor(dc, fill);
        SetDCPenColor(dc, edge);
        let _ = RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
    }
}
fn text_line(
    dc: HDC,
    font: HFONT,
    color: COLORREF,
    value: &str,
    mut r: RECT,
    flags: DRAW_TEXT_FORMAT,
) {
    let Some(_saved) = SavedDc::new(dc) else {
        return;
    };
    let mut value: Vec<u16> = value.encode_utf16().collect();
    unsafe {
        SelectObject(dc, HGDIOBJ(font.0));
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            &mut value,
            &mut r,
            flags | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
    }
}
fn focus(dc: HDC, r: RECT, v: &Visual) {
    let Some(_saved) = SavedDc::new(dc) else {
        return;
    };
    unsafe {
        SetTextColor(dc, v.colors.focus);
        SetBkColor(dc, v.colors.surface);
        let _ = DrawFocusRect(dc, &inset(r, scale(3, v.dpi)));
    }
}
fn selected_text(v: &Visual) -> COLORREF {
    if v.high_contrast {
        v.colors.on_accent
    } else {
        v.colors.text
    }
}
fn fill(dc: HDC, r: RECT, color: COLORREF) {
    let Some(_saved) = SavedDc::new(dc) else {
        return;
    };
    unsafe {
        SetDCBrushColor(dc, color);
        FillRect(dc, &r, HBRUSH(GetStockObject(DC_BRUSH).0));
    }
}
fn rule(dc: HDC, x1: i32, y1: i32, x2: i32, y2: i32, color: COLORREF) {
    let Some(_saved) = SavedDc::new(dc) else {
        return;
    };
    unsafe {
        SelectObject(dc, GetStockObject(DC_PEN));
        SetDCPenColor(dc, color);
        let _ = MoveToEx(dc, x1, y1, None);
        let _ = LineTo(dc, x2, y2);
    }
}
fn blend(base: COLORREF, selected: COLORREF, alpha: u8) -> COLORREF {
    let mut result = 0;
    for shift in [0, 8, 16] {
        let a = (base.0 >> shift) & 255;
        let b = (selected.0 >> shift) & 255;
        result |= ((a * (255 - u32::from(alpha)) + b * u32::from(alpha)) / 255) << shift;
    }
    COLORREF(result)
}
fn divider(v: &Visual, base: COLORREF) -> COLORREF {
    if v.high_contrast {
        v.colors.border
    } else {
        blend(base, v.colors.text, 46)
    }
}
pub(super) fn shell(dc: HDC, v: &Visual, layout: &Layout) {
    let panels: Vec<_> = layout.panels().collect();
    for panel in panels.iter().skip(1) {
        fill(dc, *panel, v.colors.surface);
    }
    for panel in panels.iter().take(2) {
        rule(
            dc,
            panel.right,
            panel.top,
            panel.right,
            panel.bottom,
            divider(v, v.colors.surface),
        );
    }
    if let Some(last) = panels.last() {
        rule(
            dc,
            0,
            scale(42, v.dpi),
            last.right,
            scale(42, v.dpi),
            divider(v, v.colors.background),
        );
        rule(
            dc,
            panels[1].left,
            panels[1].top,
            last.right,
            panels[1].top,
            divider(v, v.colors.surface),
        );
        rule(
            dc,
            0,
            last.bottom,
            last.right,
            last.bottom,
            divider(v, v.colors.background),
        );
    }
    let p = |n| scale(n, v.dpi);
    text_line(
        dc,
        v.fonts.icon.0,
        v.colors.accent,
        "\u{e715}",
        RECT {
            left: p(12),
            top: p(10),
            right: p(32),
            bottom: p(32),
        },
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}
pub(super) fn item(context: &Context, draw: &DRAWITEMSTRUCT) -> bool {
    let id = draw.CtlID as i32;
    match draw.CtlType {
        ODT_BUTTON
            if matches!(
                id,
                REFRESH | CONNECT | REGISTRATION_STATUS | NEXT_PAGE | FOLDERS_NEXT | RELOAD_FOLDER
            ) =>
        {
            button(context, draw);
            true
        }
        ODT_LISTBOX if id == NAVIGATION => {
            navigation(context, draw);
            true
        }
        ODT_LISTBOX if id == MESSAGE_LIST => {
            message(context, draw);
            true
        }
        ODT_COMBOBOX if matches!(id, LANGUAGE | THEME | ACCOUNT | FOLDER | FILTER) => {
            combo(context, draw);
            true
        }
        _ => false,
    }
}
fn button(context: &Context, draw: &DRAWITEMSTRUCT) {
    let (v, label) = {
        let s = context.state.borrow();
        (Rc::clone(&s.visual), caption(&s.model, draw.CtlID as i32))
    };
    let disabled = draw.itemState.0 & ODS_DISABLED.0 != 0;
    let primary = draw.CtlID == CONNECT as u32 && !disabled;
    let fill = if primary {
        v.colors.accent
    } else {
        v.colors.raised
    };
    let color = if disabled {
        v.colors.muted
    } else if primary {
        v.colors.on_accent
    } else {
        v.colors.text
    };
    unsafe {
        FillRect(draw.hDC, &draw.rcItem, v.background.0);
    }
    rounded(
        draw.hDC,
        inset(draw.rcItem, 1),
        scale(6, v.dpi),
        fill,
        if v.high_contrast {
            v.colors.border
        } else {
            fill
        },
    );
    let glyph = match draw.CtlID as i32 {
        REFRESH | RELOAD_FOLDER => Some("\u{e72c}"),
        FOLDERS_NEXT => Some("\u{e76c}"),
        _ => None,
    };
    text_line(
        draw.hDC,
        if glyph.is_some() {
            v.fonts.icon.0
        } else {
            v.fonts.body.0
        },
        color,
        glyph.unwrap_or(&label),
        inset(draw.rcItem, scale(6, v.dpi)),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    if draw.itemState.0 & ODS_FOCUS.0 != 0 {
        focus(draw.hDC, draw.rcItem, &v);
    }
}
fn navigation(context: &Context, draw: &DRAWITEMSTRUCT) {
    let selected = draw.itemState.0 & ODS_SELECTED.0 != 0;
    let (v, lang, opacity) = {
        let s = context.state.borrow();
        (
            Rc::clone(&s.visual),
            s.model.language(),
            s.motion
                .opacity(NAVIGATION, draw.itemID as usize, selected, Instant::now()),
        )
    };
    unsafe {
        FillRect(draw.hDC, &draw.rcItem, v.background.0);
    }
    let Some(nav) = Navigation::ALL.get(draw.itemID as usize) else {
        return;
    };
    let r = inset(draw.rcItem, scale(2, v.dpi));
    if opacity > 0 {
        let color = blend(v.colors.background, v.colors.selected, opacity);
        rounded(draw.hDC, r, scale(6, v.dpi), color, color);
    }
    let color = if selected {
        selected_text(&v)
    } else {
        v.colors.muted
    };
    let glyph = match nav {
        Navigation::Inbox => "\u{e715}",
        Navigation::Sent => "\u{e8f1}",
        Navigation::Drafts => "\u{e70f}",
        Navigation::Scheduled => "\u{e787}",
        Navigation::Starred => "\u{e734}",
        Navigation::Spam => "\u{e7ba}",
        Navigation::Trash => "\u{e74d}",
        Navigation::Promotions => "\u{e8d4}",
        Navigation::Folders => "\u{e8b7}",
        Navigation::Tags => "\u{e8ec}",
        Navigation::Contacts => "\u{e77b}",
        Navigation::Rules => "\u{e8d7}",
        Navigation::Settings => "\u{e713}",
    };
    let icon = RECT {
        left: r.left + scale(10, v.dpi),
        right: r.left + scale(32, v.dpi),
        ..r
    };
    text_line(
        draw.hDC,
        v.fonts.icon.0,
        color,
        glyph,
        icon,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    let label = RECT {
        left: r.left + scale(44, v.dpi),
        right: r.right - scale(10, v.dpi),
        ..r
    };
    text_line(
        draw.hDC,
        v.fonts.body.0,
        color,
        text(lang, nav.key()),
        label,
        DT_VCENTER | DT_SINGLELINE,
    );
    if draw.itemState.0 & ODS_FOCUS.0 != 0 {
        focus(draw.hDC, r, &v);
    }
}
fn message(context: &Context, draw: &DRAWITEMSTRUCT) {
    let selected = draw.itemState.0 & ODS_SELECTED.0 != 0;
    let (v, lang, row, opacity) = {
        let s = context.state.borrow();
        (
            Rc::clone(&s.visual),
            s.model.language(),
            s.workspace.messages().get(draw.itemID as usize).cloned(),
            s.motion
                .opacity(MESSAGE_LIST, draw.itemID as usize, selected, Instant::now()),
        )
    };
    unsafe {
        FillRect(draw.hDC, &draw.rcItem, v.surface.0);
    }
    let Some(row) = row else {
        return;
    };
    let p = |n| scale(n, v.dpi);
    let r = inset(draw.rcItem, p(2));
    if opacity > 0 {
        fill(
            draw.hDC,
            r,
            blend(v.colors.surface, v.colors.selected, opacity),
        );
    }
    rule(
        draw.hDC,
        r.left,
        r.bottom,
        r.right,
        r.bottom,
        divider(&v, v.colors.surface),
    );
    let foreground = if selected {
        selected_text(&v)
    } else {
        v.colors.text
    };
    let secondary = if selected && v.high_contrast {
        foreground
    } else {
        v.colors.muted
    };
    let sender = RECT {
        left: r.left + p(16),
        top: r.top + p(4),
        right: r.right - p(112),
        bottom: r.top + p(23),
    };
    let date = RECT {
        left: sender.right + p(8),
        right: r.right - p(12),
        ..sender
    };
    let subject = RECT {
        left: sender.left,
        top: r.top + p(25),
        right: r.right - p(14),
        bottom: r.bottom - p(3),
    };
    text_line(
        draw.hDC,
        if row.is_read {
            v.fonts.body.0
        } else {
            v.fonts.heading.0
        },
        foreground,
        &row.sender,
        sender,
        DT_SINGLELINE | DT_VCENTER,
    );
    text_line(
        draw.hDC,
        v.fonts.caption.0,
        secondary,
        &row.date_utc,
        date,
        DT_SINGLELINE | DT_RIGHT | DT_VCENTER,
    );
    text_line(
        draw.hDC,
        v.fonts.body.0,
        secondary,
        if row.subject.is_empty() {
            text(lang, "subject_empty")
        } else {
            &row.subject
        },
        subject,
        DT_SINGLELINE | DT_VCENTER,
    );
    if !row.is_read {
        let dot = RECT {
            left: r.left + p(5),
            top: r.top + p(15),
            right: r.left + p(9),
            bottom: r.top + p(19),
        };
        rounded(
            draw.hDC,
            dot,
            p(4),
            if selected && v.high_contrast {
                foreground
            } else {
                v.colors.accent
            },
            foreground,
        );
    }
    if draw.itemState.0 & ODS_FOCUS.0 != 0 {
        focus(draw.hDC, r, &v);
    }
}
fn combo(context: &Context, draw: &DRAWITEMSTRUCT) {
    let (v, value) = {
        let s = context.state.borrow();
        let lang = s.model.language();
        let i = draw.itemID as usize;
        let value = match draw.CtlID as i32 {
            LANGUAGE => ["language_en", "language_ko"]
                .get(i)
                .map(|key| text(lang, key).to_owned()),
            THEME => ["theme_light", "theme_dark", "theme_system"]
                .get(i)
                .map(|key| text(lang, key).to_owned()),
            FILTER => ["filter_all", "filter_unread", "filter_favorite"]
                .get(i)
                .map(|key| text(lang, key).to_owned()),
            ACCOUNT => s.workspace.accounts().get(i).map(|a| a.label.clone()),
            FOLDER => s.workspace.folders().get(i).map(|f| f.display_name.clone()),
            _ => None,
        }
        .unwrap_or_else(|| {
            text(
                lang,
                if draw.CtlID == ACCOUNT as u32 {
                    "account"
                } else {
                    "folder"
                },
            )
            .into()
        });
        (Rc::clone(&s.visual), value)
    };
    let selected = draw.itemState.0 & ODS_SELECTED.0 != 0;
    let fill = if selected {
        v.colors.selected
    } else {
        v.colors.raised
    };
    rounded(draw.hDC, draw.rcItem, scale(8, v.dpi), fill, fill);
    let color = if draw.itemState.0 & ODS_DISABLED.0 != 0 {
        v.colors.muted
    } else if selected {
        selected_text(&v)
    } else {
        v.colors.text
    };
    let r = RECT {
        left: draw.rcItem.left + scale(10, v.dpi),
        right: draw.rcItem.right - scale(8, v.dpi),
        ..draw.rcItem
    };
    text_line(
        draw.hDC,
        v.fonts.body.0,
        color,
        &value,
        r,
        DT_SINGLELINE | DT_VCENTER,
    );
    if draw.itemState.0 & ODS_FOCUS.0 != 0 {
        focus(draw.hDC, draw.rcItem, &v);
    }
}

const EMPTY_SUBCLASS: usize = 1;
const COMBO_SUBCLASS: usize = 0x4843_4f4d;

pub(super) fn install_combo_skin(hwnd: HWND, owner: &Rc<Context>) -> Result<(), ErrorCode> {
    let pointer = Rc::into_raw(Rc::clone(owner));
    if !unsafe { SetWindowSubclass(hwnd, Some(combo_proc), COMBO_SUBCLASS, pointer as usize) }
        .as_bool()
    {
        unsafe {
            drop(Rc::from_raw(pointer));
        }
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}
unsafe extern "system" fn combo_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    let pointer = data as *const Context;
    if message == WM_NCDESTROY {
        let removed = unsafe { RemoveWindowSubclass(hwnd, Some(combo_proc), id) }.as_bool();
        let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
        if removed {
            unsafe {
                drop(Rc::from_raw(pointer));
            }
        }
        return result;
    }
    // refdata originates at into_raw; a callback owns a temporary reference so a
    // synchronous default procedure may destroy the window without freeing us.
    unsafe {
        Rc::increment_strong_count(pointer);
    }
    let owner = unsafe { Rc::from_raw(pointer) };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
        if owner.closed.get() || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return result;
        }
        if matches!(message, WM_PAINT | WM_NCPAINT | WM_PRINTCLIENT) {
            let mut r = RECT::default();
            if message == WM_PRINTCLIENT {
                if unsafe { GetClientRect(hwnd, &mut r) }.is_ok() {
                    paint_combo(hwnd, &owner, HDC(wp.0 as *mut _), r);
                }
            } else if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() {
                r = RECT {
                    left: 0,
                    top: 0,
                    right: r.right - r.left,
                    bottom: r.bottom - r.top,
                };
                let dc = unsafe { GetWindowDC(Some(hwnd)) };
                if !dc.0.is_null() {
                    paint_combo(hwnd, &owner, dc, r);
                    unsafe {
                        ReleaseDC(Some(hwnd), dc);
                    }
                }
            }
        } else if matches!(
            message,
            WM_ENABLE | WM_SETFOCUS | WM_KILLFOCUS | CB_SETCURSEL
        ) {
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
        }
        result
    })) {
        Ok(result) => result,
        Err(_) => {
            owner.error.set(Some(ErrorCode::InternalError));
            unsafe {
                let _ = PostMessageW(
                    Some(GetAncestor(hwnd, GA_ROOT)),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
            LRESULT(0)
        }
    }
}
fn paint_combo(hwnd: HWND, context: &Context, dc: HDC, r: RECT) {
    if dc.0.is_null() {
        return;
    }
    let v = Rc::clone(&context.state.borrow().visual);
    let disabled = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32 & WS_DISABLED.0 != 0;
    let focused = unsafe { GetFocus() } == hwnd;
    fill(dc, r, v.colors.raised);
    rounded(
        dc,
        r,
        scale(4, v.dpi),
        v.colors.raised,
        if focused {
            v.colors.focus
        } else {
            v.colors.border
        },
    );
    let id = unsafe { GetDlgCtrlID(hwnd) } as u32;
    let index = unsafe { SendMessageW(hwnd, CB_GETCURSEL, None, None) }.0;
    let draw = DRAWITEMSTRUCT {
        CtlType: ODT_COMBOBOX,
        CtlID: id,
        itemID: u32::try_from(index).unwrap_or(u32::MAX),
        itemState: if disabled {
            ODS_DISABLED
        } else {
            windows::Win32::UI::Controls::ODS_FLAGS(0)
        },
        hwndItem: hwnd,
        hDC: dc,
        rcItem: RECT {
            right: r.right - scale(26, v.dpi),
            ..inset(r, scale(2, v.dpi))
        },
        ..Default::default()
    };
    combo(context, &draw);
    let arrow = RECT {
        left: r.right - scale(24, v.dpi),
        right: r.right - scale(4, v.dpi),
        ..r
    };
    text_line(
        dc,
        v.fonts.icon.0,
        if disabled {
            v.colors.muted
        } else {
            v.colors.text
        },
        "\u{e70d}",
        arrow,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

pub(super) fn install_empty_state(hwnd: HWND, owner: &Rc<Context>) -> Result<(), ErrorCode> {
    // The subclass reference has independent ownership from the parent's HWND.
    // It is created and removed on this UI thread only.
    let pointer = Rc::into_raw(Rc::clone(owner));
    if !unsafe { SetWindowSubclass(hwnd, Some(empty_proc), EMPTY_SUBCLASS, pointer as usize) }
        .as_bool()
    {
        unsafe {
            drop(Rc::from_raw(pointer));
        }
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}
unsafe extern "system" fn empty_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    let pointer = data as *const Context;
    if message == WM_NCDESTROY {
        let removed = unsafe { RemoveWindowSubclass(hwnd, Some(empty_proc), id) }.as_bool();
        let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
        // A failed removal retains ownership rather than leave callable refdata dangling.
        if removed {
            unsafe {
                drop(Rc::from_raw(pointer));
            }
        }
        return result;
    }
    // A nested default callback may destroy the child and release its installed
    // reference. Keep this callback independently alive until it returns.
    unsafe {
        Rc::increment_strong_count(pointer);
    }
    let owner = unsafe { Rc::from_raw(pointer) };
    let context = &owner;
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if message == WM_PAINT {
            let snapshot = {
                let s = context.state.borrow();
                s.workspace
                    .messages()
                    .is_empty()
                    .then(|| (Rc::clone(&s.visual), s.model.language(), s.notice.clone()))
            };
            if let Some((v, lang, notice)) = snapshot {
                let mut paint = PAINTSTRUCT::default();
                let dc = unsafe { BeginPaint(hwnd, &mut paint) };
                let mut r = RECT::default();
                let valid = !dc.0.is_null() && unsafe { GetClientRect(hwnd, &mut r) }.is_ok();
                if valid {
                    empty(dc, r, &v, lang, &notice);
                }
                let ended = unsafe { EndPaint(hwnd, &paint) }.as_bool();
                if !valid || !ended {
                    context.error.set(Some(ErrorCode::InternalError));
                }
                return LRESULT(0);
            }
        }
        // The snapshot borrow above has ended before native reentrancy.
        unsafe { DefSubclassProc(hwnd, message, wp, lp) }
    })) {
        Ok(result) => result,
        Err(_) => {
            context.error.set(Some(ErrorCode::InternalError));
            unsafe {
                let _ = PostMessageW(
                    Some(GetAncestor(hwnd, GA_ROOT)),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
            LRESULT(0)
        }
    }
}
fn empty(dc: HDC, r: RECT, v: &Visual, lang: Language, notice: &Notice) {
    unsafe {
        FillRect(dc, &r, v.surface.0);
    }
    let p = |n| scale(n, v.dpi);
    let center = (r.left + r.right) / 2;
    let top = r.top + ((r.bottom - r.top - p(98)) / 2).max(p(4));
    let envelope = RECT {
        left: center - p(16),
        top,
        right: center + p(16),
        bottom: top + p(22),
    };
    rounded(dc, envelope, p(8), v.colors.raised, v.colors.border);
    {
        let Some(_saved) = SavedDc::new(dc) else {
            return;
        };
        unsafe {
            SelectObject(dc, GetStockObject(DC_PEN));
            SetDCPenColor(dc, v.colors.muted);
            let _ = MoveToEx(dc, envelope.left + p(3), top + p(5), None);
            let _ = LineTo(dc, center, top + p(14));
            let _ = LineTo(dc, envelope.right - p(3), top + p(5));
        }
    }
    let title = RECT {
        left: r.left + p(12),
        top: top + p(32),
        right: r.right - p(12),
        bottom: top + p(54),
    };
    let hint = RECT {
        top: title.bottom + p(4),
        bottom: (title.bottom + p(44)).min(r.bottom),
        ..title
    };
    let key = match notice {
        Notice::Unobserved => "metadata_unobserved",
        Notice::Messages(_) => "empty_matches",
        Notice::Loading => "loading",
        Notice::Error(_) => "status_error",
        _ => "empty_title",
    };
    text_line(
        dc,
        v.fonts.heading.0,
        v.colors.text,
        text(lang, key),
        title,
        DT_CENTER | DT_SINGLELINE | DT_VCENTER,
    );
    if hint.bottom > hint.top {
        text_line(
            dc,
            v.fonts.caption.0,
            v.colors.muted,
            text(lang, "empty_hint"),
            hint,
            DT_CENTER | DT_WORDBREAK,
        );
    }
}
