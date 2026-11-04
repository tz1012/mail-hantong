//! Native console surface for the communications-style TUI.
//!
//! This module owns console modes for the lifetime of `TerminalSession` and
//! restores the exact modes it found on entry. It is a terminal interaction
//! surface only; it never injects input into another process.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Tab,
    BackTab,
    PageUp,
    PageDown,
    Home,
    End,
    Char(char),
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseKind {
    LeftClick,
    DoubleClick,
    ScrollUp,
    ScrollDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    Key(TerminalKey),
    Mouse {
        column: u16,
        row: u16,
        kind: MouseKind,
    },
    Resize,
    Tick,
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use super::{MouseKind, TerminalEvent, TerminalKey};
    use hantong_contracts::ErrorCode;
    use std::io::{self, IsTerminal, Write};
    use windows_sys::Win32::Foundation::{
        HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Console::*;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    pub struct TerminalSession {
        input: HANDLE,
        output: HANDLE,
        input_mode: u32,
        output_mode: u32,
        cursor_info: CONSOLE_CURSOR_INFO,
        entered: bool,
    }

    impl TerminalSession {
        pub fn enter() -> Result<Self, ErrorCode> {
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                return Err(ErrorCode::HumanStepRequired);
            }
            let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
            let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
            if input.is_null()
                || input == INVALID_HANDLE_VALUE
                || output.is_null()
                || output == INVALID_HANDLE_VALUE
            {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            let (mut old_input, mut old_output) = (0, 0);
            if unsafe {
                GetConsoleMode(input, &mut old_input) == 0
                    || GetConsoleMode(output, &mut old_output) == 0
            } {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            let mut cursor_info = unsafe { std::mem::zeroed::<CONSOLE_CURSOR_INFO>() };
            if unsafe { GetConsoleCursorInfo(output, &mut cursor_info) } == 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            let new_input =
                (old_input | ENABLE_EXTENDED_FLAGS | ENABLE_WINDOW_INPUT | ENABLE_MOUSE_INPUT)
                    & !(ENABLE_LINE_INPUT
                        | ENABLE_ECHO_INPUT
                        | ENABLE_PROCESSED_INPUT
                        | ENABLE_QUICK_EDIT_MODE
                        | ENABLE_VIRTUAL_TERMINAL_INPUT);
            let new_output = old_output | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
            if unsafe { SetConsoleMode(input, new_input) == 0 } {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            if unsafe { SetConsoleMode(output, new_output) == 0 } {
                unsafe {
                    SetConsoleMode(input, old_input);
                }
                return Err(ErrorCode::CapabilityUnsupported);
            }
            if write!(io::stdout(), "\x1b[?1049h\x1b[?25l")
                .and_then(|_| io::stdout().flush())
                .is_err()
            {
                let _ =
                    write!(io::stdout(), "\x1b[?25h\x1b[?1049l").and_then(|_| io::stdout().flush());
                unsafe {
                    SetConsoleMode(input, old_input);
                    SetConsoleMode(output, old_output);
                    SetConsoleCursorInfo(output, &cursor_info);
                }
                return Err(ErrorCode::InternalError);
            }
            Ok(Self {
                input,
                output,
                input_mode: old_input,
                output_mode: old_output,
                cursor_info,
                entered: true,
            })
        }

        pub fn size(&self) -> (u16, u16) {
            let mut info = unsafe { std::mem::zeroed::<CONSOLE_SCREEN_BUFFER_INFO>() };
            if unsafe { GetConsoleScreenBufferInfo(self.output, &mut info) } == 0 {
                return (0, 0);
            }
            let width = info
                .srWindow
                .Right
                .saturating_sub(info.srWindow.Left)
                .saturating_add(1) as u16;
            let height = info
                .srWindow
                .Bottom
                .saturating_sub(info.srWindow.Top)
                .saturating_add(1) as u16;
            (width, height)
        }

        pub fn read_event(&mut self) -> Result<TerminalEvent, ErrorCode> {
            loop {
                let wait = unsafe { WaitForSingleObject(self.input, 250) };
                if wait == WAIT_TIMEOUT {
                    return Ok(TerminalEvent::Tick);
                }
                if wait != WAIT_OBJECT_0 {
                    return Err(ErrorCode::CoreUnavailable);
                }
                let mut record = unsafe { std::mem::zeroed::<INPUT_RECORD>() };
                let mut count = 0;
                if unsafe { ReadConsoleInputW(self.input, &mut record, 1, &mut count) } == 0
                    || count == 0
                {
                    return Err(ErrorCode::CoreUnavailable);
                }
                match record.EventType as u32 {
                    WINDOW_BUFFER_SIZE_EVENT => return Ok(TerminalEvent::Resize),
                    MOUSE_EVENT => {
                        let event = unsafe { record.Event.MouseEvent };
                        let kind = if event.dwButtonState & FROM_LEFT_1ST_BUTTON_PRESSED != 0
                            && (event.dwEventFlags == 0 || event.dwEventFlags & DOUBLE_CLICK != 0)
                        {
                            if event.dwEventFlags & DOUBLE_CLICK != 0 {
                                Some(MouseKind::DoubleClick)
                            } else {
                                Some(MouseKind::LeftClick)
                            }
                        } else if event.dwEventFlags & MOUSE_WHEELED != 0 {
                            if ((event.dwButtonState >> 16) as i16) > 0 {
                                Some(MouseKind::ScrollUp)
                            } else {
                                Some(MouseKind::ScrollDown)
                            }
                        } else {
                            None
                        };
                        if let Some(kind) = kind {
                            let mut info =
                                unsafe { std::mem::zeroed::<CONSOLE_SCREEN_BUFFER_INFO>() };
                            if unsafe { GetConsoleScreenBufferInfo(self.output, &mut info) } == 0 {
                                continue;
                            }
                            return Ok(TerminalEvent::Mouse {
                                column: event
                                    .dwMousePosition
                                    .X
                                    .saturating_sub(info.srWindow.Left)
                                    .max(0) as u16,
                                row: event
                                    .dwMousePosition
                                    .Y
                                    .saturating_sub(info.srWindow.Top)
                                    .max(0) as u16,
                                kind,
                            });
                        }
                    }
                    KEY_EVENT => {
                        let event = unsafe { record.Event.KeyEvent };
                        if event.bKeyDown == 0 {
                            continue;
                        }
                        let state = event.dwControlKeyState;
                        if state & (LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) != 0
                            && event.wVirtualKeyCode as u32 == 0x43
                        {
                            return Ok(TerminalEvent::Key(TerminalKey::Quit));
                        }
                        if state
                            & (LEFT_ALT_PRESSED
                                | RIGHT_ALT_PRESSED
                                | LEFT_CTRL_PRESSED
                                | RIGHT_CTRL_PRESSED)
                            != 0
                        {
                            continue;
                        }
                        let modified = state
                            & (LEFT_ALT_PRESSED
                                | RIGHT_ALT_PRESSED
                                | LEFT_CTRL_PRESSED
                                | RIGHT_CTRL_PRESSED)
                            != 0;
                        let key = match event.wVirtualKeyCode as u32 {
                            0x26 => TerminalKey::Up,
                            0x28 => TerminalKey::Down,
                            0x25 => TerminalKey::Left,
                            0x27 => TerminalKey::Right,
                            0x0d => TerminalKey::Enter,
                            0x1b => TerminalKey::Escape,
                            0x09 if state & SHIFT_PRESSED != 0 => TerminalKey::BackTab,
                            0x09 => TerminalKey::Tab,
                            0x21 => TerminalKey::PageUp,
                            0x22 => TerminalKey::PageDown,
                            0x24 => TerminalKey::Home,
                            0x23 => TerminalKey::End,
                            _ if !modified && unsafe { event.uChar.UnicodeChar } != 0 => {
                                match char::from_u32(unsafe { event.uChar.UnicodeChar as u32 }) {
                                    Some(ch) => TerminalKey::Char(ch),
                                    None => continue,
                                }
                            }
                            _ => continue,
                        };
                        return Ok(TerminalEvent::Key(key));
                    }
                    _ => {}
                }
            }
        }
    }

    impl Drop for TerminalSession {
        fn drop(&mut self) {
            if !self.entered {
                return;
            }
            let _ = write!(io::stdout(), "\x1b[?25h\x1b[?1049l").and_then(|_| io::stdout().flush());
            unsafe {
                SetConsoleCursorInfo(self.output, &self.cursor_info);
                SetConsoleMode(self.input, self.input_mode);
                SetConsoleMode(self.output, self.output_mode);
            }
            self.entered = false;
        }
    }
}

#[cfg(windows)]
pub use windows::TerminalSession;

#[cfg(unix)]
mod unix {
    use super::{MouseKind, TerminalEvent, TerminalKey};
    use hantong_contracts::ErrorCode;
    use std::cell::Cell;
    use std::io::{self, IsTerminal, Read, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[cfg(target_os = "macos")]
    const STTY: &str = "/bin/stty";
    #[cfg(not(target_os = "macos"))]
    const STTY: &str = "/bin/stty";

    fn stty(args: &[&str]) -> Result<Vec<u8>, ErrorCode> {
        let output = Command::new(STTY)
            .args(args)
            .stdin(Stdio::inherit())
            .output()
            .map_err(|_| ErrorCode::CapabilityUnsupported)?;
        if !output.status.success() {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        Ok(output.stdout)
    }

    fn read_utf8(first: u8, input: &mut impl Read) -> Result<char, ErrorCode> {
        let width = if first < 0x80 {
            1
        } else if first & 0xe0 == 0xc0 {
            2
        } else if first & 0xf0 == 0xe0 {
            3
        } else if first & 0xf8 == 0xf0 {
            4
        } else {
            return Ok('\u{fffd}');
        };
        let mut bytes = [0u8; 4];
        bytes[0] = first;
        for byte in &mut bytes[1..width] {
            if input.read_exact(std::slice::from_mut(byte)).is_err() {
                return Ok('\u{fffd}');
            }
        }
        Ok(std::str::from_utf8(&bytes[..width])
            .ok()
            .and_then(|value| value.chars().next())
            .unwrap_or('\u{fffd}'))
    }

    fn parse_escape(input: &mut impl Read) -> Result<TerminalEvent, ErrorCode> {
        let mut next = [0u8; 1];
        if input.read_exact(&mut next).is_err() {
            return Ok(TerminalEvent::Key(TerminalKey::Escape));
        }
        if next[0] != b'[' {
            return Ok(TerminalEvent::Key(TerminalKey::Escape));
        }
        let mut sequence = Vec::with_capacity(16);
        loop {
            if input.read_exact(&mut next).is_err() {
                return Ok(TerminalEvent::Key(TerminalKey::Escape));
            }
            sequence.push(next[0]);
            if (next[0] as char).is_ascii_alphabetic() || next[0] == b'~' {
                break;
            }
            if sequence.len() >= 32 {
                return Ok(TerminalEvent::Key(TerminalKey::Escape));
            }
        }
        match sequence.as_slice() {
            [b'A'] => Ok(TerminalEvent::Key(TerminalKey::Up)),
            [b'B'] => Ok(TerminalEvent::Key(TerminalKey::Down)),
            [b'C'] => Ok(TerminalEvent::Key(TerminalKey::Right)),
            [b'D'] => Ok(TerminalEvent::Key(TerminalKey::Left)),
            [b'H'] | [b'1', b'~'] => Ok(TerminalEvent::Key(TerminalKey::Home)),
            [b'F'] | [b'4', b'~'] => Ok(TerminalEvent::Key(TerminalKey::End)),
            [b'5', b'~'] => Ok(TerminalEvent::Key(TerminalKey::PageUp)),
            [b'6', b'~'] => Ok(TerminalEvent::Key(TerminalKey::PageDown)),
            [b'Z'] => Ok(TerminalEvent::Key(TerminalKey::BackTab)),
            sequence if sequence.first() == Some(&b'<') => parse_mouse(sequence),
            _ => Ok(TerminalEvent::Key(TerminalKey::Escape)),
        }
    }

    fn parse_mouse(sequence: &[u8]) -> Result<TerminalEvent, ErrorCode> {
        let end = *sequence.last().ok_or(ErrorCode::InternalError)?;
        let fields = std::str::from_utf8(&sequence[1..sequence.len() - 1])
            .ok()
            .and_then(|value| {
                let mut parts = value.split(';');
                let button = parts.next()?.parse::<u16>().ok()?;
                let column = parts.next()?.parse::<u16>().ok()?;
                let row = parts.next()?.parse::<u16>().ok()?;
                if parts.next().is_some() || column == 0 || row == 0 {
                    return None;
                }
                Some((button, column, row))
            });
        let Some((button, column, row)) = fields else {
            return Ok(TerminalEvent::Key(TerminalKey::Escape));
        };
        let kind = if button & 64 != 0 {
            if button & 1 == 0 {
                MouseKind::ScrollUp
            } else {
                MouseKind::ScrollDown
            }
        } else if button & 3 == 0 && end == b'M' {
            MouseKind::LeftClick
        } else {
            return Ok(TerminalEvent::Key(TerminalKey::Escape));
        };
        Ok(TerminalEvent::Mouse {
            column: column.saturating_sub(1),
            row: row.saturating_sub(1),
            kind,
        })
    }

    pub struct TerminalSession {
        saved_mode: String,
        entered: bool,
        alternate_screen: bool,
        cached_size: Cell<Option<(Instant, (u16, u16))>>,
        last_click: Cell<Option<(Instant, u16, u16)>>,
    }

    impl TerminalSession {
        pub fn enter() -> Result<Self, ErrorCode> {
            Self::enter_with_screen(true)
        }

        pub fn enter_inline() -> Result<Self, ErrorCode> {
            Self::enter_with_screen(false)
        }

        fn enter_with_screen(alternate_screen: bool) -> Result<Self, ErrorCode> {
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                return Err(ErrorCode::HumanStepRequired);
            }
            let saved_mode = String::from_utf8(stty(&["-g"])?)
                .map_err(|_| ErrorCode::CapabilityUnsupported)?
                .trim()
                .to_owned();
            if saved_mode.is_empty() {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            if stty(&[
                "-icanon", "-echo", "-isig", "-ixon", "-iexten", "min", "0", "time", "1",
            ])
            .is_err()
            {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            let screen = if alternate_screen { "\x1b[?1049h" } else { "" };
            if write!(io::stdout(), "{screen}\x1b[?25l\x1b[?1000h\x1b[?1006h")
                .and_then(|_| io::stdout().flush())
                .is_err()
            {
                let _ = stty(&[&saved_mode]);
                return Err(ErrorCode::InternalError);
            }
            Ok(Self {
                saved_mode,
                entered: true,
                alternate_screen,
                cached_size: Cell::new(None),
                last_click: Cell::new(None),
            })
        }

        fn query_size() -> (u16, u16) {
            let Ok(output) = stty(&["size"]) else {
                return (0, 0);
            };
            let Ok(text) = std::str::from_utf8(&output) else {
                return (0, 0);
            };
            let mut values = text
                .split_whitespace()
                .filter_map(|value| value.parse::<u16>().ok());
            (
                values.next_back().unwrap_or(0),
                values.next_back().unwrap_or(0),
            )
        }

        pub fn size(&self) -> (u16, u16) {
            if let Some((at, size)) = self.cached_size.get() {
                if at.elapsed() < Duration::from_millis(250) {
                    return size;
                }
            }
            let size = Self::query_size();
            self.cached_size.set(Some((Instant::now(), size)));
            size
        }

        fn poll_resize(&self) -> bool {
            let Some((at, previous)) = self.cached_size.get() else {
                return false;
            };
            if at.elapsed() < Duration::from_millis(250) {
                return false;
            }
            let current = Self::query_size();
            self.cached_size.set(Some((Instant::now(), current)));
            current != previous
        }

        fn decorate_event(&self, event: TerminalEvent) -> TerminalEvent {
            let TerminalEvent::Mouse {
                column,
                row,
                kind: MouseKind::LeftClick,
            } = event
            else {
                if matches!(
                    event,
                    TerminalEvent::Key(_)
                        | TerminalEvent::Mouse {
                            kind: MouseKind::ScrollUp | MouseKind::ScrollDown,
                            ..
                        }
                ) {
                    self.last_click.set(None);
                }
                return event;
            };
            let now = Instant::now();
            let double = self
                .last_click
                .get()
                .is_some_and(|(at, old_column, old_row)| {
                    old_column == column
                        && old_row == row
                        && at.elapsed() <= Duration::from_millis(400)
                });
            self.last_click.set(if double {
                None
            } else {
                Some((now, column, row))
            });
            TerminalEvent::Mouse {
                column,
                row,
                kind: if double {
                    MouseKind::DoubleClick
                } else {
                    MouseKind::LeftClick
                },
            }
        }

        pub fn read_event(&mut self) -> Result<TerminalEvent, ErrorCode> {
            let mut input = io::stdin().lock();
            let mut byte = [0u8; 1];
            let event = match input.read(&mut byte) {
                Ok(0) => {
                    if self.poll_resize() {
                        return Ok(TerminalEvent::Resize);
                    }
                    TerminalEvent::Tick
                }
                Err(_) => return Err(ErrorCode::CoreUnavailable),
                Ok(_) => match byte[0] {
                    0x03 => TerminalEvent::Key(TerminalKey::Quit),
                    b'\r' | b'\n' => TerminalEvent::Key(TerminalKey::Enter),
                    0x09 => TerminalEvent::Key(TerminalKey::Tab),
                    0x1b => parse_escape(&mut input)?,
                    byte => TerminalEvent::Key(TerminalKey::Char(read_utf8(byte, &mut input)?)),
                },
            };
            Ok(self.decorate_event(event))
        }
    }

    impl Drop for TerminalSession {
        fn drop(&mut self) {
            if !self.entered {
                return;
            }
            let screen = if self.alternate_screen {
                "\x1b[?1049l"
            } else {
                ""
            };
            let _ = write!(io::stdout(), "\x1b[?1006l\x1b[?1000l\x1b[?25h{screen}")
                .and_then(|_| io::stdout().flush());
            let _ = stty(&[&self.saved_mode]);
            self.entered = false;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::Cursor;

        #[test]
        fn parses_arrow_and_mouse_sequences_without_shell_or_path_lookup() {
            let mut arrow = Cursor::new(b"[A".to_vec());
            assert_eq!(
                parse_escape(&mut arrow).unwrap(),
                TerminalEvent::Key(TerminalKey::Up)
            );
            assert_eq!(
                parse_mouse(b"<0;4;3M").unwrap(),
                TerminalEvent::Mouse {
                    column: 3,
                    row: 2,
                    kind: MouseKind::LeftClick,
                }
            );
            assert_eq!(
                parse_mouse(b"<64;1;1M").unwrap(),
                TerminalEvent::Mouse {
                    column: 0,
                    row: 0,
                    kind: MouseKind::ScrollUp,
                }
            );
            assert!(matches!(
                parse_mouse(b"<0;4;3;99M").unwrap(),
                TerminalEvent::Key(TerminalKey::Escape)
            ));
            assert!(matches!(
                parse_mouse(b"<0;0;3M").unwrap(),
                TerminalEvent::Key(TerminalKey::Escape)
            ));
        }

        #[test]
        fn same_position_click_pair_becomes_double_click() {
            let session = TerminalSession {
                saved_mode: String::new(),
                entered: false,
                alternate_screen: false,
                cached_size: Cell::new(None),
                last_click: Cell::new(None),
            };
            let click = TerminalEvent::Mouse {
                column: 4,
                row: 2,
                kind: MouseKind::LeftClick,
            };
            assert_eq!(session.decorate_event(click), click);
            assert_eq!(
                session.decorate_event(click),
                TerminalEvent::Mouse {
                    column: 4,
                    row: 2,
                    kind: MouseKind::DoubleClick,
                }
            );
        }

        #[test]
        fn non_tty_entry_fails_before_changing_terminal_state() {
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                assert!(matches!(
                    TerminalSession::enter(),
                    Err(ErrorCode::HumanStepRequired)
                ));
            }
        }
    }
}

#[cfg(unix)]
pub use unix::TerminalSession;
