use super::*;
use hantong_contracts::{
    message_search::ImapMetadataFilter,
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use serde_json::json;
fn request(filter: ImapMetadataFilter, limit: u32) -> ImapMetadataSearchRequest {
    ImapMetadataSearchRequest::new(
        "a",
        UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap(),
        Some(filter),
        Some(limit),
        None,
    )
    .unwrap()
}
fn response() -> Value {
    let item = json!({"internal_message_id":format!("msg_{}","a".repeat(64)),"account_id":"a","uid":3,"internal_date_utc_ms":0,"size_bytes":10,"is_read":false,"is_favorite":true,"header_content_index":0});
    let header = "{\"from\":null,\"subject_hex\":\"73656e74696e656c\"}";
    json!({"jsonrpc":"2.0","id":"r","result":{"schema_version":1,"ok":true,"items":[item.clone()],"next_cursor":null,"account_results":[{"account_id":"a","status":"partial","items":[item]}],"completeness":"partial","warnings":["LOCAL_METADATA_ONLY"],"observed_at":"2026-10-30T00:00:00.001Z","metadata":{"provider":"imap","source":"local_store","cached":true},"untrusted_content":[{"content_type":"header","content":header,"byte_length":header.len(),"truncated":false}]}})
}
fn parse(value: &Value) -> Result<ValidatedMessageSearchResponse, ErrorCode> {
    parse_search_response(
        &serde_json::to_vec(value).unwrap(),
        "r",
        &request(ImapMetadataFilter::All, 16),
    )
}
#[test]
fn mail03_search_client_accepts_observed_unobserved_and_stale_without_false_complete() {
    let original = response();
    let parsed = parse(&original).unwrap();
    assert_eq!(parsed.value(), &original["result"]);
    let mut empty = response();
    let r = &mut empty["result"];
    r["items"] = json!([]);
    r["untrusted_content"] = json!([]);
    r["account_results"][0]["items"] = json!([]);
    r["observed_at"] = Value::Null;
    r["completeness"] = json!("unavailable");
    r["account_results"][0]["status"] = json!("unavailable");
    r["warnings"] = json!(["LOCAL_CACHE_UNOBSERVED", "STALE_BINDING"]);
    parse(&empty).unwrap();
}
#[test]
fn sec08_search_client_rejects_original_stream_duplicates_at_every_object_boundary() {
    let original = response().to_string();
    for (key, value) in [
        ("jsonrpc", "\"2.0\""),
        ("schema_version", "1"),
        ("uid", "3"),
        ("status", "\"partial\""),
        ("provider", "\"imap\""),
        ("byte_length", "0"),
    ] {
        let marker = format!("\"{key}\":");
        let corrupted = original.replacen(&marker, &format!("{marker}{value},{marker}"), 1);
        assert!(
            parse_search_response(
                corrupted.as_bytes(),
                "r",
                &request(ImapMetadataFilter::All, 16)
            )
            .is_err(),
            "duplicate {key}"
        );
    }
}
#[test]
fn sec08_search_client_rejects_bad_binding_structure_and_untrusted_payload() {
    for (path, bad) in [
        ("/extra", json!(true)),
        ("/result/metadata/cached", json!(false)),
        ("/result/metadata/provider", json!("graph")),
        ("/result/account_results/0/status", json!("complete")),
        ("/result/account_results/0/items/0/uid", json!(4)),
        ("/result/items/0/uid", json!(0)),
        ("/result/items/0/internal_message_id", json!("bad")),
        ("/result/items/0/is_read", json!("false")),
        ("/result/items/0/header_content_index", json!(1)),
        ("/result/items/0/size_bytes", json!(-1)),
        ("/result/items/0/internal_date_utc_ms", json!(1)),
        ("/result/untrusted_content/0/byte_length", json!(0)),
        (
            "/result/untrusted_content/0/content",
            json!("{\"from\":null,\"from\":null,\"subject_hex\":null}"),
        ),
        ("/result/observed_at", json!("2026-02-30T00:00:00.000Z")),
        ("/result/next_cursor", json!("bad")),
        ("/result/warnings", json!([])),
        (
            "/result/warnings",
            json!(["STALE_BINDING", "LOCAL_METADATA_ONLY"]),
        ),
    ] {
        let mut value = response();
        if path == "/extra" {
            value["extra"] = bad;
        } else {
            *value.pointer_mut(path).unwrap() = bad;
        }
        assert!(parse(&value).is_err(), "accepted corruption {path}");
    }
}
#[test]
fn mail03_search_client_binds_limit_filter_and_descending_unique_items() {
    let mut value = response();
    let mut second = value["result"]["items"][0].clone();
    second["uid"] = json!(2);
    second["header_content_index"] = json!(1);
    second["internal_message_id"] = json!(format!("msg_{}", "b".repeat(64)));
    value["result"]["items"]
        .as_array_mut()
        .unwrap()
        .push(second);
    value["result"]["account_results"][0]["items"] = value["result"]["items"].clone();
    let header = value["result"]["untrusted_content"][0].clone();
    value["result"]["untrusted_content"]
        .as_array_mut()
        .unwrap()
        .push(header);
    let raw = value.to_string();
    assert!(
        parse_search_response(raw.as_bytes(), "r", &request(ImapMetadataFilter::All, 1)).is_err()
    );
    parse_search_response(
        raw.as_bytes(),
        "r",
        &request(ImapMetadataFilter::Favorite, 2),
    )
    .unwrap();
    value["result"]["items"][0]["is_favorite"] = json!(false);
    value["result"]["account_results"][0]["items"] = value["result"]["items"].clone();
    assert!(
        parse_search_response(
            value.to_string().as_bytes(),
            "r",
            &request(ImapMetadataFilter::Favorite, 2)
        )
        .is_err()
    );
}

#[test]
fn sec08_search_client_requires_object_fields_and_explicit_nullable_members() {
    for pointer in [
        "",
        "/result",
        "/result/items/0",
        "/result/account_results/0",
        "/result/account_results/0/items/0",
        "/result/metadata",
        "/result/untrusted_content/0",
    ] {
        let original = response();
        let fields = original
            .pointer(pointer)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for field in fields {
            let mut value = original.clone();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(&field);
            assert!(parse(&value).is_err(), "missing {pointer}/{field}");
        }
        let mut value = original.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), json!(true));
        assert!(parse(&value).is_err(), "unknown at {pointer}");
        let mut value = original.clone();
        let object = value.pointer_mut(pointer).unwrap();
        *object = Value::Array(object.as_object().unwrap().values().cloned().collect());
        assert!(parse(&value).is_err(), "array at {pointer}");
    }
}

#[test]
fn sec08_search_client_validates_canonical_header_json_and_raw_limits() {
    for payload in [
        json!({"from":null}),
        json!({"subject_hex":null}),
        json!({"from":[],"subject_hex":null}),
        json!({"from":null,"subject_hex":"A0"}),
        json!({"from":null,"subject_hex":"0"}),
        json!({"from":null,"subject_hex":"00".repeat(8193)}),
        json!({"from":[{"host_hex":null,"mailbox_hex":null,"name_hex":null}],"subject_hex":null}),
        json!({"from":[{"host_hex":null,"mailbox_hex":null,"name_hex":null,"route_hex":false}],"subject_hex":null}),
    ] {
        let mut value = response();
        let content = payload.to_string();
        value["result"]["untrusted_content"][0]["byte_length"] = json!(content.len());
        value["result"]["untrusted_content"][0]["content"] = json!(content);
        assert!(parse(&value).is_err());
    }
    let mut valid = response();
    let content = r#"{"from":[{"host_hex":"","mailbox_hex":null,"name_hex":"ff00eab080","route_hex":null}],"subject_hex":""}"#;
    valid["result"]["untrusted_content"][0]["content"] = json!(content);
    valid["result"]["untrusted_content"][0]["byte_length"] = json!(content.len());
    let decoded = parse(&valid).unwrap();
    assert_eq!(decoded.headers()[0].subject(), Some(&[][..]));
    assert_eq!(
        decoded.headers()[0].from().unwrap()[0].host(),
        Some(&[][..])
    );
    assert!(!format!("{decoded:?}").contains("ff00"));
    assert_eq!(
        serde_json::from_str::<Value>(&search_json(&decoded).unwrap()).unwrap(),
        valid["result"]
    );
}

#[test]
fn sec08_search_client_exact_frame_bound_and_stable_error_envelope() {
    let request = request(ImapMetadataFilter::All, 16);
    let mut bytes = response().to_string().into_bytes();
    bytes.resize(MAX_FRAME, b' ');
    parse_search_response(&bytes, "r", &request).unwrap();
    bytes.push(b' ');
    assert!(parse_search_response(&bytes, "r", &request).is_err());
    for code in ErrorCode::ALL {
        let value = json!({"jsonrpc":"2.0","id":"r","error":{"code":code.as_str(),"message":code.as_str()}});
        assert_eq!(
            parse_search_response(value.to_string().as_bytes(), "r", &request).unwrap_err(),
            code
        );
        let mut bad = value.clone();
        bad["result"] = json!({});
        assert!(parse(&bad).is_err());
        let mut bad = value.clone();
        bad["error"]["message"] = json!("untrusted diagnostic");
        assert!(parse(&bad).is_err());
        let mut bad = value;
        bad["error"]["data"] = json!({});
        assert!(parse(&bad).is_err());
    }
}

#[test]
fn sec08_search_client_enforces_complete_result_byte_budget() {
    let mut value = response();
    let template = value["result"]["items"][0].clone();
    let items = (1..=32)
        .rev()
        .enumerate()
        .map(|(index, uid)| {
            let mut item = template.clone();
            item["uid"] = json!(uid);
            item["header_content_index"] = json!(index);
            item["internal_message_id"] = json!(format!("msg_{uid:064x}"));
            item
        })
        .collect::<Vec<_>>();
    let content = format!(
        "{{\"from\":null,\"subject_hex\":\"{}\"}}",
        "61".repeat(8192)
    );
    let header = json!({"content_type":"header","content":content,"byte_length":content.len(),"truncated":false});
    value["result"]["items"] = json!(items);
    value["result"]["account_results"][0]["items"] = json!(items);
    value["result"]["untrusted_content"] = json!(vec![header; 32]);
    let bytes = value.to_string().into_bytes();
    assert!(bytes.len() > MAX_RESULT && bytes.len() < MAX_FRAME);
    assert!(parse_search_response(&bytes, "r", &request(ImapMetadataFilter::All, 32)).is_err());
}
