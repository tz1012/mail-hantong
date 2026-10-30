use super::*;
use hantong_contracts::registration::RegistrationProgress;
#[derive(Default)]
struct Fake {
    calls: Vec<&'static str>,
    encoding: Option<String>,
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
        cursor: None,
    };
    b.action(message.clone()).unwrap();
    assert_eq!(b.backend.encoding.as_deref(), Some("modified_utf7"));
    assert_eq!(b.backend.calls, vec!["accounts", "folders", "messages"]);
    let wrong = MailAction::Messages {
        account_id: "a".into(),
        folder_wire_hex: "494e424f58".into(),
        encoding: "utf8".into(),
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
