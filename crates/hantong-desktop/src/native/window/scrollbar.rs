//! Native SCROLLBAR input/accessibility, with themed painting and native data controls.
//! Each subclass holds an Rc; callbacks take temporary owners across reentrancy.
use super::*;
use windows::Win32::{
    Foundation::{COLORREF, POINT},
    System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
    UI::{
        Accessibility::{CLSID_AccPropServices, IAccPropServices, NotifyWinEvent, PROPID_ACC_NAME},
        Controls::{
            EM_GETFIRSTVISIBLELINE, EM_GETLINECOUNT, EM_GETRECT, EM_LINESCROLL,
            STATE_SYSTEM_PRESSED, SetScrollInfo, ShowScrollBar, WM_MOUSELEAVE,
        },
        Input::KeyboardAndMouse::{
            GetCapture, IsWindowEnabled, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT,
            TrackMouseEvent, VK_ESCAPE,
        },
        Shell::GetWindowSubclass,
    },
};
const RAIL_SUBCLASS: usize = 0x4854_5201;
const TARGET_SUBCLASS: usize = 0x4854_5202;
const SYNC: u32 = WM_APP + 73;
const HOLD_TIMER: usize = 0x4854_5255;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    List,
    Edit,
}
#[derive(Clone, Copy)]
pub(super) struct Theme {
    pub track: COLORREF,
    pub thumb: COLORREF,
    pub hot: COLORREF,
    pub focus: COLORREF,
    pub high_contrast: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct Metrics {
    count: i32,
    page: i32,
    pos: i32,
}
impl Metrics {
    fn max_pos(self) -> i32 {
        (self.count - self.page).max(0)
    }
    fn overflow(self) -> bool {
        self.count > self.page
    }
}
struct Rail {
    parent: HWND,
    target: Cell<HWND>,
    hwnd: Cell<HWND>,
    kind: Kind,
    theme: Cell<Theme>,
    dpi: Cell<u32>,
    pending: Cell<bool>,
    syncing: Cell<bool>,
    scrolling: Cell<bool>,
    hover: Cell<usize>,
    dragging: Cell<bool>,
    pressed: Cell<usize>,
    grab_offset: Cell<i32>,
    drag_origin: Cell<i32>,
    pointer_x: Cell<i32>,
    pointer_y: Cell<i32>,
    repeating: Cell<bool>,
    dirty: Cell<bool>,
    annotation: RefCell<Option<IAccPropServices>>,
    name: RefCell<String>,
    error: Cell<Option<ErrorCode>>,
}
struct Reset<'a>(&'a Cell<bool>);
impl Drop for Reset<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
unsafe fn callback_owner(data: usize) -> Rc<Rail> {
    let pointer = data as *const Rail;
    unsafe {
        Rc::increment_strong_count(pointer);
        Rc::from_raw(pointer)
    }
}
fn owner(hwnd: HWND) -> Result<Rc<Rail>, ErrorCode> {
    let mut data = 0;
    if !unsafe { GetWindowSubclass(hwnd, Some(rail_proc), RAIL_SUBCLASS, Some(&mut data)) }
        .as_bool()
        || data == 0
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(unsafe { callback_owner(data) })
}
fn queue(state: &Rail) {
    let hwnd = state.hwnd.get();
    if state.pressed.get() != 0 {
        state.dirty.set(true);
        return;
    }
    if hwnd.0.is_null() || state.syncing.get() || state.pending.replace(true) {
        return;
    }
    if unsafe { PostMessageW(Some(hwnd), SYNC, WPARAM(0), LPARAM(0)) }.is_err() {
        state.pending.set(false);
        state.error.set(Some(ErrorCode::InternalError));
    }
}
pub(super) fn attach(parent: HWND, target: HWND, id: i32, kind: Kind) -> Result<HWND, ErrorCode> {
    if !unsafe { IsWindow(Some(parent)) }.as_bool() || !unsafe { IsWindow(Some(target)) }.as_bool()
    {
        return Err(ErrorCode::InternalError);
    }
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("SCROLLBAR"),
            None,
            WS_CHILD | WS_TABSTOP | WINDOW_STYLE(SBS_VERT as u32),
            0,
            0,
            10,
            10,
            Some(parent),
            Some(HMENU(id as usize as *mut _)),
            None,
            None,
        )
    }
    .map_err(|_| ErrorCode::InternalError)?;
    let state = Rc::new(Rail {
        parent,
        target: Cell::new(target),
        hwnd: Cell::new(hwnd),
        kind,
        theme: Cell::new(Theme {
            track: COLORREF(0),
            thumb: COLORREF(0),
            hot: COLORREF(0),
            focus: COLORREF(0),
            high_contrast: false,
        }),
        dpi: Cell::new(96),
        pending: Cell::new(false),
        syncing: Cell::new(false),
        scrolling: Cell::new(false),
        hover: Cell::new(0),
        dragging: Cell::new(false),
        pressed: Cell::new(0),
        grab_offset: Cell::new(0),
        drag_origin: Cell::new(0),
        pointer_x: Cell::new(0),
        pointer_y: Cell::new(0),
        repeating: Cell::new(false),
        dirty: Cell::new(false),
        annotation: RefCell::new(None),
        name: RefCell::new(String::new()),
        error: Cell::new(None),
    });
    let raw = Rc::into_raw(Rc::clone(&state));
    if !unsafe { SetWindowSubclass(hwnd, Some(rail_proc), RAIL_SUBCLASS, raw as usize) }.as_bool() {
        unsafe {
            drop(Rc::from_raw(raw));
            let _ = DestroyWindow(hwnd);
        }
        return Err(ErrorCode::InternalError);
    }
    let raw = Rc::into_raw(Rc::clone(&state));
    if !unsafe { SetWindowSubclass(target, Some(target_proc), TARGET_SUBCLASS, raw as usize) }
        .as_bool()
    {
        state.target.set(HWND::default());
        unsafe {
            drop(Rc::from_raw(raw));
            let _ = DestroyWindow(hwnd);
        }
        return Err(ErrorCode::InternalError);
    }
    queue(&state);
    Ok(hwnd)
}
pub(super) fn set_name(hwnd: HWND, name: &str) -> Result<(), ErrorCode> {
    let state = owner(hwnd)?;
    if state.name.borrow().as_str() == name {
        return Ok(());
    }
    let cached = state.annotation.borrow().clone();
    let annotation = match cached {
        Some(value) => value,
        None => {
            let value: IAccPropServices =
                unsafe { CoCreateInstance(&CLSID_AccPropServices, None, CLSCTX_INPROC_SERVER) }
                    .map_err(|_| ErrorCode::InternalError)?;
            *state.annotation.borrow_mut() = Some(value.clone());
            value
        }
    };
    // Native SCROLLBAR otherwise derives its name from the previous static label.
    unsafe {
        annotation.SetHwndPropStr(
            hwnd,
            OBJID_CLIENT.0 as u32,
            0,
            PROPID_ACC_NAME,
            &HSTRING::from(name),
        )
    }
    .map_err(|_| ErrorCode::InternalError)?;
    *state.name.borrow_mut() = name.into();
    unsafe {
        NotifyWinEvent(EVENT_OBJECT_NAMECHANGE, hwnd, OBJID_CLIENT.0, 0);
    }
    Ok(())
}
fn metrics(state: &Rail) -> Result<Metrics, ErrorCode> {
    let target = state.target.get();
    if target.0.is_null() {
        return Err(ErrorCode::InternalError);
    }
    let (count, page, pos) = match state.kind {
        Kind::List => {
            let mut rect = RECT::default();
            unsafe { GetClientRect(target, &mut rect) }.map_err(|_| ErrorCode::InternalError)?;
            let count = unsafe { SendMessageW(target, LB_GETCOUNT, None, None) }.0 as i32;
            let line =
                unsafe { SendMessageW(target, LB_GETITEMHEIGHT, Some(WPARAM(0)), None) }.0 as i32;
            let pos = unsafe { SendMessageW(target, LB_GETTOPINDEX, None, None) }.0 as i32;
            if count < 0 || line <= 0 {
                return Err(ErrorCode::InternalError);
            }
            (count, ((rect.bottom - rect.top) / line).max(1), pos)
        }
        Kind::Edit => {
            let count = unsafe { SendMessageW(target, EM_GETLINECOUNT, None, None) }.0 as i32;
            let pos = unsafe { SendMessageW(target, EM_GETFIRSTVISIBLELINE, None, None) }.0 as i32;
            let mut rect = RECT::default();
            unsafe {
                SendMessageW(
                    target,
                    EM_GETRECT,
                    None,
                    Some(LPARAM((&mut rect as *mut RECT) as isize)),
                );
            }
            let dc = unsafe { GetDC(Some(target)) };
            if dc.0.is_null() {
                return Err(ErrorCode::InternalError);
            }
            let font = unsafe { SendMessageW(target, WM_GETFONT, None, None) }.0;
            let old = if font != 0 {
                Some(unsafe { SelectObject(dc, HGDIOBJ(font as *mut _)) })
            } else {
                None
            };
            let mut metric = TEXTMETRICW::default();
            let valid = unsafe { GetTextMetricsW(dc, &mut metric) }.as_bool();
            unsafe {
                if let Some(old) = old {
                    SelectObject(dc, old);
                }
                ReleaseDC(Some(target), dc);
            }
            if !valid || metric.tmHeight <= 0 || count < 0 {
                return Err(ErrorCode::InternalError);
            }
            (
                count,
                ((rect.bottom - rect.top) / metric.tmHeight).max(1),
                pos,
            )
        }
    };
    let mut result = Metrics {
        count,
        page,
        pos: 0,
    };
    result.pos = pos.clamp(0, result.max_pos());
    Ok(result)
}
pub(super) fn update(hwnd: HWND, theme: Theme, dpi: u32) -> Result<(), ErrorCode> {
    let state = owner(hwnd)?;
    if let Some(error) = state.error.get() {
        return Err(error);
    }
    state.theme.set(theme);
    state.dpi.set(dpi.max(96));
    if state.scrolling.get() {
        queue(&state);
        return Ok(());
    }
    sync(&state)
}
fn sync(state: &Rail) -> Result<(), ErrorCode> {
    if state.pressed.get() != 0 {
        state.dirty.set(true);
        return Ok(());
    }
    let target = state.target.get();
    let rail = state.hwnd.get();
    if target.0.is_null() || rail.0.is_null() || state.syncing.replace(true) {
        return Ok(());
    }
    let _reset = Reset(&state.syncing);
    // Runs after scrollbar-message processing. Hide before measuring wrapped lines.
    unsafe { ShowScrollBar(target, SB_VERT, false) }.map_err(|_| ErrorCode::InternalError)?;
    let data = metrics(state)?;
    let mut rect = RECT::default();
    unsafe { GetWindowRect(target, &mut rect) }.map_err(|_| ErrorCode::InternalError)?;
    let mut point = POINT {
        x: rect.right,
        y: rect.top,
    };
    if !unsafe { ScreenToClient(state.parent, &mut point) }.as_bool() {
        return Err(ErrorCode::InternalError);
    }
    let dpi = state.dpi.get();
    unsafe {
        MoveWindow(
            rail,
            point.x + scale(2, dpi),
            point.y,
            scale(10, dpi),
            (rect.bottom - rect.top).max(1),
            false,
        )
    }
    .map_err(|_| ErrorCode::InternalError)?;
    let info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_RANGE | SIF_PAGE | SIF_POS | SIF_DISABLENOSCROLL,
        nMin: 0,
        nMax: (data.count - 1).max(0),
        nPage: data.page as u32,
        nPos: data.pos,
        ..Default::default()
    };
    unsafe {
        SetScrollInfo(rail, SB_CTL, &info, false);
        if !data.overflow() && GetFocus() == rail {
            let _ = SetFocus(Some(target));
        }
        let _ = ShowWindow(rail, if data.overflow() { SW_SHOWNA } else { SW_HIDE });
        let _ = EnableWindow(rail, IsWindowEnabled(target).as_bool());
        let _ = InvalidateRect(Some(rail), None, false);
    }
    if data.overflow() {
        paint(rail, state)?;
    }
    Ok(())
}
fn destination(current: Metrics, code: u16, track: i32) -> i32 {
    let desired = match u32::from(code) {
        x if x == SB_LINEUP.0 as u32 => current.pos.saturating_sub(1),
        x if x == SB_LINEDOWN.0 as u32 => current.pos.saturating_add(1),
        x if x == SB_PAGEUP.0 as u32 => current.pos.saturating_sub(current.page),
        x if x == SB_PAGEDOWN.0 as u32 => current.pos.saturating_add(current.page),
        x if x == SB_TOP.0 as u32 => 0,
        x if x == SB_BOTTOM.0 as u32 => current.max_pos(),
        x if x == SB_THUMBTRACK.0 as u32 || x == SB_THUMBPOSITION.0 as u32 => track,
        _ => current.pos,
    };
    desired.clamp(0, current.max_pos())
}
pub(super) fn scroll(hwnd: HWND, code: u16) -> Result<(), ErrorCode> {
    let state = owner(hwnd)?;
    if state.target.get().0.is_null() || state.scrolling.replace(true) {
        return Ok(());
    }
    let _reset = Reset(&state.scrolling);
    let current = metrics(&state)?;
    let mut info = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: SIF_TRACKPOS,
        ..Default::default()
    };
    unsafe { GetScrollInfo(hwnd, SB_CTL, &mut info) }.map_err(|_| ErrorCode::InternalError)?;
    let desired = destination(current, code, info.nTrackPos);
    apply_position(&state, desired)?;
    queue(&state);
    Ok(())
}
fn apply_position(state: &Rail, desired: i32) -> Result<(), ErrorCode> {
    let current = metrics(state)?;
    let desired = desired.clamp(0, current.max_pos());
    let target = state.target.get();
    unsafe {
        match state.kind {
            Kind::List => {
                SendMessageW(target, LB_SETTOPINDEX, Some(WPARAM(desired as usize)), None);
            }
            Kind::Edit => {
                SendMessageW(
                    target,
                    EM_LINESCROLL,
                    Some(WPARAM(0)),
                    Some(LPARAM((desired - current.pos) as isize)),
                );
            }
        }
    }
    let actual = metrics(state)?.pos;
    let rail = state.hwnd.get();
    if !rail.0.is_null() {
        let info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_POS,
            nPos: actual,
            ..Default::default()
        };
        unsafe {
            SetScrollInfo(rail, SB_CTL, &info, false);
        }
        if actual != current.pos {
            unsafe {
                NotifyWinEvent(EVENT_OBJECT_VALUECHANGE, rail, OBJID_CLIENT.0, 0);
            }
        }
    }
    Ok(())
}
fn region_code(region: usize) -> Option<u16> {
    match region {
        1 => Some(SB_LINEUP.0 as u16),
        2 => Some(SB_PAGEUP.0 as u16),
        4 => Some(SB_PAGEDOWN.0 as u16),
        5 => Some(SB_LINEDOWN.0 as u16),
        _ => None,
    }
}
fn pointer_end(state: &Rail, cancel: bool) -> Result<(), ErrorCode> {
    let was_pressed = state.pressed.replace(0) != 0;
    let was_dragging = state.dragging.replace(false);
    let rail = state.hwnd.get();
    state.repeating.set(false);
    state.dirty.set(false);
    if !rail.0.is_null() {
        unsafe {
            let _ = KillTimer(Some(rail), HOLD_TIMER);
        }
    }
    if !was_pressed {
        return Ok(());
    }
    let restored = if cancel && was_dragging && !state.target.get().0.is_null() {
        apply_position(state, state.drag_origin.get())
    } else {
        Ok(())
    };
    if !rail.0.is_null() && unsafe { GetCapture() } == rail {
        unsafe {
            let _ = ReleaseCapture();
        }
    }
    queue(state);
    restored
}
fn pointer_down(hwnd: HWND, state: &Rail, lp: LPARAM) -> Result<(), ErrorCode> {
    pointer_end(state, false)?;
    let region = hover_region(hwnd, lp);
    if region == 0 {
        return Ok(());
    }
    let data = metrics(state)?;
    if !data.overflow() {
        return Ok(());
    }
    let mut native = SCROLLBARINFO {
        cbSize: std::mem::size_of::<SCROLLBARINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetScrollBarInfo(hwnd, OBJID_CLIENT, &mut native) }
        .map_err(|_| ErrorCode::InternalError)?;
    let y = (lp.0 >> 16) as i16 as i32;
    state.pressed.set(region);
    state.hover.set(region);
    state.pointer_x.set(lp.0 as i16 as i32);
    state.pointer_y.set(y);
    state.dragging.set(region == 3);
    state.grab_offset.set(y - native.xyThumbTop);
    state.drag_origin.set(data.pos);
    unsafe {
        let _ = SetFocus(Some(hwnd));
    }
    if state.hwnd.get() != hwnd || state.pressed.get() == 0 {
        return Ok(());
    }
    unsafe {
        SetCapture(hwnd);
    }
    if state.hwnd.get().0.is_null() || unsafe { GetCapture() } != hwnd {
        pointer_end(state, false)?;
        return Ok(());
    }
    if let Some(code) = region_code(region) {
        apply_position(state, destination(data, code, 0))?;
        if unsafe { SetTimer(Some(hwnd), HOLD_TIMER, 400, None) } == 0 {
            pointer_end(state, false)?;
        }
    }
    paint(hwnd, state)
}
fn pointer_move(hwnd: HWND, state: &Rail, lp: LPARAM) -> Result<(), ErrorCode> {
    let y = (lp.0 >> 16) as i16 as i32;
    state.pointer_x.set(lp.0 as i16 as i32);
    state.pointer_y.set(y);
    if !state.dragging.get() {
        return paint(hwnd, state);
    }
    let data = metrics(state)?;
    let mut native = SCROLLBARINFO {
        cbSize: std::mem::size_of::<SCROLLBARINFO>() as u32,
        ..Default::default()
    };
    let mut rect = RECT::default();
    unsafe { GetScrollBarInfo(hwnd, OBJID_CLIENT, &mut native) }
        .map_err(|_| ErrorCode::InternalError)?;
    unsafe { GetClientRect(hwnd, &mut rect) }.map_err(|_| ErrorCode::InternalError)?;
    let travel =
        rect.bottom - native.dxyLineButton * 2 - (native.xyThumbBottom - native.xyThumbTop);
    if travel > 0 {
        let offset = (y - state.grab_offset.get() - native.dxyLineButton).clamp(0, travel);
        let desired = (i64::from(offset) * i64::from(data.max_pos()) / i64::from(travel)) as i32;
        apply_position(state, desired)?;
    }
    paint(hwnd, state)
}
fn repeat_pointer(hwnd: HWND, state: &Rail) -> Result<(), ErrorCode> {
    if state.pressed.get() == 0 || unsafe { GetCapture() } != hwnd {
        return pointer_end(state, false);
    }
    if !state.repeating.replace(true) && unsafe { SetTimer(Some(hwnd), HOLD_TIMER, 50, None) } == 0
    {
        return pointer_end(state, false);
    }
    let point = LPARAM(
        ((state.pointer_y.get() as u16 as usize) << 16 | state.pointer_x.get() as u16 as usize)
            as isize,
    );
    if hover_region(hwnd, point) == state.pressed.get()
        && let Some(code) = region_code(state.pressed.get())
    {
        apply_position(state, destination(metrics(state)?, code, 0))?;
    }
    paint(hwnd, state)
}
fn paint(hwnd: HWND, state: &Rail) -> Result<(), ErrorCode> {
    let dc = unsafe { GetDC(Some(hwnd)) };
    if dc.0.is_null() {
        return Err(ErrorCode::InternalError);
    }
    let result = draw(hwnd, dc, state);
    unsafe {
        ReleaseDC(Some(hwnd), dc);
    }
    result
}
fn draw(hwnd: HWND, dc: HDC, state: &Rail) -> Result<(), ErrorCode> {
    let saved = unsafe { SaveDC(dc) };
    let mut rect = RECT::default();
    let mut native = SCROLLBARINFO {
        cbSize: std::mem::size_of::<SCROLLBARINFO>() as u32,
        ..Default::default()
    };
    let result = (|| {
        unsafe { GetClientRect(hwnd, &mut rect) }.map_err(|_| ErrorCode::InternalError)?;
        unsafe { GetScrollBarInfo(hwnd, OBJID_CLIENT, &mut native) }
            .map_err(|_| ErrorCode::InternalError)?;
        if saved == 0 {
            return Err(ErrorCode::InternalError);
        }
        let theme = state.theme.get();
        let active = state.hover.get() == 3
            || state.dragging.get()
            || native.rgstate[3] & STATE_SYSTEM_PRESSED.0 != 0;
        unsafe {
            SelectObject(dc, GetStockObject(DC_BRUSH));
            SelectObject(dc, GetStockObject(DC_PEN));
            SetDCBrushColor(dc, theme.track);
            SetDCPenColor(dc, theme.track);
            let _ = Rectangle(dc, rect.left, rect.top, rect.right, rect.bottom);
            let width = scale(
                if active || theme.high_contrast { 8 } else { 6 },
                state.dpi.get(),
            );
            let left = (rect.right - width) / 2;
            let color = if active { theme.hot } else { theme.thumb };
            SetDCBrushColor(dc, color);
            SetDCPenColor(dc, color);
            if native.xyThumbBottom > native.xyThumbTop {
                let _ = RoundRect(
                    dc,
                    left,
                    native.xyThumbTop,
                    left + width,
                    native.xyThumbBottom,
                    width,
                    width,
                );
            }
            // Match the actual native line-button hit regions rather than hide them.
            let x = rect.right / 2;
            let half = scale(2, state.dpi.get());
            for (y, direction, region) in [
                (native.dxyLineButton / 2, -1, 1),
                (rect.bottom - native.dxyLineButton / 2, 1, 5),
            ] {
                SetDCPenColor(
                    dc,
                    if state.hover.get() == region
                        || state.pressed.get() == region
                        || native.rgstate[region] & STATE_SYSTEM_PRESSED.0 != 0
                    {
                        theme.hot
                    } else {
                        theme.thumb
                    },
                );
                let _ = MoveToEx(dc, x - half, y - direction, None);
                let _ = LineTo(dc, x, y + direction);
                let _ = LineTo(dc, x + half, y - direction);
            }
            if GetFocus() == hwnd {
                SetTextColor(dc, theme.focus);
                SetBkColor(dc, theme.track);
                let _ = DrawFocusRect(dc, &rect);
            }
        }
        Ok(())
    })();
    unsafe {
        if saved != 0 {
            let _ = RestoreDC(dc, saved);
        }
    }
    result
}
fn hover_region(hwnd: HWND, lp: LPARAM) -> usize {
    let mut rect = RECT::default();
    let mut info = SCROLLBARINFO {
        cbSize: std::mem::size_of::<SCROLLBARINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetClientRect(hwnd, &mut rect) }.is_err()
        || unsafe { GetScrollBarInfo(hwnd, OBJID_CLIENT, &mut info) }.is_err()
    {
        return 0;
    }
    let x = lp.0 as i16 as i32;
    let y = (lp.0 >> 16) as i16 as i32;
    if x < 0 || x >= rect.right || y < 0 || y >= rect.bottom {
        0
    } else if y < info.dxyLineButton {
        1
    } else if y < info.xyThumbTop {
        2
    } else if y < info.xyThumbBottom {
        3
    } else if y < rect.bottom - info.dxyLineButton {
        4
    } else {
        5
    }
}
unsafe extern "system" fn rail_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    let state = unsafe { callback_owner(data) };
    if message == WM_NCDESTROY {
        let _ = pointer_end(&state, false);
        state.hwnd.set(HWND::default());
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(rail_proc), RAIL_SUBCLASS);
        }
        let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
        // NCDESTROY is terminal. The temporary owner survives nested callbacks.
        unsafe {
            drop(Rc::from_raw(data as *const Rail));
        }
        return result;
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if message == SYNC {
            state.pending.set(false);
            if let Err(error) = sync(&state) {
                state.error.set(Some(error));
            }
            return LRESULT(0);
        }
        if message == WM_PAINT {
            let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
            if !state.hwnd.get().0.is_null()
                && let Err(error) = paint(hwnd, &state)
            {
                state.error.set(Some(error));
            }
            return result;
        }
        if message == WM_PRINTCLIENT && wp.0 != 0 {
            let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
            if !state.hwnd.get().0.is_null()
                && let Err(error) = draw(hwnd, HDC(wp.0 as *mut _), &state)
            {
                state.error.set(Some(error));
            }
            return result;
        }
        if message == WM_ERASEBKGND {
            return LRESULT(1);
        }
        if message == WM_MOUSEMOVE {
            state.hover.set(hover_region(hwnd, lp));
            let mut tracking = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: hwnd,
                dwHoverTime: 0,
            };
            unsafe {
                let _ = TrackMouseEvent(&mut tracking);
            }
        }
        if message == WM_MOUSELEAVE {
            state.hover.set(0);
        }
        if message == WM_KILLFOCUS || (message == WM_ENABLE && wp.0 == 0) {
            let _ = pointer_end(&state, false);
        }
        // Never enter the native scrollbar's modal pointer-tracking loop. It
        // paints outside WM_PAINT and pumps deferred layout while captured.
        let pointer = match message {
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => Some(pointer_down(hwnd, &state, lp)),
            WM_MOUSEMOVE if state.pressed.get() != 0 => Some(pointer_move(hwnd, &state, lp)),
            WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE => Some(pointer_end(&state, false)),
            WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize && state.pressed.get() != 0 => {
                Some(pointer_end(&state, true))
            }
            WM_TIMER if wp.0 == HOLD_TIMER => Some(repeat_pointer(hwnd, &state)),
            _ => None,
        };
        if let Some(result) = pointer {
            if let Err(error) = result {
                state.error.set(Some(error));
                let _ = pointer_end(&state, false);
            }
            if !state.hwnd.get().0.is_null() {
                let _ = paint(hwnd, &state);
            }
            return LRESULT(0);
        }
        if message == WM_MOUSEWHEEL {
            let target = state.target.get();
            if !target.0.is_null() {
                unsafe {
                    SendMessageW(target, message, Some(wp), Some(lp));
                }
            }
            queue(&state);
            return LRESULT(0);
        }
        let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
        if !state.hwnd.get().0.is_null()
            && matches!(
                message,
                WM_MOUSEMOVE
                    | WM_MOUSELEAVE
                    | WM_LBUTTONDOWN
                    | WM_LBUTTONUP
                    | WM_CAPTURECHANGED
                    | WM_SETFOCUS
                    | WM_KILLFOCUS
                    | WM_KEYDOWN
                    | WM_KEYUP
                    | WM_TIMER
                    | WM_ENABLE
            )
        {
            // Native tracking also draws directly, outside WM_PAINT. Overlay
            // after it returns so the final pixels retain the active theme.
            if let Err(error) = paint(hwnd, &state) {
                state.error.set(Some(error));
            }
        }
        result
    }));
    result.unwrap_or_else(|_| {
        state.error.set(Some(ErrorCode::InternalError));
        let _ = pointer_end(&state, false);
        LRESULT(0)
    })
}
unsafe extern "system" fn target_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    let state = unsafe { callback_owner(data) };
    if message == WM_NCDESTROY {
        state.target.set(HWND::default());
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(target_proc), TARGET_SUBCLASS);
        }
        let rail = state.hwnd.get();
        if !rail.0.is_null() {
            unsafe {
                let _ = DestroyWindow(rail);
            }
        }
        let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
        unsafe {
            drop(Rc::from_raw(data as *const Rail));
        }
        return result;
    }
    let result = unsafe { DefSubclassProc(hwnd, message, wp, lp) };
    if (message == WM_MOUSEMOVE && unsafe { GetCapture() } == hwnd)
        || matches!(
            message,
            WM_MOUSEWHEEL
                | WM_VSCROLL
                | WM_KEYDOWN
                | WM_KEYUP
                | WM_SIZE
                | WM_SETFONT
                | WM_SETTEXT
                | WM_ENABLE
                | WM_LBUTTONDOWN
                | LB_ADDSTRING
                | LB_INSERTSTRING
                | LB_DELETESTRING
                | LB_RESETCONTENT
                | LB_SETITEMHEIGHT
                | LB_SETTOPINDEX
                | LB_SETCURSEL
                | EM_LINESCROLL
        )
    {
        queue(&state);
    }
    result
}

#[cfg(test)]
mod tests;
