use super::*;
use std::sync::Mutex;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, VK_END, VK_ESCAPE};

// Pointer capture is desktop state. Do not let the two visible pointer fixtures
// steal capture from one another when the Rust test runner uses multiple threads.
static VISIBLE_FIXTURE: Mutex<()> = Mutex::new(());

struct Fixture {
    parent: HWND,
    target: HWND,
    rail: HWND,
    _dpi: DpiContext,
}
impl Fixture {
    fn new(kind: Kind) -> Self {
        let dpi = DpiContext::enter();
        let parent = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("scroll fixture"),
                WS_OVERLAPPED,
                90,
                110,
                500,
                400,
                None,
                None,
                None,
                None,
            )
        }
        .unwrap();
        let (class, specific) = match kind {
            Kind::List => (w!("LISTBOX"), LBS_NOINTEGRALHEIGHT),
            Kind::Edit => (w!("EDIT"), ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL),
        };
        let target = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                w!(""),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(specific as u32),
                20,
                30,
                240,
                100,
                Some(parent),
                None,
                None,
                None,
            )
        }
        .unwrap();
        unsafe {
            SendMessageW(
                target,
                WM_SETFONT,
                Some(WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize)),
                Some(LPARAM(0)),
            );
        }
        let rail = attach(parent, target, 1001, kind).unwrap();
        Self {
            parent,
            target,
            rail,
            _dpi: dpi,
        }
    }
    fn populate_list(&self, count: usize) {
        for index in 0..count {
            let text = HSTRING::from(format!("Synthetic row {index}"));
            assert_eq!(
                unsafe {
                    SendMessageW(
                        self.target,
                        LB_ADDSTRING,
                        None,
                        Some(LPARAM(text.as_ptr() as isize)),
                    )
                }
                .0,
                index as isize
            );
        }
    }
    fn text(&self, text: &str) {
        unsafe {
            SetWindowTextW(self.target, &HSTRING::from(text)).unwrap();
        }
    }
    fn pump(&self) {
        let mut message = MSG::default();
        for _ in 0..512 {
            if !unsafe { PeekMessageW(&mut message, Some(self.parent), 0, 0, PM_REMOVE) }.as_bool()
            {
                if unsafe { IsWindow(Some(self.rail)) }.as_bool() {
                    assert!(owner(self.rail).unwrap().error.get().is_none());
                }
                return;
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        panic!("scroll synchronization did not quiesce");
    }
    fn update(&self, dpi: u32) {
        update(
            self.rail,
            Theme {
                track: COLORREF(0x202020),
                thumb: COLORREF(0x808080),
                hot: COLORREF(0xc0c0c0),
                focus: COLORREF(0xffffff),
                high_contrast: false,
            },
            dpi,
        )
        .unwrap();
        self.pump();
    }
    fn show_for_native_geometry(&self) {
        // Hidden controls may return a successful GetScrollBarInfo call with
        // zero geometry. Materialize the native control without activating it
        // or putting a test window over the user's desktop.
        unsafe {
            SetWindowPos(
                self.parent,
                None,
                -20000,
                -20000,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .unwrap();
            let _ = ShowWindow(self.parent, SW_SHOWNOACTIVATE);
            let _ = UpdateWindow(self.parent);
            let _ = UpdateWindow(self.target);
            let _ = UpdateWindow(self.rail);
        }
        self.pump();
        // An entirely off-screen window has no drawable screen clip. Native
        // WM_PAINT can therefore return before computing scrollbar geometry.
        // WM_PRINTCLIENT supplies a real memory surface and runs the same
        // default scrollbar painting before our theme overlay.
        let mut client = RECT::default();
        unsafe {
            GetClientRect(self.rail, &mut client).unwrap();
        }
        let surface = MemorySurface::new(client.right, client.bottom);
        unsafe {
            SendMessageW(
                self.rail,
                WM_PRINTCLIENT,
                Some(WPARAM(surface.dc.0 as usize)),
                Some(LPARAM(PRF_CLIENT as isize)),
            );
        }
        self.native_geometry();
    }
    fn native_geometry(&self) -> SCROLLBARINFO {
        let mut info = SCROLLBARINFO {
            cbSize: std::mem::size_of::<SCROLLBARINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            GetScrollBarInfo(self.rail, OBJID_CLIENT, &mut info).unwrap();
        }
        assert!(
            info.dxyLineButton > 0,
            "native line buttons must be laid out"
        );
        assert!(info.xyThumbTop >= info.dxyLineButton);
        assert!(info.xyThumbBottom > info.xyThumbTop);
        info
    }
    fn metrics(&self) -> Metrics {
        metrics(&owner(self.rail).unwrap()).unwrap()
    }
    fn info(&self) -> SCROLLINFO {
        let mut info = SCROLLINFO {
            cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
            fMask: SIF_RANGE | SIF_PAGE | SIF_POS,
            ..Default::default()
        };
        unsafe {
            GetScrollInfo(self.rail, SB_CTL, &mut info).unwrap();
        }
        info
    }
    fn assert_synced(&self) {
        let data = self.metrics();
        let info = self.info();
        assert_eq!(info.nMin, 0);
        assert_eq!(info.nMax, (data.count - 1).max(0));
        assert_eq!(info.nPos, data.pos);
        if data.overflow() {
            assert_eq!(info.nPage, data.page as u32);
        }
        assert_eq!(has_style(self.rail, WS_VISIBLE), data.overflow());
        assert!(
            !has_style(self.target, WS_VSCROLL),
            "original native chrome remains hidden"
        );
    }
    fn scroll(&self, code: SCROLLBAR_COMMAND) {
        scroll(self.rail, code.0 as u16).unwrap();
        self.pump();
        self.assert_synced();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.parent);
        }
    }
}
fn has_style(hwnd: HWND, style: WINDOW_STYLE) -> bool {
    (unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) }) as u32 & style.0 != 0
}

struct MemorySurface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}
impl MemorySurface {
    fn new(width: i32, height: i32) -> Self {
        unsafe {
            let screen = GetDC(None);
            assert!(!screen.0.is_null());
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, width, height);
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
impl Drop for MemorySurface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}

#[test]
fn ops08_scroll_printclient_overlays_native_chrome_with_theme_at_actual_thumb_geometry() {
    let _visible = VISIBLE_FIXTURE.lock().unwrap();
    let f = Fixture::new(Kind::List);
    f.populate_list(32);
    f.update(96);
    f.show_for_native_geometry();
    f.assert_synced();
    let theme = owner(f.rail).unwrap().theme.get();
    let mut client = RECT::default();
    unsafe {
        GetClientRect(f.rail, &mut client).unwrap();
    }
    let surface = MemorySurface::new(client.right, client.bottom);
    let previous_pen = unsafe { GetCurrentObject(surface.dc, OBJ_PEN) };
    let mut previous_thumb_top = None;
    for position in [SB_TOP, SB_BOTTOM] {
        f.scroll(position);
        unsafe {
            SendMessageW(
                f.rail,
                WM_PRINTCLIENT,
                Some(WPARAM(surface.dc.0 as usize)),
                Some(LPARAM(PRF_CLIENT as isize)),
            );
        }
        let mut native = SCROLLBARINFO {
            cbSize: std::mem::size_of::<SCROLLBARINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            GetScrollBarInfo(f.rail, OBJID_CLIENT, &mut native).unwrap();
        }
        assert!(native.dxyLineButton > 0);
        assert!(native.xyThumbTop >= native.dxyLineButton);
        assert!(native.xyThumbBottom > native.xyThumbTop);
        assert!(native.xyThumbBottom <= client.bottom - native.dxyLineButton);
        if let Some(top) = previous_thumb_top {
            assert!(
                native.xyThumbTop > top,
                "native painted thumb must move between the scroll limits"
            );
        }
        previous_thumb_top = Some(native.xyThumbTop);
        let thumb_y = (native.xyThumbTop + native.xyThumbBottom) / 2;
        assert!(owner(f.rail).unwrap().error.get().is_none());
        assert_eq!(
            unsafe { GetPixel(surface.dc, 0, thumb_y) },
            theme.track,
            "default native rendering must not leave a white track"
        );
        assert_eq!(
            unsafe { GetPixel(surface.dc, client.right / 2, thumb_y) },
            theme.thumb,
            "the themed thumb follows native geometry at both scroll limits"
        );
        assert_eq!(
            unsafe { GetCurrentObject(surface.dc, OBJ_PEN) },
            previous_pen
        );
    }
}

#[test]
fn ops07_ops08_list_rail_matches_range_scroll_and_target_geometry_at_all_scales() {
    let f = Fixture::new(Kind::List);
    f.populate_list(32);
    for dpi in [96, 144, 192] {
        f.update(dpi);
        f.assert_synced();
        assert_eq!(f.metrics().count, 32);
        assert!(f.metrics().page > 0 && f.metrics().page < 32);
        let mut target = RECT::default();
        let mut rail = RECT::default();
        unsafe {
            GetWindowRect(f.target, &mut target).unwrap();
            GetWindowRect(f.rail, &mut rail).unwrap();
        }
        assert_eq!(rail.left, target.right + scale(2, dpi));
        assert_eq!(rail.right - rail.left, scale(10, dpi));
        assert_eq!(rail.top, target.top);
        assert_eq!(rail.bottom, target.bottom);
    }
    f.scroll(SB_TOP);
    f.scroll(SB_LINEDOWN);
    assert_eq!(f.metrics().pos, 1);
    let page = f.metrics().page;
    f.scroll(SB_PAGEDOWN);
    assert_eq!(f.metrics().pos, 1 + page);
    f.scroll(SB_PAGEUP);
    assert_eq!(f.metrics().pos, 1);
    f.scroll(SB_LINEUP);
    assert_eq!(f.metrics().pos, 0);
    f.scroll(SB_BOTTOM);
    assert_eq!(f.metrics().pos, f.metrics().max_pos());
    assert_eq!(
        unsafe { SendMessageW(f.target, LB_GETTOPINDEX, None, None) }.0,
        f.metrics().pos as isize
    );
}

#[test]
fn ops07_list_target_wheel_keyboard_and_topindex_changes_synchronize_without_manual_update() {
    let f = Fixture::new(Kind::List);
    f.populate_list(40);
    f.update(96);
    unsafe {
        SendMessageW(f.target, LB_SETTOPINDEX, Some(WPARAM(10)), None);
    }
    assert!(owner(f.rail).unwrap().pending.get());
    f.pump();
    f.assert_synced();
    assert_eq!(f.info().nPos, 10);
    // System wheel preferences are retained. Even a zero-line preference must
    // queue synchronization and publish the actual native target position.
    let wheel = WPARAM((u32::from((-120i16) as u16) << 16) as usize);
    unsafe {
        SendMessageW(f.target, WM_MOUSEWHEEL, Some(wheel), Some(LPARAM(0)));
    }
    assert!(owner(f.rail).unwrap().pending.get());
    f.pump();
    f.assert_synced();
    assert_eq!(
        f.info().nPos,
        unsafe { SendMessageW(f.target, LB_GETTOPINDEX, None, None) }.0 as i32
    );
    unsafe {
        SendMessageW(f.target, LB_SETCURSEL, Some(WPARAM(0)), None);
        SendMessageW(
            f.target,
            WM_KEYDOWN,
            Some(WPARAM(usize::from(VK_END.0))),
            None,
        );
    }
    assert!(owner(f.rail).unwrap().pending.get());
    f.pump();
    f.assert_synced();
    assert_eq!(
        unsafe { SendMessageW(f.target, LB_GETCURSEL, None, None) }.0,
        39
    );
    assert!(
        f.metrics().pos > 0,
        "keyboard selection exposes the last real row"
    );
    unsafe {
        SendMessageW(f.target, LB_RESETCONTENT, None, None);
    }
    f.pump();
    f.assert_synced();
    assert_eq!(f.metrics().count, 0);
    assert!(!has_style(f.rail, WS_VISIBLE));
}

#[test]
fn ops07_edit_rail_tracks_wrapped_readonly_text_and_hides_after_shortening() {
    let f = Fixture::new(Kind::Edit);
    let text = (0..80)
        .map(|n| {
            format!("Synthetic detail line {n} with enough text to wrap in the narrow control.\r\n")
        })
        .collect::<String>();
    f.text(&text);
    f.update(96);
    f.assert_synced();
    assert!(f.metrics().count > 80 && f.metrics().overflow());
    f.scroll(SB_LINEDOWN);
    assert_eq!(f.metrics().pos, 1);
    let page = f.metrics().page;
    f.scroll(SB_PAGEDOWN);
    assert_eq!(f.metrics().pos, 1 + page);
    f.scroll(SB_BOTTOM);
    assert_eq!(f.metrics().pos, f.metrics().max_pos());
    assert_eq!(
        unsafe { SendMessageW(f.target, EM_GETFIRSTVISIBLELINE, None, None) }.0,
        f.info().nPos as isize
    );
    f.text("Short guidance");
    assert!(owner(f.rail).unwrap().pending.get());
    f.pump();
    f.assert_synced();
    assert_eq!(f.metrics().count, 1);
    assert!(!has_style(f.rail, WS_VISIBLE));
    assert_ne!(
        unsafe { GetWindowLongPtrW(f.target, GWL_STYLE) } as u32 & ES_READONLY as u32,
        0
    );
    f.text(&text);
    f.pump();
    f.assert_synced();
    unsafe {
        let _ = EnableWindow(f.target, false);
    }
    f.pump();
    assert!(!unsafe { IsWindowEnabled(f.rail) }.as_bool());
    unsafe {
        let _ = EnableWindow(f.target, true);
    }
    f.pump();
    assert!(unsafe { IsWindowEnabled(f.rail) }.as_bool());
}

#[test]
fn ops02_scroll_owners_release_in_both_destroy_orders_and_invalid_attach_is_rejected() {
    for target_first in [true, false] {
        let f = Fixture::new(Kind::List);
        f.populate_list(12);
        f.update(96);
        let state = owner(f.rail).unwrap();
        let weak = Rc::downgrade(&state);
        drop(state);
        assert_eq!(
            weak.strong_count(),
            2,
            "rail and target each retain one reference"
        );
        assert_eq!(
            attach(f.parent, HWND::default(), 1002, Kind::List),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            attach(HWND::default(), f.target, 1002, Kind::List),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(weak.strong_count(), 2);
        if target_first {
            unsafe {
                DestroyWindow(f.target).unwrap();
            }
            assert!(!unsafe { IsWindow(Some(f.rail)) }.as_bool());
            assert!(weak.upgrade().is_none());
        } else {
            unsafe {
                DestroyWindow(f.rail).unwrap();
            }
            assert_eq!(
                weak.strong_count(),
                1,
                "live target keeps its callback data alive"
            );
            unsafe {
                SendMessageW(f.target, LB_SETTOPINDEX, Some(WPARAM(1)), None);
            }
            assert!(unsafe { IsWindow(Some(f.target)) }.as_bool());
            unsafe {
                DestroyWindow(f.target).unwrap();
            }
            assert!(weak.upgrade().is_none());
        }
        drop(f);
        assert!(weak.upgrade().is_none());
    }
}

#[test]
fn ops07_scroll_destinations_preserve_32_bit_positions_and_clamp_both_edges() {
    let data = Metrics {
        count: 150_000,
        page: 40,
        pos: 70_000,
    };
    assert_eq!(destination(data, SB_THUMBTRACK.0 as u16, 120_123), 120_123);
    assert_eq!(
        destination(data, SB_THUMBPOSITION.0 as u16, 140_456),
        140_456
    );
    assert_eq!(destination(data, SB_BOTTOM.0 as u16, 0), 149_960);
    assert_eq!(destination(data, SB_THUMBTRACK.0 as u16, i32::MAX), 149_960);
    assert_eq!(destination(data, SB_THUMBTRACK.0 as u16, -1), 0);
    assert_eq!(
        destination(
            Metrics {
                count: i32::MAX,
                page: 1,
                pos: i32::MAX - 1
            },
            SB_LINEDOWN.0 as u16,
            0
        ),
        i32::MAX - 1
    );
    assert_eq!(
        destination(
            Metrics {
                count: 0,
                page: 1,
                pos: 0
            },
            SB_PAGEUP.0 as u16,
            0
        ),
        0
    );
    assert_eq!(destination(data, u16::MAX, 0), 70_000);
}

#[test]
fn ops08_pointer_drag_is_non_modal_and_releases_capture_after_thumb_move() {
    let _visible = VISIBLE_FIXTURE.lock().unwrap();
    let f = Fixture::new(Kind::List);
    f.populate_list(120);
    f.update(96);
    f.show_for_native_geometry();
    let origin = f.metrics().pos;
    let native = f.native_geometry();
    let x = 5i32;
    let thumb_y = (native.xyThumbTop + native.xyThumbBottom) / 2;
    let bottom_y = native.xyThumbBottom + native.dxyLineButton;
    let point = |x: i32, y: i32| LPARAM(((y as u16 as usize) << 16 | x as u16 as usize) as isize);
    // SendMessage must return immediately: the themed rail owns pointer capture
    // rather than entering the native scrollbar's modal tracking loop.
    unsafe {
        SendMessageW(
            f.rail,
            WM_LBUTTONDOWN,
            Some(WPARAM(0)),
            Some(point(x, thumb_y)),
        );
        assert_eq!(GetCapture(), f.rail);
        assert_eq!(owner(f.rail).unwrap().pressed.get(), 3);
        assert!(owner(f.rail).unwrap().dragging.get());
        SendMessageW(
            f.rail,
            WM_MOUSEMOVE,
            Some(WPARAM(0)),
            Some(point(x, bottom_y)),
        );
        SendMessageW(
            f.rail,
            WM_LBUTTONUP,
            Some(WPARAM(0)),
            Some(point(x, bottom_y)),
        );
    }
    f.pump();
    assert_eq!(unsafe { GetCapture() }, HWND::default());
    assert!(f.metrics().pos > origin);
    assert_eq!(owner(f.rail).unwrap().pressed.get(), 0);
    assert!(!owner(f.rail).unwrap().dragging.get());
    f.assert_synced();
}

#[test]
fn ops08_pointer_escape_cancels_drag_and_capture_loss_cleans_state() {
    let _visible = VISIBLE_FIXTURE.lock().unwrap();
    let f = Fixture::new(Kind::List);
    f.populate_list(96);
    f.update(96);
    f.show_for_native_geometry();
    let native = f.native_geometry();
    let point = |x: i32, y: i32| LPARAM(((y as u16 as usize) << 16 | x as u16 as usize) as isize);
    let thumb_y = (native.xyThumbTop + native.xyThumbBottom) / 2;
    let origin = f.metrics().pos;
    unsafe {
        SendMessageW(
            f.rail,
            WM_LBUTTONDOWN,
            Some(WPARAM(0)),
            Some(point(5, thumb_y)),
        );
        assert_eq!(GetCapture(), f.rail);
        assert_eq!(owner(f.rail).unwrap().pressed.get(), 3);
        assert!(owner(f.rail).unwrap().dragging.get());
        SendMessageW(
            f.rail,
            WM_MOUSEMOVE,
            Some(WPARAM(0)),
            Some(point(5, native.xyThumbBottom + native.dxyLineButton)),
        );
        assert!(
            f.metrics().pos > origin,
            "Escape must cancel a real thumb movement"
        );
        SendMessageW(f.rail, WM_KEYDOWN, Some(WPARAM(VK_ESCAPE.0 as usize)), None);
    }
    f.pump();
    assert_eq!(unsafe { GetCapture() }, HWND::default());
    assert_eq!(f.metrics().pos, origin);
    assert_eq!(owner(f.rail).unwrap().pressed.get(), 0);
    assert!(!owner(f.rail).unwrap().dragging.get());

    // A capture transfer follows the same cleanup path as WM_CAPTURECHANGED.
    unsafe {
        SendMessageW(
            f.rail,
            WM_LBUTTONDOWN,
            Some(WPARAM(0)),
            Some(point(5, thumb_y)),
        );
        assert_eq!(GetCapture(), f.rail);
        let _ = SetCapture(f.target);
    }
    f.pump();
    assert_eq!(owner(f.rail).unwrap().pressed.get(), 0);
    assert!(!owner(f.rail).unwrap().dragging.get());
    unsafe {
        let _ = ReleaseCapture();
    }
    f.assert_synced();
}
