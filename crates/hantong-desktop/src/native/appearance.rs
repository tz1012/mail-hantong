use super::*;
use windows::{
    UI::ViewManagement::{UIColorType, UISettings},
    Win32::{
        Foundation::COLORREF,
        Globalization::GetUserDefaultUILanguage,
        Graphics::Gdi::*,
        System::WinRT::{RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize},
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            HiDpi::*,
            WindowsAndMessaging::{
                SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
            },
        },
    },
    core::w,
};

pub(super) struct Sta;
impl Sta {
    pub(super) fn initialize() -> Result<Self, ErrorCode> {
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
pub(super) struct DpiContext(DPI_AWARENESS_CONTEXT);
impl DpiContext {
    pub(super) fn enter() -> Self {
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
pub(super) fn scale(value: i32, dpi: u32) -> i32 {
    value.saturating_mul(dpi.clamp(96, 768) as i32) / 96
}
pub(super) fn os_korean() -> bool {
    unsafe { GetUserDefaultUILanguage() & 0x03ff == 0x12 }
}
pub(super) fn os_colors() -> (bool, bool) {
    let dark = UISettings::new()
        .and_then(|s| s.GetColorValue(UIColorType::Background))
        .map(|c| u32::from(c.R) * 299 + u32::from(c.G) * 587 + u32::from(c.B) * 114 < 128000)
        .unwrap_or(false);
    let mut contrast = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let high = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            contrast.cbSize,
            Some((&mut contrast as *mut HIGHCONTRASTW).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
    .is_ok()
        && contrast.dwFlags.contains(HCF_HIGHCONTRASTON);
    (dark, high)
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Colors {
    pub background: COLORREF,
    pub surface: COLORREF,
    pub raised: COLORREF,
    pub text: COLORREF,
    pub muted: COLORREF,
    pub border: COLORREF,
    pub accent: COLORREF,
    pub selected: COLORREF,
    pub focus: COLORREF,
    pub on_accent: COLORREF,
}
fn color(v: crate::Rgb) -> COLORREF {
    COLORREF(u32::from(v.r) | (u32::from(v.g) << 8) | (u32::from(v.b) << 16))
}
impl Colors {
    pub(super) fn new(theme: EffectiveTheme) -> Self {
        if let Some(p) = palette(theme) {
            Self {
                background: color(p.background),
                surface: color(p.surface),
                raised: color(p.raised),
                text: color(p.text),
                muted: color(p.muted),
                border: color(p.border),
                accent: color(p.accent),
                selected: color(p.selected),
                focus: color(p.focus),
                on_accent: color(p.on_accent),
            }
        } else {
            unsafe {
                Self {
                    background: COLORREF(GetSysColor(COLOR_WINDOW)),
                    surface: COLORREF(GetSysColor(COLOR_WINDOW)),
                    raised: COLORREF(GetSysColor(COLOR_3DFACE)),
                    text: COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                    muted: COLORREF(GetSysColor(COLOR_GRAYTEXT)),
                    border: COLORREF(GetSysColor(COLOR_WINDOWTEXT)),
                    accent: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                    selected: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                    focus: COLORREF(GetSysColor(COLOR_HIGHLIGHT)),
                    on_accent: COLORREF(GetSysColor(COLOR_HIGHLIGHTTEXT)),
                }
            }
        }
    }
}
pub(super) struct Brush(pub HBRUSH);
impl Brush {
    pub(super) fn new(color: COLORREF) -> Result<Self, ErrorCode> {
        let b = unsafe { CreateSolidBrush(color) };
        if b.0.is_null() {
            Err(ErrorCode::InternalError)
        } else {
            Ok(Self(b))
        }
    }
}
impl Drop for Brush {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}
pub(super) struct Font(pub HFONT);
impl Font {
    fn new(
        dpi: u32,
        size: i32,
        bold: bool,
        face: windows::core::PCWSTR,
    ) -> Result<Self, ErrorCode> {
        let font = unsafe {
            CreateFontW(
                -scale(size, dpi),
                0,
                0,
                0,
                if bold {
                    FW_SEMIBOLD.0 as i32
                } else {
                    FW_NORMAL.0 as i32
                },
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                DEFAULT_PITCH.0 as u32,
                face,
            )
        };
        if font.0.is_null() {
            Err(ErrorCode::InternalError)
        } else {
            Ok(Self(font))
        }
    }
}
impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}
#[allow(dead_code)]
pub(super) struct Fonts {
    pub body: Font,
    pub heading: Font,
    pub brand: Font,
    pub caption: Font,
    pub icon: Font,
}
impl Fonts {
    pub(super) fn new(dpi: u32) -> Result<Self, ErrorCode> {
        Ok(Self {
            body: Font::new(dpi, 14, false, w!("Malgun Gothic"))?,
            heading: Font::new(dpi, 16, true, w!("Segoe UI"))?,
            brand: Font::new(dpi, 18, true, w!("Segoe UI"))?,
            caption: Font::new(dpi, 12, false, w!("Malgun Gothic"))?,
            icon: Font::new(dpi, 16, false, w!("Segoe MDL2 Assets"))?,
        })
    }
}
