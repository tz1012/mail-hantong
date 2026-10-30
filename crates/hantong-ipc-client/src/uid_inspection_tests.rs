use super::*;
use serde_json::{Value, json};

fn request() -> UidCursorRequest {
    UidCursorRequest::new(
        "acct",
        UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap(),
    )
    .unwrap()
}
fn snapshot() -> Value {
    json!({"schema_version":1,"ok":true,"account_id":"acct","wire_hex":"494e424f58","encoding":"modified_utf7","catalog_generation":4,"binding_current":false,"observation":{"cursor_version":3,"epoch_generation":2,"covered_through_uid":64,"observed_uid_ceiling":500,"stored_at_utc_ms":1767225600000_i64,"coverage_at_observed_ceiling":false}})
}
fn rpc(value: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":"inspect-test","result":value})).unwrap()
}
fn parse(value: Value) -> Result<UidInspectionSnapshot, ErrorCode> {
    parse_inspection_snapshot(&rpc(value), "inspect-test", &request())
}

#[test]
fn sync02_inspection_client_roundtrips_unobserved_empty_and_sparse_bounds() {
    for observation in [
        Value::Null,
        json!({"cursor_version":1,"epoch_generation":1,"covered_through_uid":0,"observed_uid_ceiling":0,"stored_at_utc_ms":1,"coverage_at_observed_ceiling":true}),
        snapshot()["observation"].clone(),
    ] {
        let mut value = snapshot();
        value["observation"] = observation;
        let decoded = parse(value.clone()).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&inspection_json(&decoded)).unwrap(),
            value
        );
        assert_eq!(
            decoded.observation().is_none(),
            value["observation"].is_null()
        );
    }
    let mut max = snapshot();
    max["catalog_generation"] = json!(i64::MAX);
    max["observation"] = json!({"cursor_version":i64::MAX,"epoch_generation":i64::MAX,"covered_through_uid":u32::MAX-1,"observed_uid_ceiling":u32::MAX-1,"stored_at_utc_ms":i64::MAX,"coverage_at_observed_ceiling":true});
    assert!(parse(max).is_ok());
}

#[test]
fn sync02_inspection_client_requires_every_field_and_exact_nested_types() {
    for key in snapshot().as_object().unwrap().keys() {
        let mut missing = snapshot();
        missing.as_object_mut().unwrap().remove(key);
        assert!(parse(missing).is_err(), "missing {key}");
        if key != "observation" {
            let mut null = snapshot();
            null[key] = Value::Null;
            assert!(parse(null).is_err(), "null {key}");
        }
    }
    for key in snapshot()["observation"].as_object().unwrap().keys() {
        let mut missing = snapshot();
        missing["observation"].as_object_mut().unwrap().remove(key);
        assert!(parse(missing).is_err(), "missing nested {key}");
        for value in [Value::Null, json!("1"), json!([]), json!({})] {
            let mut invalid = snapshot();
            invalid["observation"][key] = value;
            assert!(parse(invalid).is_err(), "invalid nested {key}");
        }
    }
    for (key, value) in [
        ("cursor_version", json!(0)),
        ("cursor_version", json!(u64::MAX)),
        ("epoch_generation", json!(0)),
        ("epoch_generation", json!(4)),
        ("covered_through_uid", json!(501)),
        ("covered_through_uid", json!(500)),
        ("covered_through_uid", json!(64.0)),
        ("observed_uid_ceiling", json!(u32::MAX)),
        ("observed_uid_ceiling", json!(-1)),
        ("stored_at_utc_ms", json!(0)),
        ("stored_at_utc_ms", json!(-1)),
        ("stored_at_utc_ms", json!(u64::MAX)),
        ("stored_at_utc_ms", json!(1.0)),
        ("coverage_at_observed_ceiling", json!(true)),
        ("coverage_at_observed_ceiling", json!(0)),
    ] {
        let mut invalid = snapshot();
        invalid["observation"][key] = value;
        assert!(parse(invalid).is_err(), "{key}");
    }
    for (key, value) in [
        ("schema_version", json!(2)),
        ("schema_version", json!(1.0)),
        ("ok", json!(false)),
        ("ok", json!(1)),
        ("catalog_generation", json!(0)),
        ("catalog_generation", json!(u64::MAX)),
        ("binding_current", json!("false")),
        ("observation", json!([])),
        ("observation", json!(false)),
        ("observation", json!(0)),
    ] {
        let mut invalid = snapshot();
        invalid[key] = value;
        assert!(parse(invalid).is_err(), "{key}");
    }
}

#[test]
fn sec08_inspection_client_rejects_duplicates_unknowns_and_oversize_frames() {
    let raw = String::from_utf8(rpc(snapshot())).unwrap();
    for malformed in [
        raw.replace(
            "\"cursor_version\":3",
            "\"cursor_version\":3,\"cursor_version\":3",
        ),
        raw.replace(
            "\"coverage_at_observed_ceiling\":false",
            "\"coverage_at_observed_ceiling\":false,\"coverage_at_observed_ceiling\":false",
        ),
        raw.replace(
            "\"account_id\":\"acct\"",
            "\"account_id\":\"acct\",\"account_id\":\"acct\"",
        ),
        raw.replace(
            "\"jsonrpc\":\"2.0\"",
            "\"jsonrpc\":\"2.0\",\"jsonrpc\":\"2.0\"",
        ),
        raw.replace(
            "\"id\":\"inspect-test\"",
            "\"id\":\"inspect-test\",\"id\":\"inspect-test\"",
        ),
        raw.replace(
            "\"cursor_version\":3",
            "\"cursor_version\":3,\"uid_validity\":9",
        ),
        raw.replace(
            "\"account_id\":\"acct\"",
            "\"account_id\":\"acct\",\"endpoint\":\"private\"",
        ),
        raw.replace(
            "\"id\":\"inspect-test\"",
            "\"id\":\"inspect-test\",\"extra\":true",
        ),
        format!("{raw} {{}}"),
    ] {
        assert!(
            parse_inspection_snapshot(malformed.as_bytes(), "inspect-test", &request()).is_err()
        );
    }
    let null = String::from_utf8(rpc({
        let mut value = snapshot();
        value["observation"] = Value::Null;
        value
    }))
    .unwrap();
    assert!(
        parse_inspection_snapshot(
            null.replace(
                "\"observation\":null",
                "\"observation\":null,\"observation\":null"
            )
            .as_bytes(),
            "inspect-test",
            &request()
        )
        .is_err()
    );
    let mut bytes = rpc(snapshot());
    bytes.resize(8192, b' ');
    assert!(parse_inspection_snapshot(&bytes, "inspect-test", &request()).is_ok());
    bytes.push(b' ');
    assert!(parse_inspection_snapshot(&bytes, "inspect-test", &request()).is_err());
    assert!(parse_inspection_snapshot(&[], "inspect-test", &request()).is_err());
}

#[test]
fn sec08_inspection_client_binds_rpc_account_and_full_mailbox_identity() {
    for (key, value) in [
        ("account_id", json!("other")),
        ("account_id", json!("")),
        ("wire_hex", json!("41")),
        ("wire_hex", json!("494E424F58")),
        ("encoding", json!("utf8")),
        ("encoding", json!("UTF8")),
    ] {
        let mut invalid = snapshot();
        invalid[key] = value;
        assert!(parse(invalid).is_err());
    }
    for id in ["wrong", "", "bad id"] {
        assert!(parse_inspection_snapshot(&rpc(snapshot()), id, &request()).is_err());
    }
    let mut envelope: Value = serde_json::from_slice(&rpc(snapshot())).unwrap();
    envelope["jsonrpc"] = json!("1.0");
    assert!(
        parse_inspection_snapshot(
            &serde_json::to_vec(&envelope).unwrap(),
            "inspect-test",
            &request()
        )
        .is_err()
    );
}

#[test]
fn sec08_inspection_client_accepts_only_stable_error_envelopes() {
    let mut error = json!({"jsonrpc":"2.0","id":"inspect-test","error":{"code":"NOT_FOUND","message":"NOT_FOUND"}});
    assert_eq!(
        parse_inspection_snapshot(
            &serde_json::to_vec(&error).unwrap(),
            "inspect-test",
            &request()
        ),
        Err(ErrorCode::NotFound)
    );
    error["error"]["message"] = json!("private details");
    assert_eq!(
        parse_inspection_snapshot(
            &serde_json::to_vec(&error).unwrap(),
            "inspect-test",
            &request()
        ),
        Err(ErrorCode::InvalidInput)
    );
}
