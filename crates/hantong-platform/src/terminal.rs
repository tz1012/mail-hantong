//! Windows console surface for the communications-style TUI.
//!
//! This module owns console modes for the lifetime of `TerminalSession` and
//! restores the exact modes it found on entry.  It is a terminal interaction
//! surface only; it never injects input into another process.

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use hantong_contracts::ErrorCode;
    use std::io::{self, IsTerminal, Write};
    use windows_sys::Win32::Foundation::{
        HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Console::*;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

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
pub use windows::{MouseKind, TerminalEvent, TerminalKey, TerminalSession};
