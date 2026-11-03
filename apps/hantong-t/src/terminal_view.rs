//! A small, dependency-free renderer for the optional BBS-style terminal view.
//!
//! The renderer deliberately owns no terminal state.  The caller can redraw the
//! returned frame and route mouse events through [`hit_test`].  Keeping this
//! module pure makes the retro view usable from the keyboard-first TUI without
//! changing the mail or approval flows.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Theme {
    Blue,
    Green,
    Amber,
    Mono,
    Bbs8,
    Bbs16,
}

const TUI_EN: &str = include_str!("../resources/tui-en.txt");
const TUI_KO: &str = include_str!("../resources/tui-ko.txt");

fn resource(table: &'static str, key: &str) -> Option<&'static str> {
    table.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        (name == key).then_some(value)
    })
}

/// Stable UI copy for statuses shared by the command loop and renderer.
/// Unknown keys intentionally fall back to an English diagnostic.
pub(crate) fn text(lang_ko: bool, key: &str) -> &'static str {
    resource(if lang_ko { TUI_KO } else { TUI_EN }, key).unwrap_or("Unknown")
}

impl Theme {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "blue" | "default" => Some(Self::Blue),
            "green" | "mono-green" => Some(Self::Green),
            "amber" | "mono-amber" => Some(Self::Amber),
            "mono" | "monochrome" => Some(Self::Mono),
            "bbs8" | "classic8" => Some(Self::Bbs8),
            "bbs16" | "classic16" => Some(Self::Bbs16),
            _ => None,
        }
    }

    pub(crate) fn next(self) -> Self {
        match self {
            Self::Blue => Self::Green,
            Self::Green => Self::Amber,
            Self::Amber => Self::Mono,
            Self::Mono => Self::Bbs8,
            Self::Bbs8 => Self::Bbs16,
            Self::Bbs16 => Self::Blue,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Amber => "amber",
            Self::Mono => "mono",
            Self::Bbs8 => "bbs8",
            Self::Bbs16 => "bbs16",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Hit {
    Row(usize),
    Body,
    Account(usize),
    Folder(usize),
    Theme,
    Compose,
    Refresh,
    Back,
    Help,
    None,
}

pub(crate) struct Frame<'a> {
    pub lang_ko: bool,
    pub width: u16,
    pub height: u16,
    pub theme: Theme,
    pub screen: u8,
    pub selected: usize,
    pub account: &'a str,
    pub folder: &'a str,
    pub accounts: &'a [String],
    pub folders: &'a [String],
    pub rows: &'a [String],
    pub body: &'a str,
    pub body_offset: usize,
    pub status: &'a str,
    pub help: bool,
}

#[derive(Clone, Copy)]
struct Layout {
    left: usize,
    center: usize,
    list_start: usize,
    list_height: usize,
}

fn layout(width: u16, height: u16) -> Layout {
    let width = width as usize;
    let left = if width >= 80 { 24 } else { 0 };
    let right = if width >= 110 {
        (width - left) * 3 / 5
    } else {
        0
    };
    let center = width.saturating_sub(left + right);
    let compact = (80..110).contains(&width);
    let reserved = if compact { 9 } else { 6 };
    Layout {
        left,
        center,
        list_start: 4,
        list_height: (height as usize).saturating_sub(reserved),
    }
}

fn color(theme: Theme) -> (&'static str, &'static str, &'static str) {
    match theme {
        Theme::Blue => ("\x1b[38;5;153m", "\x1b[48;5;18m", "\x1b[38;5;75m"),
        Theme::Green => ("\x1b[38;5;121m", "\x1b[48;5;0m", "\x1b[38;5;84m"),
        Theme::Amber => ("\x1b[38;5;223m", "\x1b[48;5;0m", "\x1b[38;5;215m"),
        Theme::Mono => ("\x1b[38;5;252m", "\x1b[48;5;235m", "\x1b[38;5;250m"),
        Theme::Bbs8 => ("\x1b[37m", "\x1b[44m", "\x1b[36m"),
        Theme::Bbs16 => ("\x1b[97m", "\x1b[44m", "\x1b[96m"),
    }
}

fn selected_color(theme: Theme) -> &'static str {
    match theme {
        Theme::Blue => "\x1b[38;5;231m",
        Theme::Green => "\x1b[38;5;159m",
        Theme::Amber => "\x1b[38;5;255m",
        Theme::Mono => "\x1b[38;5;255m",
        Theme::Bbs8 => "\x1b[33m",
        Theme::Bbs16 => "\x1b[93m",
    }
}

fn visible_width(ch: char) -> usize {
    if ch == '\t' || ch.is_control() || is_combining(ch) {
        return 0;
    }
    if is_wide(ch) { 2 } else { 1 }
}

fn is_combining(ch: char) -> bool {
    matches!(ch as u32,
        0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff |
        0x20d0..=0x20ff | 0xfe20..=0xfe2f)
}

fn is_wide(ch: char) -> bool {
    matches!(ch as u32,
        0x1100..=0x115f | 0x2329..=0x232a | 0x2e80..=0xa4cf |
        0xac00..=0xd7a3 | 0xf900..=0xfaff | 0xfe10..=0xfe19 |
        0xfe30..=0xfe6f | 0xff00..=0xff60 | 0xffe0..=0xffe6 |
        0x1f300..=0x1faff)
}

fn safe_char(ch: char) -> char {
    if ch.is_control()
        || is_combining(ch)
        || matches!(ch as u32, 0x061c | 0x1160..=0x11ff | 0xfe00..=0xfe0f | 0xe0100..=0xe01ef)
        || matches!(ch, '\u{2028}' | '\u{2029}')
        || matches!(ch as u32, 0x200b..=0x200f | 0x202a..=0x202e | 0x2060..=0x206f | 0xfeff)
    {
        return '�';
    }
    ch
}

fn safe(value: &str) -> String {
    value.chars().map(safe_char).collect()
}

fn clip(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let ch = safe_char(ch);
        let w = visible_width(ch).max(1);
        if used + w > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    while used < width {
        out.push(' ');
        used += 1;
    }
    out
}

fn line(width: usize, value: &str) -> String {
    clip(value, width)
}

fn body_width(width: usize) -> usize {
    if width >= 110 { 36 } else { width.max(1) }
}

fn wrap_body(body: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut wrapped = Vec::new();
    for source in body.split('\n') {
        let mut current = String::new();
        let mut used = 0;
        for ch in source.chars().map(safe_char) {
            let w = visible_width(ch).max(1);
            if used > 0 && used + w > width {
                wrapped.push(std::mem::take(&mut current));
                used = 0;
            }
            current.push(ch);
            used += w;
        }
        wrapped.push(current);
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

pub(crate) fn body_line_count(body: &str, width: u16) -> usize {
    wrap_body(body, body_width(width as usize)).len()
}

pub(crate) fn expanded_body_line_count(body: &str, width: u16) -> usize {
    wrap_body(body, width.max(1) as usize).len()
}

fn nav_text(frame: &Frame<'_>, row: usize) -> String {
    if row < 8 {
        frame
            .accounts
            .get(row)
            .map(|name| {
                format!(
                    "{} {}",
                    if name == frame.account { "*" } else { " " },
                    safe(name)
                )
            })
            .unwrap_or_default()
    } else {
        frame
            .folders
            .get(row - 8)
            .map(|name| {
                format!(
                    "{} {}",
                    if name == frame.folder { "*" } else { " " },
                    safe(name)
                )
            })
            .unwrap_or_default()
    }
}

fn footer_label(lang_ko: bool, slot: usize) -> &'static str {
    match slot {
        0 => text(lang_ko, "back"),
        1 => text(lang_ko, "refresh"),
        2 => text(lang_ko, "compose"),
        3 => text(lang_ko, "help"),
        _ => text(lang_ko, "theme"),
    }
}

/// Render one complete deterministic frame using only ANSI controls emitted by
/// this module.  All caller-provided values are sanitized and clipped first.
pub(crate) fn render(frame: &Frame<'_>) -> String {
    let width = frame.width.max(1) as usize;
    let height = frame.height.max(1) as usize;
    if width < 40 || height < 10 {
        return format!(
            "\x1b[2J\x1b[H{}",
            clip(text(frame.lang_ko, "resize"), width.saturating_sub(1))
        );
    }
    let l = layout(frame.width, frame.height);
    let (fg, panel, accent) = color(frame.theme);
    let reset = "\x1b[0m";
    let mut out = String::new();
    out.push_str("\x1b[2J\x1b[H");
    let screen_label = text(
        frame.lang_ko,
        match frame.screen {
            0 => "mail",
            1 => "folders",
            2 => "messages",
            _ => "body",
        },
    );
    let title = format!(
        "{}  {}",
        text(frame.lang_ko, "title"),
        text(frame.lang_ko, "connected")
    );
    out.push_str(fg);
    out.push_str(panel);
    out.push_str(&line(width, &format!("{} {}", title, screen_label)));
    out.push_str(reset);
    out.push('\n');
    out.push_str(panel);
    out.push_str(&line(
        width,
        &format!(
            " {}  {}  [{}]",
            safe(frame.account),
            safe(frame.folder),
            frame.theme.name()
        ),
    ));
    out.push('\n');
    out.push_str(panel);
    out.push_str(&line(width, ""));
    out.push('\n');
    let info = if frame.help {
        text(frame.lang_ko, "help_commands")
    } else {
        frame.status
    };
    out.push_str(fg);
    out.push_str(panel);
    out.push_str(&line(width, info));
    out.push_str(reset);
    out.push('\n');

    let list_count = frame.rows.len();
    let start = viewport_start(frame.selected, list_count, l.list_height);
    let only_body = frame.screen == 4 || (frame.screen == 3 && width < 110);
    let visible_rows = if only_body {
        0
    } else {
        l.list_height.min(height.saturating_sub(5))
    };
    let wrapped_body = wrap_body(
        frame.body,
        if frame.screen == 4 {
            width
        } else {
            body_width(width)
        },
    );
    for row in 0..visible_rows {
        let idx = start + row;
        let value = frame.rows.get(idx).map(String::as_str).unwrap_or("");
        let selected = idx == frame.selected && idx < list_count;
        let prefix = if selected { "> " } else { "  " };
        let rendered = format!("{}{}", prefix, value);
        let row_fg = if selected {
            selected_color(frame.theme)
        } else {
            fg
        };
        if l.left > 0 && width >= 110 {
            let nav = nav_text(frame, row);
            let body_line = wrapped_body
                .get(frame.body_offset + row)
                .map(String::as_str)
                .unwrap_or("");
            out.push_str(panel);
            out.push_str(row_fg);
            out.push_str(&line(l.left.saturating_sub(1), &nav));
            out.push('│');
            out.push_str(&line(l.center.saturating_sub(1), &rendered));
            out.push('│');
            out.push_str(&line(width.saturating_sub(l.left + l.center), body_line));
            out.push_str(reset);
        } else {
            out.push_str(panel);
            out.push_str(row_fg);
            out.push_str(&line(width, &rendered));
            out.push_str(reset);
        }
        out.push('\n');
    }
    let mut used = 4 + visible_rows;
    if (80..110).contains(&width) || only_body {
        for body_line in wrap_body(
            frame.body,
            if frame.screen == 4 {
                width
            } else {
                body_width(width)
            },
        )
        .iter()
        .skip(frame.body_offset)
        {
            if used >= height.saturating_sub(1) {
                break;
            }
            out.push_str(panel);
            out.push_str(fg);
            out.push_str(&line(width, body_line));
            out.push_str(reset);
            out.push('\n');
            used += 1;
        }
    }
    for _ in used..height.saturating_sub(1) {
        out.push_str(panel);
        out.push_str(&line(width, ""));
        out.push_str(reset);
        out.push('\n');
    }
    out.push_str(accent);
    let segment = (width / 5).max(1);
    for slot in 0..5 {
        let label = if slot == 3 && frame.help {
            format!("{}*", footer_label(frame.lang_ko, slot))
        } else {
            footer_label(frame.lang_ko, slot).to_owned()
        };
        let label = if slot == 4 {
            format!("{} {}", label, frame.theme.name())
        } else {
            label
        };
        out.push_str(panel);
        let slot_width = if slot == 4 && remainder_for_footer(width) == 0 {
            segment.saturating_sub(1)
        } else {
            segment
        };
        out.push_str(&line(slot_width, &label));
        out.push_str(reset);
    }
    let remainder = width.saturating_sub(segment * 5);
    if remainder > 0 {
        out.push_str(panel);
        out.push_str(&line(remainder.saturating_sub(1), ""));
        out.push_str(reset);
    }
    out.push_str(reset);
    let mut positioned = String::from("\x1b[2J\x1b[H");
    let body = out.strip_prefix("\x1b[2J\x1b[H").unwrap_or(&out);
    for (index, row) in body.split('\n').take(height).enumerate() {
        positioned.push_str(&format!("\x1b[{};1H{}", index + 1, row));
    }
    positioned
}

fn remainder_for_footer(width: usize) -> usize {
    width.saturating_sub((width / 5).max(1) * 5)
}

fn viewport_start(selected: usize, len: usize, height: usize) -> usize {
    if height == 0 || len <= height {
        return 0;
    }
    let half = height / 2;
    selected.saturating_sub(half).min(len - height)
}

/// Map a terminal mouse coordinate to the same viewport used by [`render`].
pub(crate) fn hit_test(
    width: u16,
    height: u16,
    column: u16,
    row: u16,
    screen: u8,
    selected: usize,
    list_len: usize,
) -> Hit {
    let l = layout(width, height);
    let x = column as usize;
    let y = row as usize;
    if width < 40 || height < 10 || x >= width as usize || y >= height as usize {
        return Hit::None;
    }
    if (screen == 4 || width < 110 && screen == 3)
        && y >= l.list_start
        && y < height.saturating_sub(1) as usize
    {
        return Hit::Body;
    }
    if y == height.saturating_sub(1) as usize {
        let segment = (width as usize / 5).max(1);
        if x >= segment * 5 {
            return Hit::None;
        }
        return match x / segment {
            0 => Hit::Back,
            1 => Hit::Refresh,
            2 => Hit::Compose,
            3 => Hit::Help,
            _ => Hit::Theme,
        };
    }
    if width >= 110
        && l.left > 0
        && x < l.left
        && y >= l.list_start
        && y < l.list_start + l.list_height
    {
        let nav = y - l.list_start;
        if nav < 16 {
            return if nav < 8 {
                Hit::Account(nav)
            } else {
                Hit::Folder(nav - 8)
            };
        }
    }
    if width >= 110
        && x >= l.left + l.center
        && y >= l.list_start
        && y < l.list_start + l.list_height
    {
        return Hit::Body;
    }
    if y >= l.list_start + l.list_height
        && y < height.saturating_sub(1) as usize
        && (width < 110 || x >= l.left + l.center)
    {
        return Hit::Body;
    }
    if y >= l.list_start && y < l.list_start + l.list_height && (width < 110 || x >= l.left) {
        let idx = viewport_start(selected, list_len, l.list_height) + y - l.list_start;
        return if idx < list_len {
            Hit::Row(idx)
        } else {
            Hit::None
        };
    }
    Hit::None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame<'a>(rows: &'a [String], body: &'a str, theme: Theme) -> Frame<'a> {
        Frame {
            lang_ko: true,
            width: 120,
            height: 20,
            theme,
            screen: 0,
            selected: 0,
            account: "a\u{1b}[31m",
            folder: "INBOX\u{202e}",
            accounts: &[],
            folders: &[],
            rows,
            body,
            body_offset: 0,
            status: "ok\u{2028}",
            help: false,
        }
    }

    #[test]
    fn frame_bounds_and_untrusted_content_are_safe() {
        let rows = vec!["제목\u{1b}[2J".to_owned()];
        let rendered = render(&frame(&rows, "본문\u{2029}", Theme::Blue));
        assert_eq!(rendered.matches("\x1b[2J").count(), 1);
        assert!(!rendered.contains("\x1b[31m"));
        assert!(rendered.len() < 20_000);
        assert!(rendered.contains('�'));
    }

    #[test]
    fn themes_change_output_and_parse_round_trips() {
        let rows = vec!["one".to_owned()];
        let blue = render(&frame(&rows, "", Theme::Blue));
        let green = render(&frame(&rows, "", Theme::Green));
        assert_ne!(blue, green);
        assert_eq!(Theme::parse(Theme::Amber.name()), Some(Theme::Amber));
        assert_eq!(Theme::Blue.next(), Theme::Green);
        assert_eq!(Theme::parse("monochrome"), Some(Theme::Mono));
        assert_eq!(Theme::Amber.next(), Theme::Mono);
    }

    #[test]
    fn bbs_palettes_use_only_standard_8_or_16_color_sgr() {
        assert_eq!(color(Theme::Bbs8), ("\x1b[37m", "\x1b[44m", "\x1b[36m"));
        assert_eq!(selected_color(Theme::Bbs8), "\x1b[33m");
        assert_eq!(color(Theme::Bbs16), ("\x1b[97m", "\x1b[44m", "\x1b[96m"));
        assert_eq!(selected_color(Theme::Bbs16), "\x1b[93m");
        for value in [
            color(Theme::Bbs8).0,
            color(Theme::Bbs8).1,
            color(Theme::Bbs8).2,
            selected_color(Theme::Bbs8),
            color(Theme::Bbs16).0,
            color(Theme::Bbs16).1,
            color(Theme::Bbs16).2,
            selected_color(Theme::Bbs16),
        ] {
            assert!(!value.contains("38;5;") && !value.contains("48;5;"));
        }
    }

    #[test]
    fn bbs_theme_aliases_and_cycle_cover_all_palettes() {
        assert_eq!(Theme::parse("bbs8"), Some(Theme::Bbs8));
        assert_eq!(Theme::parse("classic8"), Some(Theme::Bbs8));
        assert_eq!(Theme::parse("bbs16"), Some(Theme::Bbs16));
        assert_eq!(Theme::parse("classic16"), Some(Theme::Bbs16));

        let mut theme = Theme::Blue;
        let mut names = Vec::new();
        for _ in 0..6 {
            names.push(theme.name());
            theme = theme.next();
        }
        assert_eq!(
            names,
            vec!["blue", "green", "amber", "mono", "bbs8", "bbs16"]
        );
        assert_eq!(theme, Theme::Blue);
    }

    #[test]
    fn selected_row_uses_viewport_mapping() {
        let rows = (0..30).map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(hit_test(120, 12, 30, 7, 0, 12, rows.len()), Hit::Row(12));
        assert_eq!(hit_test(120, 12, 30, 4, 0, 12, rows.len()), Hit::Row(9));
    }
    #[test]
    fn cli16_long_folder_list_keeps_selected_ninetieth_row_visible() {
        let rows = (1..=90)
            .map(|n| format!("Folder {n:02}"))
            .collect::<Vec<_>>();
        let mut f = frame(&rows, "", Theme::Bbs16);
        f.screen = 1;
        f.selected = 89;
        let rendered = render(&f);
        assert!(rendered.contains("Folder 90"));
        assert!(!rendered.contains("Folder 01"));
        assert_eq!(hit_test(120, 24, 10, 23, 4, 0, 90), Hit::Back);
        assert_eq!(hit_test(120, 24, 10, 0, 4, 0, 90), Hit::None);
    }

    #[test]
    fn korean_width_is_bounded() {
        assert_eq!(visible_width('한'), 2);
        assert_eq!(clip("한통", 3), "한 ");
    }

    #[test]
    fn localized_status_keys_are_stable() {
        assert_eq!(text(true, "loading"), "불러오는 중...");
        assert_eq!(text(false, "open_failed"), "Unable to open message");
        assert_eq!(text(true, "missing"), "Unknown");
        assert_eq!(body_line_count("abcdef", 3), 2);
    }

    #[test]
    fn body_hit_is_available_in_wide_and_compact_layouts() {
        assert_eq!(hit_test(120, 20, 110, 5, 1, 0, 1), Hit::Body);
        assert_eq!(hit_test(90, 20, 10, 15, 1, 0, 1), Hit::Body);
    }

    #[test]
    fn compact_render_stays_bounded_with_long_lists() {
        let rows = (0..100)
            .map(|n| format!("메일 {} 한글", n))
            .collect::<Vec<_>>();
        let mut f = frame(&rows, "본문 한 줄", Theme::Mono);
        f.width = 80;
        f.height = 24;
        f.selected = 99;
        let rendered = render(&f);
        assert_eq!(rendered.matches(";1H").count(), 24);
        assert!(!rendered.contains('\n'));
        assert!(rendered.len() < 20_000);
        assert!(rendered.contains("mono"));
    }
    #[test]
    fn cli16_mouse_geometry_matches_visible_rows_and_uneven_footer() {
        assert_eq!(hit_test(121, 24, 23, 23, 2, 0, 1), Hit::Back);
        assert_eq!(hit_test(121, 24, 24, 23, 2, 0, 1), Hit::Refresh);
        assert_eq!(hit_test(121, 24, 120, 23, 2, 0, 1), Hit::None);
        assert_eq!(hit_test(120, 20, 0, 18, 2, 0, 1), Hit::None);
        assert_eq!(hit_test(120, 24, 1, 4, 2, 0, 1), Hit::Account(0));
        assert_eq!(hit_test(120, 24, 1, 12, 2, 0, 1), Hit::Folder(0));
        assert_eq!(hit_test(20, 6, 1, 5, 2, 0, 1), Hit::None);
    }
    #[test]
    fn ops07_resources_have_identical_unique_keys() {
        fn keys(value: &str) -> std::collections::BTreeSet<&str> {
            let mut keys = std::collections::BTreeSet::new();
            for line in value.lines().filter(|v| !v.is_empty()) {
                let (key, value) = line.split_once('=').unwrap();
                assert!(!value.is_empty());
                assert!(keys.insert(key));
            }
            keys
        }
        assert_eq!(keys(TUI_EN), keys(TUI_KO));
    }
    #[test]
    fn cli16_body_wrap_retains_long_lines_and_narrow_body_is_readable() {
        let body = "0123456789".repeat(20);
        assert_eq!(wrap_body(&body, 36).concat(), body);
        let rows = vec!["list".into()];
        let mut f = frame(&rows, "BODY_SENTINEL", Theme::Blue);
        f.width = 60;
        f.screen = 3;
        assert!(render(&f).contains("BODY_SENTINEL"));
        assert_eq!(hit_test(60, 20, 2, 5, 3, 0, 1), Hit::Body);
        f.help = true;
        f.status = "STATUS";
        assert!(render(&f).contains("c"));
    }
}
