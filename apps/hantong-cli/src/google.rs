//! Public Google setup and authenticated Gmail read commands.
use super::{Lang, emit_error, escape_metadata_bytes};
use hantong_contracts::{
    ErrorCode,
    google_mail::GoogleMailRequest,
    google_setup::{GoogleLocalProfile, GoogleSetupCommand, GoogleSetupInput},
    registration::{GoogleReadAccount, RegistrationCommand, RegistrationProgress},
    scope_refresh::{
        GoogleSendAccounts, ManageAccounts, ScopeRefreshCommand, ScopeRefreshProgress,
    },
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
    ConnectOnly(GoogleLocalProfile),
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
    Ok(wire_hex::encode(&hantong_platform::random_nonce()?))
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
        if let Some(text) =
            hantong_platform::mail_display::reading_text(value, &Value::Null, lang == Lang::Ko)
        {
            println!(
                "{}",
                hantong_contracts::message_body::sanitize_terminal_text(&text)
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
fn wait_google_send(account: GoogleReadAccount, lang: Lang) -> Result<Value, ErrorCode> {
    println!("{}", resource(lang, "refresh"));
    let accounts =
        hantong_ipc_client::google_mail::default_google_account_union(&account.account_id())?;
    let accounts = GoogleSendAccounts::parse(&accounts)?;
    let mut r =
        super::terminal_workflows::scope(ScopeRefreshCommand::RequestGoogleSend(accounts), lang)?;
    show_deadline(lang, r.expires_at_utc_ms())?;
    let until = Instant::now() + Duration::from_secs(300);
    loop {
        match r.state() {
            ScopeRefreshProgress::Ready => {
                println!("{}", resource(lang, "guided_ready"));
                return Ok(json!({
                    "state":"ready",
                    "account_id":account.account_id()
                }));
            }
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

fn wait_google_manage(account: GoogleReadAccount, lang: Lang) -> Result<Value, ErrorCode> {
    println!("{}", resource(lang, "refresh"));
    let accounts =
        hantong_ipc_client::google_mail::default_registered_account_union(&account.account_id())?;
    let existing_google = accounts
        .iter()
        .filter(|id| id.starts_with("google_") && *id != &account.account_id())
        .cloned()
        .collect::<Vec<_>>();
    ensure_manage_google_accounts(&existing_google, lang)?;
    let accounts = ManageAccounts::parse(&accounts)?;
    let mut r =
        super::terminal_workflows::scope(ScopeRefreshCommand::RequestManage(accounts), lang)?;
    show_deadline(lang, r.expires_at_utc_ms())?;
    let until = Instant::now() + Duration::from_secs(300);
    loop {
        match r.state() {
            ScopeRefreshProgress::Ready => {
                if !hantong_ipc_client::google_mail::verify_default_manage_accounts(
                    &account.account_id(),
                )? {
                    return Err(ErrorCode::ScopeDenied);
                }
                println!("{}", resource(lang, "guided_ready"));
                return Ok(json!({
                    "state":"ready",
                    "account_id":account.account_id()
                }));
            }
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
fn finish_personal_login(
    account: GoogleReadAccount,
    profile: GoogleLocalProfile,
    lang: Lang,
    details: bool,
    verify: &mut impl FnMut(&str, GoogleLocalProfile) -> Result<bool, ErrorCode>,
    fallback: &mut impl FnMut(GoogleReadAccount, Lang, bool) -> Result<Value, ErrorCode>,
) -> Result<Value, ErrorCode> {
    if verify(&account.account_id(), profile)? {
        println!("{}", resource(lang, "guided_ready"));
        return Ok(json!({"state":"ready","account_id":account.account_id()}));
    }
    if matches!(
        profile,
        GoogleLocalProfile::PersonalRead
            | GoogleLocalProfile::PersonalReadSend
            | GoogleLocalProfile::PersonalManage
    ) {
        return fallback(account, lang, details);
    }
    Err(ErrorCode::ScopeDenied)
}
fn resume_flow(
    flow: LoginFlow,
    receipt_profile: GoogleLocalProfile,
) -> Result<LoginFlow, ErrorCode> {
    match flow {
        LoginFlow::Advanced if receipt_profile == GoogleLocalProfile::None => Ok(flow),
        LoginFlow::Advanced => Ok(LoginFlow::Personal(receipt_profile)),
        LoginFlow::Personal(expected)
            if receipt_profile == GoogleLocalProfile::None || receipt_profile == expected =>
        {
            Ok(flow)
        }
        LoginFlow::Personal(_) => Err(ErrorCode::Conflict),
        LoginFlow::ConnectOnly(expected)
            if receipt_profile == GoogleLocalProfile::None || receipt_profile == expected =>
        {
            Ok(flow)
        }
        LoginFlow::ConnectOnly(_) => Err(ErrorCode::Conflict),
    }
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
            if let Some(key) = setup_stage_message(&receipt.state) {
                println!("{}", resource(lang, key));
            }
            #[cfg(unix)]
            if receipt.state == "awaiting_browser" {
                let url = receipt
                    .login_url
                    .as_deref()
                    .ok_or(ErrorCode::InvalidInput)?;
                hantong_platform::google_browser::validate_google_authorization_url(url)?;
                println!("{}\n{}", resource(lang, "browser_url"), url);
                // The same foreground displays the URL when no desktop browser is available.
                match hantong_platform::google_browser::open_google_authorization_url(url) {
                    Ok(()) | Err(ErrorCode::HumanStepRequired) => {}
                    Err(error) => return Err(error),
                }
            }
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
    let flow = resume_flow(flow, receipt.local_profile)?;
    if let LoginFlow::ConnectOnly(_) = flow {
        return Ok(json!({"state":"ready","account_id":account.account_id()}));
    }
    if let LoginFlow::Personal(profile) = flow {
        return finish_personal_login(
            account,
            profile,
            lang,
            details,
            &mut |account, profile| {
                if profile == GoogleLocalProfile::PersonalReadSend {
                    hantong_ipc_client::google_mail::verify_default_personal_accounts(account)
                } else if profile == GoogleLocalProfile::PersonalManage {
                    hantong_ipc_client::google_mail::verify_default_manage_accounts(account)
                } else {
                    hantong_ipc_client::google_mail::verify_default_personal(account, profile)
                }
            },
            &mut |account, lang, details| {
                if profile == GoogleLocalProfile::PersonalRead {
                    continue_account_inner(account, lang, details)
                } else if profile == GoogleLocalProfile::PersonalReadSend {
                    wait_google_send(account, lang)
                } else if profile == GoogleLocalProfile::PersonalManage {
                    wait_google_manage(account, lang)
                } else {
                    Err(ErrorCode::ScopeDenied)
                }
            },
        );
    }
    continue_account_inner(account, lang, details)
}

fn setup_stage_message(state: &str) -> Option<&'static str> {
    match state {
        "awaiting_client" => Some("client"),
        "awaiting_browser" => Some("browser"),
        "awaiting_review" => Some("review"),
        _ => None,
    }
}

#[cfg(test)]
#[test]
fn cli04_google_queued_stage_does_not_repeat_the_opening_message() {
    assert_eq!(setup_stage_message("queued"), None);
    assert_eq!(setup_stage_message("awaiting_browser"), Some("browser"));
    assert_eq!(setup_stage_message("awaiting_review"), Some("review"));
}

pub(super) fn connect_guided(
    lang: Lang,
    _profile: GoogleLocalProfile,
) -> Result<String, ErrorCode> {
    match hantong_platform::load_default_client_profile() {
        Ok(loaded) => {
            let profile =
                hantong_ipc_client::profile::ParsedClientProfile::from_json(loaded.bytes())?;
            if profile.profile() != hantong_ipc_client::profile::ProfileSelector::Default {
                return Err(ErrorCode::ScopeDenied);
            }
            let receipt = super::core_start::account_connect(
                hantong_contracts::account_connect::AccountConnectProvider::Google,
                None,
            )?;
            return receipt.account_id.ok_or(ErrorCode::InternalError);
        }
        Err(ErrorCode::ClientNotRegistered) => {}
        Err(code) => return Err(code),
    }
    // Only initial registration uses bootstrap. An existing Default consumer
    // goes through the signed, process-bound app intent above, never a union.
    let input = GoogleSetupInput {
        access_mode: hantong_contracts::google_write::GoogleAccessMode::Modify,
        local_profile: GoogleLocalProfile::PersonalManage,
        client_id: None,
        display_name: None,
        reauthorize_account: None,
        force_consent: false,
    };
    let value = login_inner(
        GoogleSetupCommand::Request(input),
        lang,
        false,
        LoginFlow::ConnectOnly(GoogleLocalProfile::PersonalManage),
    )?;
    let account = value["account_id"]
        .as_str()
        .ok_or(ErrorCode::InternalError)?;
    GoogleReadAccount::parse(account)?;
    if !hantong_ipc_client::google_mail::verify_default_manage_accounts(account)? {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(account.to_owned())
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
    if hantong_ipc_client::google_mail::verify_default_manage_accounts(&account.account_id())? {
        println!("{}", resource(lang, "guided_ready"));
        return Ok(());
    }
    let receipt = super::core_start::account_connect(
        hantong_contracts::account_connect::AccountConnectProvider::Google,
        Some(&account.account_id()),
    )?;
    if receipt.account_id.as_deref() != Some(account.account_id().as_str()) {
        return Err(ErrorCode::ScopeDenied);
    }
    println!("{}", resource(lang, "guided_ready"));
    Ok(())
}

fn reauthorize_manage_google(account: &str, lang: Lang) -> Result<(), ErrorCode> {
    let account = GoogleReadAccount::parse(account)?;
    let input = GoogleSetupInput {
        access_mode: hantong_contracts::google_write::GoogleAccessMode::Modify,
        local_profile: GoogleLocalProfile::PersonalManage,
        client_id: None,
        display_name: None,
        reauthorize_account: Some(account.account_id().to_owned()),
        force_consent: true,
    };
    let value = login_inner(
        GoogleSetupCommand::Request(input),
        lang,
        false,
        LoginFlow::ConnectOnly(GoogleLocalProfile::PersonalManage),
    )?;
    if value["account_id"].as_str() != Some(&account.account_id()) {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}

/// Reauthorize only the existing Google accounts that still need Modify.
/// Each account's explicit feature choice and OAuth step can activate its own
/// features. The caller subsequently reviews the combined account scope with
/// RequestManage; cancelling that review does not undo earlier feature choices.
pub(super) fn ensure_manage_google_accounts(
    accounts: &[String],
    lang: Lang,
) -> Result<(), ErrorCode> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err(ErrorCode::HumanStepRequired);
    }
    let mut io = super::onboarding::Stdio::new();
    ensure_manage_google_accounts_with_io(accounts, lang, &mut io)
}

fn ensure_manage_google_accounts_with_io(
    accounts: &[String],
    lang: Lang,
    io: &mut dyn super::onboarding::Io,
) -> Result<(), ErrorCode> {
    ensure_manage_google_accounts_flow(
        accounts,
        lang,
        io,
        &mut |account| hantong_ipc_client::google_mail::verify_default_manage_accounts(account),
        &mut |account, lang| reauthorize_manage_google(account, lang),
    )
}

fn ensure_manage_google_accounts_flow(
    accounts: &[String],
    lang: Lang,
    io: &mut dyn super::onboarding::Io,
    verify: &mut impl FnMut(&str) -> Result<bool, ErrorCode>,
    reauthorize: &mut impl FnMut(&str, Lang) -> Result<(), ErrorCode>,
) -> Result<(), ErrorCode> {
    let mut seen = std::collections::BTreeSet::new();
    for account in accounts {
        let account = GoogleReadAccount::parse(account)?;
        if !seen.insert(account.account_id().to_owned()) {
            continue;
        }
        if verify(&account.account_id())? {
            continue;
        }
        io.write(resource(lang, "existing_account_feature_change"))?;
        io.write("\n")?;
        io.write(&account.account_id())?;
        io.write("\n")?;
        io.write(resource(lang, "existing_account_feature_choice"))?;
        io.write("\n")?;
        match io.read_line()? {
            Some(choice) if choice == "1" => reauthorize(&account.account_id(), lang)?,
            Some(choice) if choice == "0" => return Err(ErrorCode::ApprovalDenied),
            None => return Err(ErrorCode::ApprovalDenied),
            Some(_) => return Err(ErrorCode::InvalidInput),
        }
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

pub(super) fn refresh_guided_inbox(account: &str) -> Result<(), ErrorCode> {
    inbox_page(
        account,
        &nonce()?,
        &mut hantong_ipc_client::google_mail::call_default,
        &mut || std::thread::sleep(Duration::from_millis(250)),
    )?;
    Ok(())
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
    fn acc02_google_resume_restores_send_profile_from_receipt_metadata() {
        assert_eq!(
            resume_flow(LoginFlow::Advanced, GoogleLocalProfile::PersonalReadSend),
            Ok(LoginFlow::Personal(GoogleLocalProfile::PersonalReadSend))
        );
        assert_eq!(
            resume_flow(
                LoginFlow::Personal(GoogleLocalProfile::PersonalReadSend),
                GoogleLocalProfile::PersonalRead
            ),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            resume_flow(LoginFlow::Advanced, GoogleLocalProfile::None),
            Ok(LoginFlow::Advanced)
        );
        assert_eq!(
            resume_flow(
                LoginFlow::ConnectOnly(GoogleLocalProfile::PersonalManage),
                GoogleLocalProfile::PersonalManage
            ),
            Ok(LoginFlow::ConnectOnly(GoogleLocalProfile::PersonalManage))
        );
    }
    #[test]
    fn acc02_personal_read_missing_exact_scope_hands_off_to_readonly_access_once() {
        let account = GoogleReadAccount::parse(&format!("google_{}", "a".repeat(64))).unwrap();
        let expected_account_id = account.account_id().to_owned();
        let mut fallback_calls = 0;
        let value = finish_personal_login(
            account,
            GoogleLocalProfile::PersonalRead,
            Lang::Ko,
            false,
            &mut |_, _| Ok(false),
            &mut |selected, lang, details| {
                fallback_calls += 1;
                assert_eq!(selected.account_id(), expected_account_id);
                assert!(matches!(lang, Lang::Ko));
                assert!(!details);
                Ok(json!({"state":"ready","account_id":selected.account_id()}))
            },
        )
        .unwrap();
        assert_eq!(fallback_calls, 1);
        assert_eq!(value["state"], "ready");
    }
    #[test]
    fn acc02_personal_read_exact_scope_does_not_handoff() {
        let account = GoogleReadAccount::parse(&format!("google_{}", "a".repeat(64))).unwrap();
        let value = finish_personal_login(
            account,
            GoogleLocalProfile::PersonalRead,
            Lang::En,
            false,
            &mut |_, _| Ok(true),
            &mut |_, _, _| panic!("exact personal read must not add a scope"),
        )
        .unwrap();
        assert_eq!(value["state"], "ready");
    }
    #[test]
    fn acc02_personal_read_send_missing_scope_hands_off_once() {
        let account = GoogleReadAccount::parse(&format!("google_{}", "a".repeat(64))).unwrap();
        let mut fallback_calls = 0;
        let value = finish_personal_login(
            account,
            GoogleLocalProfile::PersonalReadSend,
            Lang::En,
            false,
            &mut |_, _| Ok(false),
            &mut |selected, _, _| {
                fallback_calls += 1;
                Ok(json!({"state":"ready","account_id":selected.account_id()}))
            },
        )
        .unwrap();
        assert_eq!(fallback_calls, 1);
        assert_eq!(value["state"], "ready");
    }
    #[test]
    fn ai09_personal_manage_missing_scope_uses_manage_scope_fallback() {
        let account = GoogleReadAccount::parse(&format!("google_{}", "a".repeat(64))).unwrap();
        let mut fallback_calls = 0;
        let value = finish_personal_login(
            account,
            GoogleLocalProfile::PersonalManage,
            Lang::En,
            false,
            &mut |_, profile| {
                assert_eq!(profile, GoogleLocalProfile::PersonalManage);
                Ok(false)
            },
            &mut |selected, _, _| {
                fallback_calls += 1;
                Ok(json!({"state":"ready","account_id":selected.account_id()}))
            },
        )
        .unwrap();
        assert_eq!(value["state"], "ready");
        assert_eq!(fallback_calls, 1);
    }
    #[test]
    fn acc02_personal_login_auth_error_does_not_handoff() {
        let account = GoogleReadAccount::parse(&format!("google_{}", "a".repeat(64))).unwrap();
        assert_eq!(
            finish_personal_login(
                account,
                GoogleLocalProfile::PersonalRead,
                Lang::En,
                false,
                &mut |_, _| Err(ErrorCode::AuthRequired),
                &mut |_, _, _| panic!("auth errors must not retry through scope handoff"),
            ),
            Err(ErrorCode::AuthRequired)
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
    fn sync02_cli_gmail_implicit_sync_ids_use_native_randomness_and_exact_account() {
        let account = format!("google_{}", "a".repeat(64));
        let mut calls = 0;
        let params = sync_params(
            json!({"mode":"gmail_page","account_id":account,"limit":20}),
            &mut |tool, params| {
                calls += 1;
                assert_eq!(tool, "mail.folders.list");
                assert_eq!(params, json!({"mode":"gmail_labels","account_id":account}));
                Ok(json!({"sync_version":7}))
            },
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(params["account_id"], account);
        assert_eq!(params["expected_version"], 7);
        assert_eq!(params["label_id"], "INBOX");
        let operation = params["operation_id"].as_str().unwrap();
        let key = params["idempotency_key"].as_str().unwrap();
        assert_ne!(wire_hex::decode_fixed::<32>(operation).unwrap(), [0; 32]);
        assert_ne!(wire_hex::decode_fixed::<32>(key).unwrap(), [0; 32]);
        assert_ne!(operation, key);
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
        let operation = nonce().unwrap();
        assert_ne!(wire_hex::decode_fixed::<32>(&operation).unwrap(), [0; 32]);
        let page = inbox_page(
            &account,
            &operation,
            &mut |tool, p| {
                modes.push((tool.to_owned(), p["mode"].as_str().unwrap().to_owned()));
                assert_eq!(p["account_id"], account);
                Ok(match p["mode"].as_str().unwrap() {
                    "gmail_labels" => json!({"sync_version":3}),
                    "gmail_page" => {
                        assert_eq!(p["operation_id"], operation);
                        assert_eq!(p["idempotency_key"], operation);
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
    fn acc02_personal_read_fallback_adds_selected_account_once_after_readonly_probe() {
        let selected = GoogleReadAccount::parse(&format!("google_{}", "a".repeat(64))).unwrap();
        let selected_id = selected.account_id().to_owned();
        let existing_id = format!("google_{}", "b".repeat(64));
        let grants = RefCell::new(vec![existing_id.clone()]);
        let probes = RefCell::new(0usize);
        let adds = RefCell::new(0usize);
        let published = RefCell::new(None::<String>);
        let value = finish_personal_login(
            selected,
            GoogleLocalProfile::PersonalRead,
            Lang::En,
            false,
            &mut |_, _| Ok(false),
            &mut |account, _, _| {
                ensure_access(
                    account,
                    &mut |id| {
                        *probes.borrow_mut() += 1;
                        Ok(grants.borrow().iter().any(|grant| grant == id))
                    },
                    &mut |_| panic!("an existing client must not register again"),
                    &mut |chosen| {
                        assert_eq!(*grants.borrow(), vec![existing_id.clone()]);
                        *adds.borrow_mut() += 1;
                        grants.borrow_mut().push(chosen.account_id());
                        *published.borrow_mut() = Some(chosen.account_id());
                        Ok(())
                    },
                )?;
                Ok(json!({"state":"ready","account_id":account.account_id()}))
            },
        )
        .unwrap();
        assert_eq!(value["account_id"], selected_id.as_str());
        assert_eq!(*probes.borrow(), 3);
        assert_eq!(*adds.borrow(), 1);
        assert_eq!(*published.borrow(), Some(selected_id.clone()));
        assert_eq!(*grants.borrow(), vec![existing_id, selected_id]);
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

    struct FeatureChoiceIo {
        input: Vec<Option<String>>,
        output: String,
    }

    impl FeatureChoiceIo {
        fn new(input: &[&str]) -> Self {
            Self {
                input: input
                    .iter()
                    .map(|value| Some((*value).to_owned()))
                    .collect(),
                output: String::new(),
            }
        }
    }

    impl crate::onboarding::Io for FeatureChoiceIo {
        fn write(&mut self, text: &str) -> Result<(), ErrorCode> {
            self.output.push_str(text);
            Ok(())
        }

        fn read_line(&mut self) -> Result<Option<String>, ErrorCode> {
            Ok(self.input.pop().flatten())
        }
    }

    #[test]
    fn acc02_existing_google_feature_cancel_and_eof_never_reauthorize() {
        let account = format!("google_{}", "a".repeat(64));
        for input in [&["0"][..], &[][..]] {
            let mut io = FeatureChoiceIo::new(input);
            let mut reauth = 0;
            let result = ensure_manage_google_accounts_flow(
                std::slice::from_ref(&account),
                Lang::En,
                &mut io,
                &mut |_| Ok(false),
                &mut |_, _| {
                    reauth += 1;
                    Ok(())
                },
            );
            assert_eq!(result, Err(ErrorCode::ApprovalDenied));
            assert_eq!(reauth, 0);
        }
    }

    #[test]
    fn acc02_existing_google_feature_choice_reauthorizes_exact_account_once() {
        let account = format!("google_{}", "b".repeat(64));
        let mut io = FeatureChoiceIo::new(&["1"]);
        let mut reauthorized = Vec::new();
        ensure_manage_google_accounts_flow(
            std::slice::from_ref(&account),
            Lang::En,
            &mut io,
            &mut |_| Ok(false),
            &mut |id, _| {
                reauthorized.push(id.to_owned());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(reauthorized, vec![account]);
    }

    #[test]
    fn acc02_existing_google_feature_headless_guard_has_no_effects() {
        let account = format!("google_{}", "c".repeat(64));
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            assert_eq!(
                ensure_manage_google_accounts(&[account], Lang::En),
                Err(ErrorCode::HumanStepRequired)
            );
        }
    }
}
