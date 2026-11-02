//! Exact-account authenticated mutation requests; never OS approval.
use hantong_contracts::{
    ErrorCode,
    google_mutation::{MutationPayload, REQUEST_TOOL},
    wire_hex,
};
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptWire {
    schema_version: u32,
    ok: bool,
    review_id: String,
    state: String,
    error_code: Option<String>,
}
pub struct MutationReceipt {
    pub review_id: [u8; 32],
    pub state: String,
    pub error: Option<ErrorCode>,
}
impl std::fmt::Debug for MutationReceipt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MutationReceipt(<redacted>)")
    }
}
pub fn parse(bytes: &[u8], expected_id: Option<[u8; 32]>) -> Result<MutationReceipt, ErrorCode> {
    let r: ReceiptWire = crate::mail_send::decode(bytes, "mail-write-1")?;
    let id = wire_hex::decode_fixed(&r.review_id)?;
    let error = r
        .error_code
        .map(|s| {
            ErrorCode::ALL
                .into_iter()
                .find(|e| e.as_str() == s)
                .ok_or(ErrorCode::InvalidInput)
        })
        .transpose()?;
    let valid = match r.state.as_str() {
        "pending" | "claimed" | "accepted" => error.is_none(),
        "unknown" => error == Some(ErrorCode::SendUnknown),
        "expired" => error == Some(ErrorCode::ApprovalExpired),
        "denied" | "rejected" => error.is_some(),
        _ => false,
    };
    if r.schema_version != 1
        || !r.ok
        || id == [0; 32]
        || expected_id.is_some_and(|x| x != id)
        || !valid
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(MutationReceipt {
        review_id: id,
        state: r.state,
        error,
    })
}
pub fn request_default(r: &MutationPayload) -> Result<MutationReceipt, ErrorCode> {
    if MutationPayload::from_json(&r.to_json())? != *r {
        return Err(ErrorCode::InvalidInput);
    }
    exchange(
        r.account_id(),
        serde_json::json!({"mode":"request","request":r.to_json()}),
        None,
    )
}
pub fn status_default(account: &str, id: [u8; 32]) -> Result<MutationReceipt, ErrorCode> {
    hantong_contracts::google::validate_google_account_id(account)?;
    if id == [0; 32] {
        return Err(ErrorCode::InvalidInput);
    }
    exchange(
        account,
        serde_json::json!({"mode":"status","account_id":account,"review_id":wire_hex::encode(&id)}),
        Some(id),
    )
}
fn exchange(
    account: &str,
    params: serde_json::Value,
    id: Option<[u8; 32]>,
) -> Result<MutationReceipt, ErrorCode> {
    #[cfg(windows)]
    {
        parse(
            &crate::mail_send::exchange(account, REQUEST_TOOL, params)?,
            id,
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (account, params, id, REQUEST_TOOL);
        Err(ErrorCode::CapabilityUnsupported)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn apr01_google_mutation_status_never_claims_success_for_unknown() {
        for (state, error, valid) in [
            ("accepted", None, true),
            ("pending", None, true),
            ("claimed", None, true),
            ("unknown", Some("SEND_UNKNOWN"), true),
            ("unknown", None, false),
            ("accepted", Some("SEND_UNKNOWN"), false),
            ("sent", None, false),
        ] {
            let v=serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0","id":"mail-write-1","result":{"schema_version":1,"ok":true,"review_id":"11".repeat(32),"state":state,"error_code":error}})).unwrap();
            assert_eq!(parse(&v, Some([0x11; 32])).is_ok(), valid);
            assert!(parse(&v, Some([0x22; 32])).is_err());
        }
    }
}
