//! Plain, keyboard-first mail reader used by the command CLI.
//!
//! This is intentionally separate from the communications-style full-screen
//! TUI. It keeps the selected folder/message in a simple text loop and uses
//! the existing typed authenticated IPC clients directly.

use crate::Lang;
use hantong_contracts::{
    folders::FolderListRequest,
    message_search::{ImapMetadataFilter, ImapMetadataSearchRequest},
    uid_page::{UidMailboxEncoding, UidMailboxIdentity},
};
use hantong_ipc_client::{folders, google_mail, message_body, message_search};
use hantong_platform::{command_prompt, mail_display};
use serde_json::{Value, json};
use std::io::{self, Write};

const MAX_FOLDERS: usize = 256;
const MAX_MESSAGES: usize = 128;
const MAX_BODY_BYTES: usize = 65_536;
const MENU_PAGE: usize = 12;

pub(super) fn run_with<F>(
    account: &str,
    provider: &str,
    mut lang: Lang,
    mut callback: F,
) -> Result<Option<String>, hantong_contracts::ErrorCode>
where
    F: FnMut(&str, &mut Lang) -> Result<(), hantong_contracts::ErrorCode>,
{
    if provider == "gmail" {
        crate::google::refresh_guided_inbox(account)?;
    }
    let folders = if provider == "gmail" {
        gmail_folders(account)?
    } else {
        imap_folders(account)?
    };
    if folders.is_empty() {
        return Err(hantong_contracts::ErrorCode::NotFound);
    }
    let accounts = hantong_ipc_client::accounts::list_default_accounts(false)?;
    let account_label = accounts
        .items()
        .iter()
        .find(|row| row.account_id() == account)
        .and_then(|row| row.display_name().or(row.primary_address()))
        .map(mail_display::single_line)
        .unwrap_or_else(|| resource(lang, "account").into());
    let mut folder_index = 0;
    loop {
        let ko = matches!(lang, Lang::Ko);
        print_folder_menu(&account_label, provider, &folders, folder_index, ko);
        let choices = folders
            .iter()
            .enumerate()
            .map(|(index, folder)| {
                (
                    (index + 1).to_string(),
                    mail_display::single_line(&folder.1),
                )
            })
            .collect::<Vec<_>>();
        let Some(folder_value) = command_prompt::read_command_line_with_selection(
            if ko { "폴더 > " } else { "Folder > " },
            &[
                "/back".into(),
                "/help".into(),
                "/ai codex".into(),
                "/ai claude".into(),
                "/language".into(),
            ],
            &choices,
            folder_index,
        )?
        else {
            return Ok(None);
        };
        if folder_value.starts_with('/') {
            if folder_value == "/help" {
                print_help(ko);
                continue;
            }
            if folder_value == "/back" {
                return Ok(Some(folder_value));
            }
            callback(&folder_value, &mut lang)?;
            continue;
        }
        let Ok(index) = parse_choice(&folder_value, folders.len()) else {
            println!("{}", resource(lang, "invalid_choice"));
            continue;
        };
        folder_index = index;
        let folder = &folders[folder_index];
        let messages = if provider == "gmail" {
            gmail_messages(account, &folder.0)?
        } else {
            imap_messages(account, &folder.0, &folder.2)?
        };
        if messages.is_empty() {
            println!(
                "{}",
                if ko {
                    "메일이 없습니다."
                } else {
                    "No messages."
                }
            );
            continue;
        }
        let mut message_index = 0;
        loop {
            let ko = matches!(lang, Lang::Ko);
            print_message_menu(
                &account_label,
                provider,
                &folder.1,
                &messages,
                message_index,
                ko,
            );
            let choices = messages
                .iter()
                .enumerate()
                .map(|(index, message)| {
                    (
                        (index + 1).to_string(),
                        mail_display::single_line(&message.label),
                    )
                })
                .collect::<Vec<_>>();
            let Some(message_value) = command_prompt::read_command_line_with_selection(
                if ko { "메일 > " } else { "Message > " },
                &[
                    "/back".into(),
                    "/help".into(),
                    "/ai codex".into(),
                    "/ai claude".into(),
                    "/language".into(),
                ],
                &choices,
                message_index,
            )?
            else {
                return Ok(None);
            };
            if message_value.starts_with('/') {
                if message_value == "/help" {
                    print_help(ko);
                    continue;
                }
                if message_value == "/back" {
                    break;
                }
                callback(&message_value, &mut lang)?;
                continue;
            }
            let Ok(index) = parse_choice(&message_value, messages.len()) else {
                println!("{}", resource(lang, "invalid_choice"));
                continue;
            };
            message_index = index;
            println!("{}", resource(lang, "loading"));
            let message = &messages[message_index];
            let response = if provider == "gmail" {
                google_mail::call_default(
                    "mail.messages.get",
                    json!({
                        "mode": "gmail_body",
                        "account_id": account,
                        "message_id": message.id,
                        "max_bytes": MAX_BODY_BYTES,
                    }),
                )?
            } else {
                read_imap_body(account, message)?
            };
            if !read_pages(&response, &message.fallback, &mut lang, &mut callback)? {
                return Ok(None);
            }
        }
    }
}

fn resource(lang: Lang, key: &str) -> &'static str {
    let source = match lang {
        Lang::En => include_str!("../resources/reading-en.txt"),
        Lang::Ko => include_str!("../resources/reading-ko.txt"),
    };
    source
        .lines()
        .find_map(|line| {
            line.split_once('=')
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v)
        })
        .expect("reading resource key")
}

const BODY_PAGE_LINES: usize = 18;
fn body_lines(body: &str) -> Vec<String> {
    body.lines()
        .flat_map(|line| {
            let clean = mail_display::single_line(line);
            let chars: Vec<char> = clean.chars().collect();
            if chars.is_empty() {
                vec![String::new()]
            } else {
                chars
                    .chunks(72)
                    .map(|chunk| chunk.iter().collect())
                    .collect()
            }
        })
        .collect()
}
fn read_pages<F>(
    response: &Value,
    fallback: &Value,
    lang: &mut Lang,
    callback: &mut F,
) -> Result<bool, hantong_contracts::ErrorCode>
where
    F: FnMut(&str, &mut Lang) -> Result<(), hantong_contracts::ErrorCode>,
{
    let mut page = 0usize;
    loop {
        let rendered = mail_display::reading_text(response, fallback, *lang == Lang::Ko)
            .ok_or(hantong_contracts::ErrorCode::NotFound)?;
        let (headers, body) = rendered.split_once("\n\n").unwrap_or((&rendered, ""));
        let lines = body_lines(body);
        let pages = lines.len().div_ceil(BODY_PAGE_LINES).max(1);
        page = page.min(pages - 1);
        println!("\n{headers}\n");
        for line in lines
            .iter()
            .skip(page * BODY_PAGE_LINES)
            .take(BODY_PAGE_LINES)
        {
            println!("{line}");
        }
        println!(
            "\n{}/{} - {}",
            page + 1,
            pages,
            resource(*lang, "reading_keys")
        );
        io::stdout()
            .flush()
            .map_err(|_| hantong_contracts::ErrorCode::CoreUnavailable)?;
        let commands = [
            "/back",
            "/next",
            "/prev",
            "/help",
            "/ai codex",
            "/ai claude",
            "/language",
        ]
        .map(String::from);
        let choices = vec![
            ("/next".into(), resource(*lang, "next_page").into()),
            ("/prev".into(), resource(*lang, "prev_page").into()),
            ("/back".into(), resource(*lang, "return_list").into()),
        ];
        let Some(command) = command_prompt::read_command_line_with_options(
            resource(*lang, "mail_prompt"),
            &commands,
            &choices,
        )?
        else {
            return Ok(false);
        };
        match command.as_str() {
            "/back" | "0" => return Ok(true),
            "/next" => page = (page + 1).min(pages - 1),
            "/prev" => page = page.saturating_sub(1),
            "/help" => print_help(*lang == Lang::Ko),
            command if command.starts_with('/') => callback(command, lang)?,
            _ => println!("{}", resource(*lang, "invalid_choice")),
        }
    }
}

fn parse_choice(value: &str, count: usize) -> Result<usize, hantong_contracts::ErrorCode> {
    let number = value
        .parse::<usize>()
        .map_err(|_| hantong_contracts::ErrorCode::InvalidInput)?;
    if number == 0 || number > count {
        return Err(hantong_contracts::ErrorCode::InvalidInput);
    }
    Ok(number - 1)
}

fn page_bounds(selected: usize, total: usize) -> (usize, usize) {
    let start = selected
        .saturating_sub(MENU_PAGE / 2)
        .min(total.saturating_sub(MENU_PAGE));
    (start, (start + MENU_PAGE).min(total))
}

fn print_folder_menu(
    account: &str,
    provider: &str,
    rows: &[(String, String, String)],
    selected: usize,
    ko: bool,
) {
    let (start, end) = page_bounds(selected, rows.len());
    println!(
        "\n{} / {} · {}",
        account,
        provider,
        if ko { "폴더" } else { "Folders" }
    );
    for (index, row) in rows.iter().enumerate().skip(start).take(end - start) {
        println!(
            "{} {:>3}. {}",
            if index == selected { ">" } else { " " },
            index + 1,
            mail_display::single_line(&row.1)
        );
    }
    println!(
        "{}-{}/{} · {}",
        start + 1,
        end,
        rows.len(),
        if ko {
            "번호 또는 ↑↓"
        } else {
            "number or ↑↓"
        }
    );
}

fn print_message_menu(
    account: &str,
    provider: &str,
    folder: &str,
    rows: &[MailRow],
    selected: usize,
    ko: bool,
) {
    let (start, end) = page_bounds(selected, rows.len());
    println!(
        "\n{} / {} / {}",
        account,
        provider,
        mail_display::single_line(folder)
    );
    for (index, row) in rows.iter().enumerate().skip(start).take(end - start) {
        println!(
            "{} {:>3}. {}",
            if index == selected { ">" } else { " " },
            index + 1,
            mail_display::single_line(&row.label)
        );
    }
    println!(
        "{}-{}/{} · {}",
        start + 1,
        end,
        rows.len(),
        if ko {
            "번호 또는 ↑↓"
        } else {
            "number or ↑↓"
        }
    );
}

fn print_help(ko: bool) {
    println!(
        "{}",
        if ko {
            "/back 뒤로 · /help 도움말 · /ai codex · /ai claude · /language ko|en"
        } else {
            "/back back · /help help · /ai codex · /ai claude · /language ko|en"
        }
    );
}

#[derive(Clone)]
struct MailRow {
    id: String,
    label: String,
    fallback: Value,
}

fn gmail_folders(
    account: &str,
) -> Result<Vec<(String, String, String)>, hantong_contracts::ErrorCode> {
    let value = google_mail::call_default(
        "mail.folders.list",
        json!({"mode":"gmail_labels","account_id":account}),
    )?;
    let labels = value["labels"]
        .as_array()
        .ok_or(hantong_contracts::ErrorCode::InvalidInput)?;
    let mut rows = labels
        .iter()
        .filter_map(|label| {
            Some((
                label["id"].as_str()?.to_owned(),
                label["name"].as_str()?.to_owned(),
                "gmail".into(),
            ))
        })
        .collect::<Vec<_>>();
    inbox_first(&mut rows);
    Ok(rows)
}

fn imap_folders(
    account: &str,
) -> Result<Vec<(String, String, String)>, hantong_contracts::ErrorCode> {
    let mut cursor = None;
    let mut rows = Vec::new();
    while rows.len() < MAX_FOLDERS {
        let page =
            folders::list_default_folders(FolderListRequest::new(account, Some(32), cursor)?)?;
        for item in page.items() {
            rows.push((
                item.wire_hex().to_owned(),
                item.display_name().to_owned(),
                item.encoding().to_owned(),
            ));
            if rows.len() == MAX_FOLDERS {
                break;
            }
        }
        cursor = page
            .next_cursor()
            .map(hantong_contracts::folders::FolderCursor::parse)
            .transpose()?;
        if cursor.is_none() {
            break;
        }
    }
    inbox_first(&mut rows);
    Ok(rows)
}

fn inbox_first(rows: &mut [(String, String, String)]) {
    if let Some(index) = rows
        .iter()
        .position(|(_, label, _)| label.eq_ignore_ascii_case("inbox"))
    {
        rows.swap(0, index);
    }
}

fn gmail_messages(
    account: &str,
    label: &str,
) -> Result<Vec<MailRow>, hantong_contracts::ErrorCode> {
    let value = google_mail::call_default(
        "mail.messages.search",
        json!({"mode":"gmail_messages","account_id":account,"label_id":label,"query":null,"limit":32,"after":null}),
    )?;
    let rows = value["messages"]
        .as_array()
        .ok_or(hantong_contracts::ErrorCode::InvalidInput)?;
    Ok(rows
        .iter()
        .take(MAX_MESSAGES)
        .filter_map(|row| {
            let id = row["id"].as_str()?.to_owned();
            let subject = row["subject"].as_str().unwrap_or("(no subject)").to_owned();
            Some(MailRow {
                id,
                label: subject,
                fallback: row.clone(),
            })
        })
        .collect())
}

fn imap_messages(
    account: &str,
    wire: &str,
    encoding: &str,
) -> Result<Vec<MailRow>, hantong_contracts::ErrorCode> {
    let identity = UidMailboxIdentity::new(wire, UidMailboxEncoding::parse(encoding)?)?;
    let request = ImapMetadataSearchRequest::new(
        account,
        identity.clone(),
        Some(ImapMetadataFilter::All),
        Some(32),
        None,
    )?;
    let page = message_search::search_default(request)?;
    Ok(page
        .items()
        .iter()
        .take(MAX_MESSAGES)
        .filter_map(|item| {
            let header = page.headers().get(item.header_content_index() as usize)?;
            let subject = header
                .subject()
                .map(mail_display::decode_header)
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| "(no subject)".into());
            Some(MailRow {
                id: item.internal_message_id().into(),
                label: subject.clone(),
                fallback: json!({"subject":subject}),
            })
        })
        .collect())
}

fn read_imap_body(account: &str, message: &MailRow) -> Result<Value, hantong_contracts::ErrorCode> {
    let request = message_body::locate_default(account, &message.id, MAX_BODY_BYTES)?;
    let mut response = message_body::get_default(&request)?;
    for _ in 0..30 {
        if response.state == "ready" {
            let mut value = json!({"text": response.text, "truncated": response.truncated});
            if let Some(headers) = response.headers {
                value["headers"] = headers.to_json();
            }
            return Ok(value);
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
        response = message_body::get_default(&request)?;
    }
    Err(hantong_contracts::ErrorCode::Offline)
}

#[cfg(test)]
mod tests {
    use super::{page_bounds, parse_choice};
    use hantong_contracts::ErrorCode;

    #[test]
    fn cli_reader_numeric_selection_is_strict_and_one_based() {
        assert_eq!(parse_choice("1", 15), Ok(0));
        assert_eq!(parse_choice("15", 15), Ok(14));
        assert_eq!(parse_choice("0", 15), Err(ErrorCode::InvalidInput));
        assert_eq!(parse_choice("16", 15), Err(ErrorCode::InvalidInput));
        assert_eq!(parse_choice("folder-id", 15), Err(ErrorCode::InvalidInput));
    }

    #[test]
    fn cli_reader_menu_page_keeps_selected_row_visible() {
        assert_eq!(page_bounds(0, 40), (0, 12));
        assert_eq!(page_bounds(20, 40), (14, 26));
        assert_eq!(page_bounds(39, 40), (28, 40));
    }

    #[test]
    fn cli_reader_pages_preserve_blank_lines_and_neutralize_terminal_controls() {
        let lines = super::body_lines("first\n\n\u{1b}[31mcontent\u{202e}");
        assert_eq!(lines, ["first", "", " [31mcontent "]);
        assert_eq!(
            super::body_lines(&"a".repeat(145))
                .iter()
                .map(String::len)
                .collect::<Vec<_>>(),
            [72, 72, 1]
        );
    }

    #[test]
    fn cli_reader_language_resource_keys_match() {
        let keys = |source: &str| {
            source
                .lines()
                .filter_map(|line| line.split_once('=').map(|(key, _)| key.to_owned()))
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(
            keys(include_str!("../resources/reading-en.txt")),
            keys(include_str!("../resources/reading-ko.txt"))
        );
    }
}
