//! Development-only visual fixture. No IPC, accounts, credentials or network.
//! Run explicitly with `cargo run -p hantong-desktop --example fixture_preview`.
#![forbid(unsafe_code)]
#![cfg_attr(windows, windows_subsystem = "windows")]

use hantong_contracts::{ErrorCode, message_search::ImapMetadataFilter};
use hantong_desktop::{
    CoreStatus, MailAccount, MailAction, MailActionResult, MailFolder, MailMessage, ShellCallbacks,
    ShellOptions,
};
use std::sync::Arc;

fn respond(action: MailAction) -> Result<MailActionResult, ErrorCode> {
    match action {
        MailAction::Accounts => Ok(MailActionResult::Accounts(vec![
            MailAccount::new("fixture", "[Sample / 예시] Preview").expect("fixture account"),
        ])),
        MailAction::Folders { account_id, .. } if account_id == "fixture" => {
            Ok(MailActionResult::Folders {
                rows: vec![
                    MailFolder::new_with_encoding(
                        "fixture",
                        "Inbox / 받은편지함",
                        "494e424f58",
                        "utf8",
                    )
                    .expect("fixture folder"),
                ],
                next_cursor: None,
            })
        }
        MailAction::Messages {
            account_id,
            folder_wire_hex,
            encoding,
            filter,
            cursor: None,
        } if account_id == "fixture" && folder_wire_hex == "494e424f58" && encoding == "utf8" => {
            let subjects = [
                ("Design studio", "[Sample] A quieter space for your mail"),
                ("한통 디자인", "[예시] 이번 주 디자인 검토와 다음 작업"),
                (
                    "Project notebook",
                    "[Sample] Notes from our planning session",
                ),
                ("제품 연구팀", "[예시] 읽기 화면과 키보드 탐색 개선안"),
                (
                    "Weekly digest",
                    "[Sample] Small ideas, thoughtfully collected",
                ),
                ("개발 노트", "[예시] 라이트·다크 화면과 한글 제목 확인"),
                ("Workshop", "[Sample] Your place at the next workshop"),
                (
                    "한통 미리보기",
                    "[예시] 실제 메일이 아닌 화면 검증용 데이터입니다",
                ),
            ];
            let rows = (0..32)
                .filter_map(|index| {
                    let read = index % 3 != 0;
                    let favorite = index % 5 == 0;
                    if (filter == ImapMetadataFilter::Unread && read)
                        || (filter == ImapMetadataFilter::Favorite && !favorite)
                    {
                        return None;
                    }
                    let (sender, subject) = subjects[index % subjects.len()];
                    Some(
                        MailMessage::new(
                            "fixture",
                            &format!("msg_{index:064x}"),
                            sender,
                            subject,
                            "2026-10-30 09:00 UTC",
                            read,
                            favorite,
                        )
                        .expect("fixture message"),
                    )
                })
                .collect();
            Ok(MailActionResult::Messages {
                rows,
                next_cursor: None,
                observed: true,
            })
        }
        // The preview cannot register, authenticate, send, or access the core.
        _ => Err(ErrorCode::CapabilityUnsupported),
    }
}

fn main() {
    let result = hantong_desktop::run_with_callbacks(
        ShellOptions::default(),
        Arc::new(|| CoreStatus::Available { mail_ready: true }),
        ShellCallbacks {
            action: Some(Arc::new(respond)),
        },
    );
    if result.is_err() {
        std::process::exit(1);
    }
}
