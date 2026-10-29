#![forbid(unsafe_code)]

use hantong_contracts::ErrorCode;
use std::io::{self, Write};

#[cfg(all(test, windows))]
mod authentication_tests;
#[cfg(windows)]
mod execution_adapter;
#[cfg(windows)]
mod folder_worker;
#[cfg(windows)]
mod identity;
#[cfg(windows)]
mod registration;
#[cfg(windows)]
mod scope_refresh;
#[cfg(windows)]
mod setup_imap;
#[cfg(windows)]
mod storage;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lang {
    En,
    Ko,
}

fn resource(lang: Lang, key: &str) -> &'static str {
    let table = match lang {
        Lang::En => include_str!("../resources/en.txt"),
        Lang::Ko => include_str!("../resources/ko.txt"),
    };
    table
        .lines()
        .find_map(|line| {
            line.split_once('=')
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v)
        })
        .expect("validated core resource")
}

fn emit_error(code: ErrorCode, lang: Lang) -> i32 {
    let key = match code {
        ErrorCode::InvalidInput => "invalid_input",
        ErrorCode::Conflict => "already_running",
        ErrorCode::CapabilityUnsupported => "unsupported",
        _ => "unavailable",
    };
    let _ = writeln!(io::stderr(), "{}: {}", code.as_str(), resource(lang, key));
    code.exit_code()
}

#[cfg(any(windows, test))]
use hantong_application::{DiagnosticEvent, EventKind, LogError, Severity};

#[cfg(any(windows, test))]
fn event_line(kind: EventKind, utc_ms: i64) -> Result<String, LogError> {
    DiagnosticEvent::new(kind, Severity::Info, utc_ms)?.to_json_line()
}

#[cfg(windows)]
fn event(kind: EventKind) {
    let Ok(elapsed) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else {
        return;
    };
    let Ok(utc_ms) = i64::try_from(elapsed.as_millis()) else {
        return;
    };
    if let Ok(line) = event_line(kind, utc_ms) {
        let _ = io::stderr().write_all(line.as_bytes());
    }
}

#[cfg(windows)]
fn start() -> Result<(), ErrorCode> {
    let directories = hantong_platform::app_directories()?;
    // Keep the encrypted store and exclusive file locks alive until transport
    // shutdown. Any schema, key or persisted-config failure precedes readiness.
    let store = storage::initialize_storage(&directories)?;
    let server_identity = identity::initialize_server_identity(&store)?;
    let server_instance = hantong_platform::random_nonce()?;
    let mut read_policy = hantong_policy::ReadPolicy::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::InternalError)?;
    runtime.block_on(async {
        let mut interrupt =
            tokio::signal::windows::ctrl_c().map_err(|_| ErrorCode::InternalError)?;
        let mut terminate =
            tokio::signal::windows::ctrl_break().map_err(|_| ErrorCode::InternalError)?;
        let shutdown = async move {
            let signal =
                tokio::select! { value=interrupt.recv()=>value, value=terminate.recv()=>value };
            signal.ok_or(ErrorCode::InternalError)?;
            event(EventKind::CoreStopping);
            Ok(())
        };
        let (registration, registration_worker) =
            registration::coordinator(&store, &server_identity, &directories.secrets);
        let (folder_worker_handle, folder_worker) =
            folder_worker::coordinator_for_store(&store, &directories.secrets);
        let server = hantong_ipc_server::authenticated_wire::serve_until_with_setup(
            shutdown,
            || event(EventKind::CoreTransportReady),
            &server_identity,
            server_instance,
            |id| execution_adapter::load_registration(&store, id),
            |session, request| {
                execution_adapter::execute(
                    &store,
                    &mut read_policy,
                    &folder_worker_handle,
                    session,
                    request,
                )
            },
            |command| registration.setup_command(command),
        );
        tokio::select! {
            biased;
            result=registration_worker.run()=>{result?;return Err(ErrorCode::InternalError);},
            result=folder_worker.run()=>{result?;return Err(ErrorCode::InternalError);},
            result=server=>result?,
        }
        event(EventKind::CoreStopped);
        Ok(())
    })
}

#[cfg(not(windows))]
fn start() -> Result<(), ErrorCode> {
    Err(ErrorCode::CapabilityUnsupported)
}

fn run(args: impl IntoIterator<Item = std::ffi::OsString>) -> i32 {
    let mut lang = Lang::En;
    let mut command = None;
    let mut it = args.into_iter();
    while let Some(value) = it.next() {
        let Some(value) = value.to_str() else {
            return emit_error(ErrorCode::InvalidInput, lang);
        };
        match value {
            "--lang" => match it.next().as_deref().and_then(std::ffi::OsStr::to_str) {
                Some("en") => lang = Lang::En,
                Some("ko") => lang = Lang::Ko,
                _ => return emit_error(ErrorCode::InvalidInput, lang),
            },
            "run" | "--help" | "-h" | "--version" | "-V" if command.is_none() => {
                command = Some(value.to_owned())
            }
            _ => return emit_error(ErrorCode::InvalidInput, lang),
        }
    }
    match command.as_deref() {
        Some("--help" | "-h") => {
            println!(
                "mail-hantong-core\n{}\n{}",
                resource(lang, "usage"),
                resource(lang, "description")
            );
            0
        }
        Some("--version" | "-V") => {
            println!("mail-hantong-core {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("run") => match start() {
            Ok(()) => 0,
            Err(code) => emit_error(code, lang),
        },
        _ => emit_error(ErrorCode::InvalidInput, lang),
    }
}

fn main() {
    std::process::exit(run(std::env::args_os().skip(1)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec08_lifecycle_logs_have_no_user_controlled_fields() {
        for value in [
            EventKind::CoreTransportReady,
            EventKind::CoreStopping,
            EventKind::CoreStopped,
        ] {
            let line = event_line(value, 1_700_000_000_000).unwrap();
            assert!(line.contains("\"observed_at_utc_ms\":1700000000000"));
            assert_eq!(line.chars().filter(|c| *c == '\n').count(), 1);
            assert!(!line.contains('@'));
            assert!(!line.contains("token"));
            assert!(!line.contains("body"));
            assert!(!line.contains("subject"));
        }
    }
}
