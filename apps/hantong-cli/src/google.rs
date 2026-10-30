//! Public Google setup and authenticated Gmail read commands.
use super::{Lang, emit_error, escape_metadata_bytes};
use hantong_contracts::{
    ErrorCode,
    google_mail::GoogleMailRequest,
    google_setup::{GoogleSetupCommand, GoogleSetupInput},
    registration::{RegistrationCommand, RegistrationProgress},
    scope_refresh::{ScopeRefreshCommand, ScopeRefreshProgress},
    wire_hex,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::IsTerminal,
    time::{Duration, Instant},
};
enum Action {
    Setup(GoogleSetupCommand),
    Login(GoogleSetupInput),
    Mail(&'static str, Value),
}
fn words(c: &[String], expected: &[&str]) -> bool {
    c.iter().map(String::as_str).eq(expected.iter().copied())
}
fn parse(c: &[String], options: &[(String, String)]) -> Result<Action, ErrorCode> {
    let mut fields = BTreeMap::new();
    for (k, v) in options {
        if fields.insert(k.as_str(), v.as_str()).is_some() || v.chars().any(char::is_control) {
            return Err(ErrorCode::InvalidInput);
        }
    }
    let is_login = words(c, &["login", "google"]);
    let is_request = words(c, &["setup", "google", "request"]);
    if is_login || is_request {
        if fields.keys().any(|k| {
            ![
                "--client-id",
                "--name",
                "--reauthorize-account",
                "--force-consent",
            ]
            .contains(k)
        }) {
            return Err(ErrorCode::InvalidInput);
        }
        let force_consent = match fields.get("--force-consent").copied() {
            None | Some("false") => false,
            Some("true") => true,
            _ => return Err(ErrorCode::InvalidInput),
        };
        let input = GoogleSetupInput {
            client_id: fields.get("--client-id").map(|s| (*s).into()),
            display_name: fields.get("--name").map(|s| (*s).into()),
            reauthorize_account: fields.get("--reauthorize-account").map(|s| (*s).into()),
            force_consent,
        };
        input.validate()?;
        return Ok(if is_login {
            Action::Login(input)
        } else {
            Action::Setup(GoogleSetupCommand::Request(input))
        });
    }
    if words(c, &["setup", "google", "status"]) {
        if fields.len() != 1 {
            return Err(ErrorCode::InvalidInput);
        }
        let id = fields.get("--request-id").ok_or(ErrorCode::InvalidInput)?;
        let id = wire_hex::decode_fixed(id)?;
        if id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        return Ok(Action::Setup(GoogleSetupCommand::Status(id)));
    }
    if c.len() != 2 || c[0] != "gmail" {
        return Err(ErrorCode::InvalidInput);
    }
    let (tool, mode, allowed) = match c[1].as_str() {
        "labels" => ("mail.folders.list", "gmail_labels", vec!["--account"]),
        "list" => (
            "mail.messages.search",
            "gmail_messages",
            vec!["--account", "--label-id", "--query", "--limit", "--after"],
        ),
        "read" => (
            "mail.messages.get",
            "gmail_body",
            vec!["--account", "--message-id", "--max-bytes"],
        ),
        "sync" => (
            "mail.sync.page",
            "gmail_page",
            vec![
                "--account",
                "--label-id",
                "--page-token",
                "--limit",
                "--expected-version",
                "--operation-id",
                "--idempotency-key",
            ],
        ),
        "status" => (
            "mail.sync.page",
            "gmail_status",
            vec!["--account", "--execution-id"],
        ),
        _ => return Err(ErrorCode::InvalidInput),
    };
    if fields.keys().any(|key| !allowed.contains(key)) {
        return Err(ErrorCode::InvalidInput);
    }
    let account = fields.get("--account").ok_or(ErrorCode::InvalidInput)?;
    let mut p = json!({"mode":mode,"account_id":account});
    for (key, v) in &fields {
        let key = match *key {
            "--account" => continue,
            "--expected-version" => "expected_version",
            k => k.trim_start_matches("--"),
        };
        let key = key.replace('-', "_");
        p[&key] = if matches!(key.as_str(), "limit" | "expected_version" | "max_bytes") {
            if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                return Err(ErrorCode::InvalidInput);
            }
            Value::from(v.parse::<u64>().map_err(|_| ErrorCode::InvalidInput)?)
        } else {
            Value::from(*v)
        };
    }
    if mode == "gmail_page" && p.get("operation_id").is_some() != p.get("idempotency_key").is_some()
    {
        return Err(ErrorCode::InvalidInput);
    }
    // Sync's generated identifiers/version are filled only after input validation.
    let mut checked = p.clone();
    if mode == "gmail_page" {
        for (key, default) in [
            ("operation_id", json!("validation")),
            ("idempotency_key", json!("validation")),
            ("expected_version", json!(0)),
        ] {
            if checked.get(key).is_none() {
                checked[key] = default;
            }
        }
    }
    GoogleMailRequest::parse(tool, checked.as_object().ok_or(ErrorCode::InvalidInput)?)?;
    Ok(Action::Mail(tool, p))
}
fn resource(lang: Lang, key: &str) -> &'static str {
    let source = match lang {
        Lang::En => include_str!("../resources/google-en.txt"),
        Lang::Ko => include_str!("../resources/google-ko.txt"),
    };
    source
        .lines()
        .find_map(|l| l.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
        .unwrap_or("Google")
}
fn nonce() -> Result<String, ErrorCode> {
    #[cfg(windows)]
    {
        Ok(wire_hex::encode(&hantong_platform::random_nonce()?))
    }
    #[cfg(not(windows))]
    {
        Err(ErrorCode::CapabilityUnsupported)
    }
}
fn sync_params(
    mut params: Value,
    call: &mut impl FnMut(&str, Value) -> Result<Value, ErrorCode>,
) -> Result<Value, ErrorCode> {
    if params.get("expected_version").is_none() {
        let labels = call(
            "mail.folders.list",
            json!({"mode":"gmail_labels","account_id":params["account_id"]}),
        )?;
        params["expected_version"] = labels["sync_version"].clone();
    }
    for key in ["operation_id", "idempotency_key"] {
        if params.get(key).is_none() {
            params[key] = nonce()?.into();
        }
    }
    if params.get("label_id").is_none() {
        params["label_id"] = "INBOX".into();
    }
    GoogleMailRequest::parse(
        "mail.sync.page",
        params.as_object().ok_or(ErrorCode::InvalidInput)?,
    )?;
    Ok(params)
}
fn print_value(value: &Value, lang: Lang, json_output: bool) {
    if json_output {
        println!("{value}");
        return;
    }
    if let Some(labels) = value.get("labels").and_then(Value::as_array) {
        for l in labels {
            println!("{}  {}", safe(l, "id"), safe(l, "name"));
        }
    } else if let Some(messages) = value.get("messages").and_then(Value::as_array) {
        for m in messages {
            println!(
                "{}  {}  {}",
                safe(m, "id"),
                safe(m, "subject"),
                safe(m, "sender")
            );
        }
        if let Some(cursor) = value.get("next_cursor").and_then(Value::as_str) {
            println!(
                "{} {}",
                resource(lang, "next_cursor"),
                escape_metadata_bytes(cursor.as_bytes())
            );
        }
    } else if let Some(body) = value.get("body") {
        if let Some(text) = body.get("text").and_then(Value::as_str) {
            println!(
                "{}",
                hantong_contracts::message_body::render_terminal_text(text.as_bytes())
            );
        }
        if body["truncated"] == true {
            println!("{}", resource(lang, "truncated"));
        }
    } else if let Some(sync) = value.get("sync") {
        println!(
            "{} {} {}",
            resource(lang, "sync"),
            safe(sync, "state"),
            safe(sync, "execution_id")
        );
    } else {
        println!("{} {}", resource(lang, "setup"), safe(value, "state"));
        if let Some(id) = value.get("request_id").and_then(Value::as_str) {
            println!(
                "{} {}",
                resource(lang, "request_id"),
                escape_metadata_bytes(id.as_bytes())
            );
        }
    }
}
fn safe(v: &Value, key: &str) -> String {
    escape_metadata_bytes(v.get(key).and_then(Value::as_str).unwrap_or("").as_bytes())
}
fn wait_registration(lang: Lang) -> Result<(), ErrorCode> {
    println!("{}", resource(lang, "register"));
    let mut r = hantong_ipc_client::registration::exchange(RegistrationCommand::Request)?;
    let until = Instant::now() + Duration::from_secs(300);
    loop {
        match r.state() {
            RegistrationProgress::Ready => return Ok(()),
            RegistrationProgress::Denied => return Err(ErrorCode::ApprovalDenied),
            RegistrationProgress::Expired => return Err(ErrorCode::ApprovalExpired),
            _ => {}
        }
        if Instant::now() >= until {
            return Err(ErrorCode::ApprovalExpired);
        }
        std::thread::sleep(Duration::from_secs(1));
        r = hantong_ipc_client::registration::exchange(RegistrationCommand::Status(
            *r.request_id(),
        ))?;
    }
}
fn wait_refresh(lang: Lang) -> Result<(), ErrorCode> {
    println!("{}", resource(lang, "refresh"));
    let mut r = hantong_ipc_client::scope_refresh::exchange(ScopeRefreshCommand::Request)?;
    let until = Instant::now() + Duration::from_secs(300);
    loop {
        match r.state() {
            ScopeRefreshProgress::Ready => return Ok(()),
            ScopeRefreshProgress::Denied => return Err(ErrorCode::ApprovalDenied),
            ScopeRefreshProgress::Expired => return Err(ErrorCode::ApprovalExpired),
            ScopeRefreshProgress::Interrupted => return Err(ErrorCode::AuthRequired),
            _ => {}
        }
        if Instant::now() >= until {
            return Err(ErrorCode::ApprovalExpired);
        }
        std::thread::sleep(Duration::from_secs(1));
        r = hantong_ipc_client::scope_refresh::exchange(ScopeRefreshCommand::Status(
            *r.refresh_id(),
        ))?;
    }
}
fn login(input: GoogleSetupInput, lang: Lang) -> Result<Value, ErrorCode> {
    println!("{}", resource(lang, "opening"));
    let mut receipt =
        hantong_ipc_client::google_setup::exchange(GoogleSetupCommand::Request(input))?;
    let until = Instant::now() + Duration::from_secs(600);
    let mut previous = String::new();
    while receipt.state != "ready" {
        if let Some(error) = receipt.error_code {
            return Err(error);
        }
        if receipt.state != previous {
            println!(
                "{} {}",
                resource(lang, "setup"),
                escape_metadata_bytes(receipt.state.as_bytes())
            );
            if let Some(url) = &receipt.login_url {
                println!(
                    "{}\n{}",
                    resource(lang, "browser_fallback"),
                    escape_metadata_bytes(url.as_bytes())
                );
            }
            if receipt.state == "awaiting_review" {
                println!("{}", resource(lang, "review"));
            }
            previous = receipt.state.clone();
        }
        if Instant::now() >= until {
            return Err(ErrorCode::ApprovalExpired);
        }
        std::thread::sleep(Duration::from_secs(1));
        receipt = hantong_ipc_client::google_setup::exchange(GoogleSetupCommand::Status(
            receipt.request_id,
        ))?;
    }
    let account = receipt.account_id.ok_or(ErrorCode::InternalError)?;
    let labels = json!({"mode":"gmail_labels","account_id":account});
    let mut registered = false;
    let mut refreshed = false;
    loop {
        match hantong_ipc_client::google_mail::call_default("mail.folders.list", labels.clone()) {
            Ok(_) => break,
            Err(ErrorCode::AuthRequired | ErrorCode::ClientNotRegistered) if !registered => {
                wait_registration(lang)?;
                registered = true;
            }
            Err(ErrorCode::ScopeDenied) if !refreshed => {
                wait_refresh(lang)?;
                refreshed = true;
            }
            Err(e) => return Err(e),
        }
    }
    // The separate native registration/refresh review decides the exact scope.
    // A Google connection alone never changes a client's account permissions.
    let mut call = hantong_ipc_client::google_mail::call_default;
    let mut value = call(
        "mail.sync.page",
        sync_params(
            json!({"mode":"gmail_page","account_id":account,"label_id":"INBOX","limit":32}),
            &mut call,
        )?,
    )?;
    let until = Instant::now() + Duration::from_secs(600);
    while matches!(
        value.pointer("/sync/state").and_then(Value::as_str),
        Some("pending" | "running")
    ) {
        if Instant::now() >= until {
            return Err(ErrorCode::Offline);
        }
        std::thread::sleep(Duration::from_secs(1));
        value = call(
            "mail.sync.page",
            json!({"mode":"gmail_status","account_id":account,"execution_id":value["sync"]["execution_id"]}),
        )?;
    }
    if let Some(code) = value.pointer("/sync/error_code").and_then(Value::as_str) {
        return Err(ErrorCode::ALL
            .into_iter()
            .find(|e| e.as_str() == code)
            .ok_or(ErrorCode::InternalError)?);
    }
    println!(
        "{} {}",
        resource(lang, "connected"),
        escape_metadata_bytes(account.as_bytes())
    );
    call(
        "mail.messages.search",
        json!({"mode":"gmail_messages","account_id":account,"label_id":"INBOX","limit":32}),
    )
}
pub(super) fn try_run(
    command: &[String],
    options: &[(String, String)],
    lang: Lang,
    json_output: bool,
    no_input: bool,
) -> Option<i32> {
    let handles_command = command.first().is_some_and(|s| s == "gmail")
        || words(command, &["login", "google"])
        || (command.first().is_some_and(|s| s == "setup")
            && command.get(1).is_some_and(|s| s == "google"));
    if !handles_command {
        return None;
    }
    let action = match parse(command, options) {
        Ok(v) => v,
        Err(e) => return Some(emit_error(e, lang, json_output)),
    };
    if matches!(action, Action::Login(_))
        && (no_input || json_output || !std::io::stdin().is_terminal())
    {
        return Some(emit_error(ErrorCode::HumanStepRequired, lang, json_output));
    }
    let result = match action {
        Action::Setup(command) => {
            hantong_ipc_client::google_setup::exchange(command).map(|r| r.to_json())
        }
        Action::Login(input) => login(input, lang),
        Action::Mail(tool, params) => {
            let mut call = hantong_ipc_client::google_mail::call_default;
            let params = if params["mode"] == "gmail_page" {
                sync_params(params, &mut call)
            } else {
                Ok(params)
            };
            params.and_then(|params| call(tool, params))
        }
    };
    Some(match result {
        Ok(v) => {
            print_value(&v, lang, json_output);
            0
        }
        Err(e) => emit_error(e, lang, json_output),
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn c(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| (*s).into()).collect()
    }
    fn opts(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    }
    #[test]
    fn acc02_cli_google_strict_setup_and_noninteractive_login_guard() {
        assert!(parse(&c(&["login", "google"]), &[]).is_ok());
        assert!(
            parse(
                &c(&["login", "google"]),
                &opts(&[("--client-id", "invalid")])
            )
            .is_err()
        );
        assert!(
            parse(
                &c(&["setup", "google", "request"]),
                &opts(&[("--access-token", "forbidden")])
            )
            .is_err()
        );
        assert_eq!(
            try_run(&c(&["login", "google"]), &[], Lang::En, true, false),
            Some(ErrorCode::HumanStepRequired.exit_code())
        );
        assert_eq!(
            try_run(&c(&["login", "google"]), &[], Lang::Ko, false, true),
            Some(ErrorCode::HumanStepRequired.exit_code())
        );
    }
    #[test]
    fn acc03_cli_gmail_requires_account_rejects_duplicate_extra_and_bad_numbers() {
        let account = format!("google_{}", "a".repeat(64));
        assert!(parse(&c(&["gmail", "list"]), &[]).is_err());
        let mut o = opts(&[("--account", &account)]);
        assert!(parse(&c(&["gmail", "list"]), &o).is_ok());
        o.push(("--account".into(), account.clone()));
        assert!(parse(&c(&["gmail", "list"]), &o).is_err());
        assert!(
            parse(
                &c(&["gmail", "sync"]),
                &opts(&[("--account", &account), ("--limit", "+2")])
            )
            .is_err()
        );
        assert!(
            parse(
                &c(&["gmail", "read"]),
                &opts(&[
                    ("--account", &account),
                    ("--message-id", "m1"),
                    ("--approved", "true")
                ])
            )
            .is_err()
        );
    }
    #[test]
    fn sync02_cli_gmail_explicit_sync_identifiers_and_cas_need_no_implicit_read() {
        let p = json!({"mode":"gmail_page","account_id":format!("google_{}","a".repeat(64)),"expected_version":7,"operation_id":"op","idempotency_key":"key"});
        let p = sync_params(p, &mut |_, _| panic!("explicit CAS must not query")).unwrap();
        assert_eq!(p["expected_version"], 7);
        assert_eq!(p["label_id"], "INBOX");
    }
}
