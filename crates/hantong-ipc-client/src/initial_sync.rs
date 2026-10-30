//! Strict initial-preview requests and responses over the existing UID tool.
use crate::{
    accounts::Object,
    display_settings::{Nullable, decode_app_response},
};
use hantong_contracts::{
    ErrorCode,
    initial_sync::*,
    uid_page::{UidMailboxEncoding, UidMailboxIdentity, UidPageStatusRequest},
    wire_hex,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgressWire {
    uid_validity: u32,
    uid_next: u32,
    recent_pages: u8,
    completed_page_count: u8,
    state: String,
    model_version: u64,
    target_uid_ceiling: u32,
    selected_lower_uid: Nullable<u32>,
    selected_page_count: u8,
    remaining_page_count: u8,
    remaining_uid_span: u32,
    not_selected_before_uid: Nullable<u32>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanWire {
    plan_id: String,
    account_id: String,
    wire_hex: String,
    encoding: String,
    generation: u64,
    version: u64,
    is_current: bool,
    binding_current: bool,
    stored_at_utc_ms: i64,
    progress: Object<ProgressWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptWire {
    schema_version: u32,
    ok: bool,
    account_id: String,
    execution_id: String,
    action: String,
    state: String,
    plan: Nullable<Object<PlanWire>>,
    error_code: Nullable<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanResponse {
    schema_version: u32,
    ok: bool,
    plan: Object<PlanWire>,
}
fn fixed_id(value: &str) -> Result<[u8; 32], ErrorCode> {
    let decoded = wire_hex::decode_fixed(value)?;
    if wire_hex::encode(&decoded) != value {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(decoded)
}
fn plan(raw: PlanWire) -> Result<InitialSyncPlanSnapshot, ErrorCode> {
    let p = raw.progress.0;
    let progress = InitialSyncProgress::new(
        p.uid_validity,
        p.uid_next,
        p.recent_pages,
        p.completed_page_count,
        InitialSyncState::parse(&p.state)?,
        p.model_version,
    )?;
    if progress.target_uid_ceiling() != p.target_uid_ceiling
        || progress.selected_lower_uid() != p.selected_lower_uid.into_option()
        || progress.selected_page_count() != p.selected_page_count
        || progress.remaining_page_count() != p.remaining_page_count
        || progress.remaining_uid_span() != p.remaining_uid_span
        || progress.not_selected_before_uid() != p.not_selected_before_uid.into_option()
    {
        return Err(ErrorCode::InvalidInput);
    }
    let identity = InitialSyncPlanIdentity::new(
        fixed_id(&raw.plan_id)?,
        &raw.account_id,
        UidMailboxIdentity::new(&raw.wire_hex, UidMailboxEncoding::parse(&raw.encoding)?)?,
        raw.generation,
    )?;
    InitialSyncPlanSnapshot::new(
        identity,
        raw.version,
        progress,
        raw.is_current,
        raw.binding_current,
        raw.stored_at_utc_ms,
    )
}
fn receipt(bytes: &[u8], id: &str, account: &str) -> Result<InitialSyncReceipt, ErrorCode> {
    let raw: ReceiptWire = decode_app_response(bytes, id, 8192)?;
    if raw.schema_version != 1 || !raw.ok || raw.account_id != account {
        return Err(ErrorCode::InvalidInput);
    }
    let error = raw
        .error_code
        .into_option()
        .map(|text| crate::uid_page::error_code(&text))
        .transpose()?;
    let receipt = InitialSyncReceipt::new(
        &raw.account_id,
        fixed_id(&raw.execution_id)?,
        InitialSyncAction::parse(&raw.action)?,
        InitialSyncJobState::parse(&raw.state)?,
        raw.plan.into_option().map(|v| plan(v.0)).transpose()?,
        error,
    )?;
    if receipt.action() == InitialSyncAction::Start
        && receipt
            .plan()
            .is_some_and(|p| p.plan_id() != receipt.execution_id())
    {
        return Err(ErrorCode::InvalidInput);
    }
    if receipt.state() == InitialSyncJobState::Succeeded {
        let p = receipt.plan().ok_or(ErrorCode::InvalidInput)?;
        match receipt.action() {
            InitialSyncAction::Start
                if p.version() != 1
                    || p.progress().model_version() != 1
                    || p.progress().completed_page_count() != 0
                    || !matches!(
                        p.progress().state(),
                        InitialSyncState::Ready | InitialSyncState::Complete
                    ) =>
            {
                return Err(ErrorCode::InvalidInput);
            }
            InitialSyncAction::Resume
                if !matches!(
                    p.progress().state(),
                    InitialSyncState::PausedBudget | InitialSyncState::Complete
                ) =>
            {
                return Err(ErrorCode::InvalidInput);
            }
            InitialSyncAction::Pause
                if !matches!(
                    p.progress().state(),
                    InitialSyncState::Pausing | InitialSyncState::PausedUser
                ) =>
            {
                return Err(ErrorCode::InvalidInput);
            }
            _ => {}
        }
    }
    Ok(receipt)
}
pub fn parse_mutation_receipt(
    bytes: &[u8],
    id: &str,
    request: &InitialSyncRequest,
) -> Result<InitialSyncReceipt, ErrorCode> {
    let value = receipt(bytes, id, request.account_id())?;
    if value.action() != request.action() {
        return Err(ErrorCode::InvalidInput);
    }
    if let Some(p) = value.plan() {
        if p.mailbox() != request.mailbox()
            || request
                .plan_id()
                .is_some_and(|expected| expected != p.plan_id())
        {
            return Err(ErrorCode::InvalidInput);
        }
        if value.state() == InitialSyncJobState::Succeeded
            && (request
                .expected_plan_version()
                .is_some_and(|expected| p.version() <= expected)
                || request
                    .expected_preview_generation()
                    .is_some_and(|expected| p.generation() != expected + 1)
                || request
                    .recent_pages()
                    .is_some_and(|recent| p.progress().recent_pages() != recent))
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    Ok(value)
}
pub fn parse_status_receipt(
    bytes: &[u8],
    id: &str,
    request: &UidPageStatusRequest,
) -> Result<InitialSyncReceipt, ErrorCode> {
    let value = receipt(bytes, id, request.account_id())?;
    if value.execution_id() != request.execution_id() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(value)
}
pub fn parse_plan_snapshot(
    bytes: &[u8],
    id: &str,
    request: &InitialSyncPlanRequest,
) -> Result<InitialSyncPlanSnapshot, ErrorCode> {
    let raw: PlanResponse = decode_app_response(bytes, id, 8192)?;
    if raw.schema_version != 1 || !raw.ok {
        return Err(ErrorCode::InvalidInput);
    }
    let value = plan(raw.plan.0)?;
    if value.account_id() != request.account_id()
        || value.mailbox() != request.mailbox()
        || value.plan_id() != request.plan_id()
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(value)
}

fn plan_value(v: &InitialSyncPlanSnapshot) -> Value {
    let p = v.progress();
    json!({"plan_id":wire_hex::encode(v.plan_id()),"account_id":v.account_id(),"wire_hex":v.mailbox().wire_hex(),"encoding":v.mailbox().encoding().as_str(),
        "generation":v.generation(),"version":v.version(),"is_current":v.is_current(),"binding_current":v.binding_current(),"stored_at_utc_ms":v.stored_at_utc_ms(),
        "progress":{"uid_validity":p.uid_validity(),"uid_next":p.uid_next(),"recent_pages":p.recent_pages(),"completed_page_count":p.completed_page_count(),
            "state":p.state().as_str(),"model_version":p.model_version(),"target_uid_ceiling":p.target_uid_ceiling(),"selected_lower_uid":p.selected_lower_uid(),
            "selected_page_count":p.selected_page_count(),"remaining_page_count":p.remaining_page_count(),"remaining_uid_span":p.remaining_uid_span(),"not_selected_before_uid":p.not_selected_before_uid()}})
}
pub fn receipt_json(v: &InitialSyncReceipt) -> String {
    json!({"schema_version":1,"ok":true,"account_id":v.account_id(),"execution_id":wire_hex::encode(v.execution_id()),"action":v.action().as_str(),
        "state":v.state().as_str(),"plan":v.plan().map(plan_value),"error_code":v.error().map(ErrorCode::as_str)}).to_string()
}
pub fn plan_json(v: &InitialSyncPlanSnapshot) -> String {
    json!({"schema_version":1,"ok":true,"plan":plan_value(v)}).to_string()
}

pub fn prepare_request(request: InitialSyncRequest) -> Result<InitialSyncRequest, ErrorCode> {
    #[cfg(windows)]
    {
        let hash = wire_hex::encode(&hantong_platform::sha256(&request.intent_bytes())?);
        let mutation =
            InitialSyncMutation::new(request.operation_id(), request.idempotency_key(), &hash)?;
        match request.action() {
            InitialSyncAction::Start => InitialSyncRequest::new_start(
                request.account_id(),
                request.mailbox().clone(),
                mutation,
                request.expected_catalog_generation().unwrap(),
                request.expected_preview_generation().unwrap(),
                request.recent_pages().unwrap(),
            ),
            InitialSyncAction::Resume => InitialSyncRequest::new_resume(
                request.account_id(),
                request.mailbox().clone(),
                mutation,
                *request.plan_id().unwrap(),
                request.expected_plan_version().unwrap(),
                request.expected_catalog_generation().unwrap(),
            ),
            InitialSyncAction::Pause => InitialSyncRequest::new_pause(
                request.account_id(),
                request.mailbox().clone(),
                mutation,
                *request.plan_id().unwrap(),
                request.expected_plan_version().unwrap(),
            ),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(any(windows, test))]
fn mutation_params(request: &InitialSyncRequest) -> Value {
    let mut params = json!({"action":request.action().as_str(),"account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),
        "encoding":request.mailbox().encoding().as_str(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"intent_hash":request.intent_hash()});
    if let Some(value) = request.expected_catalog_generation() {
        params["expected_catalog_generation"] = json!(value);
    }
    if let Some(value) = request.expected_preview_generation() {
        params["expected_preview_generation"] = json!(value);
    }
    if let Some(value) = request.recent_pages() {
        params["recent_pages"] = json!(value);
    }
    if let Some(value) = request.plan_id() {
        params["plan_id"] = json!(wire_hex::encode(value));
    }
    if let Some(value) = request.expected_plan_version() {
        params["expected_plan_version"] = json!(value);
    }
    params
}
pub fn mutate_default(request: InitialSyncRequest) -> Result<InitialSyncReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        if prepare_request(request.clone())?.intent_hash() != request.intent_hash() {
            return Err(ErrorCode::InvalidInput);
        }
        let raw = crate::uid_page::exchange(mutation_params(&request), request.account_id())?;
        parse_mutation_receipt(&raw, "uid-page-1", &request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn status_default(request: UidPageStatusRequest) -> Result<InitialSyncReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        let params = json!({"action":"initial-status","account_id":request.account_id(),"execution_id":wire_hex::encode(request.execution_id())});
        let raw = crate::uid_page::exchange(params, request.account_id())?;
        parse_status_receipt(&raw, "uid-page-1", &request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn plan_default(request: InitialSyncPlanRequest) -> Result<InitialSyncPlanSnapshot, ErrorCode> {
    #[cfg(windows)]
    {
        let params = json!({"action":"initial-plan","account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),
            "encoding":request.mailbox().encoding().as_str(),"plan_id":wire_hex::encode(request.plan_id())});
        let raw = crate::uid_page::exchange(params, request.account_id())?;
        parse_plan_snapshot(&raw, "uid-page-1", &request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(test)]
#[path = "initial_sync_tests.rs"]
mod tests;
