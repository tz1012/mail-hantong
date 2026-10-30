//! Strict codecs for an explicit local UID observation summary.
use hantong_contracts::{
    ErrorCode,
    uid_inspection::{UidInspectionSnapshot, UidObservationSummary},
    uid_page::{UidCursorRequest, UidMailboxEncoding, UidMailboxIdentity},
};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSummary {
    cursor_version: u64,
    epoch_generation: u64,
    covered_through_uid: u32,
    observed_uid_ceiling: u32,
    stored_at_utc_ms: i64,
    coverage_at_observed_ceiling: bool,
}
// Unlike Option, this required field rejects omission while accepting explicit null.
#[derive(Deserialize)]
#[serde(untagged)]
enum WireObservation {
    Present(WireSummary),
    Null(()),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSnapshot {
    schema_version: u64,
    ok: bool,
    account_id: String,
    wire_hex: String,
    encoding: String,
    catalog_generation: u64,
    binding_current: bool,
    observation: WireObservation,
}
pub fn parse_inspection_snapshot(
    bytes: &[u8],
    expected_id: &str,
    request: &UidCursorRequest,
) -> Result<UidInspectionSnapshot, ErrorCode> {
    let r: WireSnapshot = crate::uid_page::decode(bytes, expected_id)?;
    let mailbox = UidMailboxIdentity::new(&r.wire_hex, UidMailboxEncoding::parse(&r.encoding)?)?;
    if r.schema_version != 1
        || !r.ok
        || r.account_id != request.account_id()
        || mailbox != *request.mailbox()
    {
        return Err(ErrorCode::InvalidInput);
    }
    let observation = match r.observation {
        WireObservation::Null(()) => None,
        WireObservation::Present(v) => {
            let summary = UidObservationSummary::new(
                v.cursor_version,
                v.epoch_generation,
                v.covered_through_uid,
                v.observed_uid_ceiling,
                v.stored_at_utc_ms,
            )?;
            if summary.coverage_at_observed_ceiling() != v.coverage_at_observed_ceiling {
                return Err(ErrorCode::InvalidInput);
            }
            Some(summary)
        }
    };
    UidInspectionSnapshot::new(
        &r.account_id,
        mailbox,
        r.catalog_generation,
        r.binding_current,
        observation,
    )
}
pub fn inspection_json(v: &UidInspectionSnapshot) -> String {
    let observation = v.observation().map(|o| json!({"cursor_version":o.cursor_version(),"epoch_generation":o.epoch_generation(),"covered_through_uid":o.covered_through_uid(),"observed_uid_ceiling":o.observed_uid_ceiling(),"stored_at_utc_ms":o.stored_at_utc_ms(),"coverage_at_observed_ceiling":o.coverage_at_observed_ceiling()}));
    json!({"schema_version":1,"ok":true,"account_id":v.account_id(),"wire_hex":v.mailbox().wire_hex(),"encoding":v.mailbox().encoding().as_str(),"catalog_generation":v.catalog_generation(),"binding_current":v.binding_current(),"observation":observation}).to_string()
}
pub fn inspect_default(request: UidCursorRequest) -> Result<UidInspectionSnapshot, ErrorCode> {
    #[cfg(windows)]
    {
        let params = json!({"action":"inspect","account_id":request.account_id(),"wire_hex":request.mailbox().wire_hex(),"encoding":request.mailbox().encoding().as_str()});
        parse_inspection_snapshot(
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
#[path = "uid_inspection_tests.rs"]
mod tests;
