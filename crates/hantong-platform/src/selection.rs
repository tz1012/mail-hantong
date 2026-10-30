//! Preference selection only. Never use this surface as approval evidence.
use hantong_contracts::ErrorCode;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKey {
    Up,
    Down,
    Enter,
    Escape,
    Other,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    len: usize,
    index: usize,
}
impl Selection {
    pub fn new(len: usize, index: usize) -> Result<Self, ErrorCode> {
        if len == 0 || len > 256 || index >= len {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self { len, index })
    }
    pub fn index(self) -> usize {
        self.index
    }
    pub fn key(&mut self, key: SelectionKey) -> Option<Option<usize>> {
        match key {
            SelectionKey::Up => self.index = (self.index + self.len - 1) % self.len,
            SelectionKey::Down => self.index = (self.index + 1) % self.len,
            SelectionKey::Enter => return Some(Some(self.index)),
            SelectionKey::Escape => return Some(None),
            SelectionKey::Other => {}
        }
        None
    }
}
pub fn interactive_select(labels: &[String], initial: usize) -> Result<Option<usize>, ErrorCode> {
    let selection = Selection::new(labels.len(), initial)?;
    if labels
        .iter()
        .any(|s| s.len() > 1024 || s.chars().any(char::is_control))
    {
        return Err(ErrorCode::InvalidInput);
    }
    #[cfg(windows)]
    {
        windows::select(labels, selection)
    }
    #[cfg(not(windows))]
    {
        let _ = (labels, selection);
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows {
    use super::*;
    use std::io::{self, IsTerminal, Write};
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::Console::*,
    };
    struct Mode {
        handle: HANDLE,
        old: u32,
    }
    impl Drop for Mode {
        fn drop(&mut self) {
            unsafe {
                SetConsoleMode(self.handle, self.old);
            }
        }
    }
    fn mode(handle: HANDLE, change: impl FnOnce(u32) -> u32) -> Result<Mode, ErrorCode> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        let mut old = 0;
        unsafe {
            if GetConsoleMode(handle, &mut old) == 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
            if SetConsoleMode(handle, change(old)) == 0 {
                return Err(ErrorCode::CapabilityUnsupported);
            }
        }
        Ok(Mode { handle, old })
    }
    pub fn select(labels: &[String], mut selection: Selection) -> Result<Option<usize>, ErrorCode> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(ErrorCode::HumanStepRequired);
        }
        // Both guards live across every read/render and restore modes on all exits.
        let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        let _input = mode(input, |v| {
            (v & !(ENABLE_LINE_INPUT
                | ENABLE_ECHO_INPUT
                | ENABLE_PROCESSED_INPUT
                | ENABLE_QUICK_EDIT_MODE))
                | ENABLE_EXTENDED_FLAGS
        })?;
        let _output = mode(output, |v| v | ENABLE_VIRTUAL_TERMINAL_PROCESSING)?;
        let mut first = true;
        loop {
            if !first {
                print!("\x1b[9A")
            }
            first = false;
            let start = selection
                .index()
                .saturating_sub(3)
                .min(labels.len().saturating_sub(8));
            for offset in 0..8 {
                let index = start + offset;
                print!("\r\x1b[2K");
                if let Some(label) = labels.get(index) {
                    println!(
                        "{} {label}",
                        if index == selection.index() { ">" } else { " " }
                    )
                } else {
                    println!();
                }
            }
            println!("\r\x1b[2K↑/↓  Enter  Esc");
            io::stdout().flush().map_err(|_| ErrorCode::InternalError)?;
            loop {
                let mut record: INPUT_RECORD = unsafe { std::mem::zeroed() };
                let mut count = 0;
                if unsafe { ReadConsoleInputW(input, &mut record, 1, &mut count) } == 0
                    || count == 0
                {
                    return Err(ErrorCode::CoreUnavailable);
                }
                if record.EventType != KEY_EVENT as u16 {
                    continue;
                }
                let event = unsafe { record.Event.KeyEvent };
                if event.bKeyDown == 0 {
                    continue;
                }
                let modified = event.dwControlKeyState
                    & (LEFT_ALT_PRESSED
                        | RIGHT_ALT_PRESSED
                        | LEFT_CTRL_PRESSED
                        | RIGHT_CTRL_PRESSED)
                    != 0;
                let key = match event.wVirtualKeyCode {
                    0x26 if !modified => SelectionKey::Up,
                    0x28 if !modified => SelectionKey::Down,
                    0x0d if !modified => SelectionKey::Enter,
                    0x1b => SelectionKey::Escape,
                    0x43 if modified => SelectionKey::Escape,
                    _ => SelectionKey::Other,
                };
                if let Some(result) = selection.key(key) {
                    return Ok(result);
                }
                if key != SelectionKey::Other {
                    break;
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli16_selection_wraps_cancels_and_never_accepts_other_keys() {
        let mut s = Selection::new(3, 0).unwrap();
        assert_eq!(s.key(SelectionKey::Up), None);
        assert_eq!(s.index(), 2);
        s.key(SelectionKey::Down);
        assert_eq!(s.index(), 0);
        assert_eq!(s.key(SelectionKey::Other), None);
        assert_eq!(s.key(SelectionKey::Enter), Some(Some(0)));
        assert_eq!(s.key(SelectionKey::Escape), Some(None));
        assert_eq!(Selection::new(0, 0), Err(ErrorCode::InvalidInput));
    }
}
