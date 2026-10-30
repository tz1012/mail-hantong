use crate::{ToolRequest, render_imap_metadata_search};
use hantong_contracts::{
    ErrorCode,
    audit::AuditTool,
    message_search::{
        HeaderAddressProjection, HeaderProjection, ImapMetadataFilter, ImapMetadataSearchItem,
        ImapMetadataSearchPage,
    },
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use serde_json::{Value, json};

fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}
fn params() -> Value {
    json!({"mode":"imap_metadata","account_id":"acct","wire_hex":"494e424f58","encoding":"modified_utf7"})
}
fn envelope(params: Value) -> Value {
    json!({"schema_version":1,"request_id":"metadata-7","tool":"mail.messages.search","params":params})
}
fn page(
    headers: Vec<HeaderProjection>,
    observed: Option<i64>,
    current: bool,
) -> ImapMetadataSearchPage {
    let items = (0..headers.len())
        .map(|n| {
            ImapMetadataSearchItem::new(
                &format!("msg_{:064x}", n + 1),
                "acct",
                (headers.len() - n) as u32,
                1767225600000,
                1234,
                n % 2 == 0,
                n % 2 != 0,
                n as u32,
            )
            .unwrap()
        })
        .collect();
    ImapMetadataSearchPage::new(
        "acct",
        mailbox(),
        3,
        current,
        observed,
        items,
        headers,
        None,
    )
    .unwrap()
}

#[test]
fn mail03_metadata_search_request_preserves_identity_defaults_and_nullable_cursor() {
    for cursor in [None, Some(Value::Null)] {
        let mut p = params();
        if let Some(value) = cursor {
            p["cursor"] = value;
        }
        let parsed = ToolRequest::validate_method("mail.messages.search", &envelope(p)).unwrap();
        assert_eq!(parsed.request_id(), "metadata-7");
        assert_eq!(parsed.tool(), AuditTool::MessagesSearch);
        assert_eq!(parsed.account_ids(), ["acct"]);
        assert!(!format!("{parsed:?}").contains("acct"));
        let ToolRequest::MessagesSearch { request, .. } = parsed else {
            panic!("search")
        };
        assert_eq!(request.mailbox(), &mailbox());
        assert_eq!(request.filter(), ImapMetadataFilter::All);
        assert_eq!(request.limit(), 16);
        assert!(request.cursor().is_none());
    }
    let token = format!("m1:{}:1:4294967295:1767225900000", "ab".repeat(32));
    for filter in ["all", "unread", "favorite"] {
        for limit in [1, 16, 32] {
            let mut p = params();
            p["filter"] = json!(filter);
            p["limit"] = json!(limit);
            p["cursor"] = json!(token);
            let ToolRequest::MessagesSearch { request, .. } =
                ToolRequest::validate(&envelope(p)).unwrap()
            else {
                panic!("search")
            };
            assert_eq!(request.limit(), limit);
            assert_eq!(request.filter().as_str(), filter);
            assert_eq!(request.cursor().unwrap().encode(), token);
        }
    }
    assert_eq!(
        ToolRequest::validate_method("mail.sync.page", &envelope(params())),
        Err(ErrorCode::InvalidInput)
    );
}

#[test]
fn mail03_metadata_search_request_rejects_missing_types_unknown_fields_and_ranges() {
    for field in ["mode", "account_id", "wire_hex", "encoding"] {
        let mut p = params();
        p.as_object_mut().unwrap().remove(field);
        assert_eq!(
            ToolRequest::validate(&envelope(p)),
            Err(ErrorCode::InvalidInput)
        );
        for value in [Value::Null, json!(false), json!(1), json!([]), json!({})] {
            let mut p = params();
            p[field] = value;
            assert_eq!(
                ToolRequest::validate(&envelope(p)),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
    for (field, value) in [
        ("mode", json!("query")),
        ("account_id", json!("")),
        ("account_id", json!("a.b")),
        ("account_id", json!("a".repeat(129))),
        ("wire_hex", json!("")),
        ("wire_hex", json!("0")),
        ("wire_hex", json!("494E424F58")),
        ("wire_hex", json!("696e626f78")),
        ("wire_hex", json!("ff")),
        ("wire_hex", json!("00")),
        ("wire_hex", json!("61".repeat(1025))),
        ("encoding", json!("UTF8")),
        ("filter", Value::Null),
        ("filter", json!("flagged")),
        ("filter", json!(1)),
        ("limit", Value::Null),
        ("limit", json!(0)),
        ("limit", json!(33)),
        ("limit", json!(-1)),
        ("limit", json!(1.0)),
        ("limit", json!("1")),
        ("limit", json!(u64::MAX)),
        ("cursor", json!("")),
        ("cursor", json!(false)),
        ("cursor", json!("x".repeat(257))),
        ("account_ids", json!(["acct"])),
        ("query", json!("anything")),
        ("timezone", json!("UTC")),
        ("body", json!("text")),
        ("uid_validity", json!(1)),
        ("endpoint", json!(1)),
        ("approved", json!(true)),
    ] {
        let mut p = params();
        p[field] = value;
        assert_eq!(
            ToolRequest::validate(&envelope(p)),
            Err(ErrorCode::InvalidInput),
            "{field}"
        );
    }
    for field in ["schema_version", "request_id", "tool", "params"] {
        let mut value = envelope(params());
        value.as_object_mut().unwrap().remove(field);
        assert_eq!(ToolRequest::validate(&value), Err(ErrorCode::InvalidInput));
    }
    let mut value = envelope(params());
    value["approved"] = json!(true);
    assert_eq!(ToolRequest::validate(&value), Err(ErrorCode::InvalidInput));
}

#[test]
fn mail03_metadata_search_unobserved_renderer_is_unavailable_even_when_binding_stale() {
    for current in [true, false] {
        let rendered = render_imap_metadata_search(&page(vec![], None, current)).unwrap();
        assert_eq!(rendered.count(), Some(0));
        let warnings = if current {
            json!(["LOCAL_CACHE_UNOBSERVED"])
        } else {
            json!(["LOCAL_CACHE_UNOBSERVED", "STALE_BINDING"])
        };
        assert_eq!(
            rendered.value(),
            json!({
                "schema_version":1,"ok":true,"items":[],"next_cursor":null,
                "account_results":[{"account_id":"acct","status":"unavailable","items":[]}],
                "completeness":"unavailable","warnings":warnings,"observed_at":null,
                "metadata":{"provider":"imap","source":"local_store","cached":true},"untrusted_content":[]
            })
        );
    }
}

#[test]
fn mail03_metadata_search_renderer_keeps_headers_once_and_structural_count_exact() {
    let headers = vec![
        HeaderProjection::new(None, None).unwrap(),
        HeaderProjection::new(
            Some(vec![]),
            Some(vec![
                HeaderAddressProjection::new(
                    Some(vec![]),
                    Some(b"local".to_vec()),
                    Some(vec![0xff, 0x1b]),
                    None,
                )
                .unwrap(),
            ]),
        )
        .unwrap(),
    ];
    for current in [true, false] {
        let p = page(headers.clone(), Some(1767225600123), current);
        let rendered = render_imap_metadata_search(&p).unwrap();
        assert_eq!(rendered.count(), Some(2));
        let v = rendered.value();
        assert_eq!(v.as_object().unwrap().len(), 10);
        assert_eq!(v["items"], v["account_results"][0]["items"]);
        assert_eq!(v["account_results"][0]["status"], "partial");
        assert_eq!(v["completeness"], "partial");
        assert_eq!(v["observed_at"], "2026-01-01T00:00:00.123Z");
        assert_eq!(
            v["warnings"],
            if current {
                json!(["LOCAL_METADATA_ONLY"])
            } else {
                json!(["LOCAL_METADATA_ONLY", "STALE_BINDING"])
            }
        );
        assert!(v["next_cursor"].is_null());
        for (index, header) in headers.iter().enumerate() {
            assert_eq!(v["items"][index].as_object().unwrap().len(), 8);
            assert_eq!(v["items"][index]["header_content_index"], index);
            assert_eq!(
                v["untrusted_content"][index],
                json!({"content_type":"header","content":header.projection_json(),"byte_length":header.projection_json().len(),"truncated":false})
            );
        }
    }
    let empty = render_imap_metadata_search(&page(vec![], Some(1), true)).unwrap();
    assert_eq!(empty.count(), Some(0));
    assert_eq!(empty.value()["completeness"], "partial");
}

#[test]
fn mail03_metadata_search_renderer_enforces_actual_complete_result_byte_cap() {
    let raw = Some(vec![0xff; 8192]);
    let address = HeaderAddressProjection::new(raw.clone(), raw.clone(), raw.clone(), raw).unwrap();
    let header = HeaderProjection::new(None, Some(vec![address.clone(), address])).unwrap();
    let one = render_imap_metadata_search(&page(vec![header.clone()], Some(1), true))
        .unwrap()
        .value();
    assert!(serde_json::to_vec(&one).unwrap().len() < 524_288);
    assert_eq!(one["untrusted_content"][0]["truncated"], false);
    assert!(matches!(
        render_imap_metadata_search(&page(vec![header; 4], Some(1), true)),
        Err(ErrorCode::QuotaExceeded)
    ));
    let small = HeaderProjection::new(None, None).unwrap();
    for size in [1, 16, 32] {
        let rendered =
            render_imap_metadata_search(&page(vec![small.clone(); size], Some(1), true)).unwrap();
        assert_eq!(rendered.count(), Some(size as u64));
    }
}

#[test]
fn mail03_metadata_search_schema_describes_the_common_envelope_and_defaults() {
    let schema: Value = serde_json::from_str(include_str!(
        "../../../docs/contracts/tools/mail.messages.search.schema.json"
    ))
    .unwrap();
    assert_eq!(schema["$id"], "mail.messages.search.schema.json");
    assert_eq!(
        schema["required"],
        json!(["schema_version", "request_id", "tool", "params"])
    );
    assert_eq!(schema["additionalProperties"], false);
    let variants = schema["properties"]["params"]["oneOf"].as_array().unwrap();
    let p = variants
        .iter()
        .find(|p| p["properties"]["mode"]["const"] == "imap_metadata")
        .unwrap();
    assert_eq!(p["additionalProperties"], false);
    assert_eq!(
        p["required"],
        json!(["mode", "account_id", "wire_hex", "encoding"])
    );
    assert_eq!(p["properties"]["wire_hex"]["pattern"], "^(?:[0-9a-f]{2})+$");
    assert_eq!(p["properties"]["filter"]["default"], "all");
    assert_eq!(p["properties"]["limit"]["default"], 16);
    assert_eq!(p["properties"]["cursor"]["type"], json!(["string", "null"]));
    assert_eq!(p["properties"]["cursor"]["minLength"], 1);
    let gmail = variants
        .iter()
        .find(|p| p["properties"]["mode"]["const"] == "gmail_messages")
        .unwrap();
    assert_eq!(gmail["additionalProperties"], false);
    assert_eq!(
        gmail["properties"]["account_id"]["pattern"],
        "^google_[0-9a-f]{64}$"
    );
    assert_eq!(gmail["properties"]["limit"]["default"], 32);
    assert!(gmail["properties"].get("uid").is_none());
}
