#![forbid(unsafe_code)]

use hantong_contracts::ErrorCode;
use hantong_contracts::registration::{
    RegistrationCommand, RegistrationProgress, RegistrationReceipt,
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
    run_with_boundaries(
        args,
        hantong_ipc_client::registration::exchange,
        verify_profile,
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

fn run_with_boundaries(
    args: impl IntoIterator<Item = String>,
    mut registration: impl FnMut(RegistrationCommand) -> Result<RegistrationReceipt, ErrorCode>,
    mut verification: impl FnMut() -> Result<(), ErrorCode>,
) -> i32 {
    let mut lang = Lang::En;
    let mut json = false;
    let mut want_help = false;
    let mut want_version = false;
    let mut command: Vec<String> = Vec::new();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--lang" => match it.next().as_deref() {
                Some("en") => lang = Lang::En,
                Some("ko") => lang = Lang::Ko,
                _ => return emit_error(ErrorCode::InvalidInput, lang, json),
            },
            "--json" => json = true,
            "--no-input" => {}
            "--client" => {
                if it.next().as_deref().is_none_or(|value| {
                    hantong_ipc_client::profile::ProfileSelector::parse(value).is_err()
                }) {
                    return emit_error(ErrorCode::InvalidInput, lang, json);
                }
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
