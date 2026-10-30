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
        .get("display_name")
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

fn print_help(lang: Lang) {
    if lang == Lang::Ko {
        println!("hantong-t  j/k: 이동  Enter: 열기  h: 뒤로  r: 새로고침  q: 종료");
        println!("AI: ai login codex | ai login claude | ai codex | ai claude");
    } else {
        println!("hantong-t  j/k: move  Enter: open  h: back  r: refresh  q: quit");
        println!("AI: ai login codex | ai login claude | ai codex | ai claude");
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
    loop {
        let page = match (screen, account.as_deref(), folder.as_ref()) {
            (0, _, _) => runner.call(&["accounts", "list"]),
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
            "r" | "refresh" => {}
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
                                account = Some(id);
                                screen = 1;
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
