//! Strict draft/review client. These public inputs never issue approval.
use crate::accounts::Object;
use hantong_contracts::{
    ErrorCode,
    mail_send::{
        DraftCreateRequest, PersonalSendConfirmRequest, SendReviewRequest, SendReviewStatusRequest,
    },
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
    decode_limit(bytes, id, 16 * 1024)
}
fn decode_limit<T: DeserializeOwned>(bytes: &[u8], id: &str, limit: usize) -> Result<T, ErrorCode> {
    if bytes.is_empty() || bytes.len() > limit || bytes.starts_with(&[239, 187, 191]) {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonalAttachmentPreview {
    pub filename: String,
    pub media_type: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersonalSendPreview {
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
    pub attachments: Vec<PersonalAttachmentPreview>,
    pub payload_hash: [u8; 32],
    pub draft_id: [u8; 32],
    pub version: u64,
}

pub struct PersonalPrepareResponse {
    pub receipt: ReviewResponse,
    pub preview: Option<PersonalSendPreview>,
}
impl std::fmt::Debug for PersonalPrepareResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PersonalPrepareResponse(<redacted>)")
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

#[cfg(any(windows, unix, test))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalAttachmentWire {
    filename: String,
    media_type: String,
    size: u64,
    sha256: String,
}
#[cfg(any(windows, unix, test))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalPreviewWire {
    from: String,
    to: Vec<String>,
    subject: String,
    body: String,
    attachments: Vec<PersonalAttachmentWire>,
    payload_hash: String,
    draft_id: String,
    version: u64,
}
#[cfg(any(windows, unix, test))]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonalPrepareWire {
    schema_version: u32,
    ok: bool,
    account_id: String,
    review_id: String,
    state: String,
    error_code: serde_json::Value,
    preview: Option<PersonalPreviewWire>,
}

#[cfg(any(windows, unix, test))]
fn parse_personal_prepare(
    bytes: &[u8],
    id: &str,
    request: &SendReviewRequest,
) -> Result<PersonalPrepareResponse, ErrorCode> {
    let v: PersonalPrepareWire = decode_limit(bytes, id, 1024 * 1024)?;
    if v.schema_version != 1 || !v.ok || v.account_id != request.account_id() {
        return Err(ErrorCode::InvalidInput);
    }
    let review_id = wire_hex::decode_fixed(&v.review_id)?;
    if review_id == [0; 32] {
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
    let receipt = ReviewResponse {
        account_id: v.account_id,
        review_id,
        state: v.state,
        error_code: error,
    };
    let preview = match v.preview {
        None => None,
        Some(p) => {
            if !valid_address(&p.from)
                || p.to.is_empty()
                || p.to.len() > 100
                || p.subject.len() > 998
                || p.subject.chars().any(char::is_control)
                || p.body.len() > hantong_contracts::mail_send::MAX_COMPOSE_BYTES
                || p.body.contains('\0')
                || p.version == 0
                || p.attachments.len() > 8
            {
                return Err(ErrorCode::InvalidInput);
            }
            for value in &p.to {
                if !valid_address(value) {
                    return Err(ErrorCode::InvalidInput);
                }
            }
            let mut attachments = Vec::with_capacity(p.attachments.len());
            let mut total_attachment_bytes = 0u64;
            for a in p.attachments {
                if a.filename.is_empty()
                    || a.filename.len() > 255
                    || a.filename.chars().any(char::is_control)
                    || a.media_type.is_empty()
                    || a.media_type.len() > 127
                    || a.media_type.chars().any(char::is_control)
                    || a.size > hantong_contracts::google_write::MAX_ATTACHMENT_BYTES as u64
                {
                    return Err(ErrorCode::InvalidInput);
                }
                total_attachment_bytes = total_attachment_bytes
                    .checked_add(a.size)
                    .ok_or(ErrorCode::InvalidInput)?;
                if total_attachment_bytes
                    > hantong_contracts::google_write::MAX_ATTACHMENT_BYTES as u64
                {
                    return Err(ErrorCode::InvalidInput);
                }
                attachments.push(PersonalAttachmentPreview {
                    filename: a.filename,
                    media_type: a.media_type,
                    size: a.size,
                    sha256: wire_hex::decode_fixed(&a.sha256)?,
                });
            }
            let payload_hash = wire_hex::decode_fixed(&p.payload_hash)?;
            let draft_id = wire_hex::decode_fixed(&p.draft_id)?;
            if payload_hash != *request.payload_hash()
                || draft_id != *request.draft_id()
                || p.version != request.expected_version()
            {
                return Err(ErrorCode::InvalidInput);
            }
            Some(PersonalSendPreview {
                from: p.from,
                to: p.to,
                subject: p.subject,
                body: p.body,
                attachments,
                payload_hash,
                draft_id,
                version: p.version,
            })
        }
    };
    if receipt.state == "pending" && preview.is_none() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(PersonalPrepareResponse { receipt, preview })
}

#[cfg(any(windows, unix, test))]
fn valid_address(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 320
        && value.is_ascii()
        && value
            .bytes()
            .all(|b| (33..127).contains(&b) && !b"<>(),;\\\"".contains(&b))
        && value.matches('@').count() == 1
        && !value.starts_with('@')
        && !value.ends_with('@')
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
    #[cfg(any(windows, unix))]
    {
        let hash = hantong_platform::sha256_storage_payload(&raw.canonical_payload())?;
        DraftCreateRequest::new(account, to, subject, body, operation, key, 0, hash)
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = raw;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(windows)]
pub(crate) fn exchange(
    account: &str,
    tool: &str,
    params: serde_json::Value,
) -> Result<Vec<u8>, ErrorCode> {
    tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build().map_err(|_|ErrorCode::CoreUnavailable)?.block_on(async {
  let profile=crate::authenticate_default().await?;
  if !profile.scope().permits_account(account,tool){return Err(ErrorCode::ScopeDenied)}
  profile.exchange("mail-write-1",tool,&serde_json::json!({"schema_version":1,"request_id":"mail-write-1","tool":tool,"params":params})).await
 })
}
#[cfg(unix)]
pub(crate) fn exchange(
    account: &str,
    tool: &str,
    params: serde_json::Value,
) -> Result<Vec<u8>, ErrorCode> {
    let profile = hantong_platform::load_default_client_profile()?;
    let parsed = crate::profile::ParsedClientProfile::from_json(profile.bytes())?;
    if !parsed.scope().permits_account(account, tool) {
        return Err(ErrorCode::ScopeDenied);
    }
    crate::unix_relay::call(parsed.profile(), "mail-write-1", tool, &params)
}
pub fn create_default(request: &DraftCreateRequest) -> Result<DraftResponse, ErrorCode> {
    create_compose_default(&request.clone().into())
}
pub fn compose_params(
    request: &hantong_contracts::google_write::ComposeSnapshot,
) -> serde_json::Value {
    let mut params = serde_json::json!({"account_id":request.account_id(),"to":request.recipients(),"subject":request.subject(),"body":request.body(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"expected_version":0,"payload_hash":wire_hex::encode(request.payload_hash())});
    if !request.attachments().is_empty() {
        params["attachments"] = serde_json::json!(request.attachments().iter().map(|a|serde_json::json!({"filename":a.name(),"media_type":a.media_type(),"content_hex":wire_hex::encode(a.bytes())})).collect::<Vec<_>>());
    }
    params
}
pub fn prepare_compose(
    draft: DraftCreateRequest,
    attachments: Vec<hantong_contracts::google_write::AttachmentSnapshot>,
) -> Result<hantong_contracts::google_write::ComposeSnapshot, ErrorCode> {
    use hantong_contracts::google_write::ComposeSnapshot;
    let raw = ComposeSnapshot::new(draft, attachments)?;
    #[cfg(any(windows, unix))]
    {
        let hash = hantong_platform::sha256_storage_payload(&raw.canonical_payload())?;
        let draft = DraftCreateRequest::new(
            raw.account_id(),
            raw.recipients().to_vec(),
            raw.subject().into(),
            raw.body().into(),
            raw.operation_id(),
            raw.idempotency_key(),
            0,
            hash,
        )?;
        ComposeSnapshot::new(draft, raw.attachments().to_vec())
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = raw;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn create_compose_default(
    request: &hantong_contracts::google_write::ComposeSnapshot,
) -> Result<DraftResponse, ErrorCode> {
    #[cfg(any(windows, unix))]
    {
        let params = compose_params(request);
        let bytes = exchange(request.account_id(), "mail.drafts.create", params)?;
        parse_draft(&bytes, "mail-write-1", request)
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Authenticates the default CLI profile and checks the selected account's
/// persisted effective draft/send tools. This is only an eligibility probe;
/// it does not create a draft, request a send, or widen the scope.
pub fn verify_default_send(account: &str) -> Result<bool, ErrorCode> {
    #[cfg(windows)]
    {
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ErrorCode::CoreUnavailable)?
            .block_on(async {
                let profile = crate::authenticate_default().await?;
                Ok([
                    "mail.drafts.create",
                    "mail.send.request",
                    "mail.send.execute",
                ]
                .into_iter()
                .all(|tool| profile.scope().permits_account(account, tool)))
            })
    }
    #[cfg(unix)]
    {
        let profile = hantong_platform::load_default_client_profile()?;
        let parsed = crate::profile::ParsedClientProfile::from_json(profile.bytes())?;
        Ok([
            "mail.drafts.create",
            "mail.send.request",
            "mail.send.execute",
        ]
        .into_iter()
        .all(|tool| parsed.scope().permits_account(account, tool)))
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = account;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

pub fn request_default(request: &SendReviewRequest) -> Result<ReviewResponse, ErrorCode> {
    #[cfg(any(windows, unix))]
    {
        let params = serde_json::json!({"mode":"request","account_id":request.account_id(),"draft_id":wire_hex::encode(request.draft_id()),"expected_version":request.expected_version(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"payload_hash":wire_hex::encode(request.payload_hash())});
        let bytes = exchange(request.account_id(), "mail.send.request", params)?;
        parse_review(&bytes, "mail-write-1", request.account_id(), None)
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Prepare the immutable personal preview. This does not queue a send job.
pub fn prepare_default(request: &SendReviewRequest) -> Result<PersonalPrepareResponse, ErrorCode> {
    #[cfg(any(windows, unix))]
    {
        let params = serde_json::json!({"mode":"prepare","account_id":request.account_id(),"draft_id":wire_hex::encode(request.draft_id()),"expected_version":request.expected_version(),"operation_id":request.operation_id(),"idempotency_key":request.idempotency_key(),"payload_hash":wire_hex::encode(request.payload_hash())});
        let bytes = exchange(request.account_id(), "mail.send.request", params)?;
        parse_personal_prepare(&bytes, "mail-write-1", request)
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

/// Confirm exactly one already displayed personal preview.
pub fn confirm_default(request: &PersonalSendConfirmRequest) -> Result<ReviewResponse, ErrorCode> {
    #[cfg(any(windows, unix))]
    {
        let params = serde_json::json!({"mode":"confirm","account_id":request.account_id,"review_id":wire_hex::encode(&request.review_id),"payload_hash":wire_hex::encode(&request.payload_hash)});
        let bytes = exchange(&request.account_id, "mail.send.request", params)?;
        parse_review(
            &bytes,
            "mail-write-1",
            &request.account_id,
            Some(request.review_id),
        )
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}

pub fn cancel_default(request: &SendReviewStatusRequest) -> Result<ReviewResponse, ErrorCode> {
    #[cfg(any(windows, unix))]
    {
        let params = serde_json::json!({"mode":"cancel","account_id":request.account_id,"review_id":wire_hex::encode(&request.review_id)});
        let bytes = exchange(&request.account_id, "mail.send.request", params)?;
        parse_review(
            &bytes,
            "mail-write-1",
            &request.account_id,
            Some(request.review_id),
        )
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
pub fn status_default(request: &SendReviewStatusRequest) -> Result<ReviewResponse, ErrorCode> {
    #[cfg(any(windows, unix))]
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
    #[cfg(all(not(windows), not(unix)))]
    {
        let _ = request;
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn personal_request() -> SendReviewRequest {
        SendReviewRequest::new("acct", [1; 32], 1, "op", "key", [2; 32]).unwrap()
    }
    fn personal_preview() -> serde_json::Value {
        serde_json::json!({
            "from":"from@example.test", "to":["to@example.test"],
            "subject":"subject", "body":"body", "attachments":[],
            "payload_hash":"02".repeat(32), "draft_id":"01".repeat(32), "version":1
        })
    }
    fn personal_response(state: &str, preview: Option<serde_json::Value>) -> Vec<u8> {
        let mut result = serde_json::json!({"schema_version":1,"ok":true,"account_id":"acct","review_id":"33".repeat(32),"state":state,"error_code":null});
        if let Some(preview) = preview {
            result["preview"] = preview;
        }
        serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":"r","result":result})).unwrap()
    }
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

    #[test]
    fn apr01_personal_prepare_accepts_complete_large_body_and_rejects_small_frame_only() {
        let request = personal_request();
        let mut preview = personal_preview();
        preview["body"] = serde_json::Value::String("x".repeat(20_000));
        let bytes = personal_response("pending", Some(preview));
        let parsed = parse_personal_prepare(&bytes, "r", &request).unwrap();
        assert_eq!(parsed.preview.unwrap().body.len(), 20_000);
        let too_large = vec![b'x'; 1024 * 1024 + 1];
        assert!(parse_personal_prepare(&too_large, "r", &request).is_err());
    }

    #[test]
    fn apr01_personal_prepare_requires_preview_and_rejects_unknown_or_duplicate_fields() {
        let request = personal_request();
        assert!(
            parse_personal_prepare(&personal_response("pending", None), "r", &request).is_err()
        );
        assert!(
            parse_personal_prepare(&personal_response("sent", None), "r", &request)
                .unwrap()
                .preview
                .is_none()
        );
        let raw =
            String::from_utf8(personal_response("pending", Some(personal_preview()))).unwrap();
        let unknown = raw.replace("\"body\":\"body\"", "\"body\":\"body\",\"extra\":true");
        let duplicate = raw.replace(
            "\"state\":\"pending\"",
            "\"state\":\"pending\",\"state\":\"sent\"",
        );
        assert!(parse_personal_prepare(unknown.as_bytes(), "r", &request).is_err());
        assert!(parse_personal_prepare(duplicate.as_bytes(), "r", &request).is_err());
    }

    #[test]
    fn apr01_personal_prepare_rejects_binding_and_content_bounds() {
        let request = personal_request();
        for (field, value) in [
            ("payload_hash", serde_json::Value::String("44".repeat(32))),
            ("draft_id", serde_json::Value::String("55".repeat(32))),
            ("version", serde_json::json!(2)),
        ] {
            let mut preview = personal_preview();
            preview[field] = value;
            assert!(
                parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                    .is_err()
            );
        }
        let mut preview = personal_preview();
        preview["from"] = "bad-address".into();
        assert!(
            parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                .is_err()
        );
        let mut preview = personal_preview();
        preview["to"] = serde_json::json!(["bad\u{001b}address"]);
        assert!(
            parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                .is_err()
        );
        let mut preview = personal_preview();
        preview["subject"] = "x".repeat(999).into();
        assert!(
            parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                .is_err()
        );
        let mut preview = personal_preview();
        preview["body"] = "x"
            .repeat(hantong_contracts::mail_send::MAX_COMPOSE_BYTES + 1)
            .into();
        assert!(
            parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                .is_err()
        );
        let mut preview = personal_preview();
        preview["attachments"] = serde_json::Value::Array(
            (0..9)
                .map(|_| {
                    serde_json::json!({"filename":"x","media_type":"text/plain","size":1,"sha256":"66".repeat(32)})
                })
                .collect(),
        );
        assert!(
            parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                .is_err()
        );
    }

    #[test]
    fn apr01_personal_prepare_rejects_attachment_and_recipient_bounds() {
        let request = personal_request();
        let mut preview = personal_preview();
        preview["from"] = "from\n@example.test".into();
        assert!(
            parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                .is_err()
        );
        let mut preview = personal_preview();
        preview["to"] = serde_json::json!(["to@example.test", "bad@example.test\u{007f}"]);
        assert!(
            parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                .is_err()
        );
        let mut preview = personal_preview();
        preview["to"] = serde_json::Value::Array(
            (0..101)
                .map(|n| serde_json::Value::String(format!("to{n}@example.test")))
                .collect(),
        );
        assert!(
            parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                .is_err()
        );
        for field in ["filename", "media_type"] {
            let mut preview = personal_preview();
            preview["attachments"] = serde_json::json!([{"filename":"file.txt","media_type":"text/plain","size":1,"sha256":"66".repeat(32)}]);
            preview["attachments"][0][field] =
                serde_json::Value::String("x".repeat(if field == "filename" { 256 } else { 128 }));
            assert!(
                parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                    .is_err()
            );
        }
        let mut preview = personal_preview();
        preview["attachments"] = serde_json::json!([{"filename":"file.txt","media_type":"text/plain","size":1048577,"sha256":"66".repeat(32)}]);
        assert!(
            parse_personal_prepare(&personal_response("pending", Some(preview)), "r", &request)
                .is_err()
        );
    }
}
