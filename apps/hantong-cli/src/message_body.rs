use super::*;
use hantong_contracts::{
    message_body::MessageBodyRequest,
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
fn parse(
    command: &[String],
    options: &[(String, String)],
) -> Result<MessageBodyRequest, ErrorCode> {
    if command != ["read"] {
        return Err(ErrorCode::InvalidInput);
    }
    let mut fields = std::collections::BTreeMap::new();
    for (key, value) in options {
        if ![
            "--account",
            "--wire-hex",
            "--encoding",
            "--uidvalidity",
            "--uid",
            "--max-bytes",
        ]
        .contains(&key.as_str())
            || fields.insert(key.as_str(), value.as_str()).is_some()
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    let get = |key| fields.get(key).copied().ok_or(ErrorCode::InvalidInput);
    let number = |key| -> Result<u32, ErrorCode> {
        let v = get(key)?;
        if !v.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ErrorCode::InvalidInput);
        }
        v.parse().map_err(|_| ErrorCode::InvalidInput)
    };
    let encoding = match get("--encoding")? {
        "utf8" => UidMailboxEncoding::Utf8,
        "modified_utf7" => UidMailboxEncoding::ModifiedUtf7,
        _ => return Err(ErrorCode::InvalidInput),
    };
    let max = fields
        .get("--max-bytes")
        .map(|v| v.parse::<usize>().map_err(|_| ErrorCode::InvalidInput))
        .transpose()?
        .unwrap_or(hantong_contracts::message_body::MAX_BODY_BYTES);
    MessageBodyRequest::new(
        get("--account")?,
        UidMailboxIdentity::new(get("--wire-hex")?, encoding)?,
        number("--uidvalidity")?,
        number("--uid")?,
        max,
    )
}
pub(super) fn run(
    command: &[String],
    options: &[(String, String)],
    lang: Lang,
    json: bool,
    resolve_lang: &mut impl FnMut() -> Lang,
) -> i32 {
    let request = if options.iter().any(|(k, _)| k == "--message-id") {
        let mut fields = std::collections::BTreeMap::new();
        for (key, value) in options {
            if !["--account", "--message-id", "--max-bytes"].contains(&key.as_str())
                || fields.insert(key.as_str(), value.as_str()).is_some()
            {
                return emit_error(ErrorCode::InvalidInput, lang, json);
            }
        }
        if command != ["read"] {
            return emit_error(ErrorCode::InvalidInput, lang, json);
        }
        let Some(account) = fields.get("--account") else {
            return emit_error(ErrorCode::InvalidInput, lang, json);
        };
        let max = match fields
            .get("--max-bytes")
            .map(|v| v.parse::<usize>())
            .transpose()
        {
            Ok(v) => v.unwrap_or(hantong_contracts::message_body::MAX_BODY_BYTES),
            Err(_) => return emit_error(ErrorCode::InvalidInput, lang, json),
        };
        match hantong_ipc_client::message_body::locate_default(account, fields["--message-id"], max)
        {
            Ok(r) => r,
            Err(e) => return emit_error(e, lang, json),
        }
    } else {
        match parse(command, options) {
            Ok(r) => r,
            Err(e) => return emit_error(e, lang, json),
        }
    };
    let lang = resolve_lang();
    for attempt in 0..=30 {
        let response = match hantong_ipc_client::message_body::get_default(&request) {
            Ok(v) => v,
            Err(e) => return emit_error(e, lang, json),
        };
        if let Some(text) = response.text {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"schema_version":1,"ok":true,"state":"ready","account_id":request.account_id(),"mailbox_wire_hex":request.mailbox().wire_hex(),"mailbox_encoding":request.mailbox().encoding().as_str(),"uidvalidity":request.uidvalidity(),"uid":request.uid(),"text":text,"truncated":response.truncated})
                );
            } else {
                println!("{text}");
                if response.truncated {
                    println!("{}", resource(lang, "body_truncated"));
                }
            }
            return 0;
        }
        if attempt < 30 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    emit_error(ErrorCode::Offline, lang, json)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_body_bound_input_rejects_missing_identity_and_duplicate_arguments() {
        let command = vec!["read".into()];
        assert!(parse(&command, &[]).is_err());
        let mut options = vec![
            ("--account".into(), "acct".into()),
            ("--wire-hex".into(), "494e424f58".into()),
            ("--encoding".into(), "utf8".into()),
            ("--uidvalidity".into(), "42".into()),
            ("--uid".into(), "9".into()),
        ];
        assert_eq!(parse(&command, &options).unwrap().uid(), 9);
        options.push(("--uid".into(), "10".into()));
        assert!(parse(&command, &options).is_err());
    }
}
