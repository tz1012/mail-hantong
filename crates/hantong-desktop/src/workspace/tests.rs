use super::*;

#[test]
fn sec03_gui_filter_change_reload_and_late_responses_keep_exact_binding() {
    let mut empty = MailWorkspace::default();
    assert_eq!(empty.filter(), ImapMetadataFilter::All);
    assert!(!empty.set_filter(ImapMetadataFilter::Unread));
    assert!(empty.restart_current_folder().is_none());
    let mut w = selected();
    let all = w.message_action(false).unwrap();
    w.apply(
        &all,
        &MailActionResult::Messages {
            rows: vec![row("a")],
            next_cursor: Some(cursor()),
            observed: true,
        },
    )
    .unwrap();
    w.select_message(0);
    let old_cursor = w.message_action(true).unwrap();
    assert!(w.set_filter(ImapMetadataFilter::Unread));
    assert!(w.messages().is_empty() && w.next_cursor().is_none() && w.selected_message().is_none());
    let expected = w.clone();
    let page = MailActionResult::Messages {
        rows: vec![row("a")],
        next_cursor: None,
        observed: true,
    };
    assert_eq!(w.apply(&all, &page), Err(ErrorCode::Conflict));
    assert_eq!(w.apply(&old_cursor, &page), Err(ErrorCode::Conflict));
    assert_eq!(w, expected);
    let unread = w.message_action(false).unwrap();
    let mut conflicting = row("a");
    conflicting.is_read = true;
    assert_eq!(
        w.apply(
            &unread,
            &MailActionResult::Messages {
                rows: vec![conflicting],
                next_cursor: None,
                observed: true
            }
        ),
        Err(ErrorCode::InvalidInput)
    );
    w.apply(
        &unread,
        &MailActionResult::Messages {
            rows: vec![row("a")],
            next_cursor: Some(cursor()),
            observed: true,
        },
    )
    .unwrap();
    w.select_message(0);
    let stale_next = w.message_action(true).unwrap();
    assert_eq!(w.restart_current_folder(), Some(unread.clone()));
    assert!(w.messages().is_empty() && w.next_cursor().is_none() && w.selected_message().is_none());
    assert_eq!(w.apply(&stale_next, &page), Err(ErrorCode::Conflict));
    assert!(w.set_filter(ImapMetadataFilter::Favorite));
    let favorite = w.message_action(false).unwrap();
    let mut conflicting = row("a");
    conflicting.is_favorite = false;
    assert_eq!(
        w.apply(
            &favorite,
            &MailActionResult::Messages {
                rows: vec![conflicting],
                next_cursor: None,
                observed: true
            }
        ),
        Err(ErrorCode::InvalidInput)
    );
    w.apply(&favorite, &page).unwrap();
    assert!(w.select_account("b"));
    assert_eq!(w.filter(), ImapMetadataFilter::All);
    w = selected();
    w.set_filter(ImapMetadataFilter::Favorite);
    w.clear();
    assert_eq!(w.filter(), ImapMetadataFilter::All);
}
fn account(id: &str) -> MailAccount {
    MailAccount::new(id, id).unwrap()
}
fn folder(account: &str, encoding: &str) -> MailFolder {
    MailFolder::new_with_encoding(account, "Inbox", "494e424f58", encoding).unwrap()
}
fn row(account: &str) -> MailMessage {
    MailMessage::new(
        account,
        &format!("msg_{}", "a".repeat(64)),
        "sender\r\n\u{202e}",
        "title\0",
        "2026-10-30 00:00:00 UTC",
        false,
        true,
    )
    .unwrap()
}
fn cursor() -> PageCursor {
    PageCursor::new(&format!("m1:{}:1:0:1000", "a".repeat(64))).unwrap()
}
fn selected() -> MailWorkspace {
    let mut w = MailWorkspace::default();
    w.apply(
        &MailAction::Accounts,
        &MailActionResult::Accounts(vec![account("a"), account("b")]),
    )
    .unwrap();
    assert_eq!(w.selected_account(), None);
    assert!(w.select_account("a"));
    w.apply(
        &MailAction::Folders {
            account_id: "a".into(),
            cursor: None,
        },
        &MailActionResult::Folders {
            rows: vec![folder("a", "modified_utf7")],
            next_cursor: None,
        },
    )
    .unwrap();
    assert!(w.selected_folder().is_none());
    assert!(w.select_folder(0));
    w
}
#[test]
fn ops07_gui_rows_are_sanitized_bounded_and_selection_is_explicit() {
    let mut w = selected();
    let action = w.message_action(false).unwrap();
    assert!(matches!(&action,MailAction::Messages{encoding,..} if encoding=="modified_utf7"));
    w.apply(
        &action,
        &MailActionResult::Messages {
            rows: vec![row("a")],
            next_cursor: Some(cursor()),
            observed: true,
        },
    )
    .unwrap();
    assert_eq!(w.messages()[0].sender, "sender");
    assert_eq!(w.messages()[0].subject, "title");
    assert!(w.selected_message().is_none());
    assert!(w.select_message(0));
    assert_eq!(w.selected_message(), Some(&w.messages()[0]));
    assert!(w.select_account("b"));
    assert!(w.messages().is_empty());
    assert!(w.next_cursor().is_none());
    assert!(w.selected_folder().is_none());
    w.clear();
    assert!(w.selected_account().is_none());
    assert!(w.folders().is_empty());
    assert!(MailMessage::new("a", "m", "", "", "", false, false).is_none());
    assert!(MailFolder::new_with_encoding("a", "x", "abc", "utf8").is_none());
    assert!(MailFolder::new_with_encoding("a", "x", "ff", "unknown").is_none());
    assert!(PageCursor::new("arbitrary-cursor").is_none());
    assert!(
        MailMessage::new(
            "a",
            &format!("msg_{}", "0".repeat(64)),
            "",
            &"가".repeat(9000),
            "",
            false,
            false
        )
        .unwrap()
        .subject
        .len()
            <= 512
    );
    let long = MailMessage::new(
        "a",
        &format!("msg_{}", "1".repeat(64)),
        &"x\u{202e}\n".repeat(1000),
        &"가\u{2066}\0".repeat(1000),
        "",
        false,
        false,
    )
    .unwrap();
    assert!(long.sender.ends_with('…') && long.subject.ends_with('…'));
    assert!(long.sender.len() <= 512 && long.subject.len() <= 512);
    assert!(!long.sender.contains(['\n', '\u{202e}']));
    assert!(!long.subject.contains(['\0', '\u{2066}']));
    let mut display = selected();
    let request = display.message_action(false).unwrap();
    display
        .apply(
            &request,
            &MailActionResult::Messages {
                rows: vec![long],
                next_cursor: None,
                observed: true,
            },
        )
        .unwrap();
    assert_eq!(display.messages().len(), 1);
}
#[test]
fn sec03_gui_workspace_rejects_stale_scope_encoding_cursor_and_oversized_pages_atomically() {
    let mut w = selected();
    let action = w.message_action(false).unwrap();
    let before = w.clone();
    assert_eq!(
        w.apply(
            &action,
            &MailActionResult::Messages {
                rows: vec![row("b")],
                next_cursor: None,
                observed: true
            }
        ),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(w, before);
    let MailAction::Messages {
        account_id,
        folder_wire_hex,
        ..
    } = action.clone()
    else {
        panic!()
    };
    let wrong = MailAction::Messages {
        account_id,
        folder_wire_hex,
        encoding: "utf8".into(),
        filter: ImapMetadataFilter::All,
        cursor: None,
    };
    assert_eq!(
        w.apply(
            &wrong,
            &MailActionResult::Messages {
                rows: vec![],
                next_cursor: None,
                observed: true
            }
        ),
        Err(ErrorCode::Conflict)
    );
    assert_eq!(
        w.apply(
            &action,
            &MailActionResult::Messages {
                rows: vec![row("a"); 33],
                next_cursor: None,
                observed: true
            }
        ),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(w, before);
    w.apply(
        &action,
        &MailActionResult::Messages {
            rows: vec![],
            next_cursor: Some(cursor()),
            observed: true,
        },
    )
    .unwrap();
    assert!(w.message_action(true).is_some());
    assert_eq!(
        w.apply(
            &action,
            &MailActionResult::Messages {
                rows: vec![],
                next_cursor: None,
                observed: true
            }
        ),
        Err(ErrorCode::Conflict)
    );
    w.select_account("b");
    assert_eq!(
        w.apply(
            &MailAction::Folders {
                account_id: "a".into(),
                cursor: None
            },
            &MailActionResult::Folders {
                rows: vec![folder("a", "utf8")],
                next_cursor: None
            }
        ),
        Err(ErrorCode::Conflict)
    );
}
#[test]
fn ops07_gui_unobserved_is_distinct_and_next_folder_does_not_select_first_row() {
    let mut w = selected();
    let action = w.message_action(false).unwrap();
    assert_eq!(
        w.apply(
            &action,
            &MailActionResult::Messages {
                rows: vec![row("a")],
                next_cursor: None,
                observed: false
            }
        ),
        Err(ErrorCode::InvalidInput)
    );
    w.apply(
        &action,
        &MailActionResult::Messages {
            rows: vec![],
            next_cursor: None,
            observed: false,
        },
    )
    .unwrap();
    assert!(w.messages().is_empty());
    assert!(w.selected_message().is_none());
    w.select_account("a");
    let next = FolderCursor::new("a", 1, "494e424f58").unwrap().encode();
    w.apply(
        &MailAction::Folders {
            account_id: "a".into(),
            cursor: None,
        },
        &MailActionResult::Folders {
            rows: vec![],
            next_cursor: Some(next.clone()),
        },
    )
    .unwrap();
    w.apply(
        &MailAction::Folders {
            account_id: "a".into(),
            cursor: Some(next),
        },
        &MailActionResult::Folders {
            rows: vec![folder("a", "utf8")],
            next_cursor: None,
        },
    )
    .unwrap();
    assert!(w.selected_folder().is_none());
}
