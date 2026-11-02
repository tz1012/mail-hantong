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
    let op = get("--operation-id")?;
    let key = get("--idempotency-key")?;
    let r = if action == "draft-save" {
        MutationPayload::SaveDraft(SendReviewRequest::new(
            account,
            wire_hex::decode_fixed(get("--draft-id")?)?,
            version,
            op,
            key,
            wire_hex::decode_fixed(get("--payload-hash")?)?,
        )?)
    } else {
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
            operation_id: op.into(),
            idempotency_key: key.into(),
            payload_hash: hash,
        }
    };
    MutationPayload::from_json(&r.to_json())?;
    Ok(Command::Request(r))
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
        Command::Request(r) => client::request_default(&r),
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
    fn sec03_change_cli_rejects_send_delete_and_caller_approval() {
        for action in ["delete", "trash", "send", "batch"] {
            assert!(parse(&["change".into(), action.into()], &[]).is_err());
        }
        let opts = vec![
            ("--account".into(), format!("google_{}", "a".repeat(64))),
            ("--approved".into(), "true".into()),
        ];
        assert!(parse(&["change".into(), "labels".into()], &opts).is_err());
    }
}
