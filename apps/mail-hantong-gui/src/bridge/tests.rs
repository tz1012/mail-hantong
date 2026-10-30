use super::*;
use hantong_contracts::message_search::ImapMetadataFilter;
use hantong_contracts::registration::RegistrationProgress;
#[test]
fn mail01_ops07_subject_display_decodes_then_sanitizes_without_changing_address_or_identity() {
    use hantong_contracts::message_search::HeaderAddressProjection;
    let id = format!("msg_{}", "a".repeat(64));
    let item = ImapMetadataSearchItem::new(&id, "a", 3, 0, 100, false, true, 0).unwrap();
    let header = HeaderProjection::new(
        Some(b"=?UTF-8?Q?=ED=95=9C=ED=86=B5=00=E2=80=AE?=".to_vec()),
        Some(vec![
            HeaderAddressProjection::new(
                Some(b"example.invalid".to_vec()),
                Some(b"=?UTF-8?Q?admin?=".to_vec()),
                Some(b"untrusted display name".to_vec()),
                None,
            )
            .unwrap(),
        ]),
    )
    .unwrap();
    let original = header.clone();
    let row = display_message("a", &item, &header).unwrap();
    assert_eq!(row.subject, "한통");
    assert_eq!(row.sender, "=?UTF-8?Q?admin?=@example.invalid");
    assert_eq!(row.internal_id, id);
    assert_eq!(row.account_id, "a");
    assert_eq!(header, original);
    assert_eq!(
        display_message("other", &item, &header),
        Err(ErrorCode::InvalidInput)
    );
    let long = HeaderProjection::new(
        Some("=?UTF-8?Q?=EA=B0=80?= ".repeat(300).into_bytes()),
        None,
    )
    .unwrap();
    let row = display_message("a", &item, &long).unwrap();
    assert!(row.subject.len() <= 512 && row.subject.ends_with('…'));
}
#[derive(Default)]
struct Fake {
    calls: Vec<&'static str>,
    encoding: Option<String>,
    requests: Vec<(ImapMetadataFilter, Option<String>)>,
    response: Option<MailActionResult>,
}
impl Backend for Fake {
    fn registration(
        &mut self,
        command: RegistrationCommand,
    ) -> Result<RegistrationReceipt, ErrorCode> {
        self.calls.push(match command {
            RegistrationCommand::GuiRequest => "request",
            RegistrationCommand::GuiStatus(id) if id == [9; 32] => "status",
            _ => panic!("wrong registration command"),
        });
        RegistrationReceipt::new([9; 32], 1000, RegistrationProgress::Ready)
    }
    fn accounts(&mut self) -> Result<MailActionResult, ErrorCode> {
        self.calls.push("accounts");
        Ok(MailActionResult::Accounts(vec![
            MailAccount::new("a", "A").unwrap(),
        ]))
    }
    fn folders(&mut self, request: FolderListRequest) -> Result<MailActionResult, ErrorCode> {
        self.calls.push("folders");
        assert_eq!(request.account_id(), "a");
        Ok(MailActionResult::Folders {
            rows: vec![
                MailFolder::new_with_encoding("a", "Inbox", "494e424f58", "modified_utf7").unwrap(),
            ],
            next_cursor: None,
        })
    }
    fn messages(
        &mut self,
        request: ImapMetadataSearchRequest,
    ) -> Result<MailActionResult, ErrorCode> {
        self.calls.push("messages");
        self.encoding = Some(request.mailbox().encoding().as_str().into());
        self.requests.push((
            request.filter(),
            request.cursor().map(ImapMetadataSearchCursor::encode),
        ));
        if let Some(response) = self.response.take() {
            return Ok(response);
        }
        Ok(MailActionResult::Messages {
            rows: vec![],
            next_cursor: None,
            observed: false,
        })
    }
}
fn bridge() -> GuiBridge<Fake> {
    GuiBridge {
        request_id: None,
        workspace: MailWorkspace::default(),
        backend: Fake::default(),
    }
}
#[test]
fn sec03_gui_bridge_read_does_not_register_and_preserves_exact_folder_encoding() {
    let mut b = bridge();
    assert_eq!(
        b.action(MailAction::RegistrationStatus),
        Err(ErrorCode::NotFound)
    );
    assert!(b.backend.calls.is_empty());
    b.action(MailAction::Accounts).unwrap();
    assert!(b.workspace.selected_account().is_none());
    b.action(MailAction::Folders {
        account_id: "a".into(),
        cursor: None,
    })
    .unwrap();
    assert!(b.workspace.selected_folder().is_none());
    let message = MailAction::Messages {
        account_id: "a".into(),
        folder_wire_hex: "494e424f58".into(),
        encoding: "modified_utf7".into(),
        filter: ImapMetadataFilter::All,
        cursor: None,
    };
    b.action(message.clone()).unwrap();
    assert_eq!(b.backend.encoding.as_deref(), Some("modified_utf7"));
    assert_eq!(b.backend.calls, vec!["accounts", "folders", "messages"]);
    let wrong = MailAction::Messages {
        account_id: "a".into(),
        folder_wire_hex: "494e424f58".into(),
        encoding: "utf8".into(),
        filter: ImapMetadataFilter::All,
        cursor: None,
    };
    assert_eq!(b.action(wrong), Err(ErrorCode::ScopeDenied));
    assert_eq!(b.backend.calls.len(), 3);
    assert_eq!(
        b.action(MailAction::Folders {
            account_id: "other".into(),
            cursor: None
        }),
        Err(ErrorCode::ScopeDenied)
    );
}
#[test]
fn sec03_gui_bridge_registration_buttons_are_separate_and_never_load_accounts_implicitly() {
    let mut b = bridge();
    b.action(MailAction::Connect).unwrap();
    b.action(MailAction::RegistrationStatus).unwrap();
    assert_eq!(b.backend.calls, vec!["request", "status"]);
    assert!(b.workspace.accounts().is_empty());
}
#[test]
fn ops07_gui_metadata_utc_dates_cover_epoch_leap_day_and_negative_time() {
    assert_eq!(utc_date(0), "1970-01-01 00:00:00 UTC");
    assert_eq!(utc_date(-1000), "1969-12-31 23:59:59 UTC");
    assert_eq!(utc_date(951782400000), "2000-02-29 00:00:00 UTC");
}

#[test]
fn sec03_gui_bridge_passes_filter_and_rejects_old_filter_cursor_before_ipc() {
    let mut b = bridge();
    b.action(MailAction::Accounts).unwrap();
    b.action(MailAction::Folders {
        account_id: "a".into(),
        cursor: None,
    })
    .unwrap();
    let first = |filter| MailAction::Messages {
        account_id: "a".into(),
        folder_wire_hex: "494e424f58".into(),
        encoding: "modified_utf7".into(),
        filter,
        cursor: None,
    };
    let token =
        |digit: &str| PageCursor::new(&format!("m1:{}:1:0:1000", digit.repeat(64))).unwrap();
    b.backend.response = Some(MailActionResult::Messages {
        rows: vec![],
        next_cursor: Some(token("a")),
        observed: true,
    });
    b.action(first(ImapMetadataFilter::All)).unwrap();
    let old_all = b.workspace.message_action(true).unwrap();
    b.backend.response = Some(MailActionResult::Messages {
        rows: vec![],
        next_cursor: Some(token("b")),
        observed: true,
    });
    b.action(first(ImapMetadataFilter::Unread)).unwrap();
    assert_eq!(
        b.backend.requests,
        vec![
            (ImapMetadataFilter::All, None),
            (ImapMetadataFilter::Unread, None)
        ]
    );
    assert_eq!(b.action(old_all), Err(ErrorCode::Conflict));
    let mut old_cursor = b.workspace.message_action(true).unwrap();
    if let MailAction::Messages { cursor, .. } = &mut old_cursor {
        *cursor = Some(token("a").as_str().into());
    }
    assert_eq!(b.action(old_cursor), Err(ErrorCode::Conflict));
    assert_eq!(b.backend.requests.len(), 2);
    let current = b.workspace.message_action(true).unwrap();
    b.action(current).unwrap();
    assert_eq!(
        b.backend.requests[2],
        (ImapMetadataFilter::Unread, Some(token("b").as_str().into()))
    );
    b.action(first(ImapMetadataFilter::Favorite)).unwrap();
    b.action(first(ImapMetadataFilter::Favorite)).unwrap(); // explicit folder reload keeps filter and starts at None
    assert_eq!(
        &b.backend.requests[3..],
        &[
            (ImapMetadataFilter::Favorite, None),
            (ImapMetadataFilter::Favorite, None)
        ]
    );
    b.backend.response = Some(MailActionResult::Messages {
        rows: vec![
            MailMessage::new(
                "a",
                &format!("msg_{}", "a".repeat(64)),
                "sender",
                "subject",
                "date",
                false,
                false,
            )
            .unwrap(),
        ],
        next_cursor: None,
        observed: true,
    });
    assert_eq!(
        b.action(first(ImapMetadataFilter::Favorite)),
        Err(ErrorCode::InvalidInput)
    );
    assert!(b.workspace.messages().is_empty());
}
