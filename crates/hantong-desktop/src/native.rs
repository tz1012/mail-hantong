use crate::{
    CoreStatus, EffectiveTheme, Language, MailAction, MailActionResult, MailWorkspace, Model,
    Navigation, ShellCallbacks, ShellOptions, Theme, palette, text,
};
use hantong_contracts::ErrorCode;
use std::sync::Arc;

mod appearance;
mod probe;
mod window;

pub(super) fn run(
    options: ShellOptions,
    probe: Arc<dyn Fn() -> CoreStatus + Send + Sync>,
    callbacks: ShellCallbacks,
) -> Result<(), ErrorCode> {
    let result = window::run(options, probe, callbacks);
    if let Err(error) = result {
        let language = options.language.unwrap_or(if appearance::os_korean() {
            Language::Korean
        } else {
            Language::English
        });
        let message = windows::core::HSTRING::from(format!(
            "{}\n\n{}",
            text(language, "startup_error"),
            error.as_str()
        ));
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                None,
                &message,
                windows::core::w!("mail-hantong"),
                windows::Win32::UI::WindowsAndMessaging::MB_OK
                    | windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
            );
        }
    }
    result
}
