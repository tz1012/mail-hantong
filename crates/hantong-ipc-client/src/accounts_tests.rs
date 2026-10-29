use super::*;
use serde_json::Value;
use serde_json::json;

fn item(id: &str, number: u64) -> Value {
    json!({"account_id":id,"provider":"imap","primary_address":null,"display_name":"한통","color":null,"account_number":number,"status":"ready"})
}
fn envelope(items: Vec<Value>) -> Value {
    let rows: Vec<Value> = items
        .iter()
        .map(|item| json!({"account_id":item["account_id"],"status":"complete","items":[item]}))
        .collect();
    json!({"jsonrpc":"2.0","id":"read-1","result":{"schema_version":1,"ok":true,"items":items,"next_cursor":null,"account_results":rows,"completeness":"complete","warnings":[],"observed_at":"2026-01-01T00:00:00Z","metadata":{"source":"local_store","cached":true}}})
}
fn decode(value: &Value, ids: &[&str], caps: bool) -> Result<AccountListResponse, ErrorCode> {
    parse_account_list_response(
        &serde_json::to_vec(value).unwrap(),
        "read-1",
        &ids.iter().map(|v| (*v).into()).collect::<Vec<_>>(),
        caps,
    )
}
fn rejected(value: &Value) {
    assert_eq!(decode(value, &["a"], false), Err(ErrorCode::InvalidInput));
}

#[test]
fn cli_accounts_valid() {
    assert!(decode(&envelope(vec![item("a", 1)]), &["a"], false).is_ok());
}
#[test]
fn cli_accounts_reject_unknown() {
    let mut value = envelope(vec![item("a", 1)]);
    value["result"]["x"] = json!(1);
    rejected(&value);
}
#[test]
fn cli_accounts_reject_duplicate_envelope() {
    let raw=br#"{"jsonrpc":"2.0","id":"read-1","id":"read-1","error":{"code":"INVALID_INPUT","message":"INVALID_INPUT"}}"#;
    assert_eq!(
        parse_account_list_response(raw, "read-1", &["a".into()], false),
        Err(ErrorCode::InvalidInput)
    );
}

#[test]
fn cli01_accounts_sort_by_number_and_compare_requested_ids_as_a_set() {
    let value = envelope(vec![item("z", 1), item("a", 2)]);
    let parsed = decode(&value, &["a", "z"], false).unwrap();
    assert_eq!(parsed.items()[0].account_id(), "z");
    assert_eq!(
        serde_json::from_str::<Value>(&parsed.to_json()).unwrap(),
        value["result"]
    );
}

#[test]
fn cli01_accounts_reject_duplicate_fields_at_every_nested_object() {
    let raw = envelope(vec![item("a", 1)]).to_string();
    for (old, new) in [
        (
            "\"schema_version\":1",
            "\"schema_version\":0,\"schema_version\":1",
        ),
        (
            "\"account_id\":\"a\"",
            "\"account_id\":\"other\",\"account_id\":\"a\"",
        ),
        (
            "\"status\":\"complete\"",
            "\"status\":\"incomplete\",\"status\":\"complete\"",
        ),
        ("\"cached\":true", "\"cached\":false,\"cached\":true"),
    ] {
        let altered = raw.replace(old, new);
        assert_ne!(altered, raw);
        assert_eq!(
            parse_account_list_response(altered.as_bytes(), "read-1", &["a".into()], false),
            Err(ErrorCode::InvalidInput)
        );
    }
    let error=br#"{"jsonrpc":"2.0","id":"read-1","error":{"code":"SCOPE_DENIED","code":"RATE_LIMITED","message":"RATE_LIMITED"}}"#;
    assert_eq!(
        parse_account_list_response(error, "read-1", &["a".into()], false),
        Err(ErrorCode::InvalidInput)
    );
}

#[test]
fn cli01_accounts_nullable_fields_are_required_and_capabilities_cannot_be_null() {
    for name in ["primary_address", "display_name", "color"] {
        let mut value = envelope(vec![item("a", 1)]);
        value["result"]["items"][0]
            .as_object_mut()
            .unwrap()
            .remove(name);
        value["result"]["account_results"][0]["items"][0]
            .as_object_mut()
            .unwrap()
            .remove(name);
        rejected(&value);
    }
    let mut value = envelope(vec![item("a", 1)]);
    value["result"]["items"][0]["capabilities"] = Value::Null;
    value["result"]["account_results"][0]["items"][0]["capabilities"] = Value::Null;
    rejected(&value);
}

#[test]
fn cli01_accounts_objects_reject_positional_arrays() {
    let original = envelope(vec![item("a", 1)]);
    let mut value = original.clone();
    value["result"]["metadata"] = json!(["local_store", true]);
    rejected(&value);
    let mut value = original.clone();
    value["result"]["account_results"][0] = json!(["a", "complete", [item("a", 1)]]);
    rejected(&value);
    let row = json!(["a", "imap", null, "한통", null, 1, "ready", null]);
    let mut value = original;
    value["result"]["items"][0] = row.clone();
    value["result"]["account_results"][0]["items"][0] = row;
    rejected(&value);
}

#[test]
fn cli01_accounts_timestamp_checks_calendar_time_precision_and_positive_epoch() {
    for timestamp in [
        "1970-01-01T00:00:00.001Z",
        "2026-01-01T23:59:59.1Z",
        "2026-01-01T23:59:59.12Z",
        "2026-01-01T23:59:59.123Z",
        "2000-02-29T00:00:00Z",
        "9999-12-31T23:59:59.999Z",
    ] {
        let mut value = envelope(vec![item("a", 1)]);
        value["result"]["observed_at"] = json!(timestamp);
        assert!(decode(&value, &["a"], false).is_ok(), "{timestamp}");
    }
    for timestamp in [
        "1970-01-01T00:00:00Z",
        "1970-01-01T00:00:00.000Z",
        "2026-01-01T99:99:99Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T00:60:00Z",
        "2026-01-01T00:00:60Z",
        "2026-01-01T00:00:00.Z",
        "2026-01-01T00:00:00.0001Z",
        "1900-02-29T00:00:00Z",
        "2100-02-29T00:00:00Z",
        "2026-04-31T00:00:00Z",
    ] {
        let mut value = envelope(vec![item("a", 1)]);
        value["result"]["observed_at"] = json!(timestamp);
        rejected(&value);
    }
}

#[test]
fn cli01_accounts_scope_count_is_bounded_even_with_matching_reply() {
    let rows = (0..129).map(|i| item(&format!("a{i:03}"), i + 1)).collect();
    let ids: Vec<String> = (0..129).map(|i| format!("a{i:03}")).collect();
    assert_eq!(
        parse_account_list_response(
            &serde_json::to_vec(&envelope(rows)).unwrap(),
            "read-1",
            &ids,
            false
        ),
        Err(ErrorCode::InvalidInput)
    );
}

#[test]
fn cli01_accounts_malformed_envelope_has_stable_invalid_input() {
    let body = envelope(vec![item("a", 1)]).to_string();
    for bytes in [
        vec![],
        vec![b' '; MAX_FRAME + 1],
        [b"\xef\xbb\xbf".as_slice(), body.as_bytes()].concat(),
        body.replace("read-1", "other").into_bytes(),
    ] {
        assert_eq!(
            parse_account_list_response(&bytes, "read-1", &["a".into()], false),
            Err(ErrorCode::InvalidInput)
        );
    }
}

#[test]
fn cli01_accounts_reject_unknown_missing_and_wrong_types_in_every_structure() {
    let original = envelope(vec![item("a", 1)]);
    for pointer in [
        "",
        "/result",
        "/result/items/0",
        "/result/account_results/0",
        "/result/account_results/0/items/0",
        "/result/metadata",
    ] {
        let fields: Vec<String> = original
            .pointer(pointer)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for field in fields {
            let mut value = original.clone();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(&field);
            rejected(&value);
        }
        let mut value = original.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("approved".into(), json!(true));
        rejected(&value);
    }
    for (pointer, replacement) in [
        ("/result/ok", json!(1)),
        ("/result/schema_version", json!(1.0)),
        ("/result/items", Value::Null),
        ("/result/next_cursor", json!("next")),
        ("/result/account_results", json!({})),
        ("/result/completeness", json!("partial")),
        ("/result/warnings", json!([null])),
        ("/result/metadata/cached", json!(false)),
        ("/result/metadata/source", json!("provider")),
        ("/result/observed_at", json!(1)),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = replacement;
        rejected(&value);
    }
}

#[test]
fn cli01_accounts_validate_display_values_and_exact_result_rows() {
    for (field, value) in [
        ("account_id", json!("bad/path")),
        ("provider", json!("unknown")),
        ("status", json!("success")),
        ("account_number", json!(0)),
        ("account_number", json!(u64::MAX)),
        ("primary_address", json!(" ")),
        ("primary_address", json!("x".repeat(321))),
        ("display_name", json!("x".repeat(129))),
        ("display_name", json!("escape\u{1b}[31m")),
        ("color", json!("#00000G")),
        ("color", json!("#한통")),
    ] {
        let mut row = item("a", 1);
        row[field] = value;
        rejected(&envelope(vec![row]));
    }
    for (pointer, value) in [
        ("/result/account_results/0/account_id", json!("b")),
        ("/result/account_results/0/status", json!("partial")),
        ("/result/account_results/0/items", json!([])),
        (
            "/result/account_results/0/items/0/display_name",
            json!("different"),
        ),
        ("/result/account_results", json!([])),
    ] {
        let mut response = envelope(vec![item("a", 1)]);
        *response.pointer_mut(pointer).unwrap() = value;
        rejected(&response);
    }
    assert_eq!(
        decode(&envelope(vec![item("a", 1)]), &["b"], false),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        decode(
            &envelope(vec![item("a", 1), item("a", 2)]),
            &["a", "b"],
            false
        ),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(
        decode(
            &envelope(vec![item("b", 2), item("a", 1)]),
            &["a", "b"],
            false
        ),
        Err(ErrorCode::InvalidInput)
    );
}

#[test]
fn cli01_accounts_capabilities_presence_bounds_and_redacted_debug() {
    for caps in [
        json!([]),
        json!(["IMAP4rev1", "mail.read"]),
        json!((0..128).map(|i| format!("c{i:03}")).collect::<Vec<_>>()),
    ] {
        let mut row = item("a", 1);
        row["capabilities"] = caps;
        let value = envelope(vec![row]);
        let parsed = decode(&value, &["a"], true).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&parsed.to_json()).unwrap(),
            value["result"]
        );
        assert!(!format!("{parsed:?} {:?}", parsed.items()[0]).contains("한통"));
        rejected(&value);
    }
    for caps in [
        json!(null),
        json!(["b", "a"]),
        json!(["a", "a"]),
        json!([""]),
        json!(["x".repeat(97)]),
        json!(["한글"]),
        json!((0..129).map(|i| format!("c{i:03}")).collect::<Vec<_>>()),
    ] {
        let mut row = item("a", 1);
        row["capabilities"] = caps;
        assert_eq!(
            decode(&envelope(vec![row]), &["a"], true),
            Err(ErrorCode::InvalidInput)
        );
    }
    assert_eq!(
        decode(&envelope(vec![item("a", 1)]), &["a"], true),
        Err(ErrorCode::InvalidInput)
    );
}

#[test]
fn cli01_accounts_error_codes_are_exact_and_trailing_json_is_rejected() {
    for code in ErrorCode::ALL {
        let value = json!({"jsonrpc":"2.0","id":"read-1","error":{"code":code.as_str(),"message":code.as_str()}});
        assert_eq!(decode(&value, &["a"], false), Err(code));
    }
    for error in [
        json!({"code":"RATE_LIMITED","message":"sensitive detail"}),
        json!({"code":"NEW_CODE","message":"NEW_CODE"}),
        json!(["RATE_LIMITED", "RATE_LIMITED"]),
        json!({"code":"RATE_LIMITED","message":"RATE_LIMITED","data":"secret"}),
    ] {
        rejected(&json!({"jsonrpc":"2.0","id":"read-1","error":error}));
    }
    let original = envelope(vec![item("a", 1)]);
    for suffix in ["{}", "null"] {
        let bytes = format!("{original}{suffix}");
        assert_eq!(
            parse_account_list_response(bytes.as_bytes(), "read-1", &["a".into()], false),
            Err(ErrorCode::InvalidInput)
        );
    }
    let mut both = original;
    both["error"] = json!({"code":"RATE_LIMITED","message":"RATE_LIMITED"});
    rejected(&both);
}
