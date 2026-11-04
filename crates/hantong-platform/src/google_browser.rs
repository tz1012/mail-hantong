use hantong_contracts::ErrorCode;
pub fn validate_google_authorization_url(url: &str) -> Result<(), ErrorCode> {
    if url.len() > 8192
        || !url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?")
        || url
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\'' | '\\' | '#'))
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}
pub fn open_google_authorization_url(url: &str) -> Result<(), ErrorCode> {
    validate_google_authorization_url(url)?;
    #[cfg(windows)]
    {
        native::open(url)
    }
    #[cfg(target_os = "linux")]
    {
        unix::open(url)
    }
    #[cfg(target_os = "macos")]
    {
        unix::open(url)
    }
    #[cfg(all(not(windows), not(any(target_os = "linux", target_os = "macos"))))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(windows)]
#[allow(unsafe_code)]
mod native {
    use super::*;
    pub(super) fn open(url: &str) -> Result<(), ErrorCode> {
        let wide: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
        let action: Vec<u16> = "open\0".encode_utf16().collect();
        let result = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                action.as_ptr(),
                wide.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result as isize <= 32 {
            Err(ErrorCode::HumanStepRequired)
        } else {
            Ok(())
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use super::*;
    use std::process::Command;

    pub(super) fn open(url: &str) -> Result<(), ErrorCode> {
        #[cfg(target_os = "linux")]
        const LAUNCHER: &str = "/usr/bin/xdg-open";
        #[cfg(target_os = "macos")]
        const LAUNCHER: &str = "/usr/bin/open";

        if !std::path::Path::new(LAUNCHER).is_file() {
            return Err(ErrorCode::HumanStepRequired);
        }
        Command::new(LAUNCHER)
            .arg(url)
            .status()
            .map_err(|_| ErrorCode::HumanStepRequired)?
            .success()
            .then_some(())
            .ok_or(ErrorCode::HumanStepRequired)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acc02_browser_rejects_other_authorities_schemes_and_control_data() {
        for s in [
            "http://accounts.google.com/o/oauth2/v2/auth?x=1",
            "https://accounts.google.com.evil/o/oauth2/v2/auth?x=1",
            "javascript:alert(1)",
            "https://accounts.google.com/o/oauth2/v2/auth?x=\r\n",
        ] {
            assert!(validate_google_authorization_url(s).is_err())
        }
    }
}
