//! Inherited-console terminal review frontend.
//!
//! This module owns presentation and the final Confirm/Cancel choice only.
//! Core remains the authority for the typed request, ticket, expiry, and
//! transaction.  In particular, this module never manufactures a ticket or a
//! secret-capture capability.

use crate::terminal_secret_input;
use hantong_contracts::{
    ErrorCode,
    terminal_review::{
        TerminalReviewCommand, TerminalReviewGet, TerminalReviewKind, TerminalReviewReceipt,
    },
    wire_hex,
};
use std::ffi::OsString;
use std::io::{self, IsTerminal, Read, Write};

const MAX_ARGS: usize = 11;
const TERMINAL_EN: &str = include_str!("../resources/terminal-en.txt");
const TERMINAL_KO: &str = include_str!("../resources/terminal-ko.txt");

fn resource(lang: bool, key: &str) -> &'static str {
    let source = if lang { TERMINAL_KO } else { TERMINAL_EN };
    source
        .lines()
        .find_map(|line| line.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
        .unwrap_or("")
}

/// Runs one Core-owned review in the already inherited foreground terminal.
///
/// The caller passes only the review arguments; this function does not start a
/// Core, attach to a console, or read any credential. EOF is treated exactly as
/// Cancel. Secret setup kinds are rejected until the Core supplies their
/// separate typed secret ticket/status flow.
pub fn run(args: &[OsString], korean: bool) -> Result<(), ErrorCode> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(ErrorCode::InvalidInput);
    }
    let parsed = parse_args_with_default(args, korean)?;
    let request = parsed.request;
    let korean = parsed.korean;
    if request.kind == TerminalReviewKind::GoogleSetup
        && google_client_capture(request.request_id, korean)?
    {
        return Ok(());
    }
    let receipt = hantong_ipc_client::terminal_review::exchange(&TerminalReviewCommand::Get(
        request.clone(),
    ))?;
    receipt.validate()?;
    render_receipt(&receipt, korean)?;
    if receipt.ticket.is_none() || !confirm(korean)? {
        cancel(request, receipt.ticket, korean)?;
        return Ok(());
    }
    let ticket = receipt.ticket.ok_or(ErrorCode::CoreUnavailable)?;
    if matches!(
        receipt.kind,
        TerminalReviewKind::ImapSetup | TerminalReviewKind::SmtpSetup
    ) {
        let Some(secret_ticket_opaque) = receipt.secret_ticket_opaque.clone() else {
            cancel(request, Some(ticket), korean)?;
            return Err(ErrorCode::CapabilityUnsupported);
        };
        let directories = hantong_platform::app_directories()?;
        let secrets = hantong_secrets::SecretStore::open(&directories.secrets)
            .map_err(|_| ErrorCode::CapabilityUnsupported)?;
        let secret_ticket = match terminal_secret_input::ticket_from_opaque(&secret_ticket_opaque) {
            Ok(value) => value,
            Err(_) => {
                cancel(request, Some(ticket), korean)?;
                return Err(ErrorCode::ScopeDenied);
            }
        };
        println!("{}", resource(korean, "secret_prompt"));
        let handle = match terminal_secret_input::capture(&secrets, &secret_ticket) {
            Ok(value) => value,
            Err(_) => {
                cancel(request, Some(ticket), korean)?;
                return Err(ErrorCode::ScopeDenied);
            }
        };
        let staged_handle = terminal_secret_input::opaque_handle(&handle);
        let submitted =
            hantong_ipc_client::terminal_review::exchange(&TerminalReviewCommand::SecretSubmit(
                hantong_contracts::terminal_review::TerminalReviewSecretSubmit::new(
                    ticket,
                    receipt.snapshot_hash,
                    secret_ticket_opaque,
                    staged_handle,
                )?,
            ))?;
        return render_result(&submitted, korean);
    }
    let confirmed =
        hantong_ipc_client::terminal_review::exchange(&TerminalReviewCommand::Confirm(
            hantong_contracts::terminal_review::TerminalReviewConfirm::new(
                ticket,
                receipt.snapshot_hash,
            )?,
        ))?;
    render_result(&confirmed, korean)
}

fn google_client_capture(id: [u8; 32], korean: bool) -> Result<bool, ErrorCode> {
    use hantong_contracts::google_setup::GoogleSetupCommand as C;
    let status = hantong_ipc_client::google_setup::exchange(C::Status(id))?;
    if status.state != "awaiting_client" {
        return Ok(false);
    }
    println!("{}", resource(korean, "google_client"));
    if !confirm(korean)? {
        return Err(ErrorCode::ApprovalDenied);
    }
    let issued = hantong_ipc_client::google_setup::exchange(C::ClientTicket(id))?;
    if issued.state != "awaiting_client" {
        return Err(ErrorCode::Conflict);
    }
    let opaque = issued.client_ticket.ok_or(ErrorCode::CoreUnavailable)?;
    let ticket = terminal_secret_input::ticket_from_opaque(&opaque)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let directories = hantong_platform::app_directories()?;
    let secrets = hantong_secrets::SecretStore::open(&directories.secrets)
        .map_err(|_| ErrorCode::InternalError)?;
    println!("{}", resource(korean, "secret_prompt"));
    let handle = terminal_secret_input::capture(&secrets, &ticket)
        .map_err(|_| ErrorCode::ApprovalRequired)?;
    let staged = hantong_ipc_client::google_setup::exchange(C::StageClient {
        request_id: id,
        ticket: opaque,
        staged_handle: terminal_secret_input::opaque_handle(&handle),
    })?;
    println!(
        "{}: {}",
        resource(korean, "result_label"),
        terminal_safe(&staged.state)
    );
    Ok(true)
}

struct ParsedArgs {
    request: TerminalReviewGet,
    korean: bool,
}

#[cfg(test)]
fn parse_args(args: &[OsString]) -> Result<ParsedArgs, ErrorCode> {
    parse_args_with_default(args, false)
}

fn parse_args_with_default(
    args: &[OsString],
    default_korean: bool,
) -> Result<ParsedArgs, ErrorCode> {
    if args.is_empty() || args.len() > MAX_ARGS || !args.len().is_multiple_of(2) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut kind = None;
    let mut request_id = None;
    let mut account_id = None;
    let mut payload_hash = None;
    let mut language = None;
    let mut i = 0;
    while i < args.len() {
        let key = args[i].to_str().ok_or(ErrorCode::InvalidInput)?;
        let value = args[i + 1].to_str().ok_or(ErrorCode::InvalidInput)?;
        let slot = match key {
            "--kind" => &mut kind,
            "--request-id" => &mut request_id,
            "--account-id" => &mut account_id,
            "--payload-hash" => &mut payload_hash,
            "--lang" => &mut language,
            _ => return Err(ErrorCode::InvalidInput),
        };
        if slot.replace(value.to_owned()).is_some() {
            return Err(ErrorCode::InvalidInput);
        }
        i += 2;
    }
    let kind = TerminalReviewKind::parse(kind.as_deref().ok_or(ErrorCode::InvalidInput)?)?;
    let request_id = decode(request_id.as_deref().ok_or(ErrorCode::InvalidInput)?)?;
    let korean = match language.as_deref() {
        Some("ko") => true,
        Some("en") => false,
        Some(_) => return Err(ErrorCode::InvalidInput),
        None => default_korean,
    };
    let payload_hash = match payload_hash {
        Some(value) => decode(&value)?,
        None => [0; 32],
    };
    Ok(ParsedArgs {
        request: TerminalReviewGet::new(kind, request_id, account_id, payload_hash)?,
        korean,
    })
}

fn decode(value: &str) -> Result<[u8; 32], ErrorCode> {
    wire_hex::decode_fixed(value).map_err(|_| ErrorCode::InvalidInput)
}

fn render_receipt(receipt: &TerminalReviewReceipt, korean: bool) -> Result<(), ErrorCode> {
    let mut out = io::stdout().lock();
    let title = if korean {
        "터미널 확인"
    } else {
        "Terminal review"
    };
    writeln!(out, "{title}: {}", receipt.kind.as_str()).map_err(|_| ErrorCode::CoreUnavailable)?;
    for field in &receipt.fields {
        writeln!(
            out,
            "{}: {}",
            terminal_safe(&field.label),
            terminal_safe(localized_field_value(&field.label, &field.value, korean))
        )
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    }
    out.flush().map_err(|_| ErrorCode::CoreUnavailable)
}

fn localized_field_value<'a>(label: &str, value: &'a str, korean: bool) -> &'a str {
    match (label, value) {
        ("action", "move_to_trash") => resource(korean, "trash_action"),
        ("feature", "Connect all listed accounts (explicit confirmation required)") => {
            resource(korean, "ai_feature")
        }
        (
            "mail_transfer",
            "selected account mail metadata/body may be sent to the selected AI provider",
        ) => resource(korean, "ai_mail_transfer"),
        _ => value,
    }
}

#[allow(unused_variables)]
fn render_result(receipt: &TerminalReviewReceipt, korean: bool) -> Result<(), ErrorCode> {
    let mut out = io::stdout().lock();
    let localized_label = resource(korean, "result_label");
    let label = if korean { "처리 결과" } else { "Result" };
    writeln!(out, "{localized_label}: {}", terminal_safe(&receipt.state))
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    out.flush().map_err(|_| ErrorCode::CoreUnavailable)
}

#[allow(unused_variables)]
fn confirm(korean: bool) -> Result<bool, ErrorCode> {
    let mut out = io::stdout().lock();
    let localized_prompt = resource(korean, "confirm_prompt");
    let prompt = if korean {
        "내용을 확인했으면 1을 입력하세요. 취소하려면 0을 입력하세요: "
    } else {
        "Enter 1 to confirm this operation, or 0 to cancel: "
    };
    write!(out, "{localized_prompt}").map_err(|_| ErrorCode::CoreUnavailable)?;
    out.flush().map_err(|_| ErrorCode::CoreUnavailable)?;
    drop(out);

    let mut line = [0u8; 257];
    let mut input = io::stdin().lock();
    let mut used = 0usize;
    loop {
        let read = input
            .read(&mut line[used..])
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        if read == 0 {
            return Ok(false);
        }
        used += read;
        if line[..used].contains(&b'\n') {
            break;
        }
        if used == line.len() {
            return Err(ErrorCode::InvalidInput);
        }
    }
    let text = std::str::from_utf8(&line[..used]).map_err(|_| ErrorCode::InvalidInput)?;
    parse_confirmation(text)
}

fn cancel(
    request: TerminalReviewGet,
    ticket: Option<[u8; 32]>,
    korean: bool,
) -> Result<(), ErrorCode> {
    let Some(ticket) = ticket else {
        return Ok(());
    };
    let receipt = hantong_ipc_client::terminal_review::exchange(&TerminalReviewCommand::Cancel(
        hantong_contracts::terminal_review::TerminalReviewCancel::new(request, ticket)?,
    ))?;
    render_result(&receipt, korean)
}

fn terminal_safe(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '\n' => out.push('\n'),
            '\r' => out.push_str("\\r"),
            '\t' => out.push('\t'),
            c if c.is_control() || is_bidi_control(c) => {
                out.push_str(&format!("\\u{{{:04x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061c}'
            | '\u{200e}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2028}'..='\u{2029}'
            | '\u{2066}'..='\u{2069}'
            | '\u{206d}'..='\u{206f}'
    )
}

fn parse_confirmation(value: &str) -> Result<bool, ErrorCode> {
    if value.len() > 256 {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(matches!(value.trim(), "1" | "y" | "Y"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn terminal_frontend_parses_exact_review_selector() {
        let parsed = parse_args(&args(&[
            "--kind",
            "personal_send",
            "--request-id",
            &"01".repeat(32),
            "--account-id",
            "personal",
            "--payload-hash",
            &"02".repeat(32),
            "--lang",
            "ko",
        ]))
        .unwrap();
        assert_eq!(parsed.request.kind, TerminalReviewKind::PersonalSend);
        assert_eq!(parsed.request.account_id.as_deref(), Some("personal"));
        assert!(parsed.korean);
    }

    #[test]
    fn terminal_frontend_rejects_unknown_duplicate_or_incomplete_args() {
        let hash = "01".repeat(32);
        assert!(
            parse_args(&args(&[
                "--kind",
                "personal_send",
                "--request-id",
                &hash,
                "--payload-hash",
                &hash,
                "--extra",
                "x",
            ]))
            .is_err()
        );
        assert!(
            parse_args(&args(&[
                "--kind",
                "personal_send",
                "--kind",
                "registration",
                "--request-id",
                &hash,
                "--payload-hash",
                &hash,
            ]))
            .is_err()
        );
        assert!(parse_args(&args(&["--kind", "personal_send", "--request-id"])).is_err());
    }

    #[test]
    fn terminal_frontend_escapes_terminal_and_bidi_controls() {
        let escaped = terminal_safe("ok\u{1b}[31m\u{202e}x\r\u{2028}\u{2029}");
        assert_eq!(escaped, "ok\\u{001b}[31m\\u{202e}x\\r\\u{2028}\\u{2029}");
    }

    #[test]
    fn terminal_frontend_rejects_overlong_confirmation_line() {
        let line = "x".repeat(257);
        assert!(parse_confirmation(&line).is_err());
    }

    #[test]
    fn terminal_frontend_confirmation_is_explicit_and_eof_cancels() {
        assert!(!parse_confirmation("").unwrap());
        assert!(!parse_confirmation("0\n").unwrap());
        assert!(parse_confirmation("1\n").unwrap());
        assert!(!parse_confirmation("yes\n").unwrap());
    }
}
