#![forbid(unsafe_code)]

use serde_json::Value;
use std::env;
use std::io::{self, BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const MAX_OUTPUT: usize = 1024 * 1024;
const MAX_ERROR: usize = 16 * 1024;
const CHILD_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lang {
    En,
    Ko,
}

impl Lang {
    fn flag(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ko => "ko",
        }
    }
}

#[derive(Debug)]
struct Runner {
    program: std::path::PathBuf,
    lang: Lang,
}

impl Runner {
    fn sibling() -> Result<std::path::PathBuf, String> {
        let current = env::current_exe().map_err(|_| "unable to locate hantong-t".to_owned())?;
        let parent = current
            .parent()
            .ok_or_else(|| "unable to locate hantong executable".to_owned())?;
        let path = parent.join(if cfg!(windows) {
            "hantong.exe"
        } else {
            "hantong"
        });
        if path.is_file() {
            Ok(path)
        } else {
            Err("hantong CLI is not installed beside hantong-t".to_owned())
        }
    }

    fn call(&self, args: &[&str]) -> Result<Value, String> {
        let mut command = Command::new(&self.program);
        command.arg("--lang").arg(self.lang.flag()).arg("--json");
        for arg in args {
            if arg.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
                return Err("invalid command argument".to_owned());
            }
            command.arg(arg);
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "unable to run hantong".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "hantong output unavailable".to_owned())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "hantong diagnostics unavailable".to_owned())?;
        let out_thread = thread::spawn(|| read_capped(stdout, MAX_OUTPUT));
        let err_thread = thread::spawn(|| read_capped(stderr, MAX_ERROR));
        let started = Instant::now();
        let status = loop {
            match child
                .try_wait()
                .map_err(|_| "unable to read hantong status".to_owned())?
            {
                Some(status) => break status,
                None if started.elapsed()
                    >= if args.first() == Some(&"read") {
                        Duration::from_secs(40)
                    } else {
                        CHILD_TIMEOUT
                    } =>
                {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = out_thread.join();
                    let _ = err_thread.join();
                    return Err("hantong command timed out".to_owned());
                }
                None => thread::sleep(Duration::from_millis(20)),
            }
        };
        let stdout = out_thread
            .join()
            .map_err(|_| "hantong output failed".to_owned())??;
        let stderr = err_thread
            .join()
            .map_err(|_| "hantong diagnostics failed".to_owned())??;
        let text =
            String::from_utf8(stdout).map_err(|_| "hantong returned invalid UTF-8".to_owned())?;
        let value: Value =
            serde_json::from_str(&text).map_err(|_| "hantong returned invalid JSON".to_owned())?;
        if !status.success() || value.get("ok") == Some(&Value::Bool(false)) {
            return Err(error_text(&value, &stderr));
        }
        Ok(value)
    }
}

fn read_capped<R: Read>(mut reader: R, max: usize) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    let mut buf = [0_u8; 8192];
    loop {
        let count = reader
            .read(&mut buf)
            .map_err(|_| "unable to read hantong response".to_owned())?;
        if count == 0 {
            break;
        }
        if data.len().saturating_add(count) > max {
            return Err("hantong response exceeded the terminal limit".to_owned());
        }
        data.extend_from_slice(&buf[..count]);
    }
    Ok(data)
}

fn error_text(value: &Value, stderr: &[u8]) -> String {
    value
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(sanitize_single_line)
        .or_else(|| {
            String::from_utf8(stderr.to_vec())
                .ok()
                .map(|s| sanitize_single_line(&s))
        })
        .unwrap_or_else(|| "hantong command failed".to_owned())
}

fn sanitize(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' | '\t' => output.push(ch),
            '\r'
            | '\u{061c}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}' => {}
            '\u{1b}' => output.push(' '),
            ch if ch.is_control() || ('\u{80}'..='\u{9f}').contains(&ch) => output.push('�'),
            ch => output.push(ch),
        }
    }
    output
}

fn sanitize_single_line(input: &str) -> String {
    sanitize(input)
        .chars()
        .map(|ch| if matches!(ch, '\n' | '\r' | '\t') { ' ' } else { ch })
        .filter(|ch| !matches!(*ch, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}'))
        .collect()
}

fn item_label(item: &Value) -> String {
    if let Some(name) = item
        .get("name")
        .or_else(|| item.get("display_name"))
        .and_then(Value::as_str)
        .or_else(|| item.get("account_id").and_then(Value::as_str))
    {
        return sanitize_single_line(name);
    }
    let subject = item
        .get("subject")
        .and_then(Value::as_str)
        .unwrap_or("(no subject)");
    let sender = item
        .pointer("/from/mailbox")
        .and_then(Value::as_str)
        .or_else(|| item.get("from").and_then(Value::as_str))
        .or_else(|| item.get("sender").and_then(Value::as_str))
        .unwrap_or("");
    if sender.is_empty() {
        sanitize_single_line(subject)
    } else {
        format!(
            "{}  —  {}",
            sanitize_single_line(subject),
            sanitize_single_line(sender)
        )
    }
}

fn decode_header_hex(value: Option<&Value>) -> String {
    let Some(hex) = value.and_then(Value::as_str) else {
        return String::new();
    };
    if hex.len() > 16384 || hex.len() % 2 != 0 {
        return String::new();
    }
    let bytes: Option<Vec<u8>> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|p| {
            let digit = |b: u8| match b {
                b'0'..=b'9' => Some(b - b'0'),
                b'a'..=b'f' => Some(b - b'a' + 10),
                _ => None,
            };
            Some(digit(p[0])? * 16 + digit(p[1])?)
        })
        .collect();
    bytes
        .map(|b| sanitize_single_line(&String::from_utf8_lossy(&b)))
        .unwrap_or_default()
}
fn projected_label(page: &Value, item: &Value) -> String {
    let Some(index) = item
        .get("header_content_index")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
    else {
        return item_label(item);
    };
    let Some(content) = page
        .get("untrusted_content")
        .and_then(Value::as_array)
        .and_then(|a| a.get(index))
        .and_then(|v| v.get("content"))
        .and_then(Value::as_str)
    else {
        return item_label(item);
    };
    if content.len() > 65536 {
        return item_label(item);
    }
    let Ok(header) = serde_json::from_str::<Value>(content) else {
        return item_label(item);
    };
    let subject = decode_header_hex(header.get("subject_hex"));
    let sender = header
        .get("from")
        .and_then(Value::as_array)
        .and_then(|a| a.first());
    let name = decode_header_hex(sender.and_then(|s| s.get("name_hex")));
    let mailbox = decode_header_hex(sender.and_then(|s| s.get("mailbox_hex")));
    let host = decode_header_hex(sender.and_then(|s| s.get("host_hex")));
    let sender = if !name.is_empty() {
        name
    } else if !mailbox.is_empty() && !host.is_empty() {
        format!("{mailbox}@{host}")
    } else {
        mailbox
    };
    let subject = if subject.is_empty() {
        "(no subject)"
    } else {
        subject.as_str()
    };
    if sender.is_empty() {
        subject.to_owned()
    } else {
        format!("{subject}  —  {sender}")
    }
}
fn google_items(mut value: Value) -> Value {
    let items = value
        .get("labels")
        .or_else(|| value.get("messages"))
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));
    value["items"] = items;
    value
}

fn print_items(title: &str, value: &Value, selected: usize) {
    println!("\n{}", sanitize_single_line(title));
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        println!("  (no items)");
        return;
    };
    if items.is_empty() {
        println!("  (no items)");
        return;
    }
    for (index, item) in items.iter().enumerate() {
        println!(
            "{} {:>3}. {}",
            if index == selected { '>' } else { ' ' },
            index + 1,
            projected_label(value, item)
        );
    }
}

fn rows(value: &Value) -> &[Value] {
    value
        .get("items")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}
fn field(item: &Value, name: &str) -> Option<String> {
    item.get(name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn google_login_args(line: &str) -> Result<Option<Vec<String>>, String> {
    if !line.starts_with("login") {
        return Ok(None);
    }
    if line.len() > 1024 || line.chars().any(char::is_control) {
        return Err("invalid Google login arguments".into());
    }
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quoted = false;
    let mut started = false;
    for ch in line.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            ch if ch.is_whitespace() && !quoted => {
                if started {
                    tokens.push(std::mem::take(&mut token));
                    started = false;
                }
            }
            ch => {
                token.push(ch);
                started = true;
            }
        }
    }
    if quoted {
        return Err("invalid Google login arguments".into());
    }
    if started {
        tokens.push(token)
    }
    if tokens.first().map(String::as_str) != Some("login")
        || tokens.get(1).map(String::as_str) != Some("google")
    {
        return Ok(None);
    }
    if tokens.len() > 6 || tokens.len() % 2 != 0 {
        return Err("invalid Google login arguments".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    for pair in tokens[2..].chunks_exact(2) {
        if !seen.insert(&pair[0]) {
            return Err("duplicate Google login option".into());
        }
        match pair[0].as_str() {
            "--client-id" => {
                let value = &pair[1];
                let Some(prefix) = value.strip_suffix(".apps.googleusercontent.com") else {
                    return Err("invalid public Desktop client ID".into());
                };
                if value.len() > 256
                    || prefix.is_empty()
                    || !prefix
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    return Err("invalid public Desktop client ID".into());
                }
            }
            "--name"
                if !pair[1].is_empty()
                    && pair[1].len() <= 512
                    && pair[1].chars().count() <= 128 => {}
            _ => return Err("invalid Google login option".into()),
        }
    }
    Ok(Some(tokens))
}
fn refresh_google_with(
    account: &str,
    label: &str,
    timeout: Duration,
    call: &mut impl FnMut(&[&str]) -> Result<Value, String>,
    wait: &mut impl FnMut(Duration),
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let mut result = call(&[
        "gmail",
        "sync",
        "--account",
        account,
        "--label-id",
        label,
        "--limit",
        "32",
    ])?;
    loop {
        match result.pointer("/sync/state").and_then(Value::as_str) {
            Some("completed") => return Ok(()),
            Some("failed" | "interrupted") => return Err(
                "Gmail synchronization failed; reconnect this account if authorization is required"
                    .into(),
            ),
            Some("pending" | "running") => {}
            _ => return Err("invalid Gmail synchronization status".into()),
        }
        if Instant::now() >= deadline {
            return Err(
                "Gmail synchronization timed out; inspect its status before retrying".into(),
            );
        }
        let id = result
            .pointer("/sync/execution_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "invalid Gmail synchronization status".to_owned())?
            .to_owned();
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid Gmail synchronization ID".into());
        }
        wait(Duration::from_secs(1));
        result = call(&[
            "gmail",
            "status",
            "--account",
            account,
            "--execution-id",
            &id,
        ])?;
    }
}

fn print_help(lang: Lang) {
    if lang == Lang::Ko {
        println!("hantong-t  j/k: 이동  Enter: 열기  h: 뒤로  r: 새로고침  q: 종료");
        println!("AI: ai login codex | ai login claude | ai codex | ai claude");
        println!(
            "Google: login google [--client-id ID] [--name \"이름\"]  Gmail에서 r: 선택 폴더 동기화"
        );
    } else {
        println!("hantong-t  j/k: move  Enter: open  h: back  r: refresh  q: quit");
        println!("AI: ai login codex | ai login claude | ai codex | ai claude");
        println!(
            "Google: login google [--client-id ID] [--name \"Name\"]  Gmail r: synchronize selected folder"
        );
    }
}

fn read_command() -> Option<String> {
    print!("\n> ");
    io::stdout().flush().ok()?;
    bounded_command(io::stdin().lock())
}
fn bounded_command(reader: impl BufRead) -> Option<String> {
    let mut line = String::new();
    let count = reader.take(1025).read_line(&mut line).ok()?;
    if count == 0 || line.len() > 1024 {
        return None;
    }
    Some(line.trim().to_owned())
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 && args[1] == "--version" {
        println!("hantong-t {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.len() == 2 && args[1] == "--help" {
        println!("hantong-t [--lang en|ko]");
        print_help(Lang::En);
        return Ok(());
    }
    let lang = if args
        .windows(2)
        .any(|pair| pair[0] == "--lang" && pair[1] == "ko")
    {
        Lang::Ko
    } else {
        Lang::En
    };
    let program = match Runner::sibling() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("{error}");
            return Ok(());
        }
    };
    let runner = Runner { program, lang };
    print_help(lang);
    println!("mail-hantong terminal view");
    let mut screen = 0_u8;
    let mut selected = 0_usize;
    let mut account: Option<String> = None;
    let mut folder: Option<(String, String)> = None;
    let mut gmail = false;
    loop {
        let page = match (screen, account.as_deref(), folder.as_ref()) {
            (0, _, _) => runner.call(&["accounts", "list"]),
            (1, Some(id), _) if gmail => runner
                .call(&["gmail", "labels", "--account", id])
                .map(google_items),
            (_, Some(id), Some((label, _))) if gmail => runner
                .call(&[
                    "gmail",
                    "list",
                    "--account",
                    id,
                    "--label-id",
                    label,
                    "--limit",
                    "32",
                ])
                .map(google_items),
            (1, Some(id), _) => runner.call(&["folders", "list", "--account", id]),
            (_, Some(id), Some((wire, encoding))) => runner.call(&[
                "search",
                "--account",
                id,
                "--wire-hex",
                wire,
                "--encoding",
                encoding,
                "--limit",
                "32",
            ]),
            _ => Ok(serde_json::json!({"items":[]})),
        };
        let snapshot = match page {
            Ok(value) => {
                selected = selected.min(rows(&value).len().saturating_sub(1));
                let title = match (screen, lang) {
                    (0, Lang::En) => "Accounts",
                    (0, Lang::Ko) => "계정",
                    (1, Lang::En) => "Folders",
                    (1, Lang::Ko) => "폴더",
                    (_, Lang::En) => "Messages",
                    (_, Lang::Ko) => "메일",
                };
                print_items(title, &value, selected);
                Some(value)
            }
            Err(error) => {
                eprintln!("{}", sanitize(&error));
                None
            }
        };
        let Some(command) = read_command() else { break };
        match google_login_args(&command) {
            Ok(Some(arguments)) => {
                let outcome = Command::new(&runner.program)
                    .args(["--lang", runner.lang.flag()])
                    .args(&arguments)
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status();
                if !matches!(outcome,Ok(status) if status.success()) {
                    eprintln!(
                        "{}",
                        if lang == Lang::Ko {
                            "Google 연결을 완료하지 못했습니다."
                        } else {
                            "Google connection did not complete."
                        }
                    );
                }
                screen = 0;
                selected = 0;
                account = None;
                folder = None;
                gmail = false;
                continue;
            }
            Err(error) => {
                eprintln!("{}", sanitize_single_line(&error));
                continue;
            }
            Ok(None) => {}
        }
        match command.as_str() {
            "ai codex" | "ai claude" | "ai login codex" | "ai login claude" => {
                let action = if command.starts_with("ai login ") {
                    "login"
                } else {
                    "chat"
                };
                let provider = if command.ends_with(" codex") {
                    "codex"
                } else {
                    "claude"
                };
                // The human explicitly opens this session. Cached mail is not
                // attached or transferred to a model by this navigation action.
                let outcome = Command::new(&runner.program)
                    .args(["--lang", runner.lang.flag(), "ai", action, provider])
                    .stdin(Stdio::inherit())
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .status();
                if !matches!(outcome,Ok(status) if status.success()) {
                    eprintln!(
                        "{}",
                        if lang == Lang::Ko {
                            "AI 세션을 완료하지 못했습니다."
                        } else {
                            "AI session did not complete."
                        }
                    );
                }
            }
            "q" | "quit" | "exit" => break,
            "j" | "down" => selected = selected.saturating_add(1),
            "k" | "up" => selected = selected.saturating_sub(1),
            "r" | "refresh" => {
                if gmail
                    && screen > 0
                    && let Some(id) = account.as_deref()
                {
                    let selected_label = if screen == 1 {
                        snapshot
                            .as_ref()
                            .and_then(|s| rows(s).get(selected))
                            .and_then(|item| field(item, "id"))
                    } else {
                        folder.as_ref().map(|f| f.0.clone())
                    };
                    let label = selected_label.as_deref().unwrap_or("INBOX");
                    println!(
                        "{}",
                        if lang == Lang::Ko {
                            "선택한 Gmail 폴더를 동기화합니다."
                        } else {
                            "Synchronizing the selected Gmail folder."
                        }
                    );
                    if let Err(error) = refresh_google_with(
                        id,
                        label,
                        Duration::from_secs(600),
                        &mut |args| runner.call(args),
                        &mut thread::sleep,
                    ) {
                        eprintln!("{}", sanitize(&error));
                    }
                }
            }
            "h" | "back" => {
                if screen > 0 {
                    screen -= 1;
                    selected = 0;
                    if screen == 0 {
                        account = None;
                        folder = None;
                    } else {
                        folder = None;
                    }
                }
            }
            "?" | "help" => print_help(lang),
            "" => {
                if let Some(item) = snapshot
                    .as_ref()
                    .and_then(|value| rows(value).get(selected))
                {
                    match screen {
                        0 => {
                            if let Some(id) = field(item, "account_id") {
                                gmail =
                                    item.get("provider").and_then(Value::as_str) == Some("gmail");
                                account = Some(id);
                                screen = 1;
                                selected = 0;
                            }
                        }
                        1 if gmail => {
                            if let Some(label) = field(item, "id") {
                                folder = Some((label, "gmail".into()));
                                screen = 2;
                                selected = 0;
                            }
                        }
                        1 => {
                            if let (Some(wire), Some(encoding)) =
                                (field(item, "wire_hex"), field(item, "encoding"))
                            {
                                folder = Some((wire, encoding));
                                screen = 2;
                                selected = 0;
                            }
                        }
                        _ if gmail => {
                            if let (Some(id), Some(message)) =
                                (account.as_deref(), field(item, "id"))
                            {
                                match runner.call(&[
                                    "gmail",
                                    "read",
                                    "--account",
                                    id,
                                    "--message-id",
                                    &message,
                                ]) {
                                    Ok(v) => {
                                        if let Some(text) =
                                            v.pointer("/body/text").and_then(Value::as_str)
                                        {
                                            println!("\n{}\n", sanitize(text));
                                        }
                                    }
                                    Err(e) => eprintln!("{}", sanitize(&e)),
                                }
                            }
                        }
                        _ => {
                            if let (Some(id), Some(message)) =
                                (account.as_deref(), field(item, "internal_message_id"))
                            {
                                match runner.call(&[
                                    "read",
                                    "--account",
                                    id,
                                    "--message-id",
                                    &message,
                                ]) {
                                    Ok(value) => {
                                        if let Some(text) =
                                            value.get("text").and_then(Value::as_str)
                                        {
                                            println!("\n{}\n", sanitize(text));
                                        }
                                    }
                                    Err(error) => eprintln!("{}", sanitize(&error)),
                                }
                            }
                        }
                    }
                }
            }
            _ => println!("{}", sanitize("Unknown command; use ? for help.")),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tui01_google_login_passes_only_bounded_explicit_arguments_without_shell() {
        let args = google_login_args(
            "login google --client-id fixture.apps.googleusercontent.com --name \"Work Gmail\"",
        )
        .unwrap()
        .unwrap();
        assert_eq!(args.last().map(String::as_str), Some("Work Gmail"));
        for input in [
            "login google --access-token secret",
            "login google --name x --name y",
            "login google --client-id nope",
            "login google && whoami",
            "login google --name \"unfinished",
        ] {
            assert!(google_login_args(input).is_err(), "{input}");
        }
        assert!(google_login_args("ai codex").unwrap().is_none());
    }
    #[test]
    fn tui01_google_explicit_refresh_polls_one_job_and_never_resubmits() {
        let mut calls = Vec::new();
        let mut step = 0;
        refresh_google_with("account","Label_7",Duration::from_secs(10),&mut|args|{calls.push(args.iter().map(|s|(*s).to_owned()).collect::<Vec<_>>());step+=1;Ok(serde_json::json!({"sync":{"state":match step{1=>"pending",2=>"running",_=>"completed"},"execution_id":"a".repeat(64)}}))},&mut|_|{}).unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(&calls[0][..2], ["gmail", "sync"]);
        assert_eq!(calls[0][5], "Label_7");
        assert!(calls[1..].iter().all(|args| args[1] == "status"));
        let mut calls = 0;
        assert!(refresh_google_with("account","INBOX",Duration::ZERO,&mut|_|{calls+=1;Ok(serde_json::json!({"sync":{"state":"pending","execution_id":"a".repeat(64)}}))},&mut|_|{}).is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn tui01_google_labels_and_message_ids_keep_native_provider_identity() {
        let labels = google_items(
            serde_json::json!({"provider":"gmail","labels":[{"id":"Label_7","name":"Inbox\u{1b}[31m"}]}),
        );
        assert_eq!(field(&rows(&labels)[0], "id").as_deref(), Some("Label_7"));
        assert!(!item_label(&rows(&labels)[0]).contains('\u{1b}'));
        let messages = google_items(
            serde_json::json!({"provider":"gmail","messages":[{"id":"18abc","subject":"Test","sender":"sender@example.test"}]}),
        );
        assert_eq!(field(&rows(&messages)[0], "id").as_deref(), Some("18abc"));
        assert!(item_label(&rows(&messages)[0]).contains("sender@example.test"));
        assert!(field(&rows(&messages)[0], "uidvalidity").is_none());
    }

    #[test]
    fn tui01_navigation_uses_real_projection_and_bounded_input() {
        assert_eq!(bounded_command(io::Cursor::new(b"")), None);
        assert_eq!(bounded_command(io::Cursor::new(b"\n")), Some(String::new()));
        assert_eq!(bounded_command(io::Cursor::new(vec![b'a'; 1025])), None);
        let page = serde_json::json!({"items":[{"header_content_index":0}],"untrusted_content":[{"content":"{\"subject_hex\":\"48691b\",\"from\":[{\"mailbox_hex\":\"75736572\",\"host_hex\":\"6578616d706c65\"}]}"}]});
        assert_eq!(
            projected_label(&page, &rows(&page)[0]),
            "Hi   —  user@example"
        );
        assert_eq!(
            item_label(&serde_json::json!({"display_name":"Inbox"})),
            "Inbox"
        );
    }

    #[test]
    fn tui01_sanitizes_terminal_controls_without_dropping_newlines() {
        assert_eq!(
            sanitize("subject\u{1b}[31m\nnext\u{7f}"),
            "subject [31m\nnext�"
        );
    }

    #[test]
    fn tui01_rejects_control_arguments_before_process_spawn() {
        let runner = Runner {
            program: "missing-hantong".into(),
            lang: Lang::En,
        };
        assert_eq!(
            runner.call(&["search\n--limit"]),
            Err("invalid command argument".into())
        );
    }

    #[test]
    fn tui01_formats_untrusted_subject_and_sender() {
        let value = serde_json::json!({"subject": "Hi\u{1b}[2J", "from": {"mailbox": "a\nb"}});
        assert_eq!(item_label(&value), "Hi [2J  —  a b");
    }
}
