//! CLI syntax for one explicitly requested UID metadata page.
use crate::{ErrorCode, Lang, emit_error, resource};
use hantong_contracts::uid_inspection::UidInspectionSnapshot;
use hantong_contracts::uid_page::*;
use hantong_contracts::uid_reconcile::{UidReconcileReceipt, UidReconcileRequest};
use hantong_contracts::uid_recovery::{UidRecoveryReceipt, UidRecoveryRequest};
use hantong_ipc_client::uid_inspection as inspection_client;
use hantong_ipc_client::uid_page as client;
use hantong_ipc_client::uid_reconcile as reconcile_client;
use hantong_ipc_client::uid_recovery as recovery_client;
pub(super) enum Action {
    Inspect(UidCursorRequest),
    Cursor(UidCursorRequest),
    Page(UidPageRequest),
    Status(UidPageStatusRequest),
    Reconcile(UidReconcileRequest),
    ReconcileStatus(UidPageStatusRequest),
    Recover(UidRecoveryRequest),
    RecoverStatus(UidPageStatusRequest),
}
pub(super) enum ResultValue {
    Inspection(UidInspectionSnapshot),
    Cursor(UidCursorSnapshot),
    Receipt(UidPageReceipt),
    ReconcileReceipt(UidReconcileReceipt),
    RecoveryReceipt(UidRecoveryReceipt),
}
pub(super) fn real_action(action: Action) -> Result<ResultValue, ErrorCode> {
    match action {
        Action::Inspect(request) => {
            inspection_client::inspect_default(request).map(ResultValue::Inspection)
        }
        Action::Cursor(request) => client::cursor_default(request).map(ResultValue::Cursor),
        Action::Status(request) => client::status_default(request).map(ResultValue::Receipt),
        Action::ReconcileStatus(request) => {
            reconcile_client::status_default(request).map(ResultValue::ReconcileReceipt)
        }
        Action::RecoverStatus(request) => {
            recovery_client::status_default(request).map(ResultValue::RecoveryReceipt)
        }
        Action::Recover(request) => {
            let prepared = recovery_client::prepare_recovery_request(
                request.account_id(),
                request.mailbox().clone(),
                request.operation_id(),
                request.idempotency_key(),
                request.expected_catalog_generation(),
                request.expected_cursor_version(),
            )?;
            recovery_client::recover_default(prepared).map(ResultValue::RecoveryReceipt)
        }
        Action::Reconcile(request) => {
            let prepared = reconcile_client::prepare_reconcile_request(
                request.account_id(),
                request.mailbox().clone(),
                request.operation_id(),
                request.idempotency_key(),
                request.expected_catalog_generation(),
                request.expected_cursor_version(),
            )?;
            reconcile_client::reconcile_default(prepared).map(ResultValue::ReconcileReceipt)
        }
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
        "cursor" | "inspect" => &["--account", "--wire-hex", "--encoding"],
        "page" | "reconcile" | "recover" => &[
            "--account",
            "--wire-hex",
            "--encoding",
            "--expected-catalog-generation",
            "--expected-cursor-version",
            "--operation-id",
            "--idempotency-key",
        ],
        "status" | "reconcile-status" | "recover-status" => &["--account", "--execution-id"],
        _ => return Err(ErrorCode::InvalidInput),
    };
    if values.len() != allowed.len() || values.keys().any(|k| !allowed.contains(k)) {
        return Err(ErrorCode::InvalidInput);
    }
    let account = get("--account")?;
    if matches!(
        command[1].as_str(),
        "status" | "reconcile-status" | "recover-status"
    ) {
        let request = UidPageStatusRequest::new(
            account,
            hantong_contracts::wire_hex::decode_fixed(get("--execution-id")?)?,
        )?;
        return Ok(if command[1] == "recover-status" {
            Action::RecoverStatus(request)
        } else if command[1] == "reconcile-status" {
            Action::ReconcileStatus(request)
        } else {
            Action::Status(request)
        });
    }
    let mailbox = UidMailboxIdentity::new(
        get("--wire-hex")?,
        UidMailboxEncoding::parse(get("--encoding")?)?,
    )?;
    if command[1] == "cursor" {
        return UidCursorRequest::new(account, mailbox).map(Action::Cursor);
    }
    if command[1] == "inspect" {
        return UidCursorRequest::new(account, mailbox).map(Action::Inspect);
    }
    if command[1] == "reconcile" {
        return UidReconcileRequest::new(
            account,
            mailbox,
            get("--operation-id")?,
            get("--idempotency-key")?,
            number(get("--expected-catalog-generation")?)?,
            number(get("--expected-cursor-version")?)?,
            &"00".repeat(32),
        )
        .map(Action::Reconcile);
    }
    if command[1] == "recover" {
        return UidRecoveryRequest::new(
            account,
            mailbox,
            get("--operation-id")?,
            get("--idempotency-key")?,
            number(get("--expected-catalog-generation")?)?,
            number(get("--expected-cursor-version")?)?,
            &"00".repeat(32),
        )
        .map(Action::Recover);
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
    resolve_lang: &mut impl FnMut() -> Lang,
) -> i32 {
    let request = match parse(command, options) {
        Ok(v) => v,
        Err(e) => return emit_error(e, lang, json),
    };
    let lang = resolve_lang();
    let cursor = matches!(&request, Action::Cursor(_));
    let inspection = matches!(&request, Action::Inspect(_));
    let reconcile = matches!(&request, Action::Reconcile(_) | Action::ReconcileStatus(_));
    let recovery = matches!(&request, Action::Recover(_) | Action::RecoverStatus(_));
    match action(request) {
        Ok(ResultValue::Inspection(value)) if inspection => {
            if json {
                println!("{}", inspection_client::inspection_json(&value))
            } else {
                print!("{}", inspection_human(&value, lang))
            }
            0
        }
        Ok(ResultValue::Cursor(value)) if cursor => {
            if json {
                println!("{}", client::cursor_json(&value))
            } else {
                print!("{}", cursor_human(&value, lang))
            }
            0
        }
        Ok(ResultValue::Receipt(value)) if !cursor && !inspection && !reconcile && !recovery => {
            if json {
                println!("{}", client::receipt_json(&value))
            } else {
                print!("{}", receipt_human(&value, lang))
            }
            0
        }
        Ok(ResultValue::ReconcileReceipt(value)) if reconcile => {
            if json {
                println!("{}", reconcile_client::receipt_json(&value))
            } else {
                print!("{}", reconcile_human(&value, lang))
            }
            0
        }
        Ok(ResultValue::RecoveryReceipt(value)) if recovery => {
            if json {
                println!("{}", recovery_client::receipt_json(&value))
            } else {
                print!("{}", recovery_human(&value, lang))
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
fn inspection_human(value: &UidInspectionSnapshot, lang: Lang) -> String {
    let mut out = format!(
        "{}\n{}: {}\n{}: {}\n",
        resource(lang, "sync_inspection"),
        resource(lang, "sync_catalog_generation"),
        value.catalog_generation(),
        resource(lang, "sync_binding"),
        resource(
            lang,
            if value.binding_current() {
                "sync_current"
            } else {
                "sync_stale"
            }
        )
    );
    if let Some(observation) = value.observation() {
        let range = if observation.covered_through_uid() == 0 {
            resource(lang, "sync_inspection_empty").to_owned()
        } else {
            format!("1..{}", observation.covered_through_uid())
        };
        out.push_str(&format!(
            "{}: {range}\n",
            resource(lang, "sync_inspection_range")
        ));
        for (key, number) in [
            (
                "sync_inspection_ceiling",
                u64::from(observation.observed_uid_ceiling()),
            ),
            ("sync_recovery_epoch", observation.epoch_generation()),
            ("sync_cursor_version", observation.cursor_version()),
        ] {
            out.push_str(&format!("{}: {number}\n", resource(lang, key)));
        }
        out.push_str(&format!(
            "{}: {}\n",
            resource(lang, "sync_inspection_stored_at"),
            observation.stored_at_utc_ms()
        ));
        out.push_str(resource(
            lang,
            if observation.coverage_at_observed_ceiling() {
                "sync_inspection_at_ceiling"
            } else {
                "sync_inspection_below_ceiling"
            },
        ));
    } else {
        out.push_str(resource(lang, "sync_inspection_unobserved"));
    }
    out.push('\n');
    out.push_str(resource(lang, "sync_inspection_scope"));
    out.push('\n');
    out
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
fn reconcile_human(value: &UidReconcileReceipt, lang: Lang) -> String {
    let state = match value.state() {
        UidPageState::Unresolved => "sync_unresolved",
        UidPageState::Succeeded => "sync_succeeded",
        UidPageState::Failed => "sync_failed",
    };
    let mut out = format!(
        "{}: {}\n{}: {}\n",
        resource(lang, "sync_reconcile_state"),
        resource(lang, state),
        resource(lang, "sync_execution_id"),
        hantong_contracts::wire_hex::encode(value.execution_id())
    );
    if let Some(r) = value.result() {
        for (key, n) in [
            ("sync_reconcile_generation", r.generation()),
            ("sync_reconcile_ceiling", r.frozen_cursor_version()),
            ("sync_reconcile_source", r.source_cursor_version()),
            ("sync_reconcile_after", u64::from(r.after_uid())),
            ("sync_reconcile_through", u64::from(r.through_uid())),
            ("sync_reconcile_observed", u64::from(r.observed_count())),
            ("sync_reconcile_changed", u64::from(r.flags_changed_count())),
            ("sync_reconcile_absent", u64::from(r.newly_absent_count())),
            ("sync_reconcile_remaining", u64::from(r.remaining_pages())),
        ] {
            out.push_str(&format!("{}: {n}\n", resource(lang, key)));
        }
        out.push_str(resource(
            lang,
            if r.pass_complete() {
                "sync_reconcile_complete"
            } else {
                "sync_reconcile_pending"
            },
        ));
        out.push('\n');
    }
    if let Some(e) = value.error() {
        out.push_str(&format!(
            "{}: {}\n",
            resource(lang, "sync_error"),
            e.as_str()
        ));
    }
    out
}
fn recovery_human(value: &UidRecoveryReceipt, lang: Lang) -> String {
    let state = match value.state() {
        UidPageState::Unresolved => "sync_unresolved",
        UidPageState::Succeeded => "sync_succeeded",
        UidPageState::Failed => "sync_failed",
    };
    let mut out = format!(
        "{}: {}\n{}: {}\n",
        resource(lang, "sync_recovery_state"),
        resource(lang, state),
        resource(lang, "sync_execution_id"),
        hantong_contracts::wire_hex::encode(value.execution_id())
    );
    if let Some(r) = value.result() {
        for (key, n) in [
            ("sync_recovery_epoch", r.epoch_generation()),
            ("sync_cursor_version", r.cursor_version()),
            ("sync_covered_uid", u64::from(r.covered_through_uid())),
            ("sync_inserted", u64::from(r.inserted_count())),
        ] {
            out.push_str(&format!("{}: {n}\n", resource(lang, key)));
        }
        out.push_str(resource(lang, "sync_recovery_first_page"));
        out.push('\n');
    }
    if let Some(e) = value.error() {
        out.push_str(&format!(
            "{}: {}\n",
            resource(lang, "sync_error"),
            e.as_str()
        ));
    }
    out
}
#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
