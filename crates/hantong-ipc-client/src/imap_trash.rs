//! Strict authenticated IPC client for the provider-neutral IMAP trash tool.
//! This module transports only the bounded request and redacted receipt.

use hantong_contracts::{
    ErrorCode, google_mutation::TRASH_REQUEST_TOOL, imap_trash::ImapTrashRequest, wire_hex,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptWire {
    schema_version: u32,
    provider: String,
    ok: bool,
    account_id: String,
    review_id: String,
    state: String,
    error_code: Option<String>,
}

pub struct ImapTrashReceipt {
    pub account_id: String,
    pub review_id: [u8; 32],
    pub state: String,
    pub error: Option<ErrorCode>,
}
impl std::fmt::Debug for ImapTrashReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ImapTrashReceipt(<redacted>)")
    }
}

pub fn parse(
    bytes: &[u8],
    expected_account: &str,
    expected_id: Option<[u8; 32]>,
) -> Result<ImapTrashReceipt, ErrorCode> {
    let wire: ReceiptWire = crate::mail_send::decode(bytes, "mail-write-1")?;
    let id = wire_hex::decode_fixed(&wire.review_id)?;
    let error = wire
        .error_code
        .map(|value| {
            ErrorCode::ALL
                .into_iter()
                .find(|code| code.as_str() == value)
                .ok_or(ErrorCode::InvalidInput)
        })
        .transpose()?;
    let valid_state = match wire.state.as_str() {
        "pending" | "claimed" | "moving" | "moved" => error.is_none(),
        "unknown" => error == Some(ErrorCode::SendUnknown),
        "expired" => error == Some(ErrorCode::ApprovalExpired),
        "failed" | "denied" => error.is_some(),
        _ => false,
    };
    if wire.schema_version != 1
        || wire.provider != "imap"
        || !wire.ok
        || wire.account_id != expected_account
        || id == [0; 32]
        || expected_id.is_some_and(|expected| expected != id)
        || !valid_state
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(ImapTrashReceipt {
        account_id: wire.account_id,
        review_id: id,
        state: wire.state,
        error,
    })
}

pub fn request_default(request: &ImapTrashRequest) -> Result<ImapTrashReceipt, ErrorCode> {
    if ImapTrashRequest::from_json(&request.to_json())? != *request {
        return Err(ErrorCode::InvalidInput);
    }
    exchange(
        request.account_id(),
        serde_json::json!({"mode":"request","request":request.to_json()}),
        None,
    )
}

pub fn status_default(account: &str, review_id: [u8; 32]) -> Result<ImapTrashReceipt, ErrorCode> {
    validate_account(account)?;
    if review_id == [0; 32] {
        return Err(ErrorCode::InvalidInput);
    }
    exchange(
        account,
        serde_json::json!({"mode":"status","account_id":account,"review_id":wire_hex::encode(&review_id)}),
        Some(review_id),
    )
}

fn validate_account(account: &str) -> Result<(), ErrorCode> {
    if account.is_empty()
        || account.len() > 128
        || !account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}

fn exchange(
    account: &str,
    params: serde_json::Value,
    expected_id: Option<[u8; 32]>,
) -> Result<ImapTrashReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        parse(
            &crate::mail_send::exchange(account, TRASH_REQUEST_TOOL, params)?,
            account,
            expected_id,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (account, params, expected_id, TRASH_REQUEST_TOOL);
        Err(ErrorCode::CapabilityUnsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(state: &str, error: Option<&str>) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":"mail-write-1","result":{"schema_version":1,"provider":"imap","ok":true,"account_id":"acct","review_id":"11".repeat(32),"state":state,"error_code":error}})).unwrap()
    }

    #[test]
    fn sec03_receipt_requires_imap_provider_and_preserves_unknown_terminal() {
        let parsed = parse(
            &response("unknown", Some("SEND_UNKNOWN")),
            "acct",
            Some([0x11; 32]),
        )
        .unwrap();
        assert_eq!(parsed.state, "unknown");
        assert_eq!(parsed.error, Some(ErrorCode::SendUnknown));
        let mut invalid = response("moved", None);
        let text = String::from_utf8(invalid.clone())
            .unwrap()
            .replace("imap", "gmail");
        invalid = text.into_bytes();
        assert!(matches!(
            parse(&invalid, "acct", None),
            Err(ErrorCode::InvalidInput)
        ));
    }

    #[test]
    fn sec03_receipt_rejects_account_review_and_state_mismatches() {
        let account = String::from_utf8(response("moved", None))
            .unwrap()
            .replace("acct", "other");
        assert!(matches!(
            parse(account.as_bytes(), "acct", None),
            Err(ErrorCode::InvalidInput)
        ));
        assert!(matches!(
            parse(&response("moved", None), "acct", Some([0x22; 32])),
            Err(ErrorCode::InvalidInput)
        ));
        assert!(matches!(
            parse(&response("unknown", None), "acct", None),
            Err(ErrorCode::InvalidInput)
        ));
        assert!(matches!(
            parse(&response("pending", Some("APPROVAL_DENIED")), "acct", None),
            Err(ErrorCode::InvalidInput)
        ));
    }

    #[test]
    fn sec03_receipt_rejects_unknown_wire_fields() {
        let raw = String::from_utf8(response("pending", None))
            .unwrap()
            .replace("\"error_code\":null", "\"error_code\":null,\"extra\":true");
        assert!(matches!(
            parse(raw.as_bytes(), "acct", None),
            Err(ErrorCode::InvalidInput)
        ));
    }
}
