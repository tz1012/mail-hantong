//! Local account-display response; permission checks belong to execute_tool.
use crate::ReadResult;
use hantong_contracts::ErrorCode;
use hantong_domain::accounts::AccountSnapshot;
use serde_json::{Value, json};

/// The IPC frame has a 1 MiB ceiling. Leave room for its JSON-RPC wrapper/id;
/// an oversized local result is rejected as a whole, never silently shortened.
const MAX_TOOL_RESULT_BYTES: usize = 1_040_000;

pub fn render_account_list(snapshot: AccountSnapshot) -> Result<ReadResult<Value>, ErrorCode> {
    snapshot.validate().map_err(|_| ErrorCode::InternalError)?;
    let observed_at = snapshot
        .observed_at_rfc3339()
        .map_err(|_| ErrorCode::InternalError)?;
    let count = snapshot.items.len() as u64;
    let items = serde_json::to_value(&snapshot.items).map_err(|_| ErrorCode::InternalError)?;
    let rows = items.as_array().ok_or(ErrorCode::InternalError)?;
    let account_results: Vec<Value> = rows
        .iter()
        .map(|item| {
            json!({
                "account_id":item["account_id"], "status":"complete", "items":[item]
            })
        })
        .collect();
    let result = json!({
        "schema_version":1, "ok":true, "items":items,
        "next_cursor":null, "account_results":account_results,
        "completeness":"complete", "warnings":[], "observed_at":observed_at,
        "metadata":{"source":"local_store", "cached":true}
    });
    let encoded = serde_json::to_vec(&result).map_err(|_| ErrorCode::InternalError)?;
    if encoded.len() > MAX_TOOL_RESULT_BYTES {
        return Err(ErrorCode::QuotaExceeded);
    }
    ReadResult::new(result, Some(count)).map_err(|_| ErrorCode::InternalError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_domain::accounts::{AccountProvider, AccountStatus, AccountSummary};

    fn snapshot(count: usize) -> AccountSnapshot {
        AccountSnapshot {
            observed_at_utc_ms: 1,
            items: (0..count)
                .map(|i| AccountSummary {
                    account_id: format!("account-{i:03}"),
                    provider: AccountProvider::Imap,
                    primary_address: None,
                    display_name: Some("한통".into()),
                    color: None,
                    account_number: i as u64 + 1,
                    status: AccountStatus::ReauthRequired,
                    capabilities: None,
                })
                .collect(),
        }
    }
    #[test]
    fn cli01_account_response_reports_actual_local_rows_and_stable_shape() {
        let result = render_account_list(snapshot(2)).unwrap();
        assert_eq!(result.count(), Some(2));
        let body = result.value();
        assert_eq!(body["ok"], true);
        assert_eq!(body["observed_at"], "1970-01-01T00:00:00.001Z");
        assert_eq!(body["items"][0]["status"], "reauth_required");
        assert_eq!(body["items"][0]["primary_address"], Value::Null);
        assert_eq!(body["account_results"][0]["items"][0], body["items"][0]);
        assert_eq!(body["account_results"].as_array().unwrap().len(), 2);
        assert_eq!(
            body["metadata"],
            json!({"source":"local_store","cached":true})
        );
        for item in body["items"].as_array().unwrap() {
            assert_eq!(item.as_object().unwrap().len(), 7);
            for name in [
                "provider_subject",
                "granted_scopes",
                "settings_version",
                "capabilities",
                "token",
            ] {
                assert!(item.get(name).is_none());
            }
        }
        assert!(body["next_cursor"].is_null());
    }
    #[test]
    fn cli01_invalid_snapshot_is_not_a_success_or_reflected_error() {
        for count in [0, 129] {
            assert!(matches!(
                render_account_list(snapshot(count)),
                Err(ErrorCode::InternalError)
            ));
        }
        let mut invalid = snapshot(2);
        invalid.items[1].account_id = invalid.items[0].account_id.clone();
        assert!(matches!(
            render_account_list(invalid),
            Err(ErrorCode::InternalError)
        ));
        let mut invalid = snapshot(1);
        invalid.items[0].display_name = Some("sensitive\nbody".into());
        let Err(error) = render_account_list(invalid) else {
            panic!("invalid snapshot accepted")
        };
        assert_eq!(error, ErrorCode::InternalError);
        assert!(!format!("{error:?}").contains("sensitive"));
    }
    #[test]
    fn cli01_large_valid_result_is_bounded_before_wire_without_truncation() {
        assert!(render_account_list(snapshot(128)).is_ok());
        let mut large = snapshot(128);
        for item in &mut large.items {
            item.capabilities = Some(
                (0..128)
                    .map(|i| format!("cap{i:03}{}", "x".repeat(90)))
                    .collect(),
            );
        }
        assert!(large.validate().is_ok());
        assert!(matches!(
            render_account_list(large),
            Err(ErrorCode::QuotaExceeded)
        ));
    }
}
