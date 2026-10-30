use super::*;
#[derive(Clone, Copy)]
pub(super) struct Placement {
    pub id: i32,
    pub rect: RECT,
    pub visible: bool,
}
pub(super) struct Layout {
    controls: Vec<Placement>,
    panels: Vec<RECT>,
}
impl Layout {
    pub(super) fn new(width: i32, height: i32, dpi: u32) -> Self {
        let p = |n| scale(n, dpi);
        let compact = width < p(960);
        let side = p(if compact { 168 } else { 196 });
        let left = side + p(12);
        let right = width - p(12);
        let list_right = side + p(if compact { 248 } else { 320 });
        let top = p(if compact { 154 } else { 118 });
        let bottom = height - p(48);
        let mut controls = Vec::new();
        let mut add = |id, x, y, w, h, visible| {
            controls.push(Placement {
                id,
                rect: RECT {
                    left: x,
                    top: y,
                    right: x + w,
                    bottom: y + h,
                },
                visible,
            })
        };
        add(BRAND, p(40), p(10), p(150), p(22), true);
        add(SUBTITLE, p(12), p(12), p(120), p(18), false);
        add(
            STATUS,
            p(202),
            p(13),
            (width - p(580)).max(p(150)),
            p(18),
            true,
        );
        add(LANGUAGE_LABEL, width - p(362), p(14), p(64), p(16), true);
        add(LANGUAGE, width - p(294), p(8), p(110), p(28), true);
        add(THEME_LABEL, width - p(172), p(14), p(48), p(16), true);
        add(THEME, width - p(120), p(8), p(108), p(28), true);
        add(NAV_LABEL, p(16), p(54), side - p(32), p(18), true);
        add(NAVIGATION, p(8), p(78), side - p(24), height - p(262), true);
        add(COMPACT, p(16), height - p(176), side - p(32), p(48), true);
        add(CONNECT, p(12), height - p(122), side - p(24), p(30), true);
        add(
            REGISTRATION_STATUS,
            p(12),
            height - p(84),
            side - p(24),
            p(30),
            true,
        );
        add(ACCOUNT_LABEL, left, p(58), p(48), p(16), true);
        add(ACCOUNT, left + p(56), p(52), p(176), p(28), true);
        add(FOLDER_LABEL, left + p(244), p(58), p(44), p(16), true);
        let folder_end = right - p(if compact { 36 } else { 274 });
        add(
            FOLDER,
            left + p(296),
            p(52),
            folder_end - left - p(296),
            p(28),
            true,
        );
        add(FOLDERS_NEXT, folder_end + p(8), p(52), p(28), p(28), true);
        let filter_left = if compact { left } else { right - p(230) };
        let filter_y = p(if compact { 88 } else { 52 });
        add(
            FILTER_LABEL,
            filter_left,
            filter_y + p(6),
            p(40),
            p(16),
            true,
        );
        add(FILTER, filter_left + p(48), filter_y, p(112), p(28), true);
        add(
            RELOAD_FOLDER,
            if compact {
                left + p(168)
            } else {
                right - p(64)
            },
            filter_y,
            p(28),
            p(28),
            true,
        );
        add(
            REFRESH,
            if compact {
                left + p(204)
            } else {
                right - p(28)
            },
            filter_y,
            p(28),
            p(28),
            true,
        );
        add(
            SETUP_BODY,
            left,
            p(if compact { 124 } else { 88 }),
            right - left,
            p(20),
            true,
        );
        add(SECTION, left, top, p(100), p(18), false);
        add(SETUP_TITLE, left, top, p(100), p(18), false);
        add(
            LIST_TITLE,
            side + p(16),
            top + p(12),
            list_right - side - p(32),
            p(22),
            true,
        );
        add(
            MESSAGE_LIST,
            side + p(2),
            top + p(42),
            list_right - side - p(16),
            bottom - top - p(84),
            true,
        );
        add(
            NEXT_PAGE,
            list_right - p(120),
            bottom - p(34),
            p(106),
            p(26),
            true,
        );
        add(
            DETAIL_TITLE,
            list_right + p(20),
            top + p(12),
            width - list_right - p(40),
            p(22),
            true,
        );
        add(
            DETAIL_BODY,
            list_right + p(20),
            top + p(46),
            width - list_right - p(52),
            bottom - top - p(94),
            true,
        );
        add(
            ACCESS_NOTICE,
            list_right + p(20),
            bottom - p(36),
            width - list_right - p(40),
            p(30),
            true,
        );
        add(
            MAIL_STATUS,
            p(12),
            height - p(40),
            width - p(24),
            p(16),
            true,
        );
        add(
            PREFERENCES,
            p(12),
            height - p(20),
            width - p(24),
            p(16),
            true,
        );
        Self {
            controls,
            panels: vec![
                RECT {
                    left: 0,
                    top: p(42),
                    right: side,
                    bottom,
                },
                RECT {
                    left: side,
                    top,
                    right: list_right,
                    bottom,
                },
                RECT {
                    left: list_right,
                    top,
                    right: width,
                    bottom,
                },
            ],
        }
    }
    pub(super) fn control(&self, id: i32) -> Option<&Placement> {
        self.controls.iter().find(|p| p.id == id)
    }
    pub(super) fn panels(&self) -> impl Iterator<Item = RECT> + '_ {
        self.panels.iter().copied()
    }
}
