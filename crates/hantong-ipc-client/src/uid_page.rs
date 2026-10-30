//! Strict bounded codecs for the explicit public UID-page operation.
use hantong_contracts::{ErrorCode, uid_page::*};
use serde::Deserialize;
use serde_json::{Value, json};
const MAX_FRAME: usize = 8192;
#[cfg(any(windows, test))]
const TOOL: &str = "mail.sync.page";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Success<T> {
    jsonrpc: String,
    id: String,
    result: T,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    jsonrpc: String,
    id: String,
    error: WireError,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireError {
    code: String,
    message: String,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Rpc<T> {
    Success(Success<T>),
    Failure(Failure),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireReceipt {
    schema_version: u64,
    ok: bool,
    account_id: String,
    execution_id: String,
    state: String,
    cursor_version: Value,
    covered_through_uid: Value,
    inserted_count: Value,
    error_code: Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireCursor {
    schema_version: u64,
    ok: bool,
    account_id: String,
    wire_hex: String,
    encoding: String,
    catalog_generation: u64,
    cursor_version: u64,
    covered_through_uid: u32,
    binding_current: bool,
}
pub(super) fn decode<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    expected_id: &str,
) -> Result<T, ErrorCode> {
    if bytes.is_empty()
        || bytes.len() > MAX_FRAME
        || expected_id.is_empty()
        || expected_id.len() > 64
        || !expected_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
    {
        return Err(ErrorCode::InvalidInput);
    }
    match serde_json::from_slice::<Rpc<T>>(bytes).map_err(|_| ErrorCode::InvalidInput)? {
        Rpc::Success(reply) if reply.jsonrpc == "2.0" && reply.id == expected_id => {
            Ok(reply.result)
        }
        Rpc::Failure(reply)
            if reply.jsonrpc == "2.0"
                && reply.id == expected_id
                && reply.error.code == reply.error.message =>
        {
            Err(error_code(&reply.error.code)?)
        }
        _ => Err(ErrorCode::InvalidInput),
    }
}
pub(super) fn error_code(value: &str) -> Result<ErrorCode, ErrorCode> {
    ErrorCode::ALL
        .into_iter()
        .find(|e| e.as_str() == value)
        .ok_or(ErrorCode::InvalidInput)
}
fn number(value: Value) -> Result<Option<u64>, ErrorCode> {
    if value.is_null() {
        Ok(None)
    } else {
        value.as_u64().map(Some).ok_or(ErrorCode::InvalidInput)
    }
}
fn small(value: Value) -> Result<Option<u32>, ErrorCode> {
    number(value)?
        .map(|v| u32::try_from(v).map_err(|_| ErrorCode::InvalidInput))
        .transpose()
}
fn parse_receipt(
    bytes: &[u8],
    expected_id: &str,
    account: &str,
) -> Result<UidPageReceipt, ErrorCode> {
    let r: WireReceipt = decode(bytes, expected_id)?;
    if r.schema_version != 1 || !r.ok || r.account_id != account {
        return Err(ErrorCode::InvalidInput);
    }
    let error = if r.error_code.is_null() {
        None
    } else {
        Some(error_code(
            r.error_code.as_str().ok_or(ErrorCode::InvalidInput)?,
        )?)
    };
    UidPageReceipt::new(
        &r.account_id,
        hantong_contracts::wire_hex::decode_fixed(&r.execution_id)?,
        UidPageState::parse(&r.state)?,
        number(r.cursor_version)?,
        small(r.covered_through_uid)?,
        small(r.inserted_count)?,
        error,
    )
}
pub fn parse_page_receipt(
    bytes: &[u8],
    expected_id: &str,
    request: &UidPageRequest,
) -> Result<UidPageReceipt, ErrorCode> {
    let receipt = parse_receipt(bytes, expected_id, request.account_id())?;
    if receipt.state() == UidPageState::Succeeded
        && receipt.cursor_version() != Some(request.expected_cursor_version() + 1)
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(receipt)
}
pub fn parse_status_receipt(
    bytes: &[u8],
    expected_id: &str,
    request: &UidPageStatusRequest,
) -> Result<UidPageReceipt, ErrorCode> {
    let receipt = parse_receipt(bytes, expected_id, request.account_id())?;
    if receipt.execution_id() != request.execution_id() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(receipt)
}
pub fn parse_cursor_snapshot(
    bytes: &[u8],
    expected_id: &str,
    request: &UidCursorRequest,
) -> Result<UidCursorSnapshot, ErrorCode> {
    let r: WireCursor = decode(bytes, expected_id)?;
    let mailbox = UidMailboxIdentity::new(&r.wire_hex, UidMailboxEncoding::parse(&r.encoding)?)?;
    if r.schema_version != 1
        || !r.ok
        || r.account_id != request.account_id()
        || mailbox != *request.mailbox()
    {
        return Err(ErrorCode::InvalidInput);
    }
    UidCursorSnapshot::new(
        &r.account_id,
        mailbox,
        r.catalog_generation,
        r.cursor_version,
        r.covered_through_uid,
        r.binding_current,
    )
}
pub fn receipt_json(receipt: &UidPageReceipt) -> String {
    json!({"schema_version":1,"ok":true,"account_id":receipt.account_id(),"execution_id":hantong_contracts::wire_hex::encode(receipt.execution_id()),"state":receipt.state().as_str(),"cursor_version":receipt.cursor_version(),"covered_through_uid":receipt.covered_through_uid(),"inserted_count":receipt.inserted_count(),"error_code":receipt.error().map(ErrorCode::as_str)}).to_string()
}
pub fn cursor_json(cursor: &UidCursorSnapshot) -> String {
    json!({"schema_version":1,"ok":true,"account_id":cursor.account_id(),"wire_hex":cursor.mailbox().wire_hex(),"encoding":cursor.mailbox().encoding().as_str(),"catalog_generation":cursor.catalog_generation(),"cursor_version":cursor.cursor_version(),"covered_through_uid":cursor.covered_through_uid(),"binding_current":cursor.binding_current()}).to_string()
}
pub fn prepare_page_request(
    account: &str,
    mailbox: UidMailboxIdentity,
    operation: &str,
    key: &str,
    catalog_generation: u64,
    cursor_version: u64,
) -> Result<UidPageRequest, ErrorCode> {
    let request = UidPageRequest::new(
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
        UidPageRequest::new(
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
#[cfg(any(windows, test))]
fn authorize(
    scope: &hantong_contracts::auth::RequestedScope,
    account: &str,
) -> Result<(), ErrorCode> {
    if scope.permits_account(account, TOOL) {
        Ok(())
    } else {
        Err(ErrorCode::ScopeDenied)
    }
}
#[cfg(windows)]
pub(super) fn exchange(params: Value, account: &str) -> Result<Vec<u8>, ErrorCode> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(async {
            let id = "uid-page-1";
            let envelope = json!({"schema_version":1,"request_id":id,"tool":TOOL,"params":params});
            if envelope.to_string().len() + 128 > MAX_FRAME {
                return Err(ErrorCode::InvalidInput);
            }
            let profile = crate::authenticate_default().await?;
            authorize(profile.scope(), account)?;
            let bytes = profile.exchange(id, TOOL, &envelope).await?;
            if bytes.len() > MAX_FRAME {
                return Err(ErrorCode::InvalidInput);
            }
            Ok(bytes)
        })
}
pub fn page_default(request: UidPageRequest) -> Result<UidPageReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        let expected = prepare_page_request(
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
        let params = json!({"action":"request","account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_catalog_generation":request.expected_catalog_generation(),"expected_cursor_version":request.expected_cursor_version(),"intent_hash":request.intent_hash()});
        parse_page_receipt(
            &exchange(params, request.account_id())?,
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
pub fn cursor_default(request: UidCursorRequest) -> Result<UidCursorSnapshot, ErrorCode> {
    #[cfg(windows)]
    {
        let params = json!({"action":"cursor","account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str()});
        parse_cursor_snapshot(
            &exchange(params, request.account_id())?,
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
pub fn status_default(request: UidPageStatusRequest) -> Result<UidPageReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        let params = json!({"action":"status","account_id":request.account_id(),"execution_id":hantong_contracts::wire_hex::encode(request.execution_id())});
        parse_status_receipt(
            &exchange(params, request.account_id())?,
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
#[path = "uid_page_tests.rs"]
mod tests;
