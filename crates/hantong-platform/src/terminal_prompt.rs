//! Deadline-aware public metadata input. Secrets must use protected capture.
use hantong_contracts::ErrorCode;
use std::time::Instant;

pub fn read_line_until(limit: usize, deadline: Instant) -> Result<String, ErrorCode> {
    if limit == 0 || limit > 4096 {
        return Err(ErrorCode::InvalidInput);
    }
    native_read(limit, deadline)
}

#[cfg(unix)]
fn native_read(limit: usize, deadline: Instant) -> Result<String, ErrorCode> {
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open("/dev/tty")
        .map_err(|_| ErrorCode::HumanStepRequired)?;
    if unsafe { libc::isatty(tty.as_raw_fd()) } != 1 {
        return Err(ErrorCode::HumanStepRequired);
    }
    read_fd(tty.as_raw_fd(), limit, deadline)
}

#[cfg(unix)]
fn read_fd(fd: std::os::fd::RawFd, limit: usize, deadline: Instant) -> Result<String, ErrorCode> {
    let mut bytes = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(ErrorCode::ApprovalExpired)?;
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe {
            libc::poll(
                &mut poll,
                1,
                remaining.as_millis().clamp(1, i32::MAX as u128) as i32,
            )
        };
        if result == 0 {
            return Err(ErrorCode::ApprovalExpired);
        }
        if result < 0 {
            if matches!(
                std::io::Error::last_os_error().kind(),
                std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(ErrorCode::CoreUnavailable);
        }
        let mut byte = 0u8;
        let count = unsafe { libc::read(fd, (&mut byte as *mut u8).cast(), 1) };
        if count == 0 {
            return Err(ErrorCode::ApprovalDenied);
        }
        if count < 0 {
            if matches!(
                std::io::Error::last_os_error().kind(),
                std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(ErrorCode::CoreUnavailable);
        }
        if byte == b'\n' {
            break;
        }
        if bytes.len() >= limit {
            return Err(ErrorCode::InvalidInput);
        }
        bytes.push(byte);
    }
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    String::from_utf8(bytes).map_err(|_| ErrorCode::InvalidInput)
}

#[cfg(windows)]
fn native_read(limit: usize, deadline: Instant) -> Result<String, ErrorCode> {
    use std::io::{IsTerminal, Write};
    use windows_sys::Win32::{
        Foundation::{INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        System::{Console::*, Threading::WaitForSingleObject},
    };
    if !std::io::stdin().is_terminal() {
        return Err(ErrorCode::HumanStepRequired);
    }
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(ErrorCode::HumanStepRequired);
    }
    let mut original = 0;
    if unsafe { GetConsoleMode(handle, &mut original) } == 0 {
        return Err(ErrorCode::HumanStepRequired);
    }
    struct Restore(windows_sys::Win32::Foundation::HANDLE, u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe { SetConsoleMode(self.0, self.1) };
        }
    }
    if unsafe {
        SetConsoleMode(
            handle,
            original & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT),
        )
    } == 0
    {
        return Err(ErrorCode::HumanStepRequired);
    }
    let _restore = Restore(handle, original);
    let mut units = Vec::new();
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or(ErrorCode::ApprovalExpired)?;
        match unsafe {
            WaitForSingleObject(
                handle,
                remaining.as_millis().clamp(1, u32::MAX as u128 - 1) as u32,
            )
        } {
            WAIT_TIMEOUT => return Err(ErrorCode::ApprovalExpired),
            WAIT_OBJECT_0 => {}
            _ => return Err(ErrorCode::CoreUnavailable),
        }
        let mut event = unsafe { std::mem::zeroed::<INPUT_RECORD>() };
        let mut count = 0;
        if unsafe { ReadConsoleInputW(handle, &mut event, 1, &mut count) } == 0 {
            return Err(ErrorCode::CoreUnavailable);
        }
        if count != 1 || u32::from(event.EventType) != KEY_EVENT {
            continue;
        }
        let key = unsafe { event.Event.KeyEvent };
        if key.bKeyDown == 0 {
            continue;
        }
        let value = unsafe { key.uChar.UnicodeChar };
        if value == 3 || value == 27 {
            return Err(ErrorCode::ApprovalDenied);
        }
        if value == 13 {
            let _ = writeln!(std::io::stderr());
            break;
        }
        if value == 8 {
            if units.pop().is_some() {
                let _ = write!(std::io::stderr(), "\x08 \x08");
            }
        } else if value >= 32 {
            if units.len() >= limit {
                return Err(ErrorCode::InvalidInput);
            }
            units.push(value);
            let _ = write!(std::io::stderr(), "{}", String::from_utf16_lossy(&[value]));
        }
        std::io::stderr()
            .flush()
            .map_err(|_| ErrorCode::CoreUnavailable)?;
    }
    let value = String::from_utf16(&units).map_err(|_| ErrorCode::InvalidInput)?;
    if value.len() > limit {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(value)
}

#[cfg(not(any(unix, windows)))]
fn native_read(_: usize, _: Instant) -> Result<String, ErrorCode> {
    Err(ErrorCode::CapabilityUnsupported)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        io::Write,
        os::{fd::AsRawFd, unix::net::UnixStream},
        time::Duration,
    };
    #[test]
    fn sec03_prompt_deadline_eof_and_bounds() {
        let (a, mut b) = UnixStream::pair().unwrap();
        assert_eq!(
            read_fd(a.as_raw_fd(), 8, Instant::now() + Duration::from_millis(10)),
            Err(ErrorCode::ApprovalExpired)
        );
        b.write_all(b"ready\r\n").unwrap();
        assert_eq!(
            read_fd(a.as_raw_fd(), 8, Instant::now() + Duration::from_secs(1)),
            Ok("ready".into())
        );
        b.write_all(b"oversize\n").unwrap();
        assert_eq!(
            read_fd(a.as_raw_fd(), 2, Instant::now() + Duration::from_secs(1)),
            Err(ErrorCode::InvalidInput)
        );
        let (c, d) = UnixStream::pair().unwrap();
        drop(d);
        assert_eq!(
            read_fd(c.as_raw_fd(), 8, Instant::now() + Duration::from_secs(1)),
            Err(ErrorCode::ApprovalDenied)
        );
    }
}
