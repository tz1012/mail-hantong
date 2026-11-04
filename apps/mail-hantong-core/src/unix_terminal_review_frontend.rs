//! Unix foreground terminal review.
//!
//! This is presentation only. Ticket creation, expiry, scope and the final
//! Store transition remain Core-owned through the terminal-review wire.

use hantong_contracts::{ErrorCode, terminal_review::*, wire_hex};
use std::io::{self, IsTerminal, Write};

pub(crate) fn run(args: &[std::ffi::OsString]) -> Result<(), ErrorCode> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(ErrorCode::HumanStepRequired);
    }
    let request = parse(args)?;
    let lang = if args
        .windows(2)
        .any(|pair| pair[0] == "--lang" && pair[1] == "ko")
    {
        crate::Lang::Ko
    } else {
        crate::Lang::En
    };
    let receipt = hantong_ipc_client::terminal_review::exchange(&TerminalReviewCommand::Get(
        request.clone(),
    ))?;
    receipt.validate()?;
    println!(
        "{}: {}",
        crate::resource(lang, "terminal_review_title"),
        receipt.kind.as_str()
    );
    for field in &receipt.fields {
        println!(
            "{}: {}",
            terminal_safe(&field.label),
            terminal_safe(&field.value)
        );
    }
    let Some(ticket) = receipt.ticket else {
        return Err(ErrorCode::ApprovalRequired);
    };
    print!("{}", crate::resource(lang, "terminal_review_confirm"));
    io::stdout()
        .flush()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if answer.trim() != "1" {
        let result = hantong_ipc_client::terminal_review::exchange(
            &TerminalReviewCommand::Cancel(TerminalReviewCancel::new(request, ticket)?),
        )?;
        result.validate()?;
        return Ok(());
    }
    if receipt.secret_ticket_opaque.is_some() {
        let opaque = receipt
            .secret_ticket_opaque
            .as_deref()
            .ok_or(ErrorCode::CoreUnavailable)?;
        let secret_ticket = hantong_secrets::TerminalSecretTicket::from_opaque(opaque)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        let directories = hantong_platform::app_directories()?;
        let handle = capture_secret(&directories.secrets, &secret_ticket)
            .map_err(|_| ErrorCode::ApprovalRequired)?;
        let submit = match TerminalReviewSecretSubmit::new(
            ticket,
            receipt.snapshot_hash,
            opaque.to_vec(),
            handle.opaque_id(),
        ) {
            Ok(request) => request,
            Err(error) => {
                let _ = cancel_secret(&directories.secrets, &secret_ticket, handle.opaque_id());
                return Err(error);
            }
        };
        let submitted = match hantong_ipc_client::terminal_review::exchange(
            &TerminalReviewCommand::SecretSubmit(submit),
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = cancel_secret(&directories.secrets, &secret_ticket, handle.opaque_id());
                return Err(error);
            }
        };
        if let Err(error) = submitted.validate() {
            let _ = cancel_secret(&directories.secrets, &secret_ticket, handle.opaque_id());
            return Err(error);
        }
        if !matches!(submitted.state.as_str(), "ready" | "provisioning") {
            let _ = cancel_secret(&directories.secrets, &secret_ticket, handle.opaque_id());
            return Err(ErrorCode::CoreUnavailable);
        }
        return Ok(());
    }
    let command =
        TerminalReviewCommand::Confirm(TerminalReviewConfirm::new(ticket, receipt.snapshot_hash)?);
    let result = hantong_ipc_client::terminal_review::exchange(&command)?;
    result.validate()?;
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn capture_secret(
    root: &std::path::Path,
    ticket: &hantong_secrets::TerminalSecretTicket,
) -> Result<hantong_secrets::TerminalSecretHandle, hantong_secrets::SecretError> {
    hantong_secrets::SecretStore::capture_terminal_secret_at(root, ticket)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn capture_secret(
    root: &std::path::Path,
    ticket: &hantong_secrets::TerminalSecretTicket,
) -> Result<hantong_secrets::TerminalSecretHandle, hantong_secrets::SecretError> {
    let secrets =
        hantong_secrets::SecretStore::open(root).map_err(|_| hantong_secrets::SecretError::Io)?;
    secrets.capture_terminal_secret(ticket)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn cancel_secret(
    root: &std::path::Path,
    ticket: &hantong_secrets::TerminalSecretTicket,
    opaque_id: [u8; 32],
) -> Result<(), hantong_secrets::SecretError> {
    hantong_secrets::SecretStore::cancel_terminal_secret_at(root, ticket, opaque_id)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn cancel_secret(
    _root: &std::path::Path,
    _ticket: &hantong_secrets::TerminalSecretTicket,
    _opaque_id: [u8; 32],
) -> Result<(), hantong_secrets::SecretError> {
    Ok(())
}

fn parse(args: &[std::ffi::OsString]) -> Result<TerminalReviewGet, ErrorCode> {
    if args.is_empty() || args.len() > 10 || !args.len().is_multiple_of(2) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut kind = None;
    let mut request_id = None;
    let mut account_id = None;
    let mut payload_hash = None;
    let mut i = 0;
    while i < args.len() {
        let key = args[i].to_str().ok_or(ErrorCode::InvalidInput)?;
        let value = args[i + 1].to_str().ok_or(ErrorCode::InvalidInput)?;
        match key {
            "--kind" => kind = Some(value.to_owned()),
            "--request-id" => request_id = Some(wire_hex::decode_fixed(value)?),
            "--account-id" => account_id = Some(value.to_owned()),
            "--payload-hash" => payload_hash = Some(wire_hex::decode_fixed(value)?),
            "--lang" => {
                if value != "en" && value != "ko" {
                    return Err(ErrorCode::InvalidInput);
                }
            }
            _ => return Err(ErrorCode::InvalidInput),
        }
        i += 2;
    }
    TerminalReviewGet::new(
        TerminalReviewKind::parse(kind.as_deref().ok_or(ErrorCode::InvalidInput)?)?,
        request_id.ok_or(ErrorCode::InvalidInput)?,
        account_id,
        payload_hash.unwrap_or([0; 32]),
    )
}

fn terminal_safe(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\u{1b}' => out.push_str("\\x1b"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            '\u{202e}' => out.push_str("\\u202e"),
            '\r' => out.push_str("\\r"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:04x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
