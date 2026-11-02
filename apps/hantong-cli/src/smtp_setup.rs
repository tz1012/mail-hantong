use super::{Lang, emit_error, resource};
use hantong_contracts::{
    ErrorCode,
    imap_setup::ImapSetupTls,
    smtp_setup::{SmtpSetupCommand, SmtpSetupInput},
    wire_hex,
};
fn parse(command: &[String], options: &[(String, String)]) -> Result<SmtpSetupCommand, ErrorCode> {
    if command.get(..2) != Some(&["setup".to_owned(), "smtp".to_owned()]) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut fields = std::collections::BTreeMap::new();
    for (k, v) in options {
        if !["--account", "--host", "--port", "--tls", "--login"].contains(&k.as_str())
            || fields.insert(k.as_str(), v.as_str()).is_some()
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    match command.get(2).map(String::as_str) {
        Some("status") if command.len() == 4 && fields.is_empty() => {
            let id = wire_hex::decode_fixed(&command[3])?;
            if id == [0; 32] {
                Err(ErrorCode::InvalidInput)
            } else {
                Ok(SmtpSetupCommand::Status(id))
            }
        }
        Some("request") if command.len() == 3 && fields.len() == 5 => {
            let get = |k| fields.get(k).copied().ok_or(ErrorCode::InvalidInput);
            let tls = match get("--tls")? {
                "implicit_tls" => ImapSetupTls::ImplicitTls,
                "starttls_required" => ImapSetupTls::StarttlsRequired,
                _ => return Err(ErrorCode::InvalidInput),
            };
            Ok(SmtpSetupCommand::Request(SmtpSetupInput::new(
                get("--account")?.into(),
                get("--host")?.into(),
                get("--port")?
                    .parse()
                    .map_err(|_| ErrorCode::InvalidInput)?,
                tls,
                get("--login")?.into(),
            )?))
        }
        Some("approve" | "complete" | "execute") => Err(ErrorCode::HumanStepRequired),
        _ => Err(ErrorCode::InvalidInput),
    }
}
pub(super) fn run(command: &[String], options: &[(String, String)], lang: Lang, json: bool) -> i32 {
    let command = match parse(command, options) {
        Ok(c) => c,
        Err(e) => return emit_error(e, lang, json),
    };
    let is_request = matches!(command, SmtpSetupCommand::Request(_));
    if is_request {
        use std::io::IsTerminal;
        if json
            || std::env::args_os().any(|v| v == "--no-input")
            || !std::io::stdin().is_terminal()
            || !std::io::stdout().is_terminal()
        {
            return emit_error(ErrorCode::HumanStepRequired, lang, json);
        }
    }
    match hantong_ipc_client::smtp_setup::exchange(command).and_then(|r| {
        if is_request && !json && r.state == hantong_contracts::smtp_setup::SmtpSetupState::Pending
        {
            super::terminal_workflows::maybe_review(
                hantong_contracts::terminal_review::TerminalReviewKind::SmtpSetup,
                r.request_id,
                lang,
            )?;
            let mut response =
                hantong_ipc_client::smtp_setup::exchange(SmtpSetupCommand::Status(r.request_id))?;
            for _ in 0..300 {
                if response.state != hantong_contracts::smtp_setup::SmtpSetupState::Pending {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
                response = hantong_ipc_client::smtp_setup::exchange(SmtpSetupCommand::Status(
                    r.request_id,
                ))?;
            }
            return Ok(response);
        }
        Ok(r)
    }) {
        Ok(r) => {
            let id = wire_hex::encode(&r.request_id);
            if json {
                println!(
                    "{}",
                    serde_json::json!({"protocol_version":1,"request_id":id,"expires_at_utc_ms":r.expires_at_utc_ms,"state":r.state.as_str(),"error_code":r.error_code.map(ErrorCode::as_str)})
                )
            } else {
                println!(
                    "{}: {id}\n{}: {}",
                    resource(lang, "smtp_setup_request"),
                    resource(lang, "smtp_setup_state"),
                    r.state.as_str()
                )
            }
            r.error_code.map_or(0, ErrorCode::exit_code)
        }
        Err(e) => emit_error(e, lang, json),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup01_smtp_cli_requires_explicit_server_and_forbids_approval() {
        let cmd = ["setup", "smtp", "request"].map(String::from);
        assert!(parse(&cmd, &[]).is_err());
        let opts = [
            ("--account", "acct"),
            ("--host", "smtp.example"),
            ("--port", "465"),
            ("--tls", "implicit_tls"),
            ("--login", "test@example.test"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()));
        assert!(parse(&cmd, &opts).is_ok());
        let mut extra = opts.to_vec();
        extra.push(("--password".into(), "forbidden".into()));
        assert!(parse(&cmd, &extra).is_err());
        assert_eq!(
            parse(&["setup", "smtp", "approve"].map(String::from), &[]),
            Err(ErrorCode::HumanStepRequired)
        );
    }
}
