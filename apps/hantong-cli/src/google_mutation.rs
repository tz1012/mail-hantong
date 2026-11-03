use super::*;
use hantong_contracts::{
    google_mutation::MutationPayload, google_write::MessageLabelChange,
    mail_send::SendReviewRequest, wire_hex,
};
use hantong_ipc_client::google_mutation as client;
enum Command {
    Request(MutationPayload),
    Status(String, [u8; 32]),
}
fn generated_id() -> Result<String, ErrorCode> {
    #[cfg(windows)]
    {
        Ok(wire_hex::encode(&hantong_platform::random_nonce()?))
    }
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
}
fn parse(command: &[String], options: &[(String, String)]) -> Result<Command, ErrorCode> {
    let action = match command {
        [a, b] if a == "change" => b.as_str(),
        _ => return Err(ErrorCode::InvalidInput),
    };
    let allowed = match action {
        "draft-save" => vec![
            "--account",
            "--draft-id",
            "--expected-version",
            "--payload-hash",
            "--operation-id",
            "--idempotency-key",
        ],
        "labels" => vec![
            "--account",
            "--message-id",
            "--expected-version",
            "--add-labels",
            "--remove-labels",
            "--operation-id",
            "--idempotency-key",
        ],
        "trash" => vec![
            "--account",
            "--message-id",
            "--expected-version",
            "--operation-id",
            "--idempotency-key",
            "--payload-hash",
        ],
        "status" => vec!["--account", "--review-id"],
        _ => return Err(ErrorCode::InvalidInput),
    };
    let mut fields = std::collections::BTreeMap::new();
    for (k, v) in options {
        if !allowed.contains(&k.as_str()) || fields.insert(k.as_str(), v.as_str()).is_some() {
            return Err(ErrorCode::InvalidInput);
        }
    }
    let get = |key| fields.get(key).copied().ok_or(ErrorCode::InvalidInput);
    let account = get("--account")?;
    hantong_contracts::google::validate_google_account_id(account)?;
    if action == "status" {
        let id = wire_hex::decode_fixed(get("--review-id")?)?;
        if id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        return Ok(Command::Status(account.into(), id));
    }
    let version = get("--expected-version")?
        .parse()
        .map_err(|_| ErrorCode::InvalidInput)?;
    let op = match fields.get("--operation-id") {
        Some(value) => (*value).to_owned(),
        None if action == "trash" => generated_id()?,
        None => return Err(ErrorCode::InvalidInput),
    };
    let key = match fields.get("--idempotency-key") {
        Some(value) => (*value).to_owned(),
        None if action == "trash" => generated_id()?,
        None => return Err(ErrorCode::InvalidInput),
    };
    if op == key {
        return Err(ErrorCode::Conflict);
    }
    let r = if action == "draft-save" {
        MutationPayload::SaveDraft(SendReviewRequest::new(
            account,
            wire_hex::decode_fixed(get("--draft-id")?)?,
            version,
            &op,
            &key,
            wire_hex::decode_fixed(get("--payload-hash")?)?,
        )?)
    } else if action == "labels" {
        let list = |key| {
            fields
                .get(key)
                .map(|s| s.split(',').map(str::to_owned).collect())
                .unwrap_or_default()
        };
        let change = MessageLabelChange::new(
            account.into(),
            get("--message-id")?.into(),
            list("--add-labels"),
            list("--remove-labels"),
        )?;
        let hash = hantong_platform::sha256_storage_payload(&change.canonical_payload())?;
        MutationPayload::Labels {
            change,
            expected_version: version,
            operation_id: op,
            idempotency_key: key,
            payload_hash: hash,
        }
    } else {
        let message_id = get("--message-id")?;
        let account_id = account.to_owned();
        let message_id = message_id.to_owned();
        let payload = MutationPayload::Trash {
            account_id: account_id.clone(),
            message_id: message_id.clone(),
            expected_version: version,
            operation_id: op.clone(),
            idempotency_key: key.clone(),
            payload_hash: [0; 32],
        };
        let hash = hantong_platform::sha256_storage_payload(&payload.canonical_payload())?;
        if let Some(caller_hash) = fields.get("--payload-hash")
            && wire_hex::decode_fixed::<32>(caller_hash)? != hash
        {
            return Err(ErrorCode::InvalidInput);
        }
        MutationPayload::Trash {
            account_id,
            message_id,
            expected_version: version,
            operation_id: op,
            idempotency_key: key,
            payload_hash: hash,
        }
    };
    MutationPayload::from_json(&r.to_json())?;
    Ok(Command::Request(r))
}
fn interactive_request<T>(
    json: bool,
    no_input: bool,
    stdin_tty: bool,
    stdout_tty: bool,
    request: impl FnOnce() -> Result<T, ErrorCode>,
) -> Result<T, ErrorCode> {
    if json || no_input || !stdin_tty || !stdout_tty {
        return Err(ErrorCode::HumanStepRequired);
    }
    request()
}

pub(super) fn run(
    command: &[String],
    options: &[(String, String)],
    lang: Lang,
    json: bool,
    resolve_lang: &mut impl FnMut() -> Lang,
) -> i32 {
    let command = match parse(command, options) {
        Ok(c) => c,
        Err(e) => return emit_error(e, lang, json),
    };
    let lang = resolve_lang();
    let result = match command {
        Command::Request(r) => {
            use std::io::IsTerminal;
            interactive_request(
                json,
                std::env::args_os().any(|arg| arg == "--no-input"),
                std::io::stdin().is_terminal(),
                std::io::stdout().is_terminal(),
                || client::request_default(&r),
            )
            .and_then(|receipt| {
                if !json && receipt.state == "pending" {
                    super::terminal_workflows::maybe_review(
                        hantong_contracts::terminal_review::TerminalReviewKind::GoogleMutation,
                        receipt.review_id,
                        lang,
                    )?;
                    let mut response = client::status_default(r.account_id(), receipt.review_id)?;
                    for _ in 0..300 {
                        if !matches!(response.state.as_str(), "pending" | "claimed") {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        response = client::status_default(r.account_id(), receipt.review_id)?;
                    }
                    return Ok(response);
                }
                Ok(receipt)
            })
        }
        Command::Status(account, id) => client::status_default(&account, id),
    };
    match result {
        Ok(r) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"schema_version":1,"ok":true,"review_id":wire_hex::encode(&r.review_id),"state":r.state,"error_code":r.error.map(ErrorCode::as_str)})
                );
            } else {
                println!(
                    "{}: {}\n{}: {}",
                    resource(lang, "send_review_id"),
                    wire_hex::encode(&r.review_id),
                    resource(lang, "send_state"),
                    r.state
                );
            }
            r.error.map_or(0, ErrorCode::exit_code)
        }
        Err(e) => emit_error(e, lang, json),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apr01_headless_mutation_never_calls_request() {
        for (json, no_input, stdin_tty, stdout_tty) in [
            (true, false, true, true),
            (false, true, true, true),
            (false, false, false, true),
            (false, false, true, false),
        ] {
            let mut called = false;
            let result = interactive_request(json, no_input, stdin_tty, stdout_tty, || {
                called = true;
                Ok(())
            });
            assert_eq!(result, Err(ErrorCode::HumanStepRequired));
            assert!(!called);
        }
        let mut called = false;
        interactive_request(false, false, true, true, || {
            called = true;
            Ok(())
        })
        .unwrap();
        assert!(called);
    }
    #[test]
    fn sec03_change_cli_rejects_send_delete_and_caller_approval() {
        for action in ["delete", "send", "batch"] {
            assert!(parse(&["change".into(), action.into()], &[]).is_err());
        }
        let opts = vec![
            ("--account".into(), format!("google_{}", "a".repeat(64))),
            ("--approved".into(), "true".into()),
        ];
        assert!(parse(&["change".into(), "labels".into()], &opts).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn apr01_trash_change_generates_missing_intent_fields_and_hash() {
        let account = format!("google_{}", "a".repeat(64));
        let options = vec![
            ("--account".into(), account.clone()),
            ("--message-id".into(), "message-1".into()),
            ("--expected-version".into(), "7".into()),
        ];
        let Command::Request(MutationPayload::Trash {
            operation_id,
            idempotency_key,
            payload_hash,
            ..
        }) = parse(&["change".into(), "trash".into()], &options).unwrap()
        else {
            panic!("expected trash request")
        };
        assert!(!operation_id.is_empty());
        assert!(!idempotency_key.is_empty());
        assert_ne!(operation_id, idempotency_key);
        assert_ne!(payload_hash, [0; 32]);
    }
}
