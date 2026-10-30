//! Strict codecs for explicitly reconciling one previously stored UID page.
use hantong_contracts::{
    ErrorCode,
    uid_page::{UidMailboxIdentity, UidPageState, UidPageStatusRequest},
    uid_reconcile::{UidReconcileProgress, UidReconcileReceipt, UidReconcileRequest},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireProgress {
    generation: u64,
    frozen_cursor_version: u64,
    source_cursor_version: u64,
    after_uid: u32,
    through_uid: u32,
    observed_count: u32,
    flags_changed_count: u32,
    newly_absent_count: u32,
    remaining_pages: u32,
    pass_complete: bool,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum WireResult {
    Present(WireProgress),
    Null(()),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireReceipt {
    schema_version: u64,
    ok: bool,
    account_id: String,
    execution_id: String,
    state: String,
    result: WireResult,
    error_code: Value,
}
fn parse_receipt(
    bytes: &[u8],
    expected_id: &str,
    account: &str,
) -> Result<UidReconcileReceipt, ErrorCode> {
    let r: WireReceipt = crate::uid_page::decode(bytes, expected_id)?;
    if r.schema_version != 1 || !r.ok || r.account_id != account {
        return Err(ErrorCode::InvalidInput);
    }
    let result = match r.result {
        WireResult::Null(()) => None,
        WireResult::Present(v) => Some(UidReconcileProgress::new(
            v.generation,
            v.frozen_cursor_version,
            v.source_cursor_version,
            v.after_uid,
            v.through_uid,
            v.observed_count,
            v.flags_changed_count,
            v.newly_absent_count,
            v.remaining_pages,
            v.pass_complete,
        )?),
    };
    let error = if r.error_code.is_null() {
        None
    } else {
        Some(crate::uid_page::error_code(
            r.error_code.as_str().ok_or(ErrorCode::InvalidInput)?,
        )?)
    };
    UidReconcileReceipt::new(
        &r.account_id,
        hantong_contracts::wire_hex::decode_fixed(&r.execution_id)?,
        UidPageState::parse(&r.state)?,
        result,
        error,
    )
}
pub fn parse_reconcile_receipt(
    bytes: &[u8],
    expected_id: &str,
    request: &UidReconcileRequest,
) -> Result<UidReconcileReceipt, ErrorCode> {
    let r = parse_receipt(bytes, expected_id, request.account_id())?;
    if r.result()
        .is_some_and(|v| v.frozen_cursor_version() > request.expected_cursor_version())
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(r)
}
pub fn parse_status_receipt(
    bytes: &[u8],
    expected_id: &str,
    request: &UidPageStatusRequest,
) -> Result<UidReconcileReceipt, ErrorCode> {
    let r = parse_receipt(bytes, expected_id, request.account_id())?;
    if r.execution_id() != request.execution_id() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(r)
}
pub fn receipt_json(v: &UidReconcileReceipt) -> String {
    let result=v.result().map(|r|json!({"generation":r.generation(),"frozen_cursor_version":r.frozen_cursor_version(),"source_cursor_version":r.source_cursor_version(),"after_uid":r.after_uid(),"through_uid":r.through_uid(),"observed_count":r.observed_count(),"flags_changed_count":r.flags_changed_count(),"newly_absent_count":r.newly_absent_count(),"remaining_pages":r.remaining_pages(),"pass_complete":r.pass_complete()}));
    json!({"schema_version":1,"ok":true,"account_id":v.account_id(),"execution_id":hantong_contracts::wire_hex::encode(v.execution_id()),"state":v.state().as_str(),"result":result,"error_code":v.error().map(ErrorCode::as_str)}).to_string()
}
pub fn prepare_reconcile_request(
    account: &str,
    mailbox: UidMailboxIdentity,
    operation: &str,
    key: &str,
    catalog_generation: u64,
    cursor_version: u64,
) -> Result<UidReconcileRequest, ErrorCode> {
    let request = UidReconcileRequest::new(
        account,
        mailbox,
        operation,
        key,
        catalog_generation,
        cursor_version,
        &"00".repeat(32),
    )?;
    #[cfg(windows)]
    {
        let hash = hantong_contracts::wire_hex::encode(&hantong_platform::sha256(
            &request.intent_bytes(),
        )?);
        UidReconcileRequest::new(
            account,
            request.mailbox().clone(),
            operation,
            key,
            catalog_generation,
            cursor_version,
            &hash,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn reconcile_default(request: UidReconcileRequest) -> Result<UidReconcileReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        let expected = prepare_reconcile_request(
            request.account_id(),
            request.mailbox().clone(),
            request.operation_id(),
            request.idempotency_key(),
            request.expected_catalog_generation(),
            request.expected_cursor_version(),
        )?;
        if expected.intent_hash() != request.intent_hash() {
            return Err(ErrorCode::InvalidInput);
        }
        let params = json!({"action":"reconcile","account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_catalog_generation":request.expected_catalog_generation(),"expected_cursor_version":request.expected_cursor_version(),"intent_hash":request.intent_hash()});
        parse_reconcile_receipt(
            &crate::uid_page::exchange(params, request.account_id())?,
            "uid-page-1",
            &request,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn status_default(request: UidPageStatusRequest) -> Result<UidReconcileReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        let params = json!({"action":"reconcile-status","account_id":request.account_id(),"execution_id":hantong_contracts::wire_hex::encode(request.execution_id())});
        parse_status_receipt(
            &crate::uid_page::exchange(params, request.account_id())?,
            "uid-page-1",
            &request,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(test)]
#[path = "uid_reconcile_tests.rs"]
mod tests;
