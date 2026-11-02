//! Interactive wrappers around pending requests; only Core owns confirmation.
use super::{Lang, core_start};
use hantong_contracts::{ErrorCode, terminal_review::TerminalReviewKind};

fn require_interactive() -> Result<(), ErrorCode> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal()
        || !std::io::stdout().is_terminal()
        || std::env::args_os().any(|v| v == "--json" || v == "--no-input")
    {
        return Err(ErrorCode::HumanStepRequired);
    }
    Ok(())
}

pub(super) fn maybe_review(
    kind: TerminalReviewKind,
    id: [u8; 32],
    lang: Lang,
) -> Result<(), ErrorCode> {
    require_interactive()?;
    core_start::review(kind, id, None, None, lang)
}

pub(super) fn registration(
    command: hantong_contracts::registration::RegistrationCommand,
    lang: Lang,
) -> Result<hantong_contracts::registration::RegistrationReceipt, ErrorCode> {
    use hantong_contracts::registration::{RegistrationCommand as C, RegistrationProgress as P};
    // GUI registration belongs to the GUI's explicit connection action. It
    // must never become a native-window fallback from this CLI wrapper.
    if matches!(command, C::GuiRequest) {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    if !matches!(command, C::Status(_) | C::McpStatus(_) | C::GuiStatus(_)) {
        require_interactive()?;
    }
    let receipt = hantong_ipc_client::registration::exchange(command)?;
    if !matches!(command, C::Status(_) | C::McpStatus(_) | C::GuiStatus(_))
        && receipt.state() == P::Pending
    {
        maybe_review(
            TerminalReviewKind::Registration,
            *receipt.request_id(),
            lang,
        )?;
        let status = match command {
            C::McpRequest => C::McpStatus(*receipt.request_id()),
            C::GuiRequest => C::GuiStatus(*receipt.request_id()),
            _ => C::Status(*receipt.request_id()),
        };
        return hantong_ipc_client::registration::exchange(status);
    }
    Ok(receipt)
}

pub(super) fn scope(
    command: hantong_contracts::scope_refresh::ScopeRefreshCommand,
    lang: Lang,
) -> Result<hantong_contracts::scope_refresh::ScopeRefreshReceipt, ErrorCode> {
    use hantong_contracts::scope_refresh::{ScopeRefreshCommand as C, ScopeRefreshProgress as P};
    if !matches!(command, C::Status(_)) {
        require_interactive()?;
    }
    let receipt = hantong_ipc_client::scope_refresh::exchange(command)?;
    if !matches!(command, C::Status(_)) && receipt.state() == P::Pending {
        maybe_review(
            TerminalReviewKind::ScopeRefresh,
            *receipt.refresh_id(),
            lang,
        )?;
        return hantong_ipc_client::scope_refresh::exchange(C::Status(*receipt.refresh_id()));
    }
    Ok(receipt)
}

pub(super) fn imap(
    command: hantong_contracts::imap_setup::ImapSetupCommand,
    lang: Lang,
) -> Result<hantong_contracts::imap_setup::ImapSetupReceipt, ErrorCode> {
    use hantong_contracts::imap_setup::{ImapSetupCommand as C, ImapSetupState as S};
    let is_status = matches!(command, C::Status(_));
    if !is_status {
        require_interactive()?;
    }
    let receipt = hantong_ipc_client::imap_setup::exchange(command)?;
    if !is_status && receipt.state() == S::Pending {
        maybe_review(TerminalReviewKind::ImapSetup, *receipt.request_id(), lang)?;
        let mut response =
            hantong_ipc_client::imap_setup::exchange(C::Status(*receipt.request_id()))?;
        for _ in 0..300 {
            if !matches!(response.state(), S::Pending | S::Connecting) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
            response = hantong_ipc_client::imap_setup::exchange(C::Status(*receipt.request_id()))?;
        }
        return Ok(response);
    }
    Ok(receipt)
}
