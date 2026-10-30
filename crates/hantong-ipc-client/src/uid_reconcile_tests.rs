use super::*;
use hantong_contracts::uid_page::{UidMailboxEncoding, UidMailboxIdentity};
fn request() -> UidReconcileRequest {
    UidReconcileRequest::new(
        "acct",
        UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap(),
        "op",
        "key",
        1,
        3,
        &"00".repeat(32),
    )
    .unwrap()
}
fn receipt() -> Value {
    json!({"schema_version":1,"ok":true,"account_id":"acct","execution_id":"01".repeat(32),"state":"succeeded","result":{"generation":1,"frozen_cursor_version":3,"source_cursor_version":2,"after_uid":64,"through_uid":128,"observed_count":2,"flags_changed_count":1,"newly_absent_count":1,"remaining_pages":0,"pass_complete":true},"error_code":null})
}
fn rpc(value: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"test","result":value})).unwrap()
}
#[test]
fn ext04_reconcile_client_roundtrips_and_binds_account_execution_and_ceiling() {
    let value = parse_reconcile_receipt(&rpc(receipt()), "test", &request()).unwrap();
    assert_eq!(value.result().unwrap().observed_count(), 2);
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
    for (key, value) in [
        ("account_id", json!("other")),
        ("execution_id", json!("00".repeat(32))),
        ("schema_version", json!(2)),
        ("ok", json!(false)),
    ] {
        let mut r = receipt();
        r[key] = value;
        assert!(parse_reconcile_receipt(&rpc(r), "test", &request()).is_err());
    }
    let mut r = receipt();
    r["result"]["frozen_cursor_version"] = json!(4);
    assert!(parse_reconcile_receipt(&rpc(r), "test", &request()).is_err());
    assert!(parse_reconcile_receipt(&rpc(receipt()), "wrong", &request()).is_err());
}
#[test]
fn sec08_reconcile_client_raw_nested_duplicates_unknown_missing_and_types_fail() {
    let raw = String::from_utf8(rpc(receipt())).unwrap();
    for malformed in [
        raw.replace("\"generation\":1", "\"generation\":1,\"generation\":2"),
        raw.replace("\"generation\":1", "\"generation\":1,\"body\":\"private\""),
        raw.replace("\"generation\":1,", ""),
        raw.replace("\"generation\":1", "\"generation\":1.0"),
        raw.replace("\"generation\":1", "\"generation\":null"),
        raw.replace("\"error_code\":null,", ""),
        raw.replace(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
        ),
        raw.replace(
            "\"jsonrpc\":\"2.0\"",
            "\"jsonrpc\":\"2.0\",\"jsonrpc\":\"2.0\"",
        ),
    ] {
        assert!(parse_reconcile_receipt(malformed.as_bytes(), "test", &request()).is_err());
    }
    for key in ["result", "error_code"] {
        let mut r = receipt();
        r.as_object_mut().unwrap().remove(key);
        assert!(parse_reconcile_receipt(&rpc(r), "test", &request()).is_err());
    }
    assert!(parse_reconcile_receipt(&vec![b' '; 8193], "test", &request()).is_err());
    let mut at_limit = rpc(receipt());
    at_limit.resize(8192, b' ');
    assert!(parse_reconcile_receipt(&at_limit, "test", &request()).is_ok());
}
#[test]
fn sync02_reconcile_client_rejects_impossible_progress_and_state() {
    for (key, value) in [
        ("observed_count", json!(0)),
        ("observed_count", json!(65)),
        ("flags_changed_count", json!(2)),
        ("remaining_pages", json!(1)),
        ("through_uid", json!(129)),
        ("source_cursor_version", json!(4)),
    ] {
        let mut r = receipt();
        r["result"][key] = value;
        assert!(parse_reconcile_receipt(&rpc(r), "test", &request()).is_err());
    }
    for (state, result, error) in [
        ("unresolved", receipt()["result"].clone(), Value::Null),
        ("succeeded", Value::Null, Value::Null),
        ("failed", Value::Null, Value::Null),
        ("succeeded", receipt()["result"].clone(), json!("CONFLICT")),
    ] {
        let mut r = receipt();
        r["state"] = json!(state);
        r["result"] = result;
        r["error_code"] = error;
        assert!(parse_reconcile_receipt(&rpc(r), "test", &request()).is_err());
    }
    for (state, error) in [("unresolved", Value::Null), ("failed", json!("CONFLICT"))] {
        let mut r = receipt();
        r["state"] = json!(state);
        r["result"] = Value::Null;
        r["error_code"] = error;
        assert!(parse_reconcile_receipt(&rpc(r), "test", &request()).is_ok());
    }
}
#[test]
fn sec08_reconcile_client_accepts_only_fixed_error_text() {
    let mut error =
        json!({"jsonrpc":"2.0","id":"test","error":{"code":"CONFLICT","message":"CONFLICT"}});
    assert_eq!(
        parse_reconcile_receipt(&serde_json::to_vec(&error).unwrap(), "test", &request()),
        Err(ErrorCode::Conflict)
    );
    error["error"]["message"] = json!("private server text");
    assert_eq!(
        parse_reconcile_receipt(&serde_json::to_vec(&error).unwrap(), "test", &request()),
        Err(ErrorCode::InvalidInput)
    );
}
