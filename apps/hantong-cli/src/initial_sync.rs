//! Explicit initial-preview syntax and localized progress without message counts.
use crate::{ErrorCode, Lang, resource, sync::Action};
use hantong_contracts::{
    initial_sync::*,
    uid_page::{UidMailboxEncoding, UidMailboxIdentity, UidPageStatusRequest},
    wire_hex,
};

fn number(text: &str) -> Result<u64, ErrorCode> {
    if text.is_empty()
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(ErrorCode::InvalidInput);
    }
    text.parse().map_err(|_| ErrorCode::InvalidInput)
}
pub(super) fn parse(command: &[String], options: &[(String, String)]) -> Result<Action, ErrorCode> {
    if command.len() != 2 || command[0] != "sync" {
        return Err(ErrorCode::InvalidInput);
    }
    let mut values = std::collections::BTreeMap::new();
    for (key, value) in options {
        if values.insert(key.as_str(), value.as_str()).is_some() {
            return Err(ErrorCode::InvalidInput);
        }
    }
    let allowed: &[&str] = match command[1].as_str() {
        "initial-start" => &[
            "--account",
            "--wire-hex",
            "--encoding",
            "--operation-id",
            "--idempotency-key",
            "--expected-catalog-generation",
            "--expected-preview-generation",
            "--recent-pages",
        ],
        "initial-resume" => &[
            "--account",
            "--wire-hex",
            "--encoding",
            "--operation-id",
            "--idempotency-key",
            "--expected-catalog-generation",
            "--plan-id",
            "--expected-plan-version",
        ],
        "initial-pause" => &[
            "--account",
            "--wire-hex",
            "--encoding",
            "--operation-id",
            "--idempotency-key",
            "--plan-id",
            "--expected-plan-version",
        ],
        "initial-status" => &["--account", "--execution-id"],
        "initial-plan" => &["--account", "--wire-hex", "--encoding", "--plan-id"],
        _ => return Err(ErrorCode::InvalidInput),
    };
    if values.len() != allowed.len() || values.keys().any(|key| !allowed.contains(key)) {
        return Err(ErrorCode::InvalidInput);
    }
    let get = |key: &str| values.get(key).copied().ok_or(ErrorCode::InvalidInput);
    let account = get("--account")?;
    if command[1] == "initial-status" {
        return UidPageStatusRequest::new(account, wire_hex::decode_fixed(get("--execution-id")?)?)
            .map(Action::InitialStatus);
    }
    let mailbox = UidMailboxIdentity::new(
        get("--wire-hex")?,
        UidMailboxEncoding::parse(get("--encoding")?)?,
    )?;
    if command[1] == "initial-plan" {
        return InitialSyncPlanRequest::new(
            account,
            mailbox,
            wire_hex::decode_fixed(get("--plan-id")?)?,
        )
        .map(Action::InitialPlan);
    }
    // A placeholder hash validates syntax only. The platform client calculates
    // the intent after parsing, and Core independently verifies it.
    let mutation = InitialSyncMutation::new(
        get("--operation-id")?,
        get("--idempotency-key")?,
        &"00".repeat(32),
    )?;
    let request = match command[1].as_str() {
        "initial-start" => InitialSyncRequest::new_start(
            account,
            mailbox,
            mutation,
            number(get("--expected-catalog-generation")?)?,
            number(get("--expected-preview-generation")?)?,
            u8::try_from(number(get("--recent-pages")?)?).map_err(|_| ErrorCode::InvalidInput)?,
        ),
        "initial-resume" => InitialSyncRequest::new_resume(
            account,
            mailbox,
            mutation,
            wire_hex::decode_fixed(get("--plan-id")?)?,
            number(get("--expected-plan-version")?)?,
            number(get("--expected-catalog-generation")?)?,
        ),
        "initial-pause" => InitialSyncRequest::new_pause(
            account,
            mailbox,
            mutation,
            wire_hex::decode_fixed(get("--plan-id")?)?,
            number(get("--expected-plan-version")?)?,
        ),
        _ => return Err(ErrorCode::InvalidInput),
    }?;
    Ok(Action::InitialMutation(request))
}

pub(super) fn plan_human(value: &InitialSyncPlanSnapshot, lang: Lang) -> String {
    let p = value.progress();
    let state = match p.state() {
        InitialSyncState::Ready => "initial_ready",
        InitialSyncState::Running => "initial_running",
        InitialSyncState::Pausing => "initial_pausing",
        InitialSyncState::PausedUser => "initial_paused_user",
        InitialSyncState::PausedBudget => "initial_paused_budget",
        InitialSyncState::Interrupted => "initial_interrupted",
        InitialSyncState::Complete => "initial_complete",
        InitialSyncState::Failed => "initial_failed",
        InitialSyncState::StaleBinding => "initial_stale_binding",
        InitialSyncState::NeedsRecovery => "initial_needs_recovery",
    };
    let optional = |v: Option<u32>| {
        v.map(|n| n.to_string())
            .unwrap_or_else(|| resource(lang, "initial_none").into())
    };
    format!(
        "{}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}: {}\n{}\n",
        resource(lang, "initial_title"),
        resource(lang, "initial_plan_id"),
        wire_hex::encode(value.plan_id()),
        resource(lang, "initial_plan_version"),
        value.version(),
        resource(lang, "initial_plan_state"),
        resource(lang, state),
        resource(lang, "initial_current"),
        resource(
            lang,
            if value.is_current() {
                "diagnostics_yes"
            } else {
                "diagnostics_no"
            }
        ),
        resource(lang, "sync_binding"),
        resource(
            lang,
            if value.binding_current() {
                "sync_current"
            } else {
                "sync_stale"
            }
        ),
        resource(lang, "initial_ceiling"),
        p.target_uid_ceiling(),
        resource(lang, "initial_lower"),
        optional(p.selected_lower_uid()),
        resource(lang, "initial_selected_pages"),
        p.selected_page_count(),
        resource(lang, "initial_completed_pages"),
        p.completed_page_count(),
        resource(lang, "initial_remaining_pages"),
        p.remaining_page_count(),
        resource(lang, "initial_remaining_span"),
        p.remaining_uid_span(),
        resource(lang, "initial_older"),
        optional(p.not_selected_before_uid()),
        resource(lang, "initial_stored_at"),
        value.stored_at_utc_ms(),
        resource(lang, "initial_scope_note")
    )
}
pub(super) fn receipt_human(value: &InitialSyncReceipt, lang: Lang) -> String {
    let state = match value.state() {
        InitialSyncJobState::Admitted => "initial_job_admitted",
        InitialSyncJobState::Running => "initial_job_running",
        InitialSyncJobState::Succeeded => "initial_job_succeeded",
        InitialSyncJobState::Failed => "initial_job_failed",
    };
    let mut text = format!(
        "{}: {}\n{}: {}\n{}: {}\n",
        resource(lang, "initial_execution"),
        wire_hex::encode(value.execution_id()),
        resource(lang, "initial_action"),
        value.action().as_str(),
        resource(lang, "initial_job_state"),
        resource(lang, state)
    );
    if let Some(plan) = value.plan() {
        text.push_str(&plan_human(plan, lang));
    } else {
        text.push_str(resource(lang, "initial_unobserved"));
        text.push('\n');
    }
    if let Some(error) = value.error() {
        text.push_str(&format!(
            "{}: {}\n",
            resource(lang, "initial_error"),
            error.as_str()
        ));
    }
    text
}

#[cfg(test)]
#[path = "initial_sync_tests.rs"]
mod tests;
