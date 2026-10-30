//! CLI syntax for one explicitly requested UID metadata page.
use crate::{ErrorCode, Lang, emit_error, resource};
use hantong_contracts::uid_page::*;
use hantong_ipc_client::uid_page as client;
pub(super) enum Action {
    Cursor(UidCursorRequest),
    Page(UidPageRequest),
    Status(UidPageStatusRequest),
}
pub(super) enum ResultValue {
    Cursor(UidCursorSnapshot),
    Receipt(UidPageReceipt),
}
pub(super) fn real_action(action: Action) -> Result<ResultValue, ErrorCode> {
    match action {
        Action::Cursor(request) => client::cursor_default(request).map(ResultValue::Cursor),
        Action::Status(request) => client::status_default(request).map(ResultValue::Receipt),
        Action::Page(request) => {
            let prepared = client::prepare_page_request(
                request.account_id(),
                request.mailbox().clone(),
                request.operation_id(),
                request.idempotency_key(),
                request.expected_catalog_generation(),
                request.expected_cursor_version(),
            )?;
            client::page_default(prepared).map(ResultValue::Receipt)
        }
    }
}
fn number(value: &str) -> Result<u64, ErrorCode> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(ErrorCode::InvalidInput);
    }
    value.parse().map_err(|_| ErrorCode::InvalidInput)
}
fn parse(command: &[String], options: &[(String, String)]) -> Result<Action, ErrorCode> {
    if command.len() != 2 || command[0] != "sync" {
        return Err(ErrorCode::InvalidInput);
    }
    let mut values = std::collections::BTreeMap::new();
    for (key, value) in options {
        if values.insert(key.as_str(), value.as_str()).is_some() {
            return Err(ErrorCode::InvalidInput);
        }
    }
    let get = |key: &str| values.get(key).copied().ok_or(ErrorCode::InvalidInput);
    let allowed: &[&str] = match command[1].as_str() {
        "cursor" => &["--account", "--wire-hex", "--encoding"],
        "page" => &[
            "--account",
            "--wire-hex",
            "--encoding",
            "--expected-catalog-generation",
            "--expected-cursor-version",
            "--operation-id",
            "--idempotency-key",
        ],
        "status" => &["--account", "--execution-id"],
        _ => return Err(ErrorCode::InvalidInput),
    };
    if values.len() != allowed.len() || values.keys().any(|k| !allowed.contains(k)) {
        return Err(ErrorCode::InvalidInput);
    }
    let account = get("--account")?;
    if command[1] == "status" {
        return UidPageStatusRequest::new(
            account,
            hantong_contracts::wire_hex::decode_fixed(get("--execution-id")?)?,
        )
        .map(Action::Status);
    }
    let mailbox = UidMailboxIdentity::new(
        get("--wire-hex")?,
        UidMailboxEncoding::parse(get("--encoding")?)?,
    )?;
    if command[1] == "cursor" {
        return UidCursorRequest::new(account, mailbox).map(Action::Cursor);
    }
    UidPageRequest::new(
        account,
        mailbox,
        get("--operation-id")?,
        get("--idempotency-key")?,
        number(get("--expected-catalog-generation")?)?,
        number(get("--expected-cursor-version")?)?,
        &"00".repeat(32),
    )
    .map(Action::Page)
}
pub(super) fn run(
    command: &[String],
    options: &[(String, String)],
    lang: Lang,
    json: bool,
    action: &mut impl FnMut(Action) -> Result<ResultValue, ErrorCode>,
) -> i32 {
    let request = match parse(command, options) {
        Ok(v) => v,
        Err(e) => return emit_error(e, lang, json),
    };
    let cursor = matches!(&request, Action::Cursor(_));
    match action(request) {
        Ok(ResultValue::Cursor(value)) if cursor => {
            if json {
                println!("{}", client::cursor_json(&value))
            } else {
                print!("{}", cursor_human(&value, lang))
            }
            0
        }
        Ok(ResultValue::Receipt(value)) if !cursor => {
            if json {
                println!("{}", client::receipt_json(&value))
            } else {
                print!("{}", receipt_human(&value, lang))
            }
            0
        }
        Ok(_) => emit_error(ErrorCode::InternalError, lang, json),
        Err(e) => emit_error(e, lang, json),
    }
}
fn cursor_human(value: &UidCursorSnapshot, lang: Lang) -> String {
    format!(
        "{}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n",
        resource(lang, "sync_cursor"),
        resource(lang, "sync_catalog_generation"),
        value.catalog_generation(),
        resource(lang, "sync_cursor_version"),
        value.cursor_version(),
        resource(lang, "sync_covered_uid"),
        value.covered_through_uid(),
        resource(lang, "sync_binding"),
        resource(
            lang,
            if value.binding_current() {
                "sync_current"
            } else {
                "sync_stale"
            }
        )
    )
}
fn receipt_human(value: &UidPageReceipt, lang: Lang) -> String {
    let state = match value.state() {
        UidPageState::Unresolved => "sync_unresolved",
        UidPageState::Succeeded => "sync_succeeded",
        UidPageState::Failed => "sync_failed",
    };
    let mut output = format!(
        "{}: {}\n{}: {}\n",
        resource(lang, "sync_state"),
        resource(lang, state),
        resource(lang, "sync_execution_id"),
        hantong_contracts::wire_hex::encode(value.execution_id())
    );
    for (key, n) in [
        ("sync_cursor_version", value.cursor_version()),
        (
            "sync_covered_uid",
            value.covered_through_uid().map(u64::from),
        ),
        ("sync_inserted", value.inserted_count().map(u64::from)),
    ] {
        if let Some(n) = n {
            output.push_str(&format!("{}: {n}\n", resource(lang, key)));
        }
    }
    if let Some(error) = value.error() {
        output.push_str(&format!(
            "{}: {}\n",
            resource(lang, "sync_error"),
            error.as_str()
        ));
    }
    output
}
#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
