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
                        "--operation-id",
                        "--idempotency-key",
                    ],
                )?;
                let to = get(&fields, "--to")?
                    .split(',')
                    .map(|v| v.trim().to_owned())
                    .collect();
                let body = body_file(get(&fields, "--body-file")?)?;
                Ok(Command::Draft(client::prepare_default_draft(
                    get(&fields, "--account")?,
                    to,
                    get(&fields, "--subject")?.into(),
                    body,
                    get(&fields, "--operation-id")?,
                    get(&fields, "--idempotency-key")?,
                )?))
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
        Command::Draft(r) => match client::create_default(&r) {
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
        Command::Send(r) => match client::request_default(&r) {
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
    Draft(hantong_contracts::mail_send::DraftCreateRequest),
    Send(SendReviewRequest),
    Status(SendReviewStatusRequest),
}
#[cfg(test)]
mod tests {
    use super::*;
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
