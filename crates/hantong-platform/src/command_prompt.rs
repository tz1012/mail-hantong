//! Bounded interactive command input for the terminal CLI.
//!
//! This is a line editor for ordinary commands. It does not read the
//! clipboard, inject input into another process, or provide secret input.

use hantong_contracts::ErrorCode;

pub const MAX_COMMAND_BYTES: usize = 256;
pub const MAX_HINTS: usize = 32;
pub const MAX_HINT_BYTES: usize = 128;

/// Read one command from the current interactive terminal.
///
/// `hints` are display-only command suggestions. A leading `/` prints the
/// bounded suggestions immediately; Up/Down cycles them into the edit line.
#[cfg(any(windows, unix))]
pub fn read_command_line(prompt: &str, hints: &[String]) -> Result<Option<String>, ErrorCode> {
    read_command_line_with_options(prompt, hints, &[])
}

#[cfg(any(windows, unix))]
pub fn read_command_line_with_options(
    prompt: &str,
    hints: &[String],
    choices: &[(String, String)],
) -> Result<Option<String>, ErrorCode> {
    read_command_line_with_selection(prompt, hints, choices, 0)
}

#[cfg(any(windows, unix))]
pub fn read_command_line_with_selection(
    prompt: &str,
    hints: &[String],
    choices: &[(String, String)],
    initial: usize,
) -> Result<Option<String>, ErrorCode> {
    #[cfg(windows)]
    {
        windows::read(prompt, hints, choices, initial)
    }
    #[cfg(unix)]
    {
        unix::read(prompt, hints, choices, initial)
    }
}

#[cfg(not(any(windows, unix)))]
pub fn read_command_line(_prompt: &str, _hints: &[String]) -> Result<Option<String>, ErrorCode> {
    Err(ErrorCode::HumanStepRequired)
}

#[cfg(not(any(windows, unix)))]
pub fn read_command_line_with_options(
    _prompt: &str,
    _hints: &[String],
    _choices: &[(String, String)],
) -> Result<Option<String>, ErrorCode> {
    Err(ErrorCode::HumanStepRequired)
}

#[cfg(not(any(windows, unix)))]
pub fn read_command_line_with_selection(
    _prompt: &str,
    _hints: &[String],
    _choices: &[(String, String)],
    _initial: usize,
) -> Result<Option<String>, ErrorCode> {
    Err(ErrorCode::HumanStepRequired)
}

#[cfg(any(windows, unix, test))]
fn valid_prompt(prompt: &str) -> bool {
    prompt.len() <= MAX_HINT_BYTES && !prompt.chars().any(char::is_control)
}

#[cfg(any(windows, unix, test))]
fn valid_hints(hints: &[String]) -> bool {
    hints.len() <= MAX_HINTS
        && hints.iter().all(|hint| {
            hint.len() <= MAX_HINT_BYTES && !hint.is_empty() && !hint.chars().any(char::is_control)
        })
}

#[cfg(any(windows, unix, test))]
fn valid_choices(choices: &[(String, String)]) -> bool {
    choices.len() <= 256
        && choices.iter().all(|(value, label)| {
            value.len() <= MAX_COMMAND_BYTES
                && !value.is_empty()
                && !value.chars().any(char::is_control)
                && label.len() <= 1024
                && !label.is_empty()
                && !label.chars().any(char::is_control)
        })
}

#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use super::*;
    use std::io::{self, IsTerminal, Write};
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::*;

    struct InputMode {
        handle: HANDLE,
        old: u32,
    }

    impl Drop for InputMode {
        fn drop(&mut self) {
            unsafe {
                SetConsoleMode(self.handle, self.old);
            }
        }
    }

    struct OutputMode {
        handle: HANDLE,
        old: u32,
    }

    impl Drop for OutputMode {
        fn drop(&mut self) {
            unsafe {
                SetConsoleMode(self.handle, self.old);
            }
        }
    }

    fn enter_input_mode(handle: HANDLE) -> Result<InputMode, ErrorCode> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        let mut old = 0;
        unsafe {
            if GetConsoleMode(handle, &mut old) == 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            let mode = (old | ENABLE_EXTENDED_FLAGS)
                & !(ENABLE_LINE_INPUT
                    | ENABLE_ECHO_INPUT
                    | ENABLE_PROCESSED_INPUT
                    | ENABLE_QUICK_EDIT_MODE);
            if SetConsoleMode(handle, mode) == 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
        }
        Ok(InputMode { handle, old })
    }

    fn enter_output_mode(handle: HANDLE) -> Result<OutputMode, ErrorCode> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        let mut old = 0;
        unsafe {
            if GetConsoleMode(handle, &mut old) == 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            let mode = old | ENABLE_PROCESSED_OUTPUT | ENABLE_VIRTUAL_TERMINAL_PROCESSING;
            if SetConsoleMode(handle, mode) == 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
        }
        Ok(OutputMode { handle, old })
    }

    fn redraw(prompt: &str, line: &str) -> Result<(), ErrorCode> {
        print!("\r\x1b[2K{prompt}{line}");
        io::stdout().flush().map_err(|_| ErrorCode::CoreUnavailable)
    }

    fn print_hints(prompt: &str, line: &str, hints: &[String]) -> Result<(), ErrorCode> {
        println!();
        for hint in hints {
            println!("  {hint}");
        }
        redraw(prompt, line)
    }

    fn append_utf16(line: &mut String, pending: &mut Option<u16>, unit: u16) {
        if (0xD800..=0xDBFF).contains(&unit) {
            *pending = Some(unit);
            return;
        }
        let code = if (0xDC00..=0xDFFF).contains(&unit) {
            pending
                .take()
                .map(|high| 0x1_0000 + (((high as u32) - 0xD800) << 10) + ((unit as u32) - 0xDC00))
        } else {
            pending.take();
            Some(unit as u32)
        };
        if let Some(ch) = code.and_then(char::from_u32)
            && ch != '\0'
            && !ch.is_control()
            && line.len().saturating_add(ch.len_utf8()) <= MAX_COMMAND_BYTES
        {
            line.push(ch);
        }
    }

    pub(super) fn read(
        prompt: &str,
        hints: &[String],
        choices: &[(String, String)],
        initial: usize,
    ) -> Result<Option<String>, ErrorCode> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(ErrorCode::HumanStepRequired);
        }
        if !valid_prompt(prompt) || !valid_hints(hints) || !valid_choices(choices) {
            return Err(ErrorCode::InvalidInput);
        }
        let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        let _mode = enter_input_mode(input)?;
        let _output_mode = enter_output_mode(output)?;
        let mut choice_index = (!choices.is_empty()).then(|| initial.min(choices.len() - 1));
        redraw(
            prompt,
            choice_index
                .and_then(|index| choices.get(index))
                .map(|(_, label)| format!("> {label}"))
                .as_deref()
                .unwrap_or(""),
        )?;
        let mut line = String::new();
        let mut hint_index: Option<usize> = None;
        let mut pending_high = None;

        loop {
            let mut record = unsafe { std::mem::zeroed::<INPUT_RECORD>() };
            let mut count = 0;
            if unsafe { ReadConsoleInputW(input, &mut record, 1, &mut count) } == 0 || count == 0 {
                return Ok(None);
            }
            if record.EventType != KEY_EVENT as u16 {
                continue;
            }
            let event = unsafe { record.Event.KeyEvent };
            if event.bKeyDown == 0 {
                continue;
            }
            let state = event.dwControlKeyState;
            if state & (LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) != 0 {
                if event.wVirtualKeyCode as u32 == 0x43 {
                    return Ok(None);
                }
                continue;
            }
            match event.wVirtualKeyCode as u32 {
                0x0D => {
                    println!();
                    return Ok(Some(
                        choice_index
                            .and_then(|index| choices.get(index).map(|(value, _)| value.clone()))
                            .unwrap_or(line),
                    ));
                }
                0x1B => {
                    println!();
                    return Ok(Some("/back".to_owned()));
                }
                0x08 => {
                    pending_high = None;
                    choice_index = None;
                    line.pop();
                    hint_index = None;
                    redraw(prompt, &line)?;
                }
                0x26 | 0x28 if !choices.is_empty() && !line.starts_with('/') => {
                    let next = match (event.wVirtualKeyCode as u32, choice_index) {
                        (0x26, Some(index)) => index.saturating_sub(1),
                        (0x26, None) => choices.len().saturating_sub(1),
                        (_, Some(index)) => (index + 1) % choices.len(),
                        (_, None) => 0,
                    };
                    choice_index = Some(next);
                    line.clear();
                    redraw(prompt, &format!("> {}", choices[next].1))?;
                }
                0x26 | 0x28 if !hints.is_empty() => {
                    let next = match (event.wVirtualKeyCode as u32, hint_index) {
                        (0x26, Some(index)) => index.saturating_sub(1),
                        (0x26, None) => hints.len().saturating_sub(1),
                        (_, Some(index)) => (index + 1) % hints.len(),
                        (_, None) => 0,
                    };
                    hint_index = Some(next);
                    line.clear();
                    line.push_str(&hints[next]);
                    redraw(prompt, &line)?;
                }
                0x1A => return Ok(None),
                _ => {
                    let unit = unsafe { event.uChar.UnicodeChar };
                    if unit == 0 {
                        continue;
                    }
                    let first_slash = line.is_empty() && unit == '/' as u16;
                    choice_index = None;
                    append_utf16(&mut line, &mut pending_high, unit);
                    hint_index = None;
                    if first_slash {
                        print_hints(prompt, &line, hints)?;
                    } else {
                        redraw(prompt, &line)?;
                    }
                }
            }
        }
    }
}

#[cfg(unix)]
mod unix {
    use super::*;
    use crate::{TerminalEvent, TerminalKey, TerminalSession};
    use std::io::{self, IsTerminal, Write};

    fn redraw(prompt: &str, line: &str) -> Result<(), ErrorCode> {
        print!("\r\x1b[2K{prompt}{line}");
        io::stdout().flush().map_err(|_| ErrorCode::CoreUnavailable)
    }

    fn print_hints(prompt: &str, line: &str, hints: &[String]) -> Result<(), ErrorCode> {
        println!();
        for hint in hints {
            println!("  {hint}");
        }
        redraw(prompt, line)
    }

    pub(super) fn read(
        prompt: &str,
        hints: &[String],
        choices: &[(String, String)],
        initial: usize,
    ) -> Result<Option<String>, ErrorCode> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(ErrorCode::HumanStepRequired);
        }
        if !valid_prompt(prompt) || !valid_hints(hints) || !valid_choices(choices) {
            return Err(ErrorCode::InvalidInput);
        }
        let mut terminal = TerminalSession::enter_inline()?;
        let mut line = String::new();
        let mut choice_index = (!choices.is_empty()).then(|| initial.min(choices.len() - 1));
        let mut hint_index: Option<usize> = None;
        let initial_line = choice_index
            .and_then(|index| choices.get(index))
            .map(|(_, label)| format!("> {label}"))
            .unwrap_or_default();
        redraw(prompt, &initial_line)?;

        loop {
            let event = terminal.read_event()?;
            let key = match event {
                TerminalEvent::Key(key) => key,
                TerminalEvent::Resize | TerminalEvent::Tick | TerminalEvent::Mouse { .. } => {
                    continue;
                }
            };
            match key {
                TerminalKey::Enter | TerminalKey::Right => {
                    println!();
                    return Ok(Some(
                        choice_index
                            .and_then(|index| choices.get(index).map(|(value, _)| value.clone()))
                            .unwrap_or(line),
                    ));
                }
                TerminalKey::Escape | TerminalKey::Left | TerminalKey::Quit => {
                    println!();
                    return Ok(Some("/back".to_owned()));
                }
                TerminalKey::Char('\u{7f}' | '\u{8}') => {
                    choice_index = None;
                    line.pop();
                    hint_index = None;
                    redraw(prompt, &line)?;
                }
                TerminalKey::Up | TerminalKey::Down
                    if !choices.is_empty() && !line.starts_with('/') =>
                {
                    let next = match (key, choice_index) {
                        (TerminalKey::Up, Some(index)) => index.saturating_sub(1),
                        (TerminalKey::Up, None) => choices.len().saturating_sub(1),
                        (_, Some(index)) => (index + 1) % choices.len(),
                        (_, None) => 0,
                    };
                    choice_index = Some(next);
                    line.clear();
                    redraw(prompt, &format!("> {}", choices[next].1))?;
                }
                TerminalKey::Up | TerminalKey::Down if !hints.is_empty() => {
                    let next = match (key, hint_index) {
                        (TerminalKey::Up, Some(index)) => index.saturating_sub(1),
                        (TerminalKey::Up, None) => hints.len().saturating_sub(1),
                        (_, Some(index)) => (index + 1) % hints.len(),
                        (_, None) => 0,
                    };
                    hint_index = Some(next);
                    line.clear();
                    line.push_str(&hints[next]);
                    redraw(prompt, &line)?;
                }
                TerminalKey::Char(ch) if !ch.is_control() => {
                    choice_index = None;
                    if line.len().saturating_add(ch.len_utf8()) <= MAX_COMMAND_BYTES {
                        let first_slash = line.is_empty() && ch == '/';
                        line.push(ch);
                        hint_index = None;
                        if first_slash {
                            print_hints(prompt, &line, hints)?;
                        } else {
                            redraw(prompt, &line)?;
                        }
                    }
                }
                TerminalKey::Tab
                | TerminalKey::Up
                | TerminalKey::Down
                | TerminalKey::BackTab
                | TerminalKey::Home
                | TerminalKey::End
                | TerminalKey::PageUp
                | TerminalKey::PageDown
                | TerminalKey::Char(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli16_command_prompt_bounds_untrusted_prompt_and_hints() {
        assert!(valid_prompt("hantong > "));
        assert!(!valid_prompt("bad\n"));
        assert!(valid_hints(&["/help".into(), "/back".into()]));
        assert!(!valid_hints(&["bad\tcommand".into()]));
        assert!(!valid_hints(&["x".repeat(MAX_HINT_BYTES + 1)]));
        assert!(!valid_hints(&vec!["/x".into(); MAX_HINTS + 1]));
        assert!(valid_choices(&[("1".into(), "Inbox".into())]));
        assert!(!valid_choices(&[("bad\n".into(), "Inbox".into())]));
        assert!(!valid_choices(&[("1".into(), "x".repeat(1025))]));
        assert!(!valid_choices(&vec![("1".into(), "Inbox".into()); 257]));
    }

    #[cfg(not(windows))]
    #[test]
    fn cli16_command_prompt_requires_interactive_windows_terminal() {
        assert_eq!(
            read_command_line("hantong > ", &[]),
            Err(ErrorCode::HumanStepRequired)
        );
    }
}
