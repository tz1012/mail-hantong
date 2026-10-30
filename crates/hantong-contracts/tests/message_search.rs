use hantong_contracts::{
    ErrorCode,
    message_search::{
        HeaderAddressProjection, HeaderProjection, ImapMetadataFilter, ImapMetadataSearchCursor,
        ImapMetadataSearchItem, ImapMetadataSearchPage, ImapMetadataSearchRequest,
    },
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};

fn mailbox() -> UidMailboxIdentity {
    UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::ModifiedUtf7).unwrap()
}

fn cursor(resume: u32) -> ImapMetadataSearchCursor {
    ImapMetadataSearchCursor::parse(&format!("m1:{}:7:{resume}:1767225900000", "01".repeat(32)))
        .unwrap()
}

fn item(id: u8, uid: u32, index: u32) -> ImapMetadataSearchItem {
    ImapMetadataSearchItem::new(
        &format!("msg_{}", format!("{id:02x}").repeat(32)),
        "acct",
        uid,
        1_767_225_600_000,
        123,
        false,
        true,
        index,
    )
    .unwrap()
}

#[test]
fn mail03_search_request_defaults_and_cursor_wire_are_canonical() {
    let request = ImapMetadataSearchRequest::new("acct", mailbox(), None, None, None).unwrap();
    assert_eq!(request.account_id(), "acct");
    assert_eq!(request.account_ids(), &["acct"]);
    assert_eq!(request.filter(), ImapMetadataFilter::All);
    assert_eq!(request.limit(), 16);
    assert!(request.cursor().is_none());

    for (text, expected) in [
        ("all", ImapMetadataFilter::All),
        ("unread", ImapMetadataFilter::Unread),
        ("favorite", ImapMetadataFilter::Favorite),
    ] {
        assert_eq!(ImapMetadataFilter::parse(text), Ok(expected));
        assert_eq!(expected.as_str(), text);
    }
    assert_eq!(
        ImapMetadataFilter::parse("ALL"),
        Err(ErrorCode::InvalidInput)
    );

    let value = cursor(0);
    assert_eq!(value.source_cursor_version(), 7);
    assert_eq!(value.resume_uid(), 0);
    assert_eq!(value.expires_at_utc_ms(), 1_767_225_900_000);
    assert_eq!(value.digest(), &[1; 32]);
    assert_eq!(ImapMetadataSearchCursor::parse(&value.encode()), Ok(value));
    assert!(!format!("{:?}", cursor(9)).contains("1767225900000"));
}

#[test]
fn mail03_search_request_rejects_noncanonical_bounds() {
    for account in ["", "bad account", "a/b", &"a".repeat(129)] {
        assert!(ImapMetadataSearchRequest::new(account, mailbox(), None, None, None).is_err());
    }
    for limit in [0, 33, u32::MAX] {
        assert!(
            ImapMetadataSearchRequest::new("acct", mailbox(), None, Some(limit), None).is_err()
        );
    }
    for value in [
        "",
        "m1:00:1:0:1",
        &format!("m1:{}:0:0:1", "00".repeat(32)),
        &format!("m1:{}:01:0:1", "00".repeat(32)),
        &format!("m1:{}:1:00:1", "00".repeat(32)),
        &format!("m1:{}:1:0:0", "00".repeat(32)),
        &format!("m1:{}:1:0:01", "00".repeat(32)),
        &format!("m1:{}:1:0:1:extra", "00".repeat(32)),
    ] {
        assert!(ImapMetadataSearchCursor::parse(value).is_err(), "{value}");
    }
}

#[test]
fn mail03_header_projection_preserves_nil_empty_and_fixed_json() {
    let address =
        HeaderAddressProjection::new(None, Some(Vec::new()), Some(vec![0xea, 0xb0, 0x80]), None)
            .unwrap();
    let projection = HeaderProjection::new(Some(b"subject".to_vec()), Some(vec![address])).unwrap();
    assert_eq!(
        projection.projection_json(),
        r#"{"from":[{"host_hex":null,"mailbox_hex":"","name_hex":"eab080","route_hex":null}],"subject_hex":"7375626a656374"}"#
    );
    assert_eq!(
        HeaderProjection::new(None, None).unwrap().projection_json(),
        r#"{"from":null,"subject_hex":null}"#
    );
    assert!(HeaderProjection::new(None, Some(Vec::new())).is_err());
    assert!(HeaderAddressProjection::new(Some(vec![0; 8193]), None, None, None).is_err());
    assert!(HeaderProjection::new(Some(vec![0; 65_537]), None).is_err());
    assert!(!format!("{projection:?}").contains("7375626a656374"));
}

#[test]
fn mail03_page_binds_items_headers_accounts_and_observation() {
    let page = ImapMetadataSearchPage::new(
        "acct",
        mailbox(),
        2,
        false,
        Some(1_767_225_600_000),
        vec![item(1, 10, 0)],
        vec![HeaderProjection::new(None, None).unwrap()],
        Some(cursor(10)),
    )
    .unwrap();
    assert_eq!(page.account_id(), "acct");
    assert_eq!(page.catalog_generation(), 2);
    assert!(!page.binding_current());
    assert_eq!(page.items()[0].uid(), 10);
    assert_eq!(page.headers().len(), 1);
    assert!(page.next_cursor().is_some());

    assert!(
        ImapMetadataSearchPage::new(
            "acct",
            mailbox(),
            2,
            true,
            None,
            vec![item(1, 10, 0)],
            vec![HeaderProjection::new(None, None).unwrap()],
            None,
        )
        .is_err()
    );
    assert!(
        ImapMetadataSearchPage::new(
            "acct",
            mailbox(),
            2,
            true,
            Some(1_767_225_600_000),
            vec![item(1, 10, 1)],
            vec![HeaderProjection::new(None, None).unwrap()],
            None,
        )
        .is_err()
    );
}

#[test]
fn mail03_item_id_date_and_index_are_strict_and_redacted() {
    let value = item(0xab, 1, 0);
    assert_eq!(value.internal_message_id().len(), 68);
    assert_eq!(value.account_id(), "acct");
    assert_eq!(value.internal_date_utc_ms(), 1_767_225_600_000);
    assert_eq!(value.size_bytes(), 123);
    assert!(!value.is_read());
    assert!(value.is_favorite());
    assert_eq!(value.header_content_index(), 0);
    assert!(!format!("{value:?}").contains("msg_"));

    for id in [
        "msg_00",
        &format!("MSG_{}", "00".repeat(32)),
        &format!("msg_{}", "AA".repeat(32)),
    ] {
        assert!(ImapMetadataSearchItem::new(id, "acct", 1, 0, 0, false, false, 0).is_err());
    }
    assert!(
        ImapMetadataSearchItem::new(
            &format!("msg_{}", "00".repeat(32)),
            "acct",
            0,
            0,
            0,
            false,
            false,
            0,
        )
        .is_err()
    );
}
