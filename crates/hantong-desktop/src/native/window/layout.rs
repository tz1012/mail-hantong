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
        let compact = width < p(1040);
        let margin = p(24);
        let gap = p(16);
        let top = p(100);
        let bottom = height - p(156);
        let nav_right = margin + p(208);
        let main_left = nav_right + gap;
        let detail_left = width - p(344);
        let main_right = if compact {
            width - margin
        } else {
            detail_left - gap
        };
        let left = main_left + p(16);
        let content = main_right - left - p(16);
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
        add(BRAND, margin, p(14), p(300), p(38), true);
        add(SUBTITLE, margin, p(58), p(300), p(26), true);
        add(LANGUAGE_LABEL, width - p(404), p(16), p(144), p(24), true);
        add(LANGUAGE, width - p(404), p(44), p(144), p(32), true);
        add(THEME_LABEL, width - p(244), p(16), p(220), p(24), true);
        add(THEME, width - p(244), p(44), p(220), p(32), true);
        add(NAV_LABEL, margin + p(16), top + p(16), p(176), p(28), true);
        add(
            NAVIGATION,
            margin + p(12),
            top + p(54),
            p(184),
            bottom - top - p(70),
            true,
        );
        add(LIST_TITLE, left, top + p(12), content, p(28), true);
        add(SECTION, left, top + p(42), content, p(24), false);
        add(SETUP_TITLE, left, top + p(42), content, p(24), false);
        add(ACCOUNT_LABEL, left, top + p(48), p(90), p(24), true);
        add(
            ACCOUNT,
            left + p(96),
            top + p(44),
            content - p(96),
            p(30),
            true,
        );
        add(FOLDER_LABEL, left, top + p(86), p(90), p(24), true);
        add(
            FOLDER,
            left + p(96),
            top + p(82),
            content - p(96),
            p(30),
            true,
        );
        add(FOLDERS_NEXT, left, top + p(120), content, p(28), true);
        add(SETUP_BODY, left, top + p(154), content, p(40), true);
        let list_top = top + p(198);
        let list_bottom = bottom - p(if compact { 172 } else { 54 });
        add(
            MESSAGE_LIST,
            left,
            list_top,
            content,
            (list_bottom - list_top).max(p(56)),
            true,
        );
        add(NEXT_PAGE, left, list_bottom + p(6), content, p(30), true);
        add(COMPACT, left, bottom - p(36), content, p(28), false);
        if compact {
            add(
                DETAIL_TITLE,
                left,
                list_bottom + p(42),
                content,
                p(26),
                true,
            );
            add(
                DETAIL_BODY,
                left,
                list_bottom + p(72),
                content,
                bottom - list_bottom - p(82),
                true,
            );
        } else {
            add(
                DETAIL_TITLE,
                detail_left + p(20),
                top + p(16),
                p(280),
                p(30),
                true,
            );
            add(
                DETAIL_BODY,
                detail_left + p(20),
                top + p(60),
                p(280),
                bottom - top - p(80),
                true,
            );
        }
        add(
            ACCESS_NOTICE,
            margin,
            height - p(148),
            width - p(48),
            p(24),
            true,
        );
        add(CONNECT, margin, height - p(118), p(176), p(32), true);
        add(
            REGISTRATION_STATUS,
            margin + p(188),
            height - p(118),
            p(200),
            p(32),
            true,
        );
        add(
            REFRESH,
            width - p(200),
            height - p(118),
            p(176),
            p(32),
            true,
        );
        add(STATUS, margin, height - p(78), width - p(48), p(22), true);
        add(
            MAIL_STATUS,
            margin,
            height - p(56),
            width - p(48),
            p(22),
            true,
        );
        add(
            PREFERENCES,
            margin,
            height - p(30),
            width - p(48),
            p(24),
            true,
        );
        let mut panels = vec![
            RECT {
                left: margin,
                top,
                right: nav_right,
                bottom,
            },
            RECT {
                left: main_left,
                top,
                right: main_right,
                bottom,
            },
        ];
        if !compact {
            panels.push(RECT {
                left: detail_left,
                top,
                right: width - margin,
                bottom,
            });
        }
        Self { controls, panels }
    }
    pub(super) fn control(&self, id: i32) -> Option<&Placement> {
        self.controls.iter().find(|p| p.id == id)
    }
    pub(super) fn panels(&self) -> impl Iterator<Item = RECT> + '_ {
        self.panels.iter().copied()
    }
}
