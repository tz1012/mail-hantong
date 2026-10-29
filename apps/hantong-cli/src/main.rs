#![forbid(unsafe_code)]

use hantong_contracts::ErrorCode;
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
                if it
                    .next()
                    .as_deref()
                    .is_none_or(|value| value.starts_with('-'))
                {
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
