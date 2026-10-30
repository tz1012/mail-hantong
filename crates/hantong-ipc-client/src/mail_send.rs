//! Strict draft/review client. These public inputs never issue approval.
use crate::accounts::Object;
use hantong_contracts::{
    ErrorCode,
    mail_send::{DraftCreateRequest, SendReviewRequest, SendReviewStatusRequest},
    wire_hex,
};
use serde::{Deserialize, de::DeserializeOwned};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Success<T> {
    jsonrpc: String,
    id: String,
    result: Object<T>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    jsonrpc: String,
    id: String,
    error: Object<Code>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Code {
    code: String,
    message: String,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Reply<T> {
    Success(Object<Success<T>>),
    Failure(Object<Failure>),
}
pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8], id: &str) -> Result<T, ErrorCode> {
    if bytes.is_empty() || bytes.len() > 16384 || bytes.starts_with(&[239, 187, 191]) {
        return Err(ErrorCode::InvalidInput);
    }
    match serde_json::from_slice::<Reply<T>>(bytes).map_err(|_| ErrorCode::InvalidInput)? {
        Reply::Success(Object(v)) if v.jsonrpc == "2.0" && v.id == id => Ok(v.result.0),
        Reply::Failure(Object(v))
            if v.jsonrpc == "2.0" && v.id == id && v.error.0.code == v.error.0.message =>
        {
            Err(ErrorCode::ALL
                .iter()
                .find(|e| e.as_str() == v.error.0.code)
                .copied()
                .unwrap_or(ErrorCode::InvalidInput))
        }
        _ => Err(ErrorCode::InvalidInput),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftWire {
    schema_version: u32,
    ok: bool,
    account_id: String,
    draft_id: String,
    version: u64,
    payload_hash: String,
}
pub struct DraftResponse {
    pub account_id: String,
    pub draft_id: [u8; 32],
    pub version: u64,
    pub payload_hash: [u8; 32],
}
impl std::fmt::Debug for DraftResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DraftResponse(<redacted>)")
    }
}
pub fn parse_draft(
    bytes: &[u8],
    id: &str,
    request: &DraftCreateRequest,
) -> Result<DraftResponse, ErrorCode> {
    let v: DraftWire = decode(bytes, id)?;
    let draft_id = wire_hex::decode_fixed(&v.draft_id)?;
    let hash = wire_hex::decode_fixed(&v.payload_hash)?;
    if v.schema_version != 1
        || !v.ok
        || v.account_id != request.account_id()
        || v.version != 1
        || draft_id == [0; 32]
        || &hash != request.payload_hash()
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(DraftResponse {
        account_id: v.account_id,
        draft_id,
        version: v.version,
        payload_hash: hash,
    })
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewWire {
    schema_version: u32,
    ok: bool,
    account_id: String,
    review_id: String,
    state: String,
    error_code: serde_json::Value,
}
pub struct ReviewResponse {
    pub account_id: String,
    pub review_id: [u8; 32],
    pub state: String,
    pub error_code: Option<ErrorCode>,
}
impl std::fmt::Debug for ReviewResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReviewResponse(<redacted>)")
    }
}
pub fn parse_review(
    bytes: &[u8],
    id: &str,
    account: &str,
    review: Option<[u8; 32]>,
) -> Result<ReviewResponse, ErrorCode> {
    let v: ReviewWire = decode(bytes, id)?;
    let rid = wire_hex::decode_fixed(&v.review_id)?;
    if v.schema_version != 1
        || !v.ok
        || v.account_id != account
        || rid == [0; 32]
        || review.is_some_and(|r| r != rid)
    {
        return Err(ErrorCode::InvalidInput);
    }
    let error = if v.error_code.is_null() {
        None
    } else {
        Some(
            ErrorCode::ALL
                .iter()
                .find(|e| Some(e.as_str()) == v.error_code.as_str())
                .copied()
                .ok_or(ErrorCode::InvalidInput)?,
        )
    };
    let valid = match v.state.as_str() {
        "pending" | "sending" | "sent" => error.is_none(),
        "unknown" => error == Some(ErrorCode::SendUnknown),
        "denied" => error == Some(ErrorCode::ApprovalDenied),
        "expired" => error == Some(ErrorCode::ApprovalExpired),
        "interrupted" => error == Some(ErrorCode::ApprovalRequired),
        "failed" => error.is_some(),
        _ => false,
    };
    if !valid {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(ReviewResponse {
        account_id: v.account_id,
        review_id: rid,
        state: v.state,
        error_code: error,
    })
}
#[allow(clippy::too_many_arguments)]
pub fn prepare_default_draft(
    account: &str,
    to: Vec<String>,
    subject: String,
    body: String,
    operation: &str,
    key: &str,
) -> Result<DraftCreateRequest, ErrorCode> {
    let raw = DraftCreateRequest::new(
        account,
        to.clone(),
        subject.clone(),
        body.clone(),
        operation,
        key,
        0,
        [0; 32],
    )?;
    #[cfg(windows)]
    {
        let hash = hantong_platform::sha256_storage_payload(&raw.canonical_payload())?;
        DraftCreateRequest::new(account, to, subject, body, operation, key, 0, hash)
    }
    #[cfg(not(windows))]
    {
        let _ = raw;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(windows)]
fn exchange(account: &str, tool: &str, params: serde_json::Value) -> Result<Vec<u8>, ErrorCode> {
    tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_|ErrorCode::CoreUnavailable)?.block_on(async {
  let profile=crate::authenticate_default().await?;
  if !profile.scope().permits_account(account,tool){return Err(ErrorCode::ScopeDenied)}
  profile.exchange("mail-write-1",tool,&serde_json::json!({"schema_version":1,"request_id":"mail-write-1","tool":tool,"params":params})).await
 })
}
pub fn create_default(request: &DraftCreateRequest) -> Result<DraftResponse, ErrorCode> {
    #[cfg(windows)]
    {
        let params = serde_json::json!({"account_id":request.account_id(),"to":request.recipients(),"subject":request.subject(),"body":request.body(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_version":0,"payload_hash":wire_hex::encode(request.payload_hash())});
        let bytes = exchange(request.account_id(), "mail.drafts.create", params)?;
        parse_draft(&bytes, "mail-write-1", request)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn request_default(request: &SendReviewRequest) -> Result<ReviewResponse, ErrorCode> {
    #[cfg(windows)]
    {
        let params = serde_json::json!({"mode":"request","account_id":request.account_id(),"draft_id":wire_hex::encode(request.draft_id()),"expected_version":request.expected_version(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"payload_hash":wire_hex::encode(request.payload_hash())});
        let bytes = exchange(request.account_id(), "mail.send.request", params)?;
        parse_review(&bytes, "mail-write-1", request.account_id(), None)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn status_default(request: &SendReviewStatusRequest) -> Result<ReviewResponse, ErrorCode> {
    #[cfg(windows)]
    {
        let bytes = exchange(
            &request.account_id,
            "mail.send.request",
            serde_json::json!({"mode":"status","account_id":request.account_id,"review_id":wire_hex::encode(&request.review_id)}),
        )?;
        parse_review(
            &bytes,
            "mail-write-1",
            &request.account_id,
            Some(request.review_id),
        )
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
    fn response(state: &str, code: serde_json::Value) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":"r","result":{"schema_version":1,"ok":true,"account_id":"acct","review_id":"11".repeat(32),"state":state,"error_code":code}})).unwrap()
    }
    #[test]
    fn apr01_review_receipt_never_turns_pending_or_unknown_into_send_success() {
        let pending = parse_review(
            &response("pending", serde_json::Value::Null),
            "r",
            "acct",
            None,
        )
        .unwrap();
        assert_eq!(pending.state, "pending");
        let unknown = parse_review(
            &response("unknown", serde_json::json!("SEND_UNKNOWN")),
            "r",
            "acct",
            None,
        )
        .unwrap();
        assert_eq!(unknown.error_code, Some(ErrorCode::SendUnknown));
        assert!(
            parse_review(
                &response("unknown", serde_json::Value::Null),
                "r",
                "acct",
                None
            )
            .is_err()
        );
        assert!(
            parse_review(
                &response("sent", serde_json::json!("SEND_UNKNOWN")),
                "r",
                "acct",
                None
            )
            .is_err()
        );
        assert!(
            parse_review(
                &response("pending", serde_json::Value::Null),
                "r",
                "other",
                None
            )
            .is_err()
        );
        assert!(
            parse_review(
                &response("sent", serde_json::Value::Null),
                "r",
                "acct",
                Some([2; 32])
            )
            .is_err()
        );
        assert!(
            parse_review(
                &response("sent", serde_json::Value::Null),
                "wrong",
                "acct",
                None
            )
            .is_err()
        );
    }
    #[test]
    fn apr01_review_reply_rejects_duplicate_and_authority_fields() {
        let raw = String::from_utf8(response("sent", serde_json::Value::Null)).unwrap();
        let extra = raw.replace("\"state\":\"sent\"", "\"state\":\"sent\",\"approved\":true");
        let duplicate = raw.replace(
            "\"state\":\"sent\"",
            "\"state\":\"sent\",\"state\":\"pending\"",
        );
        assert!(parse_review(extra.as_bytes(), "r", "acct", None).is_err());
        assert!(parse_review(duplicate.as_bytes(), "r", "acct", None).is_err());
    }
    #[test]
    fn apr01_cancelled_expired_and_failed_reviews_preserve_stable_errors() {
        for (state, error) in [
            ("denied", ErrorCode::ApprovalDenied),
            ("interrupted", ErrorCode::ApprovalRequired),
            ("expired", ErrorCode::ApprovalExpired),
            ("failed", ErrorCode::Conflict),
            ("failed", ErrorCode::PolicyBlocked),
            ("failed", ErrorCode::RateLimited),
        ] {
            let parsed = parse_review(
                &response(state, serde_json::json!(error.as_str())),
                "r",
                "acct",
                None,
            )
            .unwrap();
            assert_eq!(parsed.state, state);
            assert_eq!(parsed.error_code, Some(error));
        }
    }
}
