//! Strict codecs for explicitly reading the first page after UIDVALIDITY changes.
use hantong_contracts::{
    ErrorCode,
    uid_page::{UidMailboxIdentity, UidPageState, UidPageStatusRequest},
    uid_recovery::{UidRecoveryProgress, UidRecoveryReceipt, UidRecoveryRequest},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireProgress {
    epoch_generation: u64,
    cursor_version: u64,
    covered_through_uid: u32,
    inserted_count: u32,
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
) -> Result<UidRecoveryReceipt, ErrorCode> {
    let r: WireReceipt = crate::uid_page::decode(bytes, expected_id)?;
    if r.schema_version != 1 || !r.ok || r.account_id != account {
        return Err(ErrorCode::InvalidInput);
    }
    let result = match r.result {
        WireResult::Null(()) => None,
        WireResult::Present(v) => Some(UidRecoveryProgress::new(
            v.epoch_generation,
            v.cursor_version,
            v.covered_through_uid,
            v.inserted_count,
        )?),
    };
    let error = if r.error_code.is_null() {
        None
    } else {
        Some(crate::uid_page::error_code(
            r.error_code.as_str().ok_or(ErrorCode::InvalidInput)?,
        )?)
    };
    UidRecoveryReceipt::new(
        &r.account_id,
        hantong_contracts::wire_hex::decode_fixed(&r.execution_id)?,
        UidPageState::parse(&r.state)?,
        result,
        error,
    )
}
pub fn parse_recovery_receipt(
    bytes: &[u8],
    expected_id: &str,
    request: &UidRecoveryRequest,
) -> Result<UidRecoveryReceipt, ErrorCode> {
    let r = parse_receipt(bytes, expected_id, request.account_id())?;
    if r.result()
        .is_some_and(|v| v.cursor_version() != request.expected_cursor_version() + 1)
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(r)
}
pub fn parse_status_receipt(
    bytes: &[u8],
    expected_id: &str,
    request: &UidPageStatusRequest,
) -> Result<UidRecoveryReceipt, ErrorCode> {
    let r = parse_receipt(bytes, expected_id, request.account_id())?;
    if r.execution_id() != request.execution_id() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(r)
}
pub fn receipt_json(v: &UidRecoveryReceipt) -> String {
    let result=v.result().map(|r|json!({"epoch_generation":r.epoch_generation(),"cursor_version":r.cursor_version(),"covered_through_uid":r.covered_through_uid(),"inserted_count":r.inserted_count()}));
    json!({"schema_version":1,"ok":true,"account_id":v.account_id(),"execution_id":hantong_contracts::wire_hex::encode(v.execution_id()),"state":v.state().as_str(),"result":result,"error_code":v.error().map(ErrorCode::as_str)}).to_string()
}
pub fn prepare_recovery_request(
    account: &str,
    mailbox: UidMailboxIdentity,
    operation: &str,
    key: &str,
    catalog_generation: u64,
    cursor_version: u64,
) -> Result<UidRecoveryRequest, ErrorCode> {
    let request = UidRecoveryRequest::new(
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
        UidRecoveryRequest::new(
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
pub fn recover_default(request: UidRecoveryRequest) -> Result<UidRecoveryReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        let expected = prepare_recovery_request(
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
        let params = json!({"action":"recover","account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_catalog_generation":request.expected_catalog_generation(),"expected_cursor_version":request.expected_cursor_version(),"intent_hash":request.intent_hash()});
        parse_recovery_receipt(
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
pub fn status_default(request: UidPageStatusRequest) -> Result<UidRecoveryReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        let params = json!({"action":"recover-status","account_id":request.account_id(),"execution_id":hantong_contracts::wire_hex::encode(request.execution_id())});
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
#[path = "uid_recovery_tests.rs"]
mod tests;
