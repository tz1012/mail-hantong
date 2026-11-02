//! Public Google setup and authenticated Gmail read commands.
use super::{Lang, emit_error, escape_metadata_bytes};
use hantong_contracts::{
    ErrorCode,
    google_mail::GoogleMailRequest,
    google_setup::{GoogleLocalProfile, GoogleSetupCommand, GoogleSetupInput},
    registration::{GoogleReadAccount, RegistrationCommand, RegistrationProgress},
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
    Resume([u8; 32]),
    Continue(GoogleReadAccount),
    Accounts,
    Mail(&'static str, Value),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoginFlow {
    Advanced,
    Personal(GoogleLocalProfile),
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
    if is_login
        && (fields.contains_key("--resume-request") || fields.contains_key("--continue-account"))
    {
        if fields.len() != 1 {
            return Err(ErrorCode::InvalidInput);
        }
        if let Some(id) = fields.get("--resume-request") {
            let id = wire_hex::decode_fixed(id)?;
            if id == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            return Ok(Action::Resume(id));
        }
        return Ok(Action::Continue(GoogleReadAccount::parse(
            fields["--continue-account"],
        )?));
    }
    if words(c, &["gmail", "accounts"]) {
        return if fields.is_empty() {
            Ok(Action::Accounts)
        } else {
            Err(ErrorCode::InvalidInput)
        };
    }

    if is_login || is_request {
        if fields.keys().any(|k| {
            ![
                "--client-id",
                "--name",
                "--reauthorize-account",
                "--force-consent",
                "--access-mode",
                "--local-profile",
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
        let local_profile =
            GoogleLocalProfile::parse(fields.get("--local-profile").copied().unwrap_or("none"))?;
        let requested_mode = hantong_contracts::google_write::GoogleAccessMode::parse(
            fields.get("--access-mode").copied().unwrap_or("readonly"),
        )?;
        if let Some(profile_mode) = local_profile.access_mode()
            && fields.contains_key("--access-mode")
            && profile_mode != requested_mode
        {
            return Err(ErrorCode::InvalidInput);
        }
        let input = GoogleSetupInput {
            access_mode: local_profile.access_mode().unwrap_or(requested_mode),
            client_id: fields.get("--client-id").map(|s| (*s).into()),
            display_name: fields.get("--name").map(|s| (*s).into()),
            reauthorize_account: fields.get("--reauthorize-account").map(|s| (*s).into()),
            force_consent,
            local_profile,
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
                "--verify-attachments",
                "--message-id",
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
        p[&key] = if key == "verify_attachments" {
            match *v {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => return Err(ErrorCode::InvalidInput),
            }
        } else if matches!(key.as_str(), "limit" | "expected_version" | "max_bytes") {
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
    if params.get("label_id").is_none() && params.get("message_id").is_none() {
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
fn show_deadline(lang: Lang, expires: i64) -> Result<(), ErrorCode> {
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ErrorCode::InternalError)?
            .as_millis(),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    println!(
        "{} {}",
        resource(lang, "remaining"),
        expires.saturating_sub(now).max(0) / 1000
    );
    Ok(())
}
fn wait_registration(account: GoogleReadAccount, lang: Lang) -> Result<(), ErrorCode> {
    println!("{}", resource(lang, "register"));
    let mut r = super::terminal_workflows::registration(
        RegistrationCommand::RequestGoogleReadOnly(account),
        lang,
    )?;
    show_deadline(lang, r.expires_at_utc_ms())?;
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
fn wait_refresh(account: GoogleReadAccount, lang: Lang) -> Result<(), ErrorCode> {
    println!("{}", resource(lang, "refresh"));
    let mut r =
        super::terminal_workflows::scope(ScopeRefreshCommand::AddGoogleReadOnly(account), lang)?;
    show_deadline(lang, r.expires_at_utc_ms())?;
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

// Each effect stays behind its existing authenticated/native approval boundary.
fn ensure_access(
    account: GoogleReadAccount,
    probe: &mut impl FnMut(&str) -> Result<bool, ErrorCode>,
    register: &mut impl FnMut(GoogleReadAccount) -> Result<(), ErrorCode>,
    add: &mut impl FnMut(GoogleReadAccount) -> Result<(), ErrorCode>,
) -> Result<(), ErrorCode> {
    let id = account.account_id();
    match probe(&id) {
        Ok(true) => return Ok(()),
        Err(ErrorCode::ClientNotRegistered) => register(account)?,
        Ok(false) => {}
        Err(e) => return Err(e),
    }
    match probe(&id) {
        Ok(true) => Ok(()),
        Ok(false) => {
            add(account)?;
            if probe(&id)? {
                Ok(())
            } else {
                Err(ErrorCode::ScopeDenied)
            }
        }
        Err(e) => Err(e),
    }
}
fn continue_account(account: GoogleReadAccount, lang: Lang) -> Result<Value, ErrorCode> {
    continue_account_inner(account, lang, true)
}
fn continue_account_inner(
    account: GoogleReadAccount,
    lang: Lang,
    details: bool,
) -> Result<Value, ErrorCode> {
    if details {
        println!(
            "{} hantong login google --continue-account {}",
            resource(lang, "continue_hint"),
            account.account_id()
        );
    }
    ensure_access(
        account,
        &mut hantong_ipc_client::google_mail::verify_default_readonly,
        &mut |a| wait_registration(a, lang),
        &mut |a| wait_refresh(a, lang),
    )?;
    if details {
        println!("{} {}", resource(lang, "connected"), account.account_id());
        println!("{}", resource(lang, "read_hint"));
    } else {
        println!("{}", resource(lang, "guided_ready"));
    }
    Ok(json!({"state":"ready","account_id":account.account_id()}))
}
fn login(command: GoogleSetupCommand, lang: Lang) -> Result<Value, ErrorCode> {
    login_inner(command, lang, true, LoginFlow::Advanced)
}
fn login_inner(
    command: GoogleSetupCommand,
    lang: Lang,
    details: bool,
    flow: LoginFlow,
) -> Result<Value, ErrorCode> {
    println!("{}", resource(lang, "opening"));
    let mut receipt = hantong_ipc_client::google_setup::exchange(command)?;
    if details {
        println!(
            "{} hantong login google --resume-request {}",
            resource(lang, "resume_hint"),
            wire_hex::encode(&receipt.request_id)
        );
    }
    let mut previous = String::new();
    let mut stage_deadline = Instant::now();
    while receipt.state != "ready" {
        if let Some(error) = receipt.error_code {
            return Err(error);
        }
        if receipt.state != previous {
            println!(
                "{}",
                resource(
                    lang,
                    match receipt.state.as_str() {
                        "awaiting_client" => "client",
                        "awaiting_browser" => "browser",
                        "awaiting_review" => "review",
                        _ => "opening",
                    }
                )
            );
            show_deadline(lang, receipt.expires_at_utc_ms)?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| ErrorCode::InternalError)?
                .as_millis();
            let remaining = (receipt.expires_at_utc_ms.max(0) as u128)
                .saturating_sub(now)
                .min(300_000);
            stage_deadline = Instant::now() + Duration::from_millis(remaining as u64);
            previous = receipt.state.clone();
            if matches!(
                receipt.state.as_str(),
                "awaiting_client" | "awaiting_review"
            ) {
                super::terminal_workflows::maybe_review(
                    hantong_contracts::terminal_review::TerminalReviewKind::GoogleSetup,
                    receipt.request_id,
                    lang,
                )?;
            }
        }
        if Instant::now() >= stage_deadline {
            return Err(ErrorCode::ApprovalExpired);
        }
        std::thread::sleep(Duration::from_secs(1));
        receipt = hantong_ipc_client::google_setup::exchange(GoogleSetupCommand::Status(
            receipt.request_id,
        ))?;
    }
    let account = GoogleReadAccount::parse(&receipt.account_id.ok_or(ErrorCode::InternalError)?)?;
    if let LoginFlow::Personal(profile) = flow {
        if !hantong_ipc_client::google_mail::verify_default_personal(
            &account.account_id(),
            profile,
        )? {
            return Err(ErrorCode::ScopeDenied);
        }
        println!("{}", resource(lang, "guided_ready"));
        return Ok(json!({"state":"ready","account_id":account.account_id()}));
    }
    continue_account_inner(account, lang, details)
}

pub(super) fn connect_guided(lang: Lang, profile: GoogleLocalProfile) -> Result<String, ErrorCode> {
    let access_mode = profile.access_mode().ok_or(ErrorCode::InvalidInput)?;
    let input = GoogleSetupInput {
        access_mode,
        local_profile: profile,
        client_id: None,
        display_name: None,
        reauthorize_account: None,
        force_consent: false,
    };
    let result = login_inner(
        GoogleSetupCommand::Request(input),
        lang,
        false,
        LoginFlow::Personal(profile),
    );
    if result == Err(ErrorCode::HumanStepRequired) {
        println!("{}", resource(lang, "guided_missing_client"));
    }
    let value = result?;
    let account = value["account_id"]
        .as_str()
        .ok_or(ErrorCode::InternalError)?;
    GoogleReadAccount::parse(account)?;
    Ok(account.into())
}

pub(super) fn continue_guided(account: &str, lang: Lang) -> Result<(), ErrorCode> {
    let account = GoogleReadAccount::parse(account)?;
    if !hantong_ipc_client::google_mail::verify_default_readonly(&account.account_id())? {
        return Err(ErrorCode::ScopeDenied);
    }
    println!("{}", resource(lang, "guided_ready"));
    Ok(())
}

pub(super) fn enable_send_guided(account: &str, lang: Lang) -> Result<(), ErrorCode> {
    let account = GoogleReadAccount::parse(account)?;
    if hantong_ipc_client::google_mail::verify_default_personal(
        &account.account_id(),
        GoogleLocalProfile::PersonalReadSend,
    )? {
        println!("{}", resource(lang, "guided_ready"));
        return Ok(());
    }
    let input = GoogleSetupInput {
        access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadSend,
        local_profile: GoogleLocalProfile::PersonalReadSend,
        client_id: None,
        display_name: None,
        reauthorize_account: Some(account.account_id().to_owned()),
        force_consent: true,
    };
    let value = login_inner(
        GoogleSetupCommand::Request(input),
        lang,
        false,
        LoginFlow::Personal(GoogleLocalProfile::PersonalReadSend),
    )?;
    let connected = value["account_id"]
        .as_str()
        .ok_or(ErrorCode::InternalError)?;
    if connected != account.account_id() {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}

fn inbox_page(
    account: &str,
    operation: &str,
    call: &mut impl FnMut(&str, Value) -> Result<Value, ErrorCode>,
    pause: &mut impl FnMut(),
) -> Result<Value, ErrorCode> {
    GoogleReadAccount::parse(account)?;
    let params = sync_params(
        json!({"mode":"gmail_page","account_id":account,
        "label_id":"INBOX","limit":20,"operation_id":operation,"idempotency_key":operation}),
        call,
    )?;
    let mut result = call("mail.sync.page", params)?;
    let execution = result["sync"]["execution_id"]
        .as_str()
        .ok_or(ErrorCode::InvalidInput)?
        .to_owned();
    if wire_hex::decode_fixed::<32>(&execution)? == [0; 32] {
        return Err(ErrorCode::InvalidInput);
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut polls = 0;
    loop {
        match result["sync"]["state"].as_str() {
            Some("completed") => break,
            Some("failed" | "interrupted") => {
                return Err(ErrorCode::ALL
                    .into_iter()
                    .find(|e| result["sync"]["error_code"].as_str() == Some(e.as_str()))
                    .unwrap_or(ErrorCode::InternalError));
            }
            Some("pending" | "running") => {}
            _ => return Err(ErrorCode::InvalidInput),
        }
        if Instant::now() >= deadline || polls >= 120 {
            return Err(ErrorCode::CoreUnavailable);
        }
        pause();
        result = call(
            "mail.sync.page",
            json!({"mode":"gmail_status","account_id":account,"execution_id":execution}),
        )?;
        polls += 1;
    }
    call(
        "mail.messages.search",
        json!({"mode":"gmail_messages","account_id":account,"label_id":"INBOX","limit":20}),
    )
}

pub(super) fn inbox_guided(account: &str, lang: Lang) -> Result<(), ErrorCode> {
    println!("{}", resource(lang, "guided_inbox"));
    let value = inbox_page(
        account,
        &nonce()?,
        &mut hantong_ipc_client::google_mail::call_default,
        &mut || std::thread::sleep(Duration::from_millis(250)),
    )?;
    let messages = value["messages"]
        .as_array()
        .ok_or(ErrorCode::InvalidInput)?;
    if messages.is_empty() {
        println!("{}", resource(lang, "guided_empty"));
        return Ok(());
    }
    use std::io::{BufRead, Read, Write};
    let mut input = std::io::stdin().lock();
    loop {
        for (i, message) in messages.iter().enumerate() {
            println!(
                "{}) {}  {}",
                i + 1,
                safe(message, "subject"),
                safe(message, "sender")
            );
        }
        println!("{}", resource(lang, "guided_read_choice"));
        std::io::stdout()
            .flush()
            .map_err(|_| ErrorCode::InternalError)?;
        let mut bytes = Vec::new();
        if input
            .by_ref()
            .take(257)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| ErrorCode::InternalError)?
            == 0
        {
            return Ok(());
        }
        if bytes.len() > 256 {
            return Err(ErrorCode::InvalidInput);
        }
        let choice = std::str::from_utf8(&bytes)
            .map_err(|_| ErrorCode::InvalidInput)?
            .trim();
        if choice == "0" || choice.is_empty() {
            return Ok(());
        }
        let selected = choice
            .parse::<usize>()
            .ok()
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| messages.get(i));
        let Some(message) = selected else {
            println!("{}", resource(lang, "guided_invalid"));
            continue;
        };
        let id = message["id"].as_str().ok_or(ErrorCode::InvalidInput)?;
        let body = hantong_ipc_client::google_mail::call_default(
            "mail.messages.get",
            json!({"mode":"gmail_body","account_id":account,"message_id":id,"max_bytes":65536}),
        )?;
        print_value(&body, lang, false);
    }
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
    if matches!(
        action,
        Action::Login(_)
            | Action::Resume(_)
            | Action::Continue(_)
            | Action::Setup(GoogleSetupCommand::Request(_))
    ) && (no_input
        || json_output
        || !std::io::stdin().is_terminal()
        || !std::io::stdout().is_terminal())
    {
        return Some(emit_error(ErrorCode::HumanStepRequired, lang, json_output));
    }
    let result = match action {
        Action::Setup(command) => {
            let is_request = matches!(command, GoogleSetupCommand::Request(_));
            hantong_ipc_client::google_setup::exchange(command).and_then(|r| {
                if is_request
                    && !json_output
                    && !no_input
                    && matches!(r.state.as_str(), "awaiting_client" | "awaiting_review")
                {
                    super::terminal_workflows::maybe_review(
                        hantong_contracts::terminal_review::TerminalReviewKind::GoogleSetup,
                        r.request_id,
                        lang,
                    )?;
                    return hantong_ipc_client::google_setup::exchange(GoogleSetupCommand::Status(
                        r.request_id,
                    ))
                    .map(|r| r.to_json());
                }
                Ok(r.to_json())
            })
        }
        Action::Login(input) => login(GoogleSetupCommand::Request(input), lang),
        Action::Resume(id) => login(GoogleSetupCommand::Status(id), lang),
        Action::Continue(account) => continue_account(account, lang),
        Action::Accounts => {
            return Some(
                match hantong_ipc_client::accounts::list_default_accounts(false) {
                    Ok(response) => {
                        println!("{}", super::account_output(&response, lang, json_output));
                        if !json_output {
                            println!("{}", resource(lang, "accounts_hint"));
                        }
                        0
                    }
                    Err(e) => emit_error(e, lang, json_output),
                },
            );
        }
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
        Err(e) => {
            if !json_output {
                println!(
                    "{}",
                    resource(
                        lang,
                        match e {
                            ErrorCode::ApprovalExpired => "expired_hint",
                            ErrorCode::AuthRequired => "auth_hint",
                            ErrorCode::Conflict => "conflict_hint",
                            _ => "retry_hint",
                        }
                    )
                );
            }
            emit_error(e, lang, json_output)
        }
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
    fn acc02_cli_google_personal_profile_is_selected_before_consent() {
        let action = parse(
            &c(&["login", "google"]),
            &opts(&[("--local-profile", "personal_read_send")]),
        )
        .unwrap();
        let Action::Login(input) = action else {
            panic!("expected login");
        };
        assert_eq!(input.local_profile, GoogleLocalProfile::PersonalReadSend);
        assert_eq!(
            input.access_mode,
            hantong_contracts::google_write::GoogleAccessMode::ReadSend
        );
        assert!(
            parse(
                &c(&["login", "google"]),
                &opts(&[("--local-profile", "unknown")])
            )
            .is_err()
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
    #[test]
    fn sync02_cli_targeted_attachment_read_does_not_default_to_inbox() {
        let account = format!("google_{}", "a".repeat(64));
        let action = parse(
            &c(&["gmail", "sync"]),
            &opts(&[
                ("--account", &account),
                ("--message-id", "message1"),
                ("--limit", "1"),
                ("--verify-attachments", "true"),
                ("--expected-version", "7"),
                ("--operation-id", "op"),
                ("--idempotency-key", "key"),
            ]),
        )
        .unwrap();
        let Action::Mail(_, p) = action else {
            panic!("expected mail")
        };
        let p = sync_params(p, &mut |_, _| panic!("explicit CAS")).unwrap();
        assert!(p.get("label_id").is_none());
        assert_eq!(p["message_id"], "message1");
    }
}

#[cfg(test)]
mod onboarding_tests {
    use super::*;
    #[test]
    fn cli04_sync02_guided_inbox_syncs_one_page_then_lists_without_write_or_body() {
        let account = format!("google_{}", "a".repeat(64));
        let mut modes = Vec::new();
        let mut pauses = 0;
        let page = inbox_page(
            &account,
            "guided-fixture",
            &mut |tool, p| {
                modes.push((tool.to_owned(), p["mode"].as_str().unwrap().to_owned()));
                assert_eq!(p["account_id"], account);
                Ok(match p["mode"].as_str().unwrap() {
                    "gmail_labels" => json!({"sync_version":3}),
                    "gmail_page" => {
                        assert_eq!(p["expected_version"], 3);
                        assert_eq!(p["limit"], 20);
                        assert_eq!(p["label_id"], "INBOX");
                        json!({"sync":{"execution_id":"11".repeat(32),"state":"pending"}})
                    }
                    "gmail_status" => {
                        json!({"sync":{"execution_id":"11".repeat(32),"state":"completed"}})
                    }
                    "gmail_messages" => json!({"messages":[]}),
                    _ => panic!("unexpected guided effect"),
                })
            },
            &mut || pauses += 1,
        )
        .unwrap();
        assert!(page["messages"].as_array().unwrap().is_empty());
        assert_eq!(pauses, 1);
        assert_eq!(
            modes.iter().map(|(_, m)| m.as_str()).collect::<Vec<_>>(),
            [
                "gmail_labels",
                "gmail_page",
                "gmail_status",
                "gmail_messages"
            ]
        );
    }
    #[test]
    fn sync02_guided_inbox_failure_never_retries_or_lists() {
        let account = format!("google_{}", "a".repeat(64));
        let mut calls = 0;
        assert_eq!(
            inbox_page(
                &account,
                "guided-failure",
                &mut |_, p| {
                    calls += 1;
                    Ok(if p["mode"] == "gmail_labels" {
                        json!({"sync_version":0})
                    } else {
                        json!({"sync":{"execution_id":"11".repeat(32),"state":"interrupted","error_code":"AUTH_REQUIRED"}})
                    })
                },
                &mut || panic!("failed operation must not poll")
            ),
            Err(ErrorCode::AuthRequired)
        );
        assert_eq!(calls, 2);
    }
    use std::cell::RefCell;
    #[test]
    fn acc02_onboarding_two_accounts_preserves_access_and_never_registers_on_auth_failure() {
        let a = GoogleReadAccount::parse(&format!("google_{}", "a".repeat(64))).unwrap();
        let b = GoogleReadAccount::parse(&format!("google_{}", "b".repeat(64))).unwrap();
        let granted = RefCell::new(Vec::<String>::new());
        let registered = RefCell::new(false);
        let events = RefCell::new(Vec::<String>::new());
        for account in [a, b, a] {
            ensure_access(
                account,
                &mut |id| {
                    if !*registered.borrow() {
                        return Err(ErrorCode::ClientNotRegistered);
                    }
                    if granted.borrow().contains(&id.to_string()) {
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                },
                &mut |selected| {
                    events
                        .borrow_mut()
                        .push(format!("register:{}", selected.account_id()));
                    *registered.borrow_mut() = true;
                    Ok(())
                },
                &mut |selected| {
                    events
                        .borrow_mut()
                        .push(format!("add:{}", selected.account_id()));
                    granted.borrow_mut().push(selected.account_id());
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(granted.into_inner(), vec![a.account_id(), b.account_id()]);
        assert_eq!(
            events.into_inner(),
            vec![
                format!("register:{}", a.account_id()),
                format!("add:{}", a.account_id()),
                format!("add:{}", b.account_id())
            ]
        );
        for error in [
            ErrorCode::AuthRequired,
            ErrorCode::ScopeDenied,
            ErrorCode::Conflict,
            ErrorCode::CoreUnavailable,
        ] {
            assert_eq!(
                ensure_access(
                    a,
                    &mut |_| Err(error),
                    &mut |_| panic!("must not register"),
                    &mut |_| panic!("must not grant")
                ),
                Err(error)
            );
        }
    }
    #[test]
    fn acc02_onboarding_denial_and_expiry_stop_without_retry_or_mail_access() {
        let a = GoogleReadAccount::parse(&format!("google_{}", "a".repeat(64))).unwrap();
        for error in [ErrorCode::ApprovalDenied, ErrorCode::ApprovalExpired] {
            let mut calls = 0;
            assert_eq!(
                ensure_access(
                    a,
                    &mut |_| Ok(false),
                    &mut |_| panic!("already registered"),
                    &mut |_| {
                        calls += 1;
                        Err(error)
                    }
                ),
                Err(error)
            );
            assert_eq!(calls, 1);
        }
    }
    #[test]
    fn acc02_onboarding_resume_modes_are_exclusive_and_noninteractive_guarded() {
        let cmd = vec!["login".into(), "google".into()];
        let id = wire_hex::encode(&[1; 32]);
        let account = format!("google_{}", "a".repeat(64));
        for (key, value) in [("--resume-request", id), ("--continue-account", account)] {
            let mut options = vec![(key.into(), value)];
            assert!(parse(&cmd, &options).is_ok());
            assert_eq!(
                try_run(&cmd, &options, Lang::Ko, true, false),
                Some(ErrorCode::HumanStepRequired.exit_code())
            );
            options.push(("--force-consent".into(), "true".into()));
            assert!(parse(&cmd, &options).is_err());
        }
    }
}
