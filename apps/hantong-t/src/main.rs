#![forbid(unsafe_code)]

#[cfg(any(windows, unix))]
mod terminal_app;
mod terminal_view;
mod unified;

use serde_json::Value;
use std::env;
use std::io::{self, BufRead, Read, Write};
#[cfg(windows)]
use std::process::Command;
use std::process::Stdio;
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
        #[cfg(target_os = "linux")]
        {
            let asset = hantong_platform::linux_identity::current_asset()
                .map_err(|_| "unable to verify hantong-t identity".to_owned())?;
            if asset != "bin/hantong-t" {
                return Err("unable to verify hantong-t identity".to_owned());
            }
            Ok(hantong_platform::linux_identity::current_release_root()
                .map_err(|_| "unable to locate hantong executable".to_owned())?
                .join("bin/hantong"))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let current =
                env::current_exe().map_err(|_| "unable to locate hantong-t".to_owned())?;
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
    }

    fn call(&self, args: &[&str]) -> Result<Value, String> {
        if args
            .iter()
            .any(|arg| arg.bytes().any(|byte| byte == 0 || byte.is_ascii_control()))
        {
            return Err("invalid command argument".to_owned());
        }
        #[cfg(unix)]
        let mut command =
            hantong_platform::unix_release::verified_sibling_command("hantong-t", "bin/hantong")
                .map_err(|_| "unable to verify hantong CLI".to_owned())?;
        #[cfg(windows)]
        let mut command = Command::new(&self.program);
        command.arg("--lang").arg(self.lang.flag()).arg("--json");
        command.args(args);
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

    fn interactive(&self, args: &[&str]) -> Result<(), String> {
        if args
            .iter()
            .any(|arg| arg.bytes().any(|byte| byte == 0 || byte.is_ascii_control()))
        {
            return Err("invalid command argument".to_owned());
        }
        #[cfg(unix)]
        let mut command =
            hantong_platform::unix_release::verified_sibling_command("hantong-t", "bin/hantong")
                .map_err(|_| "unable to verify hantong CLI".to_owned())?;
        #[cfg(windows)]
        let mut command = Command::new(&self.program);
        let status = command
            .arg("--lang")
            .arg(self.lang.flag())
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|_| "unable to run hantong".to_owned())?;
        if status.success() {
            Ok(())
        } else {
            Err("hantong command failed".to_owned())
        }
    }

    fn interactive_owned(&self, args: &[String]) -> Result<(), String> {
        self.interactive(&args.iter().map(String::as_str).collect::<Vec<_>>())
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
    if let Some(name) = ["primary_address", "address", "name", "display_name"]
        .iter()
        .find_map(|key| {
            item.get(key)
                .and_then(Value::as_str)
                .filter(|v| !v.is_empty())
        })
    {
        return sanitize_single_line(name);
    }
    if item.get("account_id").is_some() && item.get("provider").is_some() {
        return "Account".into();
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
        hantong_platform::mail_display::decode_header(subject.as_bytes())
    } else {
        format!(
            "{}  —  {}",
            hantong_platform::mail_display::decode_header(subject.as_bytes()),
            hantong_platform::mail_display::decode_header(sender.as_bytes())
        )
    }
}

fn decode_header_hex(value: Option<&Value>) -> String {
    hantong_platform::mail_display::decode_hex_header(value)
}
fn header_projection(page: &Value, item: &Value) -> Value {
    item.get("header_content_index")
        .and_then(Value::as_u64)
        .and_then(|index| {
            page.get("untrusted_content")?
                .as_array()?
                .get(index as usize)?
                .get("content")?
                .as_str()
        })
        .filter(|content| content.len() <= 65536)
        .and_then(|content| serde_json::from_str(content).ok())
        .unwrap_or_else(|| item.clone())
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

fn inbox_first(mut value: Value, gmail: bool) -> Value {
    if let Some(items) = value.get_mut("items").and_then(Value::as_array_mut) {
        items.sort_by_key(|item| {
            let inbox = if gmail {
                item.get("id").and_then(Value::as_str) == Some("INBOX")
            } else {
                item.get("wire_hex")
                    .and_then(Value::as_str)
                    .is_some_and(|wire| wire.eq_ignore_ascii_case("494e424f58"))
            };
            !inbox
        });
    }
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

fn selected_trash_args(
    account: &str,
    gmail: bool,
    folder: Option<&(String, String)>,
    page: &Value,
    item: &Value,
) -> Result<Vec<String>, String> {
    if account.is_empty() || account.bytes().any(|b| b.is_ascii_control()) {
        return Err("selected account is unavailable".to_owned());
    }
    if gmail {
        let message = field(item, "id")
            .ok_or_else(|| "selected Gmail message has no immutable message identity".to_owned())?;
        let version = page
            .get("sync_version")
            .and_then(Value::as_u64)
            .or_else(|| item.get("expected_version").and_then(Value::as_u64))
            .ok_or_else(|| "selected Gmail message has no current version".to_owned())?;
        return Ok(vec![
            "change".into(),
            "trash".into(),
            "--account".into(),
            account.into(),
            "--message-id".into(),
            message,
            "--expected-version".into(),
            version.to_string(),
        ]);
    }
    let (wire, encoding) = folder.ok_or_else(|| "current IMAP folder is unavailable".to_owned())?;
    let uid = item
        .get("uid")
        .and_then(Value::as_u64)
        .ok_or_else(|| "selected IMAP message has no UID".to_owned())?;
    let uidvalidity = item
        .get("uidvalidity")
        .and_then(Value::as_u64)
        .ok_or_else(|| "selected IMAP message has no UIDVALIDITY".to_owned())?;
    let generation = item
        .get("catalog_generation")
        .and_then(Value::as_u64)
        .ok_or_else(|| "selected IMAP message has no folder generation".to_owned())?;
    Ok(vec![
        "trash".into(),
        "move".into(),
        "--account".into(),
        account.into(),
        "--mailbox-wire-hex".into(),
        wire.clone(),
        "--mailbox-encoding".into(),
        encoding.clone(),
        "--expected-catalog-generation".into(),
        generation.to_string(),
        "--uidvalidity".into(),
        uidvalidity.to_string(),
        "--uid".into(),
        uid.to_string(),
    ])
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

fn refresh_imap_folders_with(
    account: &str,
    generation: u64,
    operation: &str,
    idempotency_key: &str,
    call: &mut impl FnMut(&[&str]) -> Result<Value, String>,
    wait: &mut impl FnMut(Duration),
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut result = call(&[
        "folders",
        "refresh",
        "--account",
        account,
        "--expected-generation",
        &generation.to_string(),
        "--operation-id",
        operation,
        "--idempotency-key",
        idempotency_key,
    ])?;
    loop {
        if result.get("account_id").and_then(Value::as_str) != Some(account) {
            return Err("IMAP folder refresh account changed".into());
        }
        match result.pointer("/state").and_then(Value::as_str) {
            Some("succeeded") => return Ok(()),
            Some("failed") => return Err("IMAP folder refresh failed".into()),
            Some("unresolved") => {}
            _ => return Err("invalid IMAP folder refresh status".into()),
        }
        let id = result
            .get("execution_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "invalid IMAP folder refresh status".to_owned())?
            .to_owned();
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid IMAP folder refresh ID".into());
        }
        if Instant::now() >= deadline {
            return Err("IMAP folder refresh timed out; inspect its status before retrying".into());
        }
        wait(Duration::from_secs(1));
        result = call(&[
            "folders",
            "refresh-status",
            "--account",
            account,
            "--execution-id",
            &id,
        ])?;
        if result.get("execution_id").and_then(Value::as_str) != Some(id.as_str()) {
            return Err("IMAP folder refresh execution changed".into());
        }
    }
}

fn poll_imap_initial_with(
    account: &str,
    mut value: Value,
    deadline: Instant,
    call: &mut impl FnMut(&[&str]) -> Result<Value, String>,
    wait: &mut impl FnMut(Duration),
) -> Result<Value, String> {
    let execution = value
        .get("execution_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "invalid IMAP initial sync status".to_owned())?
        .to_owned();
    if execution.len() != 64 || !execution.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid IMAP initial sync ID".into());
    }
    loop {
        if value.get("account_id").and_then(Value::as_str) != Some(account) {
            return Err("IMAP initial sync account changed".into());
        }
        match value.get("state").and_then(Value::as_str) {
            Some("succeeded") => return Ok(value),
            Some("failed") => return Err("IMAP initial sync failed".into()),
            Some("admitted" | "running" | "unresolved") => {}
            _ => return Err("invalid IMAP initial sync status".into()),
        }
        if Instant::now() >= deadline {
            return Err("IMAP initial sync timed out; inspect its status before retrying".into());
        }
        wait(Duration::from_secs(1));
        value = call(&[
            "sync",
            "initial-status",
            "--account",
            account,
            "--execution-id",
            &execution,
        ])?;
        if value.get("execution_id").and_then(Value::as_str) != Some(execution.as_str()) {
            return Err("IMAP initial sync execution changed".into());
        }
    }
}

fn refresh_imap_initial_preview_with(
    account: &str,
    wire: &str,
    encoding: &str,
    operation_prefix: &str,
    timeout: Duration,
    call: &mut impl FnMut(&[&str]) -> Result<Value, String>,
    wait: &mut impl FnMut(Duration),
) -> Result<(), String> {
    let current = call(&[
        "sync",
        "initial-current",
        "--account",
        account,
        "--wire-hex",
        wire,
        "--encoding",
        encoding,
    ])?;
    if current.get("account_id").and_then(Value::as_str) != Some(account)
        || current.get("wire_hex").and_then(Value::as_str) != Some(wire)
        || current.get("encoding").and_then(Value::as_str) != Some(encoding)
    {
        return Err("selected IMAP folder identity changed; refresh and retry".into());
    }
    let current_plan = current
        .get("plan")
        .ok_or_else(|| "IMAP initial sync current response has no plan field".to_owned())?;
    let expected_preview_generation = if current_plan.is_null() {
        0
    } else {
        let plan = current_plan;
        if plan.get("account_id").and_then(Value::as_str) != Some(account)
            || plan.get("wire_hex").and_then(Value::as_str) != Some(wire)
            || plan.get("encoding").and_then(Value::as_str) != Some(encoding)
            || plan.get("is_current") != Some(&Value::Bool(true))
            || plan.get("binding_current") != Some(&Value::Bool(true))
        {
            return Err("IMAP initial sync current plan identity changed".to_owned());
        }
        plan.get("generation")
            .and_then(Value::as_u64)
            .ok_or_else(|| "IMAP initial sync current plan has no generation".to_owned())?
    };
    let cursor = call(&[
        "sync",
        "cursor",
        "--account",
        account,
        "--wire-hex",
        wire,
        "--encoding",
        encoding,
    ])?;
    if cursor.get("account_id").and_then(Value::as_str) != Some(account)
        || cursor.get("wire_hex").and_then(Value::as_str) != Some(wire)
        || cursor.get("encoding").and_then(Value::as_str) != Some(encoding)
    {
        return Err("selected IMAP folder identity changed; refresh and retry".into());
    }
    let generation = cursor
        .get("catalog_generation")
        .and_then(Value::as_u64)
        .ok_or_else(|| "selected IMAP folder has no catalog generation".to_owned())?;
    let deadline = Instant::now() + timeout;
    let start_operation = format!("{operation_prefix}_start");
    let mut result = call(&[
        "sync",
        "initial-start",
        "--account",
        account,
        "--wire-hex",
        wire,
        "--encoding",
        encoding,
        "--expected-catalog-generation",
        &generation.to_string(),
        "--expected-preview-generation",
        &expected_preview_generation.to_string(),
        "--recent-pages",
        "2",
        "--operation-id",
        &start_operation,
        "--idempotency-key",
        &start_operation,
    ])?;
    result = poll_imap_initial_with(account, result, deadline, call, wait)?;
    let plan = result
        .get("plan")
        .ok_or_else(|| "IMAP initial sync returned no preview plan".to_owned())?;
    let plan_id = plan
        .get("plan_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "IMAP initial sync returned no plan ID".to_owned())?
        .to_owned();
    let plan_version = plan
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "IMAP initial sync returned no plan version".to_owned())?;
    if plan.get("account_id").and_then(Value::as_str) != Some(account)
        || plan.get("wire_hex").and_then(Value::as_str) != Some(wire)
        || plan.get("encoding").and_then(Value::as_str) != Some(encoding)
        || plan.get("generation").and_then(Value::as_u64)
            != Some(
                expected_preview_generation
                    .checked_add(1)
                    .ok_or_else(|| "IMAP initial sync preview generation overflow".to_owned())?,
            )
    {
        return Err("IMAP initial sync preview identity changed".into());
    }
    let preview_generation = plan.get("generation").and_then(Value::as_u64);
    let mut remaining = plan
        .pointer("/progress/remaining_page_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| "IMAP initial sync plan has no remaining page count".to_owned())?;
    let mut version = plan_version;
    for ordinal in 0..2 {
        if remaining == 0 {
            break;
        }
        let resume_operation = format!("{operation_prefix}_resume_{ordinal}");
        result = call(&[
            "sync",
            "initial-resume",
            "--account",
            account,
            "--wire-hex",
            wire,
            "--encoding",
            encoding,
            "--plan-id",
            &plan_id,
            "--expected-plan-version",
            &version.to_string(),
            "--expected-catalog-generation",
            &generation.to_string(),
            "--operation-id",
            &resume_operation,
            "--idempotency-key",
            &resume_operation,
        ])?;
        let resumed = poll_imap_initial_with(account, result, deadline, call, wait)?;
        let resumed_plan = resumed
            .get("plan")
            .ok_or_else(|| "IMAP initial sync resume returned no plan".to_owned())?;
        if resumed_plan.get("plan_id").and_then(Value::as_str) != Some(plan_id.as_str())
            || resumed_plan.get("account_id").and_then(Value::as_str) != Some(account)
            || resumed_plan.get("wire_hex").and_then(Value::as_str) != Some(wire)
            || resumed_plan.get("encoding").and_then(Value::as_str) != Some(encoding)
            || resumed_plan.get("generation").and_then(Value::as_u64) != preview_generation
            || resumed_plan.get("version").and_then(Value::as_u64) <= Some(version)
        {
            return Err("IMAP initial sync resume plan changed".into());
        }
        version = resumed_plan
            .get("version")
            .and_then(Value::as_u64)
            .ok_or_else(|| "IMAP initial sync resume returned no plan version".to_owned())?;
        remaining = resumed_plan
            .pointer("/progress/remaining_page_count")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                "IMAP initial sync resume returned no remaining page count".to_owned()
            })?;
    }
    Ok(())
}

fn refresh_operation_id(prefix: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    format!("tui_{prefix}_{millis}")
}

/// Resolve a displayed one-based number without clamping to another item.
fn numbered_selection(command: &str, count: usize) -> Result<Option<usize>, ()> {
    let value = command.trim();
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    let index = value
        .parse::<usize>()
        .map_err(|_| ())?
        .checked_sub(1)
        .ok_or(())?;
    if index >= count {
        return Err(());
    }
    Ok(Some(index))
}

fn print_help(lang: Lang) {
    if lang == Lang::Ko {
        println!(
            "hantong-t  번호: 바로 열기  j/k: 이동  Enter: 열기  c: 작성  t: 휴지통  h: 뒤로  r: 새로고침  q: 종료"
        );
        println!("AI: ai login codex | ai login claude | ai codex | ai claude");
        println!(
            "Google: login google [--client-id ID] [--name \"이름\"]  Gmail에서 r: 선택 폴더 동기화"
        );
    } else {
        println!(
            "hantong-t  Number: open item  j/k: move  Enter: open  c: compose  t: trash  h: back  r: refresh  q: quit"
        );
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

fn line_main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() == 2 && args[1] == "--version" {
        println!(
            "hantong-t {}",
            option_env!("HANTONG_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
        );
        return Ok(());
    }
    if args.len() == 2 && args[1] == "--help" {
        println!("hantong-t [--lang en|ko] [--account ACCOUNT_ID]");
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
    let mut requested_account = args
        .windows(2)
        .find(|pair| pair[0] == "--account")
        .map(|pair| pair[1].clone());
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
                let value = if screen == 1 {
                    inbox_first(value, gmail)
                } else {
                    value
                };
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
        if screen == 0
            && let Some(target) = requested_account.take()
        {
            let Some(item) = snapshot.as_ref().and_then(|value| {
                rows(value)
                    .iter()
                    .find(|item| field(item, "account_id").as_deref() == Some(target.as_str()))
            }) else {
                eprintln!("requested account is not available");
                break;
            };
            account = field(item, "account_id");
            gmail = item.get("provider").and_then(Value::as_str) == Some("gmail");
            screen = 1;
            selected = 0;
            continue;
        }
        let Some(mut command) = read_command() else {
            break;
        };
        match numbered_selection(&command, snapshot.as_ref().map_or(0, |v| rows(v).len())) {
            Ok(Some(index)) => {
                selected = index;
                command.clear(); // Use the same account-bound open path as Enter.
            }
            Ok(None) => {}
            Err(()) => {
                println!(
                    "{}",
                    if lang == Lang::Ko {
                        "목록에 표시된 번호를 입력하세요."
                    } else {
                        "Enter a number shown in the list."
                    }
                );
                continue;
            }
        }
        match google_login_args(&command) {
            Ok(Some(arguments)) => {
                #[cfg(unix)]
                let command = hantong_platform::unix_release::verified_sibling_command(
                    "hantong-t",
                    "bin/hantong",
                );
                #[cfg(windows)]
                let command: Result<Command, std::io::Error> = Ok(Command::new(&runner.program));
                let outcome = command
                    .map_err(|_| "unable to verify hantong CLI".to_owned())
                    .and_then(|mut command| {
                        command
                            .args(["--lang", runner.lang.flag()])
                            .args(&arguments)
                            .stdin(Stdio::inherit())
                            .stdout(Stdio::inherit())
                            .stderr(Stdio::inherit())
                            .status()
                            .map_err(|_| "unable to run hantong".to_owned())
                    });
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
                #[cfg(unix)]
                let command = hantong_platform::unix_release::verified_sibling_command(
                    "hantong-t",
                    "bin/hantong",
                );
                #[cfg(windows)]
                let command: Result<Command, std::io::Error> = Ok(Command::new(&runner.program));
                let outcome = command
                    .map_err(|_| "unable to verify hantong CLI".to_owned())
                    .and_then(|mut command| {
                        command
                            .args(["--lang", runner.lang.flag(), "ai", action, provider])
                            .stdin(Stdio::inherit())
                            .stdout(Stdio::inherit())
                            .stderr(Stdio::inherit())
                            .status()
                            .map_err(|_| "unable to run hantong".to_owned())
                    });
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
                } else if !gmail
                    && screen == 1
                    && let Some(id) = account.as_deref()
                {
                    let generation = snapshot
                        .as_ref()
                        .and_then(|value| value.pointer("/metadata/generation"))
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let operation = refresh_operation_id("folders");
                    println!("Refreshing the selected IMAP account folders.");
                    if let Err(error) = refresh_imap_folders_with(
                        id,
                        generation,
                        &operation,
                        &operation,
                        &mut |args| runner.call(args),
                        &mut thread::sleep,
                    ) {
                        eprintln!("{}", sanitize(&error));
                    }
                } else if !gmail
                    && screen > 1
                    && let (Some(id), Some((wire, encoding))) =
                        (account.as_deref(), folder.as_ref())
                {
                    let operation = refresh_operation_id("initial");
                    if let Err(error) = refresh_imap_initial_preview_with(
                        id,
                        wire,
                        encoding,
                        &operation,
                        Duration::from_secs(300),
                        &mut |args| runner.call(args),
                        &mut thread::sleep,
                    ) {
                        eprintln!("{}", sanitize(&error));
                    }
                }
            }
            "c" | "compose" => {
                if let Some(id) = account.as_deref() {
                    if let Err(error) = runner.interactive(&["compose", "--account", id]) {
                        eprintln!("{}", sanitize(&error));
                    }
                } else {
                    eprintln!("select an account before composing");
                }
            }
            "t" | "trash" => {
                let Some(id) = account.as_deref() else {
                    eprintln!("select an account before moving a message to Trash");
                    continue;
                };
                let Some(item) = snapshot
                    .as_ref()
                    .and_then(|value| rows(value).get(selected))
                else {
                    eprintln!("select a message before moving it to Trash");
                    continue;
                };
                if screen < 2 {
                    eprintln!("open a message list before moving it to Trash");
                    continue;
                }
                let mut action_item = item.clone();
                if !gmail {
                    let Some(internal_id) = field(item, "internal_message_id") else {
                        eprintln!("selected IMAP message has no immutable message identity");
                        continue;
                    };
                    let Some((wire, encoding)) = folder.as_ref() else {
                        eprintln!("current IMAP folder is unavailable");
                        continue;
                    };
                    let located =
                        match runner.call(&["read", "--account", id, "--message-id", &internal_id])
                        {
                            Ok(value) => value,
                            Err(error) => {
                                eprintln!("{}", sanitize(&error));
                                continue;
                            }
                        };
                    if located.get("account_id").and_then(Value::as_str) != Some(id)
                        || located.get("mailbox_wire_hex").and_then(Value::as_str)
                            != Some(wire.as_str())
                        || located.get("mailbox_encoding").and_then(Value::as_str)
                            != Some(encoding.as_str())
                        || item.get("uid").and_then(Value::as_u64)
                            != located.get("uid").and_then(Value::as_u64)
                    {
                        eprintln!("selected IMAP message identity changed; refresh and retry");
                        continue;
                    }
                    let inspection = match runner.call(&[
                        "sync",
                        "inspect",
                        "--account",
                        id,
                        "--wire-hex",
                        wire,
                        "--encoding",
                        encoding,
                    ]) {
                        Ok(value) => value,
                        Err(error) => {
                            eprintln!("{}", sanitize(&error));
                            continue;
                        }
                    };
                    if inspection.get("account_id").and_then(Value::as_str) != Some(id)
                        || inspection.get("wire_hex").and_then(Value::as_str) != Some(wire.as_str())
                        || inspection.get("encoding").and_then(Value::as_str)
                            != Some(encoding.as_str())
                    {
                        eprintln!("current IMAP folder identity changed; refresh and retry");
                        continue;
                    }
                    action_item["uidvalidity"] = located["uidvalidity"].clone();
                    action_item["uid"] = located["uid"].clone();
                    action_item["catalog_generation"] = inspection["catalog_generation"].clone();
                }
                match selected_trash_args(
                    id,
                    gmail,
                    folder.as_ref(),
                    snapshot.as_ref().unwrap(),
                    &action_item,
                ) {
                    Ok(args) => {
                        if let Err(error) = runner.interactive_owned(&args) {
                            eprintln!("{}", sanitize(&error));
                        }
                    }
                    Err(error) => eprintln!("{}", sanitize(&error)),
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

fn main() -> io::Result<()> {
    #[cfg(all(target_os = "linux", not(test)))]
    if let Err(code) = hantong_platform::linux_identity::ensure_entry("bin/hantong-t") {
        eprintln!("{}", code.as_str());
        std::process::exit(code.exit_code());
    }
    let args: Vec<String> = env::args().collect();
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--version" | "--plain"))
    {
        return line_main();
    }
    if args.iter().any(|arg| arg == "--help") {
        println!(
            "hantong-t [--lang en|ko] [--account ID] [--theme blue|green|amber|mono|bbs8|bbs16] [--demo] [--plain]"
        );
        println!(
            "Arrows/j/k: navigate  Enter: open  Tab: pane  PgUp/PgDn: body  F: theme  ?: help  q: quit"
        );
        println!("Mouse: click selects, double-click opens, wheel scrolls. CLI: hantong --help");
        return Ok(());
    }
    #[cfg(any(windows, unix))]
    {
        terminal_app::run(&args)
    }
    #[cfg(not(any(windows, unix)))]
    {
        eprintln!(
            "The full-screen terminal is unavailable on this operating system. Use --plain for the text view."
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tui01_numbered_selection_opens_displayed_folder_without_clamping() {
        assert_eq!(numbered_selection("15", 17), Ok(Some(14)));
        assert_eq!(numbered_selection(" 1 ", 17), Ok(Some(0)));
        for input in ["0", "18", "999999999999999999999999999999"] {
            assert_eq!(numbered_selection(input, 17), Err(()));
        }
        assert_eq!(numbered_selection("1", 0), Err(()));
        for input in ["", "j", "help", "login google", "15x"] {
            assert_eq!(numbered_selection(input, 17), Ok(None));
        }
    }
    #[test]
    fn tui01_account_display_prefers_address_and_skips_null_fields() {
        assert_eq!(
            item_label(
                &serde_json::json!({"provider":"gmail","account_id":"opaque_id","primary_address":"reader@example.invalid","display_name":"opaque_id"})
            ),
            "reader@example.invalid"
        );
        assert_eq!(
            item_label(
                &serde_json::json!({"provider":"imap","account_id":"opaque_id","primary_address":null,"display_name":"Work"})
            ),
            "Work"
        );
        assert_eq!(
            item_label(&serde_json::json!({"provider":"gmail","account_id":"opaque_id"})),
            "Account"
        );
    }
    #[test]
    fn tui01_inbox_is_first_without_reordering_other_folders_or_changing_identity() {
        for gmail in [true, false] {
            let inbox = if gmail {
                serde_json::json!({"id":"INBOX","name":"Inbox"})
            } else {
                serde_json::json!({"wire_hex":"494e424f58","encoding":"utf8","name":"Inbox"})
            };
            let first = serde_json::json!({"id":"custom","name":"Inbox copy"});
            let last = serde_json::json!({"id":"sent","name":"Sent"});
            let result = inbox_first(
                serde_json::json!({"items":[first,inbox,last],"metadata":{"generation":4}}),
                gmail,
            );
            assert_eq!(rows(&result), &[inbox, first, last]);
            assert_eq!(result["metadata"]["generation"], 4);
        }
    }
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
    fn tui01_imap_folder_refresh_is_account_qualified_and_polls_once() {
        let mut calls = Vec::new();
        let mut step = 0;
        refresh_imap_folders_with(
            "imap_account",
            7,
            "tui_folders_1",
            "tui_folders_1",
            &mut |args| {
                calls.push(args.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>());
                step += 1;
                Ok(if step == 1 {
                    serde_json::json!({"account_id":"imap_account","execution_id":"a".repeat(64),"state":"unresolved","generation":null,"error_code":null})
                } else {
                    serde_json::json!({"account_id":"imap_account","execution_id":"a".repeat(64),"state":"succeeded","generation":8,"error_code":null})
                })
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(&calls[0][..2], ["folders", "refresh"]);
        assert!(
            calls[0]
                .windows(2)
                .any(|pair| pair == ["--account", "imap_account"])
        );
        assert_eq!(&calls[1][..2], ["folders", "refresh-status"]);
    }

    #[test]
    fn tui01_imap_refresh_uses_bounded_recent_preview_plan_and_resume() {
        let mut calls = Vec::new();
        let mut step = 0;
        refresh_imap_initial_preview_with(
            "imap_account",
            "494e424f58",
            "modified_utf7",
            "tui_initial_1",
            Duration::from_secs(10),
            &mut |args| {
                calls.push(args.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>());
                step += 1;
                Ok(match step {
                    1 => serde_json::json!({"account_id":"imap_account","wire_hex":"494e424f58","encoding":"modified_utf7","plan":{"account_id":"imap_account","wire_hex":"494e424f58","encoding":"modified_utf7","generation":4,"is_current":true,"binding_current":true}}),
                    2 => serde_json::json!({"account_id":"imap_account","wire_hex":"494e424f58","encoding":"modified_utf7","catalog_generation":3,"cursor_version":0}),
                    3 => serde_json::json!({"account_id":"imap_account","execution_id":"a".repeat(64),"state":"unresolved"}),
                    4 => serde_json::json!({"account_id":"imap_account","execution_id":"a".repeat(64),"state":"succeeded","plan":{"plan_id":"c".repeat(64),"account_id":"imap_account","wire_hex":"494e424f58","encoding":"modified_utf7","generation":5,"version":1,"progress":{"remaining_page_count":2}}}),
                    5 => serde_json::json!({"account_id":"imap_account","execution_id":"b".repeat(64),"state":"unresolved"}),
                    6 => serde_json::json!({"account_id":"imap_account","execution_id":"b".repeat(64),"state":"succeeded","plan":{"plan_id":"c".repeat(64),"account_id":"imap_account","wire_hex":"494e424f58","encoding":"modified_utf7","generation":5,"version":2,"progress":{"remaining_page_count":1}}}),
                    7 => serde_json::json!({"account_id":"imap_account","execution_id":"d".repeat(64),"state":"unresolved"}),
                    _ => serde_json::json!({"account_id":"imap_account","execution_id":"d".repeat(64),"state":"succeeded","plan":{"plan_id":"c".repeat(64),"account_id":"imap_account","wire_hex":"494e424f58","encoding":"modified_utf7","generation":5,"version":3,"progress":{"remaining_page_count":0}}}),
                })
            },
            &mut |_| {},
        )
        .unwrap();
        let plan_id = "c".repeat(64);
        assert_eq!(calls.len(), 8);
        assert_eq!(&calls[0][..2], ["sync", "initial-current"]);
        assert_eq!(&calls[2][..2], ["sync", "initial-start"]);
        assert!(
            calls[2]
                .windows(2)
                .any(|pair| pair == ["--recent-pages", "2"])
        );
        assert!(
            calls[2]
                .windows(2)
                .any(|pair| pair == ["--expected-preview-generation", "4"])
        );
        assert_eq!(&calls[4][..2], ["sync", "initial-resume"]);
        assert!(
            calls[4]
                .windows(2)
                .any(|pair| pair == ["--plan-id", plan_id.as_str()])
        );
        assert!(
            calls[4]
                .windows(2)
                .any(|pair| pair == ["--expected-plan-version", "1"])
        );
        assert!(
            calls[4]
                .windows(2)
                .any(|pair| pair == ["--expected-catalog-generation", "3"])
        );
    }

    #[test]
    fn tui01_imap_initial_current_identity_mismatch_stops_before_start() {
        let mut calls = Vec::new();
        let result = refresh_imap_initial_preview_with(
            "imap_account",
            "494e424f58",
            "modified_utf7",
            "tui_initial_bad",
            Duration::from_secs(1),
            &mut |args| {
                calls.push(
                    args.iter()
                        .map(|value| (*value).to_owned())
                        .collect::<Vec<_>>(),
                );
                Ok(
                    serde_json::json!({"account_id":"imap_account","wire_hex":"444f4553","encoding":"modified_utf7","plan":null}),
                )
            },
            &mut |_| {},
        );
        assert!(result.is_err());
        assert_eq!(calls.len(), 1);
        assert_eq!(&calls[0][..2], ["sync", "initial-current"]);
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
    fn tui01_trash_requires_current_version_and_immutable_identity() {
        let page = serde_json::json!({"sync_version": 7});
        let item = serde_json::json!({});
        let account = format!("google_{}", "a".repeat(64));
        assert!(selected_trash_args(&account, true, None, &page, &item).is_err());

        let page = serde_json::json!({});
        let item = serde_json::json!({"uid":9});
        let folder = ("494e424f58".to_owned(), "utf8".to_owned());
        assert!(selected_trash_args("imap_account", false, Some(&folder), &page, &item).is_err());
    }

    #[test]
    fn tui01_trash_args_bind_the_selected_account_and_provider_route() {
        let page = serde_json::json!({"sync_version": 7});
        let account = format!("google_{}", "b".repeat(64));
        let item = serde_json::json!({
            "id":"message-1",
            "operation_id":"op-1",
            "idempotency_key":"key-1",
            "payload_hash":"00".repeat(32)
        });
        let args = selected_trash_args(&account, true, None, &page, &item).unwrap();
        assert_eq!(
            &args[0..4],
            &["change", "trash", "--account", account.as_str()].map(str::to_owned)
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--message-id", "message-1"])
        );
    }

    #[test]
    fn tui01_imap_trash_args_use_located_uid_and_inspected_generation_fixture() {
        let page = serde_json::json!({"items": []});
        let item = serde_json::json!({
            "internal_message_id":"msg_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "uid":9,
            "uidvalidity":42,
            "catalog_generation":3
        });
        let folder = ("494e424f58".to_owned(), "utf8".to_owned());
        let args = selected_trash_args("imap_account", false, Some(&folder), &page, &item).unwrap();
        assert!(args.windows(2).any(|pair| pair == ["--uid", "9"]));
        assert!(args.windows(2).any(|pair| pair == ["--uidvalidity", "42"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--expected-catalog-generation", "3"])
        );
    }

    #[test]
    fn tui01_formats_untrusted_subject_and_sender() {
        let value = serde_json::json!({"subject": "Hi\u{1b}[2J", "from": {"mailbox": "a\nb"}});
        assert_eq!(item_label(&value), "Hi [2J  —  a b");
    }
}
