use super::*;
use hantong_contracts::{
    mail_send::{SendReviewRequest, SendReviewStatusRequest},
    wire_hex,
};
use hantong_ipc_client::mail_send as client;
use std::io::Read;
fn fields<'a>(
    options: &'a [(String, String)],
    allowed: &[&str],
) -> Result<std::collections::BTreeMap<&'a str, &'a str>, ErrorCode> {
    let mut fields = std::collections::BTreeMap::new();
    for (key, value) in options {
        if !allowed.contains(&key.as_str()) || fields.insert(key.as_str(), value.as_str()).is_some()
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    Ok(fields)
}
fn get<'a>(
    fields: &std::collections::BTreeMap<&str, &'a str>,
    key: &str,
) -> Result<&'a str, ErrorCode> {
    fields.get(key).copied().ok_or(ErrorCode::InvalidInput)
}
fn body_file(path: &str) -> Result<String, ErrorCode> {
    let file = std::fs::File::open(path).map_err(|_| ErrorCode::InvalidInput)?;
    if !file
        .metadata()
        .map_err(|_| ErrorCode::InvalidInput)?
        .is_file()
    {
        return Err(ErrorCode::InvalidInput);
    }
    let mut bytes = Vec::new();
    file.take((hantong_contracts::mail_send::MAX_COMPOSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::InvalidInput)?;
    if bytes.len() > hantong_contracts::mail_send::MAX_COMPOSE_BYTES {
        return Err(ErrorCode::QuotaExceeded);
    }
    String::from_utf8(bytes).map_err(|_| ErrorCode::InvalidInput)
}
pub(super) fn attachment_file(
    path: &str,
    media_type: &str,
) -> Result<hantong_contracts::google_write::AttachmentSnapshot, ErrorCode> {
    use hantong_contracts::google_write::{AttachmentSnapshot, MAX_ATTACHMENT_BYTES};
    let path = std::path::Path::new(path);
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or(ErrorCode::InvalidInput)?;
    let file = std::fs::File::open(path).map_err(|_| ErrorCode::InvalidInput)?;
    if !file
        .metadata()
        .map_err(|_| ErrorCode::InvalidInput)?
        .is_file()
    {
        return Err(ErrorCode::InvalidInput);
    }
    let mut bytes = Vec::new();
    file.take((MAX_ATTACHMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::InvalidInput)?;
    AttachmentSnapshot::new(name.into(), media_type.into(), bytes)
}
fn output_review(v: client::ReviewResponse, lang: Lang, json: bool) -> i32 {
    if json {
        println!(
            "{}",
            serde_json::json!({"schema_version":1,"ok":true,"account_id":v.account_id,"review_id":wire_hex::encode(&v.review_id),"state":v.state,"error_code":v.error_code.map(ErrorCode::as_str)})
        );
    } else {
        println!(
            "{}: {}\n{}: {}",
            resource(lang, "send_review_id"),
            wire_hex::encode(&v.review_id),
            resource(lang, "send_state"),
            v.state
        );
    }
    v.error_code.map_or(0, ErrorCode::exit_code)
}

fn terminal_safe(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}')
        {
            out.push_str(&format!("\\u{{{:04x}}}", c as u32));
        } else if c == '\r' {
            out.push_str("\\r");
        } else if c == '\n' || c == '\t' {
            out.push(c);
        } else if c.is_control() {
            out.push('\u{fffd}');
        } else {
            out.push(c);
        }
    }
    out
}

fn interactive_send(
    request: SendReviewRequest,
    lang: Lang,
) -> Result<client::ReviewResponse, ErrorCode> {
    use std::io::{BufRead, IsTerminal, Read, Write};
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(ErrorCode::HumanStepRequired);
    }
    let personal = if request.account_id().starts_with("google_") {
        hantong_ipc_client::google_mail::verify_default_personal(
            request.account_id(),
            hantong_contracts::google_setup::GoogleLocalProfile::PersonalReadSend,
        )?
    } else {
        false
    };
    if !personal {
        let receipt = client::request_default(&request)?;
        if receipt.state == "pending" {
            super::core_start::review(
                hantong_contracts::terminal_review::TerminalReviewKind::ProtectedSend,
                receipt.review_id,
                Some(request.account_id()),
                Some(*request.payload_hash()),
                lang,
            )?;
            let status = SendReviewStatusRequest {
                account_id: request.account_id().into(),
                review_id: receipt.review_id,
            };
            let mut response = client::status_default(&status)?;
            for _ in 0..300 {
                if !matches!(response.state.as_str(), "pending" | "sending") {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
                response = client::status_default(&status)?;
            }
            return Ok(response);
        }
        return Ok(receipt);
    }
    let prepared = client::prepare_default(&request)?;
    let Some(preview) = prepared.preview.as_ref() else {
        return Ok(prepared.receipt);
    };
    println!(
        "{}: {}\n{}: {}\n{}: {}\n{}:\n{}",
        resource(lang, "preview_from"),
        terminal_safe(&preview.from),
        resource(lang, "preview_to"),
        terminal_safe(&preview.to.join(", ")),
        resource(lang, "preview_subject"),
        terminal_safe(&preview.subject),
        resource(lang, "preview_body"),
        terminal_safe(&preview.body)
    );
    if !preview.attachments.is_empty() {
        println!("{}:", resource(lang, "preview_attachments"));
        for a in &preview.attachments {
            println!(
                "- {} | {} | {} bytes",
                terminal_safe(&a.filename),
                terminal_safe(&a.media_type),
                a.size
            );
        }
    }
    print!("{} ", resource(lang, "send_prompt"));
    std::io::stdout()
        .flush()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let mut input = String::new();
    let count = std::io::stdin()
        .lock()
        .take(257)
        .read_line(&mut input)
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if count > 256 {
        let _ = client::cancel_default(&SendReviewStatusRequest {
            account_id: request.account_id().into(),
            review_id: prepared.receipt.review_id,
        });
        return Err(ErrorCode::InvalidInput);
    }
    if input.trim() != "1" {
        let _ = client::cancel_default(&hantong_contracts::mail_send::SendReviewStatusRequest {
            account_id: request.account_id().into(),
            review_id: prepared.receipt.review_id,
        });
        return Ok(client::ReviewResponse {
            state: "interrupted".into(),
            error_code: Some(ErrorCode::ApprovalRequired),
            ..prepared.receipt
        });
    }
    let mut response = client::confirm_default(
        &hantong_contracts::mail_send::PersonalSendConfirmRequest::new(
            request.account_id(),
            prepared.receipt.review_id,
            preview.payload_hash,
        )?,
    )?;
    for _ in 0..300 {
        if !matches!(response.state.as_str(), "pending" | "sending") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
        response = client::status_default(&SendReviewStatusRequest {
            account_id: request.account_id().into(),
            review_id: response.review_id,
        })?;
    }
    Ok(response)
}
pub(super) fn run(
    command: &[String],
    options: &[(String, String)],
    lang: Lang,
    json: bool,
    resolve_lang: &mut impl FnMut() -> Lang,
) -> i32 {
    let result = (|| -> Result<Command, ErrorCode> {
        match command
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .as_slice()
        {
            ["draft", "create"] => {
                let fields = fields(
                    options,
                    &[
                        "--account",
                        "--to",
                        "--subject",
                        "--body-file",
                        "--attachment-file",
                        "--attachment-type",
                        "--operation-id",
                        "--idempotency-key",
                    ],
                )?;
                let to = get(&fields, "--to")?
                    .split(',')
                    .map(|v| v.trim().to_owned())
                    .collect();
                let body = body_file(get(&fields, "--body-file")?)?;
                let draft = client::prepare_default_draft(
                    get(&fields, "--account")?,
                    to,
                    get(&fields, "--subject")?.into(),
                    body,
                    get(&fields, "--operation-id")?,
                    get(&fields, "--idempotency-key")?,
                )?;
                let attachments = match fields.get("--attachment-file") {
                    Some(path) => vec![attachment_file(
                        path,
                        fields
                            .get("--attachment-type")
                            .copied()
                            .unwrap_or("application/octet-stream"),
                    )?],
                    None if fields.contains_key("--attachment-type") => {
                        return Err(ErrorCode::InvalidInput);
                    }
                    None => vec![],
                };
                Ok(Command::Draft(client::prepare_compose(draft, attachments)?))
            }
            ["send"] => {
                let fields = fields(
                    options,
                    &[
                        "--account",
                        "--draft-id",
                        "--expected-version",
                        "--payload-hash",
                        "--operation-id",
                        "--idempotency-key",
                    ],
                )?;
                Ok(Command::Send(SendReviewRequest::new(
                    get(&fields, "--account")?,
                    wire_hex::decode_fixed(get(&fields, "--draft-id")?)?,
                    get(&fields, "--expected-version")?
                        .parse()
                        .map_err(|_| ErrorCode::InvalidInput)?,
                    get(&fields, "--operation-id")?,
                    get(&fields, "--idempotency-key")?,
                    wire_hex::decode_fixed(get(&fields, "--payload-hash")?)?,
                )?))
            }
            ["send", "status"] => {
                let fields = fields(options, &["--account", "--review-id"])?;
                let account = get(&fields, "--account")?;
                let review_id = wire_hex::decode_fixed(get(&fields, "--review-id")?)?;
                if account.is_empty()
                    || account.len() > 128
                    || !account
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                    || review_id == [0; 32]
                {
                    return Err(ErrorCode::InvalidInput);
                }
                Ok(Command::Status(SendReviewStatusRequest {
                    account_id: account.into(),
                    review_id,
                }))
            }
            _ => Err(ErrorCode::InvalidInput),
        }
    })();
    let command = match result {
        Ok(v) => v,
        Err(e) => return emit_error(e, lang, json),
    };
    let lang = resolve_lang();
    match command {
        Command::Draft(r) => match client::create_compose_default(&r) {
            Ok(v) => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"schema_version":1,"ok":true,"account_id":v.account_id,"draft_id":wire_hex::encode(&v.draft_id),"version":v.version,"payload_hash":wire_hex::encode(&v.payload_hash)})
                    );
                } else {
                    println!(
                        "{}: {}\n{}: {}\n{}: {}",
                        resource(lang, "draft_id"),
                        wire_hex::encode(&v.draft_id),
                        resource(lang, "draft_version"),
                        v.version,
                        resource(lang, "draft_hash"),
                        wire_hex::encode(&v.payload_hash)
                    );
                }
                0
            }
            Err(e) => emit_error(e, lang, json),
        },
        Command::Send(r) => match interactive_send(r, lang) {
            Ok(v) => output_review(v, lang, json),
            Err(e) => emit_error(e, lang, json),
        },
        Command::Status(r) => match client::status_default(&r) {
            Ok(v) => output_review(v, lang, json),
            Err(e) => emit_error(e, lang, json),
        },
    }
}
enum Command {
    Draft(hantong_contracts::google_write::ComposeSnapshot),
    Send(SendReviewRequest),
    Status(SendReviewStatusRequest),
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apr01_send_preview_escapes_carriage_return_and_terminal_controls() {
        assert_eq!(
            terminal_safe("first\roverwrite\u{1b}[2J\u{202e}x"),
            "first\\roverwrite\u{fffd}[2J\\u{202e}x"
        );
    }
    #[test]
    fn cli_send_fields_reject_duplicates_and_unknown_authority() {
        assert!(fields(&[("--approved".into(), "yes".into())], &["--account"]).is_err());
        assert!(
            fields(
                &[
                    ("--account".into(), "a".into()),
                    ("--account".into(), "b".into())
                ],
                &["--account"]
            )
            .is_err()
        );
    }
}
