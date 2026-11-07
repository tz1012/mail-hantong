#![forbid(unsafe_code)]

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod account_connect;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod account_connect_existing;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod account_connect_frontend;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod account_connect_generic;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod account_connect_rpc;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod account_connect_service;

use hantong_contracts::ErrorCode;
use std::io::{self, Write};

#[cfg(all(test, windows))]
mod authentication_tests;
#[cfg(windows)]
mod execution_adapter;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod folder_worker;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod google_mutation;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod google_worker;
#[cfg(windows)]
mod identity;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod imap_trash;
#[cfg(target_os = "linux")]
mod linux_update_v2;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod mail_events;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod receive_worker;
#[cfg(windows)]
mod registration;
#[cfg(windows)]
mod scope_refresh;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod send_mail;
#[cfg(windows)]
mod setup_google;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod setup_imap;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod setup_smtp;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod storage;
#[cfg(windows)]
mod terminal_google;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod terminal_review;
#[cfg(windows)]
mod terminal_review_frontend;
#[cfg(windows)]
mod terminal_secret_input;
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod test_support;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod uid_worker;
#[cfg(all(test, target_os = "linux"))]
mod unix_authentication_tests;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_core_runtime;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_execution_adapter;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_google;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod unix_identity;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_registered_profiles;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_registration;
#[cfg(unix)]
mod unix_relay_helper;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_scope_refresh;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_setup;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix_stop;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod unix_terminal_review_frontend;

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
    let directories = startup_step("directories", hantong_platform::app_directories())?;
    // Prepare and pin the profile roots before Secrets/Store can create the
    // product directory with inherited ACLs. This guard remains alive for the
    // complete core lifetime and never publishes or approves a profile.
    let _profile_directories = startup_step(
        "profiles",
        hantong_approval_windows::prepare_profile_directories(&directories),
    )?;
    // Validate an optional release-owned Google Desktop asset inside Secrets
    // before opening the persisted store. The returned credential remains
    // opaque and is intentionally discarded here; the OAuth setup path loads
    // it again only when a user explicitly starts Google account connection.
    drop(startup_step(
        "publisher",
        hantong_secrets::google::load_installed_publisher_configuration(),
    )?);
    // Storage leaves must not depend on the launching token's default DACL.
    // Existing leaves are validated and pinned, never silently repaired.
    let _storage_directories = startup_step(
        "storage",
        hantong_approval_windows::prepare_storage_directories(&directories),
    )?;
    // Keep the encrypted store and exclusive file locks alive until transport
    // shutdown. Any schema, key or persisted-config failure precedes readiness.
    let store = startup_step("storage", storage::initialize_storage(&directories))?;
    let server_identity = startup_step("identity", identity::initialize_server_identity(&store))?;
    startup_step(
        "account_grant_recovery",
        account_connect::recover_startup(&store, server_identity.public_key()?),
    )?;
    let server_instance = startup_step("instance", hantong_platform::random_nonce())?;
    let read_policy = std::rc::Rc::new(std::cell::RefCell::new(hantong_policy::ReadPolicy::new()));
    let runtime = startup_step(
        "runtime",
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ErrorCode::InternalError),
    )?;
    startup_step(
        "transport",
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
            let (registration, registration_worker) = registration::coordinator_with_policy(
                &store,
                &server_identity,
                &directories.secrets,
                read_policy.clone(),
            );
            let shared_slot = std::rc::Rc::new(std::cell::Cell::new(None));
            let (folder_worker_handle, folder_worker) = folder_worker::coordinator_for_store(
                &store,
                &directories.secrets,
                shared_slot.clone(),
            );
            let (uid_worker_handle, uid_worker) = uid_worker::coordinator_for_store(
                &store,
                &directories.secrets,
                shared_slot.clone(),
                server_instance,
            );
            let (google_handle, google_worker) =
                google_worker::coordinator(&store, &directories.secrets)?;
            let connect_public_key=server_identity.public_key()?;
            let (connect_state,connect_worker)=account_connect_service::State::new();
            let server = hantong_ipc_server::authenticated_wire::serve_until_with_setup_context(
                // All provider jobs have their own account-bound durable claims.
                shutdown,
                || event(EventKind::CoreTransportReady),
                &server_identity,
                server_instance,
                |id| execution_adapter::load_registration(&store, id),
                |session, request| {
                    if account_connect_rpc::is_method(&request.method) {
                        return account_connect_rpc::windows(&connect_state,&store,&directories.secrets,connect_public_key,registration.smtp_sessions(),session,request);
                    }
                    execution_adapter::execute_with_google(
                        &store,
                        &mut read_policy.borrow_mut(),
                        &folder_worker_handle,
                        Some(&uid_worker_handle),
                        &registration,
                        &google_handle,
                        session,
                        request,
                    )
                },
                |context, command| {
                    let capture=registration.submitted_setup_capture(&command);
                    let peer=account_connect::ObservedPeer::Windows(context.peer().clone());
                    if let Some(result)=connect_state.existing_review(&store,&directories.secrets,connect_public_key,&peer,&command) { return result; }
                    connect_state.check_setup_review_command(&store,&peer,&command,capture)?;
                    if let Some((kind,id))=capture {connect_state.check_setup_capture(&store,kind,id,&peer)?;}
                    let result=registration.setup_command_with_context(context, command)?;
                    connect_state.bind_setup_review_receipt(&result)?;
                    if let Some((kind,id))=capture {connect_state.record_setup_capture(&store,kind,id,&peer)?;}
                    Ok(result)
                },
            );
            tokio::select! {
                biased;
                result=connect_worker.run(&store,&directories.secrets,connect_public_key,registration.smtp_sessions())=>{result?;return Err(ErrorCode::InternalError);},
                result=registration_worker.run()=>{result?;return Err(ErrorCode::InternalError);},
                result=folder_worker.run()=>{result?;return Err(ErrorCode::InternalError);},
                result=uid_worker.run()=>{result?;return Err(ErrorCode::InternalError);},
                result=google_worker.run()=>{result?;return Err(ErrorCode::InternalError);},
                result=receive_worker::run(&store, &directories.secrets, shared_slot, &read_policy)=>{result?;return Err(ErrorCode::InternalError);},
                result=server=>result?,
            }
            event(EventKind::CoreStopped);
            Ok(())
        }),
    )
}

// Only compile-time stage names and stable error codes reach diagnostics.
// Never print paths, underlying errors, database content or credentials here.
#[cfg(any(windows, unix, test))]
fn startup_step<T>(stage: &'static str, result: Result<T, ErrorCode>) -> Result<T, ErrorCode> {
    result.inspect_err(|code| {
        let _ = writeln!(
            io::stderr(),
            "hantong_startup stage={stage} outcome=failed code={}",
            code.as_str()
        );
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn start(background: bool) -> Result<(), ErrorCode> {
    let directories = startup_step("directories", hantong_platform::app_directories())?;
    let _secrets_directory = startup_step(
        "secrets_directory",
        hantong_platform::unix_paths::UnixDirectoryGuard::prepare(&directories.secrets),
    )?;
    let _data_directory = startup_step(
        "data_directory",
        hantong_platform::unix_paths::UnixDirectoryGuard::prepare(&directories.data),
    )?;
    #[cfg(target_os = "linux")]
    startup_step(
        "secret_backend",
        hantong_secrets::headless_vault::prepare_backend(&directories.secrets, &directories.data)
            .map_err(|error| match error {
                hantong_secrets::SecretError::InvalidInputWindow => ErrorCode::HumanStepRequired,
                _ => ErrorCode::InternalError,
            }),
    )?;
    let store = startup_step("storage", storage::initialize_storage(&directories))?;
    let mut server_identity = startup_step(
        "identity",
        unix_identity::initialize_server_identity(&store, &directories.secrets),
    )?;
    startup_step(
        "registration_recovery",
        unix_registered_profiles::recover_startup(
            &store,
            &directories.secrets,
            &mut server_identity,
        ),
    )?;
    startup_step(
        "workflow_recovery",
        unix_registration::recover_startup(&store, &directories.secrets, &mut server_identity),
    )?;
    if background {
        startup_step(
            "detach",
            hantong_platform::unix_lifecycle::detach_background_process(),
        )?;
    }
    let server_instance = startup_step("instance", hantong_platform::random_nonce())?;
    let runtime = startup_step(
        "runtime",
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .enable_all()
            .build()
            .map_err(|_| ErrorCode::InternalError),
    )?;
    startup_step(
        "transport",
        runtime.block_on(unix_core_runtime::serve(
            store,
            server_identity,
            server_instance,
            &directories.secrets,
        )),
    )
}

fn run(args: impl IntoIterator<Item = std::ffi::OsString>) -> i32 {
    let args: Vec<_> = args.into_iter().collect();
    #[cfg(target_os = "linux")]
    if args.first().and_then(|v| v.to_str()) == Some("linux-update-v2") {
        return match linux_update_v2::run(&args) {
            Ok(()) => 0,
            Err(code) => emit_error(code, Lang::En),
        };
    }
    #[cfg(unix)]
    if args.first().and_then(|v| v.to_str()) == Some("unix-relay") {
        if args.len() != 1 {
            return emit_error(ErrorCode::InvalidInput, Lang::En);
        }
        return match unix_relay_helper::run() {
            Ok(()) => 0,
            Err(code) => emit_error(code, Lang::En),
        };
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if args.first().and_then(|v| v.to_str()) == Some("unix-stop") {
        if args.len() != 1 {
            return emit_error(ErrorCode::InvalidInput, Lang::En);
        }
        return match unix_stop::run() {
            Ok(()) => 0,
            Err(code) => emit_error(code, Lang::En),
        };
    }
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    if args.first().and_then(|v| v.to_str()) == Some("account-connect") {
        return match account_connect_frontend::run(&args) {
            Ok(()) => 0,
            Err(code) => emit_error(code, Lang::En),
        };
    }
    if args.first().and_then(|v| v.to_str()) == Some("terminal-review") {
        #[cfg(windows)]
        return match terminal_review_frontend::run(&args[1..], false) {
            Ok(()) => 0,
            Err(code) => emit_error(code, Lang::En),
        };
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        return match unix_terminal_review_frontend::run(&args[1..]) {
            Ok(()) => 0,
            Err(code) => emit_error(code, Lang::En),
        };
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        return emit_error(ErrorCode::CapabilityUnsupported, Lang::En);
    }
    let mut lang = Lang::En;
    let mut command = None;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let mut background = false;
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
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            "--background" if command.as_deref() == Some("run") && !background => background = true,
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
        Some("run") => {
            #[cfg(windows)]
            let result = start();
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            let result = start(background);
            match result {
                Ok(()) => 0,
                Err(code) => emit_error(code, lang),
            }
        }
        _ => emit_error(ErrorCode::InvalidInput, lang),
    }
}

fn main() {
    #[cfg(all(target_os = "linux", not(test)))]
    if let Err(code) = hantong_platform::linux_identity::ensure_entry("bin/mail-hantong-core") {
        std::process::exit(emit_error(code, Lang::En));
    }
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    #[cfg(all(target_os = "linux", not(test)))]
    if matches!(
        args.first().and_then(|a| a.to_str()),
        Some("unix-relay" | "terminal-review" | "unix-stop" | "account-connect")
    ) && let Err(code) = hantong_platform::linux_sealed::forbid_helper_exec()
    {
        std::process::exit(emit_error(code, Lang::En));
    }
    std::process::exit(run(args));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn inst01_hidden_console_signals_are_available() {
        const MARKER: &str = "HANTONG_TEST_HIDDEN_SIGNALS_CHILD";
        if std::env::var_os(MARKER).is_some() {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .unwrap();
            runtime.block_on(async {
                let _interrupt = tokio::signal::windows::ctrl_c().unwrap();
                let _terminate = tokio::signal::windows::ctrl_break().unwrap();
            });
            return;
        }
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::inst01_hidden_console_signals_are_available",
                "--nocapture",
            ])
            .env(MARKER, "1")
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    #[test]
    fn inst01_sec08_startup_step_preserves_success_and_stable_errors() {
        assert_eq!(startup_step("storage", Ok(7)), Ok(7));
        assert_eq!(
            startup_step::<()>("storage", Err(ErrorCode::InternalError)),
            Err(ErrorCode::InternalError)
        );
        assert_eq!(
            startup_step::<()>("identity", Err(ErrorCode::Conflict)),
            Err(ErrorCode::Conflict)
        );
    }

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
