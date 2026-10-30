//! Bounded local metadata responses decoded from the original JSON stream.
use crate::accounts::Object;
use hantong_contracts::{
    ErrorCode,
    message_search::{
        HeaderAddressProjection, HeaderProjection, ImapMetadataFilter, ImapMetadataSearchCursor,
        ImapMetadataSearchItem, ImapMetadataSearchRequest,
    },
};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeSet, fmt};
const MAX_FRAME: usize = 1_048_576;
const MAX_RESULT: usize = 524_288;

#[derive(Clone)]
pub struct ValidatedMessageSearchResponse {
    value: Value,
    items: Vec<ImapMetadataSearchItem>,
    headers: Vec<HeaderProjection>,
}
impl ValidatedMessageSearchResponse {
    pub fn value(&self) -> &Value {
        &self.value
    }
    pub fn items(&self) -> &[ImapMetadataSearchItem] {
        &self.items
    }
    pub fn headers(&self) -> &[HeaderProjection] {
        &self.headers
    }
    pub fn next_cursor(&self) -> Option<&str> {
        self.value["next_cursor"].as_str()
    }
    pub fn completeness(&self) -> &str {
        self.value["completeness"]
            .as_str()
            .expect("validated completeness")
    }
    pub fn observed_at(&self) -> Option<&str> {
        self.value["observed_at"].as_str()
    }
}
impl fmt::Debug for ValidatedMessageSearchResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ValidatedMessageSearchResponse(<redacted>)")
    }
}
// Required fields: omission is not silently converted into None.
#[derive(Deserialize)]
#[serde(untagged)]
enum Nullable<T> {
    Present(T),
    Null(()),
}
impl<T> Nullable<T> {
    fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Present(v) => Some(v),
            Self::Null(()) => None,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Success {
    jsonrpc: String,
    id: String,
    result: Object<ResultWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    jsonrpc: String,
    id: String,
    error: Object<ErrorWire>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorWire {
    code: String,
    message: String,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Rpc {
    Success(Object<Success>),
    Failure(Object<Failure>),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultWire {
    schema_version: u32,
    ok: bool,
    items: Vec<Object<ItemWire>>,
    next_cursor: Nullable<String>,
    account_results: Vec<Object<AccountWire>>,
    completeness: String,
    warnings: Vec<String>,
    observed_at: Nullable<String>,
    metadata: Object<MetadataWire>,
    untrusted_content: Vec<Object<HeaderWire>>,
}
#[derive(Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ItemWire {
    internal_message_id: String,
    account_id: String,
    uid: u32,
    internal_date_utc_ms: i64,
    size_bytes: u32,
    is_read: bool,
    is_favorite: bool,
    header_content_index: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AccountWire {
    account_id: String,
    status: String,
    items: Vec<Object<ItemWire>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetadataWire {
    provider: String,
    source: String,
    cached: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeaderWire {
    content_type: String,
    content: String,
    byte_length: usize,
    truncated: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectionWire {
    from: Nullable<Vec<Object<AddressWire>>>,
    subject_hex: Nullable<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddressWire {
    host_hex: Nullable<String>,
    mailbox_hex: Nullable<String>,
    name_hex: Nullable<String>,
    route_hex: Nullable<String>,
}
fn decode_component(value: &Nullable<String>) -> Result<Option<Vec<u8>>, ErrorCode> {
    value
        .as_ref()
        .map(|v| {
            if v.len() > 16_384
                || v.len() % 2 != 0
                || !v
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(ErrorCode::InvalidInput);
            }
            let nibble = |b: u8| {
                if b.is_ascii_digit() {
                    b - b'0'
                } else {
                    b - b'a' + 10
                }
            };
            Ok(v.as_bytes()
                .chunks_exact(2)
                .map(|p| (nibble(p[0]) << 4) | nibble(p[1]))
                .collect())
        })
        .transpose()
}
fn validate_header(header: &HeaderWire) -> Result<HeaderProjection, ErrorCode> {
    if header.content_type != "header"
        || header.truncated
        || header.byte_length != header.content.len()
        || header.content.len() > MAX_RESULT
    {
        return Err(ErrorCode::InvalidInput);
    }
    let raw: Object<ProjectionWire> =
        serde_json::from_str(&header.content).map_err(|_| ErrorCode::InvalidInput)?;
    let from = raw
        .0
        .from
        .as_ref()
        .map(|addresses| {
            if addresses.is_empty() || addresses.len() > 128 {
                return Err(ErrorCode::InvalidInput);
            }
            addresses
                .iter()
                .map(|a| {
                    HeaderAddressProjection::new(
                        decode_component(&a.0.host_hex)?,
                        decode_component(&a.0.mailbox_hex)?,
                        decode_component(&a.0.name_hex)?,
                        decode_component(&a.0.route_hex)?,
                    )
                })
                .collect::<Result<Vec<_>, ErrorCode>>()
        })
        .transpose()?;
    let projection = HeaderProjection::new(decode_component(&raw.0.subject_hex)?, from)?;
    if projection.projection_json() != header.content {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(projection)
}
fn validate_result(
    r: &ResultWire,
    request: &ImapMetadataSearchRequest,
) -> Result<(Vec<ImapMetadataSearchItem>, Vec<HeaderProjection>), ErrorCode> {
    if r.schema_version != 1
        || !r.ok
        || r.items.len() > request.limit() as usize
        || r.items.len() != r.untrusted_content.len()
        || r.account_results.len() != 1
        || r.metadata.0.provider != "imap"
        || r.metadata.0.source != "local_store"
        || !r.metadata.0.cached
    {
        return Err(ErrorCode::InvalidInput);
    }
    let (status, warning) = if let Some(time) = r.observed_at.as_ref() {
        if time.len() != 24 || !crate::accounts::valid_timestamp(time) {
            return Err(ErrorCode::InvalidInput);
        }
        ("partial", "LOCAL_METADATA_ONLY")
    } else {
        if !r.items.is_empty() || r.next_cursor.as_ref().is_some() {
            return Err(ErrorCode::InvalidInput);
        }
        ("unavailable", "LOCAL_CACHE_UNOBSERVED")
    };
    if r.completeness != status
        || !(r.warnings == [warning] || r.warnings == [warning, "STALE_BINDING"])
    {
        return Err(ErrorCode::InvalidInput);
    }
    if let Some(next) = r.next_cursor.as_ref() {
        let next = ImapMetadataSearchCursor::parse(next)?;
        if request
            .cursor()
            .is_some_and(|p| p.expires_at_utc_ms() != next.expires_at_utc_ms())
        {
            return Err(ErrorCode::InvalidInput);
        }
    }
    let account = &r.account_results[0].0;
    if account.account_id != request.account_id()
        || account.status != status
        || account.items.len() != r.items.len()
        || account.items.iter().zip(&r.items).any(|(a, b)| a.0 != b.0)
    {
        return Err(ErrorCode::InvalidInput);
    }
    let mut parsed_items = Vec::with_capacity(r.items.len());
    let mut headers = Vec::with_capacity(r.items.len());
    let mut ids = BTreeSet::new();
    let mut last_uid = u32::MAX;
    for (index, (item, header)) in r.items.iter().zip(&r.untrusted_content).enumerate() {
        let i = &item.0;
        let parsed = ImapMetadataSearchItem::new(
            &i.internal_message_id,
            &i.account_id,
            i.uid,
            i.internal_date_utc_ms,
            i.size_bytes,
            i.is_read,
            i.is_favorite,
            i.header_content_index,
        )?;
        if i.account_id != request.account_id()
            || i.header_content_index != index as u32
            || i.uid >= last_uid
            || !ids.insert(&i.internal_message_id)
            || (request.filter() == ImapMetadataFilter::Unread && i.is_read)
            || (request.filter() == ImapMetadataFilter::Favorite && !i.is_favorite)
        {
            return Err(ErrorCode::InvalidInput);
        }
        last_uid = i.uid;
        headers.push(validate_header(&header.0)?);
        parsed_items.push(parsed);
    }
    Ok((parsed_items, headers))
}
pub fn parse_search_response(
    bytes: &[u8],
    expected_id: &str,
    request: &ImapMetadataSearchRequest,
) -> Result<ValidatedMessageSearchResponse, ErrorCode> {
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
    // Never validate a Value intermediary: it has already lost duplicate keys.
    let rpc: Rpc = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    let result = match rpc {
        Rpc::Success(success) => {
            if success.0.jsonrpc != "2.0" || success.0.id != expected_id {
                return Err(ErrorCode::InvalidInput);
            }
            success.0.result.0
        }
        Rpc::Failure(failure) => {
            if failure.0.jsonrpc != "2.0"
                || failure.0.id != expected_id
                || failure.0.error.0.code != failure.0.error.0.message
            {
                return Err(ErrorCode::InvalidInput);
            }
            return Err(ErrorCode::ALL
                .into_iter()
                .find(|v| v.as_str() == failure.0.error.0.code)
                .ok_or(ErrorCode::InvalidInput)?);
        }
    };
    let (items, headers) = validate_result(&result, request)?;
    // The original stream has passed typed validation before retaining a Value.
    let mut envelope: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    let value = envelope["result"].take();
    if serde_json::to_vec(&value)
        .map_err(|_| ErrorCode::InvalidInput)?
        .len()
        > MAX_RESULT
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(ValidatedMessageSearchResponse {
        value,
        items,
        headers,
    })
}
pub fn search_json(response: &ValidatedMessageSearchResponse) -> Result<String, ErrorCode> {
    serde_json::to_string(response.value()).map_err(|_| ErrorCode::InternalError)
}
pub fn search_default(
    request: ImapMetadataSearchRequest,
) -> Result<ValidatedMessageSearchResponse, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build()
            .map_err(|_| ErrorCode::CoreUnavailable)?.block_on(async move {
                let profile = crate::authenticate_default().await?;
                if !profile.scope().permits_account(request.account_id(),"mail.messages.search") { return Err(ErrorCode::ScopeDenied); }
                let id = "message-search-1";
                let params = serde_json::json!({"mode":"imap_metadata","account_id":request.account_id(),
                    "wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str(),
                    "filter":request.filter().as_str(),"limit":request.limit(),"cursor":request.cursor().map(|c|c.encode())});
                let envelope = serde_json::json!({"schema_version":1,"request_id":id,"tool":"mail.messages.search","params":params});
                let raw = profile.exchange(id,"mail.messages.search",&envelope).await?;
                parse_search_response(&raw,id,&request)
            })
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn search_gui(
    request: ImapMetadataSearchRequest,
) -> Result<ValidatedMessageSearchResponse, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build()
            .map_err(|_| ErrorCode::CoreUnavailable)?.block_on(async move {
                let profile = crate::authenticate_gui().await?;
                if !profile.scope().permits_account(request.account_id(),"mail.messages.search") { return Err(ErrorCode::ScopeDenied); }
                let id = "gui-message-search-1";
                let params = serde_json::json!({"mode":"imap_metadata","account_id":request.account_id(),
                    "wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str(),
                    "filter":request.filter().as_str(),"limit":request.limit(),"cursor":request.cursor().map(|c|c.encode())});
                let envelope = serde_json::json!({"schema_version":1,"request_id":id,"tool":"mail.messages.search","params":params});
                let raw = profile.exchange(id,"mail.messages.search",&envelope).await?;
                parse_search_response(&raw,id,&request)
            })
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(test)]
#[path = "message_search_tests.rs"]
mod tests;
