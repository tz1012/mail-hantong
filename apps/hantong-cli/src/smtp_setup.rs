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
    match hantong_ipc_client::smtp_setup::exchange(command) {
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
