//! Strict, bounded client codecs for the public folder tools.
use hantong_contracts::{
    ErrorCode,
    folders::{FolderCursor, FolderListRequest, FolderRefreshRequest, FolderRefreshStatusRequest},
};
use serde_json::Value;
use std::fmt;

const MAX_FRAME: usize = 1_048_576;

#[derive(Clone, PartialEq, Eq)]
pub struct FolderItem {
    value: Value,
}
impl fmt::Debug for FolderItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FolderItem { <redacted> }")
    }
}
impl FolderItem {
    pub fn value(&self) -> &Value {
        &self.value
    }
    fn validate(value: Value) -> Result<Self, ErrorCode> {
        let object = value.as_object().ok_or(ErrorCode::InvalidInput)?;
        exact(
            object,
            &[
                "wire_hex",
                "display_name",
                "encoding",
                "delimiter",
                "no_select",
                "non_existent",
                "children",
                "roles",
            ],
        )?;
        let wire = string(object, "wire_hex")?;
        if wire.is_empty()
            || wire.len() > 2048
            || !wire.len().is_multiple_of(2)
            || !wire
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err(ErrorCode::InvalidInput);
        }
        if !(1..=1024).contains(&(wire.len() / 2)) {
            return Err(ErrorCode::InvalidInput);
        }
        let display = string(object, "display_name")?;
        if display.is_empty() || display.len() > 4096 || display.chars().any(char::is_control) {
            return Err(ErrorCode::InvalidInput);
        }
        if !matches!(string(object, "encoding")?, "modified_utf7" | "utf8") {
            return Err(ErrorCode::InvalidInput);
        }
        if let Some(d) = object["delimiter"].as_str() {
            if d.chars().count() != 1
                || !d.as_bytes()[0].is_ascii()
                || !(0x20..=0x7e).contains(&d.as_bytes()[0])
            {
                return Err(ErrorCode::InvalidInput);
            }
        } else if !object["delimiter"].is_null() {
            return Err(ErrorCode::InvalidInput);
        }
        if !object["no_select"].is_boolean() || !object["non_existent"].is_boolean() {
            return Err(ErrorCode::InvalidInput);
        }
        if !matches!(
            string(object, "children")?,
            "unknown" | "has_children" | "has_no_children"
        ) {
            return Err(ErrorCode::InvalidInput);
        }
        let roles = object["roles"].as_array().ok_or(ErrorCode::InvalidInput)?;
        let allowed = [
            "all", "archive", "drafts", "flagged", "junk", "sent", "trash",
        ];
        if roles.windows(2).any(|w| w[0].as_str() >= w[1].as_str())
            || roles
                .iter()
                .any(|r| !r.as_str().is_some_and(|v| allowed.contains(&v)))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self { value })
    }
    fn wire(&self) -> &str {
        self.value["wire_hex"].as_str().unwrap()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FolderPage {
    value: Value,
    items: Vec<FolderItem>,
}
impl fmt::Debug for FolderPage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FolderPage { <redacted> }")
    }
}
impl FolderPage {
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn items(&self) -> &[FolderItem] {
        &self.items
    }
    pub fn next_cursor(&self) -> Option<&str> {
        self.value["next_cursor"].as_str()
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.value).expect("validated folder page")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FolderRefreshReceipt {
    value: Value,
}
impl fmt::Debug for FolderRefreshReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FolderRefreshReceipt { <redacted> }")
    }
}
impl FolderRefreshReceipt {
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.value).expect("validated folder receipt")
    }
}

pub fn parse_refresh_receipt(
    bytes: &[u8],
    expected_id: &str,
    account_id: &str,
) -> Result<FolderRefreshReceipt, ErrorCode> {
    let value = decode_result(bytes, expected_id)?;
    FolderListRequest::new(account_id, None, None)?;
    let result = value.as_object().ok_or(ErrorCode::InvalidInput)?;
    exact(
        result,
        &[
            "schema_version",
            "ok",
            "account_id",
            "execution_id",
            "state",
            "generation",
            "error_code",
        ],
    )?;
    if result["schema_version"] != 1 || result["ok"] != true || result["account_id"] != account_id {
        return Err(ErrorCode::InvalidInput);
    }
    let execution = result["execution_id"]
        .as_str()
        .ok_or(ErrorCode::InvalidInput)?;
    if execution.len() != 64
        || !execution
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        || execution == "0".repeat(64)
    {
        return Err(ErrorCode::InvalidInput);
    }
    let state = result["state"].as_str().ok_or(ErrorCode::InvalidInput)?;
    let generation = &result["generation"];
    let error = &result["error_code"];
    match state {
        "unresolved" if !generation.is_null() || !error.is_null() => {
            return Err(ErrorCode::InvalidInput);
        }
        "succeeded"
            if generation
                .as_u64()
                .is_none_or(|n| n == 0 || n > i64::MAX as u64)
                || !error.is_null() =>
        {
            return Err(ErrorCode::InvalidInput);
        }
        "failed"
            if !generation.is_null()
                || error
                    .as_str()
                    .and_then(|v| ErrorCode::ALL.into_iter().find(|c| c.as_str() == v))
                    .is_none() =>
        {
            return Err(ErrorCode::InvalidInput);
        }
        "unresolved" | "succeeded" | "failed" => {}
        _ => return Err(ErrorCode::InvalidInput),
    }
    Ok(FolderRefreshReceipt { value })
}

pub fn parse_folder_page(
    bytes: &[u8],
    expected_id: &str,
    request: &FolderListRequest,
) -> Result<FolderPage, ErrorCode> {
    let value = decode_result(bytes, expected_id)?;
    let result = value.as_object().ok_or(ErrorCode::InvalidInput)?;
    exact(
        result,
        &[
            "schema_version",
            "ok",
            "items",
            "next_cursor",
            "account_results",
            "completeness",
            "warnings",
            "observed_at",
            "metadata",
        ],
    )?;
    if result["schema_version"] != 1 || result["ok"] != true {
        return Err(ErrorCode::InvalidInput);
    }
    let rows = result["items"].as_array().ok_or(ErrorCode::InvalidInput)?;
    if rows.len() > request.limit() as usize {
        return Err(ErrorCode::InvalidInput);
    }
    let items = rows
        .iter()
        .cloned()
        .map(FolderItem::validate)
        .collect::<Result<Vec<_>, _>>()?;
    if items.windows(2).any(|w| w[0].wire() >= w[1].wire()) {
        return Err(ErrorCode::InvalidInput);
    }
    let next = if result["next_cursor"].is_null() {
        None
    } else {
        Some(FolderCursor::parse(
            result["next_cursor"]
                .as_str()
                .ok_or(ErrorCode::InvalidInput)?,
        )?)
    };
    if next
        .as_ref()
        .is_some_and(|c| c.account_id() != request.account_id())
    {
        return Err(ErrorCode::InvalidInput);
    }
    let completeness = result["completeness"]
        .as_str()
        .ok_or(ErrorCode::InvalidInput)?;
    if completeness
        != if next.is_some() {
            "partial"
        } else {
            "complete"
        }
    {
        return Err(ErrorCode::InvalidInput);
    }
    if !result["warnings"].is_array()
        || !result["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v.is_string())
    {
        return Err(ErrorCode::InvalidInput);
    }
    if !valid_timestamp(
        result["observed_at"]
            .as_str()
            .ok_or(ErrorCode::InvalidInput)?,
    ) {
        return Err(ErrorCode::InvalidInput);
    }
    let metadata = result["metadata"]
        .as_object()
        .ok_or(ErrorCode::InvalidInput)?;
    exact(
        metadata,
        &[
            "source",
            "cached",
            "account_id",
            "generation",
            "account_version",
            "endpoint_version",
            "binding_current",
            "special_use_complete",
        ],
    )?;
    if metadata["source"] != "local_store"
        || metadata["cached"] != true
        || metadata["account_id"] != request.account_id()
        || metadata["generation"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > i64::MAX as u64)
        || metadata["account_version"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > i64::MAX as u64)
        || metadata["endpoint_version"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > i64::MAX as u64)
        || metadata["special_use_complete"] != false
    {
        return Err(ErrorCode::InvalidInput);
    }
    let binding_current = metadata["binding_current"]
        .as_bool()
        .ok_or(ErrorCode::InvalidInput)?;
    if request
        .cursor()
        .is_some_and(|c| metadata["generation"] != c.generation())
    {
        return Err(ErrorCode::CursorExpired);
    }
    if next.as_ref().is_some_and(|c| {
        metadata["generation"] != c.generation()
            || items
                .last()
                .is_none_or(|item| item.wire() != c.after_wire_hex())
    }) || request.cursor().is_some_and(|c| {
        items
            .first()
            .is_some_and(|item| item.wire() <= c.after_wire_hex())
    }) {
        return Err(ErrorCode::InvalidInput);
    }
    let stale = !binding_current;
    let warnings = result["warnings"].as_array().unwrap();
    if stale != (warnings.len() == 1 && warnings[0] == "STALE_BINDING")
        || (!stale && !warnings.is_empty())
    {
        return Err(ErrorCode::InvalidInput);
    }
    let account_results = result["account_results"]
        .as_array()
        .ok_or(ErrorCode::InvalidInput)?;
    if account_results.len() != 1 {
        return Err(ErrorCode::InvalidInput);
    }
    let row = account_results[0]
        .as_object()
        .ok_or(ErrorCode::InvalidInput)?;
    exact(row, &["account_id", "status", "items"])?;
    if row["account_id"] != request.account_id()
        || row["status"] != completeness
        || row["items"] != result["items"]
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(FolderPage { value, items })
}

pub fn list_default_folders(request: FolderListRequest) -> Result<FolderPage, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_| ErrorCode::CoreUnavailable)?.block_on(async move {
let profile=crate::authenticate_default().await?;
authorize_folder(profile.scope(),request.account_id(),"mail.folders.list")?;
let id="folders-list-1";
let mut arguments=serde_json::json!({"account_id":request.account_id(),"limit":request.limit()});
if let Some(cursor)=request.cursor() {arguments["cursor"]=serde_json::json!(cursor.encode());}
let params=serde_json::json!({"schema_version":1,"request_id":id,"tool":"mail.folders.list","params":arguments});
let raw=profile.exchange(id,"mail.folders.list",&params).await?;
parse_folder_page(&raw,id,&request) })
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

fn exact(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Result<(), ErrorCode> {
    if map.len() == keys.len() && map.keys().all(|k| keys.contains(&k.as_str())) {
        Ok(())
    } else {
        Err(ErrorCode::InvalidInput)
    }
}
fn string<'a>(map: &'a serde_json::Map<String, Value>, key: &str) -> Result<&'a str, ErrorCode> {
    map[key].as_str().ok_or(ErrorCode::InvalidInput)
}
fn valid_rpc_id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}
fn valid_timestamp(value: &str) -> bool {
    value.len() == 24 && value.as_bytes()[19] == b'.' && crate::accounts::valid_timestamp(value)
}

struct UniqueValue(Value);
impl<'de> serde::Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON value without duplicate keys")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Number(v.into())))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Number(v.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| UniqueValue(Value::Number(v)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(v.into())))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(v)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueValue(v)) = seq.next_element::<UniqueValue>()? {
                    values.push(v);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom("duplicate field"));
                    }
                    let UniqueValue(v) = map.next_value::<UniqueValue>()?;
                    values.insert(key, v);
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        d.deserialize_any(Visitor)
    }
}
fn decode_result(bytes: &[u8], expected_id: &str) -> Result<Value, ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME || !valid_rpc_id(expected_id) {
        return Err(ErrorCode::InvalidInput);
    }
    let UniqueValue(root) = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    let envelope = root.as_object().ok_or(ErrorCode::InvalidInput)?;
    if envelope.get("jsonrpc") != Some(&Value::String("2.0".into()))
        || envelope.get("id") != Some(&Value::String(expected_id.into()))
    {
        return Err(ErrorCode::InvalidInput);
    }
    if let Some(error) = envelope.get("error") {
        exact(envelope, &["jsonrpc", "id", "error"])?;
        let error = error.as_object().ok_or(ErrorCode::InvalidInput)?;
        exact(error, &["code", "message"])?;
        let code = string(error, "code")?;
        if string(error, "message")? != code {
            return Err(ErrorCode::InvalidInput);
        }
        return Err(ErrorCode::ALL
            .into_iter()
            .find(|v| v.as_str() == code)
            .ok_or(ErrorCode::InvalidInput)?);
    }
    exact(envelope, &["jsonrpc", "id", "result"])?;
    Ok(root["result"].clone())
}

pub fn parse_refresh_receipt_for_status(
    bytes: &[u8],
    expected_id: &str,
    request: &FolderRefreshStatusRequest,
) -> Result<FolderRefreshReceipt, ErrorCode> {
    let receipt = parse_refresh_receipt(bytes, expected_id, request.account_id())?;
    if receipt.value["execution_id"] != hantong_contracts::wire_hex::encode(request.execution_id())
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(receipt)
}

/// Computes an intent binding from explicit validated fields; never authorizes it.
pub fn prepare_refresh_request(
    account: &str,
    operation: &str,
    key: &str,
    generation: u64,
) -> Result<FolderRefreshRequest, ErrorCode> {
    let request = FolderRefreshRequest::new(account, operation, key, generation, &"0".repeat(64))?;
    #[cfg(windows)]
    {
        let hash = hantong_contracts::wire_hex::encode(&hantong_platform::sha256(
            &request.intent_bytes(),
        )?);
        FolderRefreshRequest::new(account, operation, key, generation, &hash)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(any(windows, test))]
fn authorize_folder(
    scope: &hantong_contracts::auth::RequestedScope,
    account: &str,
    tool: &str,
) -> Result<(), ErrorCode> {
    if scope.permits_account(account, tool) {
        Ok(())
    } else {
        Err(ErrorCode::ScopeDenied)
    }
}

pub fn refresh_default(request: FolderRefreshRequest) -> Result<FolderRefreshReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_| ErrorCode::CoreUnavailable)?.block_on(async move {
let expected=prepare_refresh_request(request.account_id(),request.operation_id(),request.idempotency_key(),request.expected_generation())?;
if expected.payload_hash()!=request.payload_hash(){return Err(ErrorCode::InvalidInput);}
let profile=crate::authenticate_default().await?;
authorize_folder(profile.scope(),request.account_id(),"mail.folders.refresh")?;
let id="folders-refresh-1";
let params=serde_json::json!({"schema_version":1,"request_id":id,"tool":"mail.folders.refresh","params":{"action":"request","account_id":request.account_id(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_generation":request.expected_generation(),"payload_hash":request.payload_hash()}});
let raw=profile.exchange(id,"mail.folders.refresh",&params).await?;
parse_refresh_receipt(&raw,id,request.account_id()) })
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn refresh_status_default(
    request: FolderRefreshStatusRequest,
) -> Result<FolderRefreshReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_| ErrorCode::CoreUnavailable)?.block_on(async move {
let profile=crate::authenticate_default().await?;
authorize_folder(profile.scope(),request.account_id(),"mail.folders.refresh")?;
let id="folders-refresh-status-1";
let params=serde_json::json!({"schema_version":1,"request_id":id,"tool":"mail.folders.refresh","params":{"action":"status","account_id":request.account_id(),"execution_id":hantong_contracts::wire_hex::encode(request.execution_id())}});
let raw=profile.exchange(id,"mail.folders.refresh",&params).await?;
parse_refresh_receipt_for_status(&raw,id,&request) })
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn req() -> FolderListRequest {
        FolderListRequest::new("acct", Some(2), None).unwrap()
    }
    #[test]
    fn m203_folder_page_accepts_bound_complete_page() {
        let raw = br#"{"jsonrpc":"2.0","id":"r","result":{"schema_version":1,"ok":true,"items":[],"next_cursor":null,"account_results":[{"account_id":"acct","status":"complete","items":[]}],"completeness":"complete","warnings":[],"observed_at":"2026-10-30T00:00:00.000Z","metadata":{"source":"local_store","cached":true,"account_id":"acct","generation":1,"account_version":1,"endpoint_version":1,"binding_current":true,"special_use_complete":false}}}"#;
        assert!(parse_folder_page(raw, "r", &req()).is_ok());
    }
    #[test]
    fn m203_refresh_receipt_rejects_state_shape_mismatch() {
        let raw = br#"{"jsonrpc":"2.0","id":"r","result":{"schema_version":1,"ok":true,"account_id":"acct","execution_id":"0000000000000000000000000000000000000000000000000000000000000001","state":"succeeded","generation":null,"error_code":null}}"#;
        assert_eq!(
            parse_refresh_receipt(raw, "r", "acct"),
            Err(ErrorCode::InvalidInput)
        );
    }
}

#[cfg(test)]
#[path = "folder_security_tests.rs"]
mod security_tests;
