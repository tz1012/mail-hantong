//! Same-terminal setup for a generic IMAP mailbox and its SMTP submission path.
//!
//! The CLI collects endpoint metadata only. Credentials are requested and held by
//! the Core-owned protected helper during terminal review.

use super::{
    Lang,
    onboarding::{self, Io},
};
use hantong_contracts::{
    ErrorCode,
    imap_setup::{ImapSetupCommand, ImapSetupInput, ImapSetupState, ImapSetupTls},
    smtp_setup::{SmtpSetupCommand, SmtpSetupInput, SmtpSetupState},
    terminal_review::TerminalReviewKind,
};
use std::{thread, time::Duration};

fn prompt(io: &mut dyn Io, lang: Lang, key: &str) -> Result<String, ErrorCode> {
    onboarding::say(io, lang, key)?;
    io.read_text_line(512)?.ok_or(ErrorCode::HumanStepRequired)
}

fn endpoint(
    io: &mut dyn Io,
    lang: Lang,
    host_key: &str,
    port_key: &str,
    tls_key: &str,
    login_key: &str,
) -> Result<(String, u16, ImapSetupTls, String), ErrorCode> {
    let host = prompt(io, lang, host_key)?;
    let port = prompt(io, lang, port_key)?
        .parse()
        .map_err(|_| ErrorCode::InvalidInput)?;
    let tls = match prompt(io, lang, tls_key)?.as_str() {
        "implicit_tls" | "1" => ImapSetupTls::ImplicitTls,
        "starttls_required" | "2" => ImapSetupTls::StarttlsRequired,
        _ => return Err(ErrorCode::InvalidInput),
    };
    let login = prompt(io, lang, login_key)?;
    Ok((host, port, tls, login))
}

fn imap_setup_error(state: ImapSetupState) -> ErrorCode {
    match state {
        ImapSetupState::Expired => ErrorCode::ApprovalExpired,
        ImapSetupState::Denied => ErrorCode::ApprovalDenied,
        ImapSetupState::Cancelled
        | ImapSetupState::Interrupted
        | ImapSetupState::Pending
        | ImapSetupState::Connecting => ErrorCode::ApprovalRequired,
        ImapSetupState::AuthRequired => ErrorCode::AuthRequired,
        ImapSetupState::TlsFailed | ImapSetupState::Offline => ErrorCode::Offline,
        ImapSetupState::Error => ErrorCode::InternalError,
        ImapSetupState::Ready => ErrorCode::CoreUnavailable,
    }
}

fn smtp(input: SmtpSetupInput, lang: Lang) -> Result<(), ErrorCode> {
    let receipt = hantong_ipc_client::smtp_setup::exchange(SmtpSetupCommand::Request(input))?;
    let receipt = if receipt.state == SmtpSetupState::Pending {
        super::terminal_workflows::maybe_review(
            TerminalReviewKind::SmtpSetup,
            receipt.request_id,
            lang,
        )?;
        let mut status =
            hantong_ipc_client::smtp_setup::exchange(SmtpSetupCommand::Status(receipt.request_id))?;
        for _ in 0..300 {
            if status.state != SmtpSetupState::Pending {
                break;
            }
            thread::sleep(Duration::from_secs(1));
            status = hantong_ipc_client::smtp_setup::exchange(SmtpSetupCommand::Status(
                receipt.request_id,
            ))?;
        }
        status
    } else {
        receipt
    };
    if receipt.state == SmtpSetupState::Ready {
        return Ok(());
    }
    Err(receipt.error_code.unwrap_or(ErrorCode::CoreUnavailable))
}

/// Authenticate the protected default profile before opening an existing IMAP
/// account. Account-scoped read permission is enforced again by each TUI call.
pub(super) fn verify_existing() -> Result<(), ErrorCode> {
    hantong_ipc_client::verify_default()
}

pub(super) fn ensure_default_scope(account: &str, lang: Lang) -> Result<(), ErrorCode> {
    let already_ready =
        match hantong_ipc_client::google_mail::verify_default_manage_accounts(account) {
            Ok(value) => value,
            Err(ErrorCode::ClientNotRegistered) => false,
            Err(error) => return Err(error),
        };
    if already_ready {
        return Ok(());
    }
    match hantong_ipc_client::verify_default() {
        Ok(()) => {}
        Err(ErrorCode::ClientNotRegistered) => {
            super::terminal_workflows::registration(
                hantong_contracts::registration::RegistrationCommand::Request,
                lang,
            )?;
        }
        Err(error) => return Err(error),
    }
    let selected = hantong_ipc_client::google_mail::default_registered_account_union(account)?;
    let google_accounts = selected
        .iter()
        .filter(|id| id.starts_with("google_"))
        .cloned()
        .collect::<Vec<_>>();
    super::google::ensure_manage_google_accounts(&google_accounts, lang)?;
    super::terminal_workflows::scope(
        hantong_contracts::scope_refresh::ScopeRefreshCommand::RequestManage(
            hantong_contracts::scope_refresh::ManageAccounts::parse(&selected)?,
        ),
        lang,
    )?;
    if hantong_ipc_client::google_mail::verify_default_manage_accounts(account)? {
        Ok(())
    } else {
        Err(ErrorCode::ScopeDenied)
    }
}

pub(super) fn run(lang: Lang, io: &mut dyn Io) -> Result<String, ErrorCode> {
    onboarding::say(io, lang, "imap_setup_title")?;
    let (host, port, tls, login) =
        endpoint(io, lang, "imap_host", "imap_port", "imap_tls", "imap_login")?;
    let display_name = format!("{login} @ {host}").chars().take(128).collect();
    let input = ImapSetupInput::new(Some(display_name), None, host, port, tls, login)?;
    let receipt = super::terminal_workflows::imap(ImapSetupCommand::Request(input), lang)?;
    if receipt.state() != ImapSetupState::Ready {
        return Err(imap_setup_error(receipt.state()));
    }
    let account = receipt
        .account_id()
        .ok_or(ErrorCode::CoreUnavailable)?
        .to_owned();
    onboarding::say(io, lang, "imap_ready")?;

    let (host, port, tls, login) =
        endpoint(io, lang, "smtp_host", "smtp_port", "smtp_tls", "smtp_login")?;
    let sender = prompt(io, lang, "smtp_sender")?;
    let smtp_input = SmtpSetupInput::new_with_sender(
        account.clone(),
        host,
        port,
        tls,
        login,
        (!sender.is_empty()).then_some(sender),
    )?;
    smtp(smtp_input, lang)?;
    onboarding::say(io, lang, "smtp_ready")?;
    ensure_default_scope(&account, lang)?;
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::imap_setup_error;
    use hantong_contracts::{ErrorCode, imap_setup::ImapSetupState};

    #[test]
    fn imap_setup_states_keep_stable_cli_errors() {
        let cases = [
            (ImapSetupState::Expired, ErrorCode::ApprovalExpired),
            (ImapSetupState::Denied, ErrorCode::ApprovalDenied),
            (ImapSetupState::Cancelled, ErrorCode::ApprovalRequired),
            (ImapSetupState::Interrupted, ErrorCode::ApprovalRequired),
            (ImapSetupState::Pending, ErrorCode::ApprovalRequired),
            (ImapSetupState::Connecting, ErrorCode::ApprovalRequired),
            (ImapSetupState::AuthRequired, ErrorCode::AuthRequired),
            (ImapSetupState::TlsFailed, ErrorCode::Offline),
            (ImapSetupState::Offline, ErrorCode::Offline),
            (ImapSetupState::Error, ErrorCode::InternalError),
        ];
        for (state, expected) in cases {
            assert_eq!(imap_setup_error(state), expected);
        }
    }
}
