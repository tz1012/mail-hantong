//! Desktop presentation and its isolated native window boundary.
#![deny(unsafe_code)]

mod model;
mod workspace;
pub use model::{
    CoreStatus, EffectiveTheme, Language, Model, Navigation, Palette, Rgb, ShellOptions, Theme,
    palette, text,
};
pub use workspace::{MailAccount, MailFolder, MailMessage, MailWorkspace, PageCursor};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MailAction {
    Connect,
    RegistrationStatus,
    Accounts,
    Folders {
        account_id: String,
        cursor: Option<String>,
    },
    Messages {
        account_id: String,
        folder_wire_hex: String,
        encoding: String,
        filter: hantong_contracts::message_search::ImapMetadataFilter,
        cursor: Option<String>,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MailActionResult {
    Registration(hantong_contracts::registration::RegistrationProgress),
    Accounts(Vec<MailAccount>),
    Folders {
        rows: Vec<MailFolder>,
        next_cursor: Option<String>,
    },
    Messages {
        rows: Vec<MailMessage>,
        next_cursor: Option<PageCursor>,
        observed: bool,
    },
}
pub type ActionCallback =
    Arc<dyn Fn(MailAction) -> Result<MailActionResult, ErrorCode> + Send + Sync>;

#[cfg(windows)]
#[allow(unsafe_code)]
mod native;

use hantong_contracts::ErrorCode;
use std::sync::Arc;

/// Optional explicit user-action bridge. The native shell treats results as
/// display data; it never creates approval or authentication objects itself.
#[derive(Clone, Default)]
pub struct ShellCallbacks {
    pub action: Option<ActionCallback>,
}

/// Displays a desktop client without creating a registration or mail authority.
pub fn run(
    options: ShellOptions,
    probe: Arc<dyn Fn() -> CoreStatus + Send + Sync>,
) -> Result<(), ErrorCode> {
    run_with_callbacks(options, probe, ShellCallbacks::default())
}
pub fn run_with_callbacks(
    options: ShellOptions,
    probe: Arc<dyn Fn() -> CoreStatus + Send + Sync>,
    callbacks: ShellCallbacks,
) -> Result<(), ErrorCode> {
    #[cfg(windows)]
    {
        native::run(options, probe, callbacks)
    }
    #[cfg(not(windows))]
    {
        let _ = (options, probe, callbacks);
        Err(ErrorCode::CapabilityUnsupported)
    }
}
