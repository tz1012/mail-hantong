use super::*;
fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}
fn request() -> UidPageRequest {
    UidPageRequest::new("acct", mailbox(), "op", "key", 1, 0, &"00".repeat(32)).unwrap()
}
fn rpc(value: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"test","result":value})).unwrap()
}
fn receipt() -> Value {
    json!({"schema_version":1,"ok":true,"account_id":"acct","execution_id":"01".repeat(32),"state":"succeeded","cursor_version":1,"covered_through_uid":64,"inserted_count":2,"error_code":null})
}
fn cursor() -> Value {
    json!({"schema_version":1,"ok":true,"account_id":"acct","wire_hex":"494e424f58","encoding":"modified_utf7","catalog_generation":1,"cursor_version":0,"covered_through_uid":0,"binding_current":true})
}
#[test]
fn ext04_uid_client_validated_roundtrip_and_status_binding() {
    let req = request();
    let value = parse_page_receipt(&rpc(receipt()), "test", &req).unwrap();
    assert_eq!(value.inserted_count(), Some(2));
    assert_eq!(
        serde_json::from_str::<Value>(&receipt_json(&value)).unwrap(),
        receipt()
    );
    let query = UidPageStatusRequest::new("acct", [1; 32]).unwrap();
    assert!(parse_status_receipt(&rpc(receipt()), "test", &query).is_ok());
    assert!(
        parse_status_receipt(
            &rpc(receipt()),
            "test",
            &UidPageStatusRequest::new("acct", [2; 32]).unwrap()
        )
        .is_err()
    );
    let view = parse_cursor_snapshot(
        &rpc(cursor()),
        "test",
        &UidCursorRequest::new("acct", mailbox()).unwrap(),
    )
    .unwrap();
    assert_eq!(view.cursor_version(), 0);
    assert_eq!(
        serde_json::from_str::<Value>(&cursor_json(&view)).unwrap(),
        cursor()
    );
}
#[test]
fn sec08_uid_client_raw_duplicate_unknown_missing_and_null_fields_fail() {
    let raw = String::from_utf8(rpc(receipt())).unwrap();
    for malformed in [
        raw.replace(
            "\"state\":\"succeeded\"",
            "\"state\":\"succeeded\",\"state\":\"failed\"",
        ),
        raw.replace(
            "\"jsonrpc\":\"2.0\"",
            "\"jsonrpc\":\"2.0\",\"jsonrpc\":\"2.0\"",
        ),
        raw.replace(
            "\"state\":\"succeeded\"",
            "\"state\":\"succeeded\",\"body\":\"private\"",
        ),
        raw.replace("\"error_code\":null,", ""),
        raw.replace("\"cursor_version\":1", "\"cursor_version\":1.0"),
        raw.replace("\"account_id\":\"acct\"", "\"account_id\":null"),
    ] {
        assert!(parse_page_receipt(malformed.as_bytes(), "test", &request()).is_err());
    }
    assert!(parse_page_receipt(&vec![b' '; MAX_FRAME + 1], "test", &request()).is_err());
}
#[test]
fn sync02_uid_client_rejects_cross_request_and_impossible_results() {
    for (key, value) in [
        ("account_id", json!("other")),
        ("cursor_version", json!(2)),
        ("inserted_count", json!(65)),
        ("covered_through_uid", json!(u32::MAX)),
        ("state", json!("running")),
        ("execution_id", json!("00".repeat(32))),
        ("error_code", json!("CONFLICT")),
    ] {
        let mut row = receipt();
        row[key] = value;
        assert!(parse_page_receipt(&rpc(row), "test", &request()).is_err());
    }
    assert!(parse_page_receipt(&rpc(receipt()), "different", &request()).is_err());
    let req = UidCursorRequest::new("acct", mailbox()).unwrap();
    for (key, value) in [
        ("wire_hex", json!("696e626f78")),
        ("encoding", json!("utf8")),
        ("catalog_generation", json!(0)),
        ("covered_through_uid", json!(1)),
    ] {
        let mut row = cursor();
        row[key] = value;
        assert!(parse_cursor_snapshot(&rpc(row), "test", &req).is_err());
    }
}
#[test]
fn sec08_uid_client_accepts_only_fixed_error_envelope_and_redacts_debug() {
    let error =
        json!({"jsonrpc":"2.0","id":"test","error":{"code":"CONFLICT","message":"CONFLICT"}});
    assert_eq!(
        parse_page_receipt(&serde_json::to_vec(&error).unwrap(), "test", &request()),
        Err(ErrorCode::Conflict)
    );
    let mut error = error;
    error["error"]["message"] = json!("private server text");
    assert_eq!(
        parse_page_receipt(&serde_json::to_vec(&error).unwrap(), "test", &request()),
        Err(ErrorCode::InvalidInput)
    );
    let value = parse_page_receipt(&rpc(receipt()), "test", &request()).unwrap();
    assert!(!format!("{value:?}").contains("acct"));
}
#[test]
fn sec03_uid_client_requires_exact_account_tool_scope() {
    let scope = hantong_contracts::auth::RequestedScope::new(
        vec!["acct".into()],
        vec!["mail.folders.list".into()],
    )
    .unwrap();
    assert_eq!(authorize(&scope, "acct"), Err(ErrorCode::ScopeDenied));
    let scope =
        hantong_contracts::auth::RequestedScope::new(vec!["acct".into()], vec![TOOL.into()])
            .unwrap();
    assert!(authorize(&scope, "acct").is_ok());
    assert_eq!(authorize(&scope, "other"), Err(ErrorCode::ScopeDenied));
}
