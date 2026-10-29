use super::*;
use serde_json::json;

fn request() -> FolderListRequest {
    FolderListRequest::new("acct", Some(2), None).unwrap()
}
fn page() -> Value {
    let items = json!([{"wire_hex":"494e424f58","display_name":"INBOX","encoding":"modified_utf7","delimiter":"/","no_select":false,"non_existent":false,"children":"unknown","roles":[]}]);
    json!({"jsonrpc":"2.0","id":"r","result":{"schema_version":1,"ok":true,"items":items,"next_cursor":null,"account_results":[{"account_id":"acct","status":"complete","items":items}],"completeness":"complete","warnings":[],"observed_at":"2026-10-30T00:00:00.001Z","metadata":{"source":"local_store","cached":true,"account_id":"acct","generation":1,"account_version":1,"endpoint_version":1,"binding_current":true,"special_use_complete":false}}})
}
fn parse(value: &Value) -> Result<FolderPage, ErrorCode> {
    parse_folder_page(&serde_json::to_vec(value).unwrap(), "r", &request())
}

#[test]
fn ext06_folder_parser_rejects_decoded_duplicate_keys_at_every_depth() {
    let raw = page().to_string();
    for (from, to) in [
        (
            "\"schema_version\":1",
            r#""schema_version":1,"\u0073chema_version":1"#,
        ),
        ("\"generation\":1", r#""generation":1,"\u0067eneration":1"#),
        (
            "\"jsonrpc\":\"2.0\"",
            r#""jsonrpc":"2.0","\u006asonrpc":"2.0"#,
        ),
    ] {
        assert!(raw.contains(from));
        assert_eq!(
            parse_folder_page(raw.replacen(from, to, 1).as_bytes(), "r", &request()),
            Err(ErrorCode::InvalidInput)
        );
    }
}
#[test]
fn ext06_folder_rpc_errors_preserve_stable_code_and_reject_conflicting_envelopes() {
    for code in ErrorCode::ALL {
        let mut value = json!({"jsonrpc":"2.0","id":"r","error":{"code":code.as_str(),"message":code.as_str()}});
        assert_eq!(parse(&value), Err(code));
        assert_eq!(
            parse_refresh_receipt(&serde_json::to_vec(&value).unwrap(), "r", "acct"),
            Err(code)
        );
        value["error"]["message"] = json!("raw provider text");
        assert_eq!(parse(&value), Err(ErrorCode::InvalidInput));
    }
    let mut value = page();
    value["error"] = json!({"code":"OFFLINE","message":"OFFLINE"});
    assert_eq!(parse(&value), Err(ErrorCode::InvalidInput));
}
#[test]
fn ext06_folder_timestamp_requires_real_utc_calendar_and_milliseconds() {
    for stamp in [
        "2026-99-30T00:00:00.000Z",
        "2026-02-29T00:00:00.000Z",
        "2026-10-30T24:00:00.000Z",
        "2026-10-30T00:60:00.000Z",
        "2026-10-30T00:00:60.000Z",
        "2026-10-30Txx:yy:zzZ",
        "2026-10-30T00:00:00Z",
        "2026-10-30T00:00:00.0000Z",
    ] {
        let mut value = page();
        value["result"]["observed_at"] = json!(stamp);
        assert_eq!(parse(&value), Err(ErrorCode::InvalidInput), "{stamp}");
    }
    let mut good = page();
    good["result"]["observed_at"] = json!("2024-02-29T23:59:59.999Z");
    assert!(parse(&good).is_ok());
}
#[test]
fn ext06_folder_page_cursor_must_match_generation_and_last_returned_wire() {
    for cursor in ["v1:acct:2:494e424f58", "v1:acct:1:5a"] {
        let mut value = page();
        value["result"]["next_cursor"] = json!(cursor);
        value["result"]["completeness"] = json!("partial");
        value["result"]["account_results"][0]["status"] = json!("partial");
        assert_eq!(parse(&value), Err(ErrorCode::InvalidInput));
    }
    let req = FolderListRequest::new(
        "acct",
        Some(2),
        Some(FolderCursor::new("acct", 1, "494e424f58").unwrap()),
    )
    .unwrap();
    assert_eq!(
        parse_folder_page(&serde_json::to_vec(&page()).unwrap(), "r", &req),
        Err(ErrorCode::InvalidInput)
    );
    let mut empty = page();
    empty["result"]["items"] = json!([]);
    empty["result"]["account_results"][0]["items"] = json!([]);
    empty["result"]["account_results"][0]["status"] = json!("partial");
    empty["result"]["completeness"] = json!("partial");
    empty["result"]["next_cursor"] = json!("v1:acct:1:494e424f58");
    assert_eq!(parse(&empty), Err(ErrorCode::InvalidInput));
}

#[test]
fn ext06_folder_status_binds_exact_execution_and_rejects_overflow() {
    let status = FolderRefreshStatusRequest::new("acct", [1; 32]).unwrap();
    let mut value = json!({"jsonrpc":"2.0","id":"r","result":{"schema_version":1,"ok":true,"account_id":"acct","execution_id":hantong_contracts::wire_hex::encode(&[1;32]),"state":"succeeded","generation":1,"error_code":null}});
    assert!(
        parse_refresh_receipt_for_status(&serde_json::to_vec(&value).unwrap(), "r", &status)
            .is_ok()
    );
    value["result"]["execution_id"] = json!(hantong_contracts::wire_hex::encode(&[2; 32]));
    assert_eq!(
        parse_refresh_receipt_for_status(&serde_json::to_vec(&value).unwrap(), "r", &status),
        Err(ErrorCode::InvalidInput)
    );
    value["result"]["generation"] = json!(u64::MAX);
    assert_eq!(
        parse_refresh_receipt(&serde_json::to_vec(&value).unwrap(), "r", "acct"),
        Err(ErrorCode::InvalidInput)
    );
    for field in ["generation", "account_version", "endpoint_version"] {
        let mut value = page();
        value["result"]["metadata"][field] = json!(u64::MAX);
        assert_eq!(parse(&value), Err(ErrorCode::InvalidInput));
    }
}
#[test]
fn sec03_folder_client_authorization_requires_both_exact_account_and_tool() {
    use hantong_contracts::auth::*;
    let legacy = RequestedScope::new(vec!["acct".into()], default_cli_legacy_tools()).unwrap();
    let folders = RequestedScope::new(vec!["acct".into()], default_cli_folder_tools()).unwrap();
    for tool in ["mail.folders.list", "mail.folders.refresh"] {
        assert_eq!(
            authorize_folder(&legacy, "acct", tool),
            Err(ErrorCode::ScopeDenied)
        );
        assert_eq!(
            authorize_folder(&folders, "other", tool),
            Err(ErrorCode::ScopeDenied)
        );
        assert_eq!(authorize_folder(&folders, "acct", tool), Ok(()));
    }
}
#[test]
fn ext06_folder_json_depth_and_frame_limits_fail_without_recursing_unbounded() {
    let raw = format!("{}0{}", "[".repeat(256), "]".repeat(256));
    assert_eq!(
        parse_folder_page(raw.as_bytes(), "r", &request()),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        parse_folder_page(&vec![b' '; MAX_FRAME + 1], "r", &request()),
        Err(ErrorCode::InvalidInput)
    );
}
#[cfg(windows)]
#[test]
fn ext06_folder_refresh_intent_hash_has_fixed_cross_implementation_vector() {
    let value = prepare_refresh_request("acct", "op", "key", 0).unwrap();
    assert_eq!(
        value.payload_hash(),
        "6b7272f84f53daf6e74ebeb5bbca10f0fdbbd2aa99a7cad7897998d79fcc4728"
    );
    assert!(prepare_refresh_request("acct", "op", "key", i64::MAX as u64).is_err());
}
