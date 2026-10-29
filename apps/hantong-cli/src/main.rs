#![forbid(unsafe_code)]

use hantong_contracts::ErrorCode;
use hantong_contracts::imap_setup::{
    ImapSetupCommand, ImapSetupInput, ImapSetupReceipt, ImapSetupTls,
};
use hantong_contracts::registration::{
    RegistrationCommand, RegistrationProgress, RegistrationReceipt,
};
use hantong_contracts::scope_refresh::{
    ScopeRefreshCommand, ScopeRefreshProgress, ScopeRefreshReceipt,
};
use std::env;
use std::io::{self, Write};

const EN_RESOURCES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/en.txt"));
const KO_RESOURCES: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/resources/ko.txt"));

#[derive(Clone, Copy)]
enum Lang {
    En,
    Ko,
}

fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => escaped.push_str(&format!("\\u{:04x}", c as u32)),
            c => escaped.push(c),
        }
    }
    escaped
}

fn resource(lang: Lang, key: &str) -> &'static str {
    let table = match lang {
        Lang::En => EN_RESOURCES,
        Lang::Ko => KO_RESOURCES,
    };
    table
        .lines()
        .find_map(|line| line.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
        .expect("validated CLI resource key")
}

fn message(code: ErrorCode, lang: Lang) -> &'static str {
    match (code, lang) {
        (ErrorCode::ClientNotRegistered, _) => resource(lang, "client_not_registered"),
        (ErrorCode::InvalidInput, _) => resource(lang, "invalid_input"),
        (ErrorCode::CoreUnavailable, _) => resource(lang, "core_unavailable"),
        (ErrorCode::CapabilityUnsupported, _) => resource(lang, "capability_unsupported"),
        (_, _) => resource(lang, "generic_error"),
    }
}

fn help(lang: Lang) -> String {
    format!(
        "mail-hantong / hantong\n{}\n{}\n{}\n{}",
        resource(lang, "usage"),
        resource(lang, "commands"),
        resource(lang, "protected"),
        resource(lang, "help_language")
    )
}

fn error_json(code: ErrorCode, lang: Lang) -> String {
    format!(
        "{{\"schema_version\":1,\"ok\":false,\"error\":{{\"code\":\"{}\",\"message\":\"{}\",\"retryable\":false}},\"items\":[],\"next_cursor\":null,\"account_results\":[],\"completeness\":\"unavailable\",\"warnings\":[],\"observed_at\":null}}",
        code.as_str(),
        json_escape(message(code, lang))
    )
}

fn run(args: impl IntoIterator<Item = String>) -> i32 {
    run_with_boundaries_and_scope(
        args,
        hantong_ipc_client::registration::exchange,
        verify_profile,
        hantong_ipc_client::accounts::list_default_accounts,
        hantong_ipc_client::imap_setup::exchange,
        hantong_ipc_client::scope_refresh::exchange,
    )
}

#[cfg(windows)]
fn verify_profile() -> Result<(), ErrorCode> {
    hantong_ipc_client::verify_default()
}
#[cfg(not(windows))]
fn verify_profile() -> Result<(), ErrorCode> {
    Err(ErrorCode::CapabilityUnsupported)
}

#[cfg(test)]
fn run_with_registration(
    args: impl IntoIterator<Item = String>,
    registration: impl FnMut(RegistrationCommand) -> Result<RegistrationReceipt, ErrorCode>,
) -> i32 {
    run_with_boundaries(args, registration, || {
        panic!("registration test must not verify a profile")
    })
}

fn verification_output(lang: Lang, json: bool) -> String {
    if json {
        "{\"schema_version\":1,\"ok\":true,\"items\":[{\"profile\":\"default\",\"authenticated\":true}],\"next_cursor\":null,\"account_results\":[],\"completeness\":\"complete\",\"warnings\":[],\"observed_at\":null}".to_owned()
    } else {
        resource(lang, "client_verified").to_owned()
    }
}

#[cfg(test)]
fn run_with_boundaries(
    args: impl IntoIterator<Item = String>,
    registration: impl FnMut(RegistrationCommand) -> Result<RegistrationReceipt, ErrorCode>,
    verification: impl FnMut() -> Result<(), ErrorCode>,
) -> i32 {
    run_with_boundaries_and_accounts(args, registration, verification, |_| {
        Err(ErrorCode::CapabilityUnsupported)
    })
}

fn account_output(
    response: &hantong_ipc_client::accounts::AccountListResponse,
    lang: Lang,
    json: bool,
) -> String {
    if json {
        return response.to_json();
    }
    let mut out = String::new();
    for item in response.items() {
        let name = item
            .display_name()
            .or(item.primary_address())
            .unwrap_or(item.account_id());
        let provider = resource(
            lang,
            match item.provider() {
                "gmail" => "provider_gmail",
                "graph" => "provider_graph",
                "imap" => "provider_imap",
                _ => "provider_archive",
            },
        );
        let status = resource(
            lang,
            match item.status() {
                "ready" => "status_ready",
                "connecting" => "status_connecting",
                "reauth_required" => "status_reauth_required",
                "offline" => "status_offline",
                "disabled" => "status_disabled",
                _ => "status_error",
            },
        );
        out.push_str(&format!(
            "{} {}: {} ({}: {}, {}: {})\n",
            resource(lang, "account_number"),
            item.account_number(),
            safe_human(name),
            resource(lang, "provider"),
            provider,
            resource(lang, "status"),
            status
        ));
        if let Some(address) = item.primary_address() {
            out.push_str(&format!(
                "  {}: {}\n",
                resource(lang, "address"),
                safe_human(address)
            ));
        }
        if let Some(capabilities) = item.capabilities() {
            out.push_str(&format!(
                "  {}: {}\n",
                resource(lang, "capabilities"),
                capabilities
                    .iter()
                    .map(|v| safe_human(v))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    out
}

fn safe_human(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\u{2028}'
            | '\u{2029}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{061c}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => format!("\\u{{{:x}}}", c as u32),
            c => c.to_string(),
        })
        .collect()
}

#[cfg(test)]
fn run_with_boundaries_and_accounts(
    args: impl IntoIterator<Item = String>,
    registration: impl FnMut(RegistrationCommand) -> Result<RegistrationReceipt, ErrorCode>,
    verification: impl FnMut() -> Result<(), ErrorCode>,
    account_list: impl FnMut(
        bool,
    )
        -> Result<hantong_ipc_client::accounts::AccountListResponse, ErrorCode>,
) -> i32 {
    run_with_boundaries_and_setup(args, registration, verification, account_list, |_| {
        Err(ErrorCode::CapabilityUnsupported)
    })
}

#[cfg(test)]
fn run_with_boundaries_and_setup(
    args: impl IntoIterator<Item = String>,
    registration: impl FnMut(RegistrationCommand) -> Result<RegistrationReceipt, ErrorCode>,
    verification: impl FnMut() -> Result<(), ErrorCode>,
    account_list: impl FnMut(
        bool,
    )
        -> Result<hantong_ipc_client::accounts::AccountListResponse, ErrorCode>,
    setup: impl FnMut(ImapSetupCommand) -> Result<ImapSetupReceipt, ErrorCode>,
) -> i32 {
    run_with_boundaries_and_scope(
        args,
        registration,
        verification,
        account_list,
        setup,
        |_| Err(ErrorCode::CapabilityUnsupported),
    )
}

#[cfg(test)]
fn run_with_boundaries_and_scope_for_test(
    args: impl IntoIterator<Item = String>,
    scope_refresh: impl FnMut(ScopeRefreshCommand) -> Result<ScopeRefreshReceipt, ErrorCode>,
) -> i32 {
    run_with_boundaries_and_scope(
        args,
        |_| panic!("registration must not run"),
        || panic!("verification must not run"),
        |_| Err(ErrorCode::CapabilityUnsupported),
        |_| Err(ErrorCode::CapabilityUnsupported),
        scope_refresh,
    )
}

fn run_with_boundaries_and_scope(
    args: impl IntoIterator<Item = String>,
    mut registration: impl FnMut(RegistrationCommand) -> Result<RegistrationReceipt, ErrorCode>,
    mut verification: impl FnMut() -> Result<(), ErrorCode>,
    mut account_list: impl FnMut(
        bool,
    )
        -> Result<hantong_ipc_client::accounts::AccountListResponse, ErrorCode>,
    mut setup: impl FnMut(ImapSetupCommand) -> Result<ImapSetupReceipt, ErrorCode>,
    mut scope_refresh: impl FnMut(ScopeRefreshCommand) -> Result<ScopeRefreshReceipt, ErrorCode>,
) -> i32 {
    let mut lang = Lang::En;
    let mut json = false;
    let mut include_capabilities = false;
    let mut want_help = false;
    let mut want_version = false;
    let mut command: Vec<String> = Vec::new();
    let mut setup_options: Vec<(String, String)> = Vec::new();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--lang" => match it.next().as_deref() {
                Some("en") => lang = Lang::En,
                Some("ko") => lang = Lang::Ko,
                _ => return emit_error(ErrorCode::InvalidInput, lang, json),
            },
            "--json" => json = true,
            "--include-capabilities" => include_capabilities = true,
            "--no-input" => {}
            "--client" => {
                if it.next().as_deref().is_none_or(|value| {
                    hantong_ipc_client::profile::ProfileSelector::parse(value).is_err()
                }) {
                    return emit_error(ErrorCode::InvalidInput, lang, json);
                }
            }
            "--host" | "--port" | "--tls" | "--login" | "--name" | "--color" => {
                let Some(value) = it.next() else {
                    return emit_error(ErrorCode::InvalidInput, lang, json);
                };
                setup_options.push((arg, value));
            }
            "--help" | "-h" if command.is_empty() => {
                want_help = true;
            }
            "--version" | "-V" if command.is_empty() => {
                want_version = true;
            }
            value if value.starts_with('-') => {
                return emit_error(ErrorCode::InvalidInput, lang, json);
            }
            value => command.push(value.to_owned()),
        }
    }
    // This option is meaningful only for the account-list action. Reject it
    // before any action branch can authenticate, register, or query the core.
    if include_capabilities
        && !matches!(command.as_slice(), [a, b] if a == "accounts" && b == "list")
    {
        return emit_error(ErrorCode::InvalidInput, lang, json);
    }
    // A help/version flag is a standalone action. Reject combinations before
    // parsing or dispatching any command so native/setup callbacks cannot run.
    if (want_help || want_version) && !command.is_empty() {
        return emit_error(ErrorCode::InvalidInput, lang, json);
    }
    if let Some(command) = parse_setup_command(&command, &setup_options) {
        return match command {
            Ok(Some(value)) => match setup(value) {
                Ok(receipt) => {
                    print_setup(&receipt, lang, json);
                    0
                }
                Err(code) => emit_error(code, lang, json),
            },
            Ok(None) => emit_error(ErrorCode::HumanStepRequired, lang, json),
            Err(code) => emit_error(code, lang, json),
        };
    }
    if !setup_options.is_empty() {
        return emit_error(ErrorCode::InvalidInput, lang, json);
    }
    if want_help && command.is_empty() {
        println!("{}", help(lang));
        return 0;
    }
    if want_version && command.is_empty() {
        println!("hantong {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if matches!(command.as_slice(), [a,b] if a == "clients" && b == "verify") {
        return match verification() {
            Ok(()) => {
                println!("{}", verification_output(lang, json));
                0
            }
            Err(error) => emit_error(error, lang, json),
        };
    }
    match scope_refresh_command(&command) {
        Ok(Some(value)) => {
            return match scope_refresh(value) {
                Ok(receipt) => {
                    print_scope_refresh(&receipt, lang, json);
                    0
                }
                Err(code) => emit_error(code, lang, json),
            };
        }
        Err(code) => return emit_error(code, lang, json),
        Ok(None) => {}
    }
    match registration_command(&command) {
        Ok(Some(value)) => {
            return match registration(value) {
                Ok(receipt) => {
                    print_registration(&receipt, lang, json);
                    0
                }
                Err(code) => emit_error(code, lang, json),
            };
        }
        Err(code) => return emit_error(code, lang, json),
        Ok(None) => {}
    }
    if matches!(command.as_slice(), [a, b] if a == "core" && b == "status") {
        return match hantong_ipc_client::core_status() {
            Ok(status) if json => {
                println!("{}", hantong_ipc_client::readiness_json(&status));
                0
            }
            Ok(_) => {
                println!("{}", resource(lang, "core_status"));
                0
            }
            Err(code) => emit_error(code, lang, json),
        };
    }
    let protected = matches!(command.as_slice(), [a, b] if (a == "accounts" && b == "list") || (a == "send" && b == "execute") || (a == "approvals" && b == "get"));
    if matches!(command.as_slice(), [a,b] if a == "accounts" && b == "list") {
        return match account_list(include_capabilities) {
            Ok(value) => {
                print!("{}", account_output(&value, lang, json));
                0
            }
            Err(code) => emit_error(code, lang, json),
        };
    }
    if protected {
        emit_error(ErrorCode::ClientNotRegistered, lang, json)
    } else {
        emit_error(ErrorCode::InvalidInput, lang, json)
    }
}

fn registration_command(command: &[String]) -> Result<Option<RegistrationCommand>, ErrorCode> {
    match command {
        [a, b] if a == "clients" && b == "register" => Ok(Some(RegistrationCommand::Request)),
        [a, b, id] if a == "clients" && b == "status" => {
            let id = hantong_contracts::wire_hex::decode_fixed::<32>(id)?;
            if id == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            Ok(Some(RegistrationCommand::Status(id)))
        }
        [a, ..] if a == "clients" => Err(ErrorCode::InvalidInput),
        _ => Ok(None),
    }
}

fn scope_refresh_command(command: &[String]) -> Result<Option<ScopeRefreshCommand>, ErrorCode> {
    match command {
        [a, b] if a == "clients" && b == "refresh" => Ok(Some(ScopeRefreshCommand::Request)),
        [a, b, id] if a == "clients" && b == "refresh-status" => {
            let id = hantong_contracts::wire_hex::decode_fixed::<32>(id)?;
            if id == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            Ok(Some(ScopeRefreshCommand::Status(id)))
        }
        _ => Ok(None),
    }
}

fn parse_setup_command(
    command: &[String],
    options: &[(String, String)],
) -> Option<Result<Option<ImapSetupCommand>, ErrorCode>> {
    if command.first().map(String::as_str) != Some("setup") {
        return None;
    }
    if command.get(1).map(String::as_str) != Some("imap") {
        return Some(Err(ErrorCode::InvalidInput));
    }
    match command.get(2).map(String::as_str) {
        Some("status" | "resume") => {
            if command.len() != 4 || !options.is_empty() {
                return Some(Err(ErrorCode::InvalidInput));
            }
            let id = match hantong_contracts::wire_hex::decode_fixed::<32>(&command[3]) {
                Ok(id) if id != [0; 32] => id,
                _ => return Some(Err(ErrorCode::InvalidInput)),
            };
            Some(Ok(Some(if command[2] == "resume" {
                ImapSetupCommand::Resume(id)
            } else {
                ImapSetupCommand::Status(id)
            })))
        }
        Some("request") => {
            if command.len() != 3 {
                return Some(Err(ErrorCode::InvalidInput));
            }
            let mut values = std::collections::BTreeMap::new();
            for (key, value) in options {
                if values.insert(key.as_str(), value.as_str()).is_some() {
                    return Some(Err(ErrorCode::InvalidInput));
                }
            }
            let host = match values.get("--host") {
                Some(v) => (*v).to_owned(),
                None => return Some(Err(ErrorCode::InvalidInput)),
            };
            let port = match values.get("--port").and_then(|v| v.parse::<u16>().ok()) {
                Some(v) if v != 0 => v,
                _ => return Some(Err(ErrorCode::InvalidInput)),
            };
            let tls = match values.get("--tls").copied() {
                Some("implicit_tls") => ImapSetupTls::ImplicitTls,
                Some("starttls_required") => ImapSetupTls::StarttlsRequired,
                _ => return Some(Err(ErrorCode::InvalidInput)),
            };
            let login = match values.get("--login") {
                Some(v) => (*v).to_owned(),
                None => return Some(Err(ErrorCode::InvalidInput)),
            };
            let input = ImapSetupInput::new(
                values.get("--name").map(|v| (*v).to_owned()),
                values.get("--color").map(|v| (*v).to_owned()),
                host,
                port,
                tls,
                login,
            );
            Some(input.map(ImapSetupCommand::Request).map(Some))
        }
        Some("complete" | "approve" | "execute") => Some(Ok(None)),
        Some("reset") => Some(Err(ErrorCode::InvalidInput)),
        _ => Some(Err(ErrorCode::InvalidInput)),
    }
}

fn print_setup(receipt: &ImapSetupReceipt, lang: Lang, json: bool) {
    let id = hantong_contracts::wire_hex::encode(receipt.request_id());
    if json {
        println!(
            "{{\"schema_version\":1,\"ok\":true,\"items\":[{{\"request_id\":\"{}\",\"expires_at_utc_ms\":{},\"state\":\"{}\",\"next_action\":\"{}\"}}],\"next_cursor\":null,\"account_results\":[],\"completeness\":\"complete\",\"warnings\":[],\"observed_at\":null}}",
            id,
            receipt.expires_at_utc_ms(),
            receipt.state().as_str(),
            receipt.next_action().as_str()
        );
    } else {
        let state_key = format!("setup_state_{}", receipt.state().as_str());
        let action_key = format!("setup_action_{}", receipt.next_action().as_str());
        println!(
            "{}\n{}: {}\n{}: {}",
            resource(lang, &state_key),
            resource(lang, "setup_request_id"),
            id,
            resource(lang, "setup_next_action"),
            resource(lang, &action_key)
        );
    }
}

fn print_registration(receipt: &RegistrationReceipt, lang: Lang, json: bool) {
    let id = hantong_contracts::wire_hex::encode(receipt.request_id());
    if json {
        println!(
            "{{\"schema_version\":1,\"ok\":true,\"items\":[{{\"request_id\":\"{}\",\"expires_at_utc_ms\":{},\"state\":\"{}\"}}],\"next_cursor\":null,\"account_results\":[],\"completeness\":\"complete\",\"warnings\":[],\"observed_at\":null}}",
            id,
            receipt.expires_at_utc_ms(),
            receipt.state().as_str()
        );
    } else {
        let key = match receipt.state() {
            RegistrationProgress::Pending => "registration_pending",
            RegistrationProgress::Denied => "registration_denied",
            RegistrationProgress::Expired => "registration_expired",
            RegistrationProgress::Provisioning => "registration_provisioning",
            RegistrationProgress::Ready => "registration_ready",
        };
        println!(
            "{}\n{}: {}",
            resource(lang, key),
            resource(lang, "registration_request_id"),
            id
        );
    }
}

fn print_scope_refresh(receipt: &ScopeRefreshReceipt, lang: Lang, json: bool) {
    let id = hantong_contracts::wire_hex::encode(receipt.refresh_id());
    if json {
        println!(
            "{{\"schema_version\":1,\"ok\":true,\"items\":[{{\"refresh_id\":\"{}\",\"expires_at_utc_ms\":{},\"state\":\"{}\"}}],\"next_cursor\":null,\"account_results\":[],\"completeness\":\"complete\",\"warnings\":[],\"observed_at\":null}}",
            id,
            receipt.expires_at_utc_ms(),
            receipt.state().as_str()
        );
    } else {
        let key = match receipt.state() {
            ScopeRefreshProgress::Pending => "scope_refresh_pending",
            ScopeRefreshProgress::Denied => "scope_refresh_denied",
            ScopeRefreshProgress::Expired => "scope_refresh_expired",
            ScopeRefreshProgress::Provisioning => "scope_refresh_provisioning",
            ScopeRefreshProgress::Ready => "scope_refresh_ready",
            ScopeRefreshProgress::Interrupted => "scope_refresh_interrupted",
        };
        println!(
            "{}\n{}: {}",
            resource(lang, key),
            resource(lang, "scope_refresh_id"),
            id
        );
    }
}

fn emit_error(code: ErrorCode, lang: Lang, json: bool) -> i32 {
    if json {
        println!("{}", error_json(code, lang));
    } else {
        let _ = writeln!(io::stderr(), "{}: {}", code.as_str(), message(code, lang));
    }
    code.exit_code()
}

fn main() {
    let mut args = Vec::new();
    for arg in env::args_os().skip(1) {
        let Some(value) = arg.to_str() else {
            std::process::exit(emit_error(ErrorCode::InvalidInput, Lang::En, false));
        };
        args.push(value.to_owned());
    }
    std::process::exit(run(args));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli15_verify_calls_only_injected_authentication_and_never_prompts() {
        for args in [
            vec!["clients", "verify"],
            vec!["--no-input", "--client", "default", "clients", "verify"],
            vec!["--lang", "ko", "--json", "clients", "verify", "--no-input"],
        ] {
            let calls = std::cell::Cell::new(0);
            assert_eq!(
                run_with_boundaries(
                    args.into_iter().map(String::from),
                    |_| panic!("verification must not request registration"),
                    || {
                        calls.set(calls.get() + 1);
                        Ok(())
                    }
                ),
                0
            );
            assert_eq!(calls.get(), 1);
        }
    }
    #[test]
    fn cli15_verify_preserves_errors_and_rejects_arbitrary_selectors() {
        for error in [
            ErrorCode::ClientNotRegistered,
            ErrorCode::AuthRequired,
            ErrorCode::ScopeDenied,
            ErrorCode::CapabilityUnsupported,
        ] {
            assert_eq!(
                run_with_boundaries(
                    ["--no-input", "--json", "clients", "verify"].map(String::from),
                    |_| panic!("must not register"),
                    || Err(error)
                ),
                error.exit_code()
            );
        }
        for args in [
            vec!["--client", "admin", "clients", "verify"],
            vec!["--client", "../default", "clients", "verify"],
            vec!["--client", "D:/default.json", "clients", "verify"],
            vec!["clients", "verify", "yes"],
            vec!["clients", "verify", "--approve"],
        ] {
            assert_eq!(
                run_with_boundaries(
                    args.into_iter().map(String::from),
                    |_| panic!("invalid input reached registration"),
                    || panic!("invalid input reached verification")
                ),
                2
            );
        }
    }
    #[test]
    fn cli02_verify_json_is_fixed_and_language_neutral_without_identity_details() {
        let expected = "{\"schema_version\":1,\"ok\":true,\"items\":[{\"profile\":\"default\",\"authenticated\":true}],\"next_cursor\":null,\"account_results\":[],\"completeness\":\"complete\",\"warnings\":[],\"observed_at\":null}";
        assert_eq!(verification_output(Lang::En, true), expected);
        assert_eq!(verification_output(Lang::Ko, true), expected);
        assert_eq!(
            verification_output(Lang::En, false),
            "Default client profile authenticated."
        );
        assert_eq!(
            verification_output(Lang::Ko, false),
            "기본 클라이언트 프로필 인증을 확인했습니다."
        );
    }
    #[test]
    fn cli15_registration_commands_request_only_and_never_accept_approval() {
        let calls = std::cell::Cell::new(0);
        let code = run_with_registration(
            [
                "--lang",
                "en",
                "--no-input",
                "--client",
                "default",
                "clients",
                "register",
            ]
            .map(String::from),
            |command| {
                assert_eq!(command, RegistrationCommand::Request);
                calls.set(calls.get() + 1);
                Err(ErrorCode::HumanStepRequired)
            },
        );
        assert_eq!(code, 24);
        assert_eq!(calls.get(), 1);
        let id = hantong_contracts::wire_hex::encode(&[1; 32]);
        let code = run_with_registration(["clients", "status", &id].map(String::from), |command| {
            assert_eq!(command, RegistrationCommand::Status([1; 32]));
            Err(ErrorCode::NotFound)
        });
        assert_eq!(code, 16);
        for args in [
            vec!["clients", "approve"],
            vec!["clients", "register", "yes"],
            vec!["clients", "status", "bad"],
            vec!["--client", "../default", "clients", "register"],
            vec!["--client", "custom", "clients", "register"],
        ] {
            assert_eq!(
                run_with_registration(args.into_iter().map(String::from), |_| panic!(
                    "invalid input reached registration"
                )),
                2
            );
        }
    }
    #[test]
    fn cli16_error_codes_are_unique_and_stable() {
        let names: Vec<_> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(names.len(), sorted.len());
        assert_eq!(ErrorCode::SendUnknown.exit_code(), 17);
    }
    #[test]
    fn cli04_protected_command_never_grants_approval() {
        assert_eq!(run(["send", "execute"].into_iter().map(String::from)), 22);
    }

    #[test]
    fn cli01_accounts_list_uses_injected_validated_response_and_capability_flag() {
        let raw = br#"{"jsonrpc":"2.0","id":"accounts-list-1","result":{"schema_version":1,"ok":true,"items":[{"account_id":"a","provider":"imap","primary_address":null,"display_name":"A","color":null,"account_number":1,"status":"ready"}],"next_cursor":null,"account_results":[{"account_id":"a","status":"complete","items":[{"account_id":"a","provider":"imap","primary_address":null,"display_name":"A","color":null,"account_number":1,"status":"ready"}]}],"completeness":"complete","warnings":[],"observed_at":"2026-01-01T00:00:00Z","metadata":{"source":"local_store","cached":true}}}"#;
        let response = hantong_ipc_client::accounts::parse_account_list_response(
            raw,
            "accounts-list-1",
            &["a".into()],
            false,
        )
        .unwrap();
        let calls = std::cell::Cell::new(false);
        let code = run_with_boundaries_and_accounts(
            ["--json", "--include-capabilities", "accounts", "list"].map(String::from),
            |_| panic!("registration must not run"),
            || panic!("verification must not run"),
            |include| {
                calls.set(include);
                Ok(response.clone())
            },
        );
        assert_eq!(code, 0);
        assert!(calls.get());
    }

    #[test]
    fn cli01_human_account_fields_escape_directional_and_line_separator_controls() {
        assert_eq!(
            safe_human("한통\u{202e}mail\u{2028}x"),
            "한통\\u{202e}mail\\u{2028}x"
        );
    }

    #[test]
    fn cli01_accounts_list_propagates_unsupported_and_rejects_flag_on_other_commands() {
        let unsupported = run_with_boundaries_and_accounts(
            ["accounts", "list"].map(String::from),
            |_| panic!("registration must not run"),
            || panic!("verification must not run"),
            |_| Err(ErrorCode::CapabilityUnsupported),
        );
        assert_eq!(unsupported, ErrorCode::CapabilityUnsupported.exit_code());

        let invalid = run_with_boundaries_and_accounts(
            ["--include-capabilities", "send", "execute"].map(String::from),
            |_| panic!("registration must not run"),
            || panic!("verification must not run"),
            |_| panic!("account list must not run"),
        );
        assert_eq!(invalid, ErrorCode::InvalidInput.exit_code());
    }

    #[test]
    fn cli01_capability_flag_rejects_every_non_account_action_before_callbacks() {
        let request_id = hantong_contracts::wire_hex::encode(&[1; 32]);
        let commands: Vec<Vec<String>> = vec![
            vec!["--include-capabilities", "clients", "register"],
            vec!["clients", "status", &request_id, "--include-capabilities"],
            vec!["--include-capabilities", "clients", "verify"],
            vec!["core", "status", "--include-capabilities"],
            vec!["--include-capabilities", "send", "execute"],
            vec!["approvals", "get", "--include-capabilities"],
            vec!["--include-capabilities", "--help"],
        ]
        .into_iter()
        .map(|args| args.into_iter().map(String::from).collect())
        .collect();
        for args in commands {
            let registrations = std::cell::Cell::new(0);
            let verifications = std::cell::Cell::new(0);
            let accounts = std::cell::Cell::new(0);
            let code = run_with_boundaries_and_accounts(
                args,
                |_| {
                    registrations.set(registrations.get() + 1);
                    Err(ErrorCode::InternalError)
                },
                || {
                    verifications.set(verifications.get() + 1);
                    Err(ErrorCode::InternalError)
                },
                |_| {
                    accounts.set(accounts.get() + 1);
                    Err(ErrorCode::InternalError)
                },
            );
            assert_eq!(code, ErrorCode::InvalidInput.exit_code());
            assert_eq!(registrations.get(), 0);
            assert_eq!(verifications.get(), 0);
            assert_eq!(accounts.get(), 0);
        }
    }

    #[test]
    fn cli01_account_json_is_language_neutral_and_human_labels_are_localized() {
        let raw = br#"{"jsonrpc":"2.0","id":"accounts-list-1","result":{"schema_version":1,"ok":true,"items":[{"account_id":"a","provider":"imap","primary_address":null,"display_name":"\uD55C\uD1B5","color":null,"account_number":1,"status":"ready"}],"next_cursor":null,"account_results":[{"account_id":"a","status":"complete","items":[{"account_id":"a","provider":"imap","primary_address":null,"display_name":"\uD55C\uD1B5","color":null,"account_number":1,"status":"ready"}]}],"completeness":"complete","warnings":[],"observed_at":"2026-01-01T00:00:00Z","metadata":{"source":"local_store","cached":true}}}"#;
        let response = hantong_ipc_client::accounts::parse_account_list_response(
            raw,
            "accounts-list-1",
            &["a".into()],
            false,
        )
        .unwrap();
        assert_eq!(
            account_output(&response, Lang::En, true),
            account_output(&response, Lang::Ko, true)
        );
        assert!(account_output(&response, Lang::En, false).contains("Account"));
        assert!(account_output(&response, Lang::Ko, false).contains("계정"));
    }

    #[test]
    fn cli01_human_output_includes_capabilities_and_localizes_status() {
        let raw = br#"{"jsonrpc":"2.0","id":"accounts-list-1","result":{"schema_version":1,"ok":true,"items":[{"account_id":"a","provider":"imap","primary_address":"u@example.test","display_name":"A","color":null,"account_number":1,"status":"reauth_required","capabilities":["mail.read","mail.send"]}],"next_cursor":null,"account_results":[{"account_id":"a","status":"complete","items":[{"account_id":"a","provider":"imap","primary_address":"u@example.test","display_name":"A","color":null,"account_number":1,"status":"reauth_required","capabilities":["mail.read","mail.send"]}]}],"completeness":"complete","warnings":[],"observed_at":"2026-01-01T00:00:00Z","metadata":{"source":"local_store","cached":true}}}"#;
        let response = hantong_ipc_client::accounts::parse_account_list_response(
            raw,
            "accounts-list-1",
            &["a".into()],
            true,
        )
        .unwrap();
        let en = account_output(&response, Lang::En, false);
        let ko = account_output(&response, Lang::Ko, false);
        assert!(
            en.contains("Capabilities")
                && en.contains("mail.read")
                && en.contains("Reauthentication required")
        );
        assert!(ko.contains("기능") && ko.contains("재인증 필요") && ko.contains("주소"));
    }

    #[test]
    fn cli01_unknown_input_is_rejected_without_reflection() {
        let secret = "fixture-secret-한통";
        let code = run_with_boundaries_and_accounts(
            ["--mystery", secret].map(String::from),
            |_| panic!("registration must not run"),
            || panic!("verification must not run"),
            |_| panic!("account list must not run"),
        );
        assert_eq!(code, ErrorCode::InvalidInput.exit_code());
    }

    #[test]
    fn cli_setup_request_and_status_use_injected_callback_without_secret_input() {
        let request_id = [7u8; 32];
        let receipt = ImapSetupReceipt::new(
            request_id,
            1_800_000_000_000,
            hantong_contracts::imap_setup::ImapSetupState::Pending,
            hantong_contracts::imap_setup::ImapSetupNextAction::HelperRequired,
        )
        .unwrap();
        let calls = std::cell::Cell::new(0);
        let request_args = [
            "--no-input",
            "setup",
            "imap",
            "request",
            "--host",
            "imap.example.test",
            "--port",
            "993",
            "--tls",
            "implicit_tls",
            "--login",
            "user@example.test",
            "--name",
            "Work",
            "--color",
            "#aBc123",
        ];
        let code = run_with_boundaries_and_setup(
            request_args.map(String::from),
            |_| panic!("registration must not run"),
            || panic!("verification must not run"),
            |_| panic!("account list must not run"),
            |command| {
                calls.set(calls.get() + 1);
                match command {
                    ImapSetupCommand::Request(input) => {
                        assert_eq!(input.host(), "imap.example.test");
                        assert_eq!(input.port(), 993);
                        assert_eq!(input.login_name(), "user@example.test");
                    }
                    ImapSetupCommand::Status(_) | ImapSetupCommand::Resume(_) => {
                        panic!("unexpected status or resume")
                    }
                }
                Ok(receipt)
            },
        );
        assert_eq!(code, 0);
        assert_eq!(calls.get(), 1);

        let id = hantong_contracts::wire_hex::encode(&request_id);
        let code = run_with_boundaries_and_setup(
            ["setup", "imap", "status", &id].map(String::from),
            |_| panic!("registration must not run"),
            || panic!("verification must not run"),
            |_| panic!("account list must not run"),
            |command| {
                assert!(matches!(command, ImapSetupCommand::Status(id) if id == request_id));
                Ok(receipt)
            },
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn cli_setup_rejects_duplicate_missing_secret_or_resume_commands_before_callback() {
        for args in [
            vec![
                "setup",
                "imap",
                "request",
                "--host",
                "a",
                "--host",
                "b",
                "--port",
                "993",
                "--tls",
                "implicit_tls",
                "--login",
                "u",
            ],
            vec![
                "setup",
                "imap",
                "request",
                "--host",
                "a",
                "--port",
                "993",
                "--tls",
                "implicit_tls",
            ],
            vec![
                "setup",
                "imap",
                "request",
                "--host",
                "a",
                "--port",
                "993",
                "--tls",
                "implicit_tls",
                "--login",
                "u",
                "--password",
                "secret",
            ],
            vec!["setup", "imap", "resume"],
            vec!["setup", "imap", "complete"],
        ] {
            let expected = if args.get(2) == Some(&"complete") {
                ErrorCode::HumanStepRequired.exit_code()
            } else {
                ErrorCode::InvalidInput.exit_code()
            };
            let calls = std::cell::Cell::new(0);
            let code = run_with_boundaries_and_setup(
                args.into_iter().map(String::from),
                |_| panic!("registration must not run"),
                || panic!("verification must not run"),
                |_| panic!("account list must not run"),
                |_| {
                    calls.set(calls.get() + 1);
                    Err(ErrorCode::InternalError)
                },
            );
            assert_eq!(calls.get(), 0);
            assert_eq!(code, expected);
        }
    }

    #[test]
    fn setup01_cli_resume_uses_parent_id_and_requires_no_terminal_secret() {
        use hantong_contracts::imap_setup::{ImapSetupNextAction, ImapSetupState};
        let parent = hantong_contracts::wire_hex::encode(&[3; 32]);
        for lang in ["en", "ko"] {
            let calls = std::cell::Cell::new(0);
            let code = run_with_boundaries_and_setup(
                [
                    "--lang",
                    lang,
                    "--json",
                    "--no-input",
                    "setup",
                    "imap",
                    "resume",
                    &parent,
                ]
                .map(String::from),
                |_| panic!("registration"),
                || panic!("verify"),
                |_| panic!("accounts"),
                |command| {
                    assert!(matches!(command,ImapSetupCommand::Resume(id) if id==[3;32]));
                    calls.set(calls.get() + 1);
                    Ok(ImapSetupReceipt::new(
                        [4; 32],
                        10,
                        ImapSetupState::Pending,
                        ImapSetupNextAction::HelperRequired,
                    )
                    .unwrap())
                },
            );
            assert_eq!(code, 0);
            assert_eq!(calls.get(), 1);
        }
        for args in [
            vec!["setup", "imap", "resume", "bad"],
            vec!["setup", "imap", "resume", &parent, "--host", "new.example"],
            vec!["setup", "imap", "resume", &parent, "--approved"],
            vec!["setup", "imap", "resume", &parent, "--include-capabilities"],
        ] {
            let code = run_with_boundaries_and_setup(
                args.into_iter().map(String::from),
                |_| panic!("registration"),
                || panic!("verify"),
                |_| panic!("accounts"),
                |_| panic!("invalid resume invoked"),
            );
            assert_eq!(code, ErrorCode::InvalidInput.exit_code());
        }
    }

    #[test]
    fn cli_rejects_help_or_version_with_command_before_callbacks() {
        let status_id = hantong_contracts::wire_hex::encode(&[1u8; 32]);
        let cases = [
            vec![
                "--help",
                "setup",
                "imap",
                "request",
                "--host",
                "imap.example.test",
                "--port",
                "993",
                "--tls",
                "implicit_tls",
                "--login",
                "user@example.test",
            ],
            vec!["setup", "imap", "status", status_id.as_str(), "--version"],
            vec!["--version", "clients", "register"],
        ];
        for args in cases {
            let registrations = std::cell::Cell::new(0);
            let verifications = std::cell::Cell::new(0);
            let accounts = std::cell::Cell::new(0);
            let setups = std::cell::Cell::new(0);
            let code = run_with_boundaries_and_setup(
                args.into_iter().map(String::from),
                |_| {
                    registrations.set(registrations.get() + 1);
                    Err(ErrorCode::InternalError)
                },
                || {
                    verifications.set(verifications.get() + 1);
                    Err(ErrorCode::InternalError)
                },
                |_| {
                    accounts.set(accounts.get() + 1);
                    Err(ErrorCode::InternalError)
                },
                |_| {
                    setups.set(setups.get() + 1);
                    Err(ErrorCode::InternalError)
                },
            );
            assert_eq!(code, ErrorCode::InvalidInput.exit_code());
            assert_eq!(registrations.get(), 0);
            assert_eq!(verifications.get(), 0);
            assert_eq!(accounts.get(), 0);
            assert_eq!(setups.get(), 0);
        }
    }

    #[test]
    fn cli15_scope_refresh_request_and_status_use_only_opaque_progress() {
        let calls = std::cell::RefCell::new(Vec::new());
        let code = run_with_boundaries_and_scope_for_test(
            ["--json", "--no-input", "clients", "refresh"].map(String::from),
            |command| {
                calls.borrow_mut().push(command);
                Ok(ScopeRefreshReceipt::new([1; 32], 123, ScopeRefreshProgress::Pending).unwrap())
            },
        );
        assert_eq!(code, 0);
        assert_eq!(*calls.borrow(), vec![ScopeRefreshCommand::Request]);

        let refresh_id = hantong_contracts::wire_hex::encode(&[2; 32]);
        let code = run_with_boundaries_and_scope_for_test(
            [
                "--lang",
                "ko",
                "clients",
                "refresh-status",
                refresh_id.as_str(),
            ]
            .map(String::from),
            |command| {
                assert_eq!(command, ScopeRefreshCommand::Status([2; 32]));
                Ok(
                    ScopeRefreshReceipt::new([2; 32], 123, ScopeRefreshProgress::Interrupted)
                        .unwrap(),
                )
            },
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn cli15_scope_refresh_rejects_approval_and_help_mixes_before_callback() {
        for args in [
            vec!["--include-capabilities", "clients", "refresh"],
            vec!["--help", "clients", "refresh"],
            vec!["clients", "refresh", "--version"],
            vec!["clients", "refresh-status", "bad"],
        ] {
            let calls = std::cell::Cell::new(0);
            let code =
                run_with_boundaries_and_scope_for_test(args.into_iter().map(String::from), |_| {
                    calls.set(calls.get() + 1);
                    Err(ErrorCode::InternalError)
                });
            assert_eq!(code, ErrorCode::InvalidInput.exit_code());
            assert_eq!(calls.get(), 0);
        }
    }

    #[test]
    fn cli06_core_json_is_language_neutral() {
        let status = hantong_ipc_client::Readiness {
            transport_ready: true,
            mail_ready: false,
            authenticated: false,
            protocol_version: 1,
        };
        let en = hantong_ipc_client::readiness_json(&status);
        let ko = hantong_ipc_client::readiness_json(&status);
        assert_eq!(en, ko);
        assert!(en.contains("\"schema_version\":1"));
        assert!(en.contains("\"completeness\":\"complete\""));
    }
}
