use super::*;
use hantong_contracts::uid_page::{UidMailboxEncoding, UidMailboxIdentity};
fn request() -> UidRecoveryRequest {
    UidRecoveryRequest::new(
        "acct",
        UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap(),
        "op",
        "key",
        1,
        4,
        &"00".repeat(32),
    )
    .unwrap()
}
fn receipt() -> Value {
    json!({"schema_version":1,"ok":true,"account_id":"acct","execution_id":"01".repeat(32),"state":"succeeded","result":{"epoch_generation":2,"cursor_version":5,"covered_through_uid":64,"inserted_count":2},"error_code":null})
}
fn rpc(value: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"test","result":value})).unwrap()
}
#[test]
fn sync02_recovery_client_roundtrip_binds_exact_next_cursor_and_status_identity() {
    let parsed = parse_recovery_receipt(&rpc(receipt()), "test", &request()).unwrap();
    assert_eq!(parsed.result().unwrap().cursor_version(), 5);
    assert_eq!(
        serde_json::from_str::<Value>(&receipt_json(&parsed)).unwrap(),
        receipt()
    );
    assert!(
        parse_status_receipt(
            &rpc(receipt()),
            "test",
            &UidPageStatusRequest::new("acct", [1; 32]).unwrap()
        )
        .is_ok()
    );
    for (account, id) in [("other", [1; 32]), ("acct", [2; 32])] {
        assert!(
            parse_status_receipt(
                &rpc(receipt()),
                "test",
                &UidPageStatusRequest::new(account, id).unwrap()
            )
            .is_err()
        );
    }
    for cursor in [4, 6] {
        let mut r = receipt();
        r["result"]["cursor_version"] = json!(cursor);
        assert!(parse_recovery_receipt(&rpc(r), "test", &request()).is_err());
    }
    let mut empty = receipt();
    empty["result"]["covered_through_uid"] = json!(0);
    empty["result"]["inserted_count"] = json!(0);
    assert!(parse_recovery_receipt(&rpc(empty), "test", &request()).is_ok());
}
#[test]
fn sec08_recovery_client_rejects_nested_duplicates_unknown_missing_types_and_oversize() {
    let raw = String::from_utf8(rpc(receipt())).unwrap();
    for malformed in [
        raw.replace(
            "\"epoch_generation\":2",
            "\"epoch_generation\":2,\"epoch_generation\":3",
        ),
        raw.replace(
            "\"epoch_generation\":2",
            "\"epoch_generation\":2,\"uid_validity\":7",
        ),
        raw.replace("\"epoch_generation\":2,", ""),
        raw.replace("\"epoch_generation\":2", "\"epoch_generation\":2.0"),
        raw.replace("\"epoch_generation\":2", "\"epoch_generation\":null"),
        raw.replace("\"error_code\":null,", ""),
        raw.replace(
            "\"jsonrpc\":\"2.0\"",
            "\"jsonrpc\":\"2.0\",\"jsonrpc\":\"2.0\"",
        ),
    ] {
        assert!(parse_recovery_receipt(malformed.as_bytes(), "test", &request()).is_err());
    }
    for key in ["result", "error_code"] {
        let mut r = receipt();
        r.as_object_mut().unwrap().remove(key);
        assert!(parse_recovery_receipt(&rpc(r), "test", &request()).is_err());
    }
    let mut boundary = rpc(receipt());
    boundary.resize(8192, b' ');
    assert!(parse_recovery_receipt(&boundary, "test", &request()).is_ok());
    boundary.push(b' ');
    assert!(parse_recovery_receipt(&boundary, "test", &request()).is_err());
}
#[test]
fn sync02_recovery_client_rejects_impossible_epoch_page_and_terminal_shapes() {
    for (key, value) in [
        ("epoch_generation", json!(1)),
        ("epoch_generation", json!(6)),
        ("covered_through_uid", json!(65)),
        ("inserted_count", json!(65)),
    ] {
        let mut r = receipt();
        r["result"][key] = value;
        assert!(parse_recovery_receipt(&rpc(r), "test", &request()).is_err());
    }
    for (key, value) in [
        ("account_id", json!("other")),
        ("execution_id", json!("00".repeat(32))),
        ("schema_version", json!(2)),
        ("state", json!("running")),
        ("ok", json!(false)),
        ("error_code", json!("CONFLICT")),
    ] {
        let mut r = receipt();
        r[key] = value;
        assert!(parse_recovery_receipt(&rpc(r), "test", &request()).is_err());
    }
    for (state, error) in [("unresolved", Value::Null), ("failed", json!("CONFLICT"))] {
        let mut r = receipt();
        r["state"] = json!(state);
        r["result"] = Value::Null;
        r["error_code"] = error;
        assert!(parse_recovery_receipt(&rpc(r), "test", &request()).is_ok());
    }
    let mut r = receipt();
    r["result"] = Value::Null;
    assert!(parse_recovery_receipt(&rpc(r), "test", &request()).is_err());
}
#[test]
fn sec08_recovery_client_uses_only_fixed_error_envelopes() {
    let mut error =
        json!({"jsonrpc":"2.0","id":"test","error":{"code":"CONFLICT","message":"CONFLICT"}});
    assert_eq!(
        parse_recovery_receipt(&serde_json::to_vec(&error).unwrap(), "test", &request()),
        Err(ErrorCode::Conflict)
    );
    error["error"]["message"] = json!("private server details");
    assert_eq!(
        parse_recovery_receipt(&serde_json::to_vec(&error).unwrap(), "test", &request()),
        Err(ErrorCode::InvalidInput)
    );
}
