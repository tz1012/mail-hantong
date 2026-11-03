//! Provider chat with automatically connected, separately configured mail tools.
use hantong_ai::{AiError, Provider, Session};
use hantong_contracts::ErrorCode;
use serde_json::{Value, json};
use std::io::{self, BufRead, IsTerminal, Read, Write};
fn text(ko: bool, key: &str) -> &'static str {
    let file = if ko {
        include_str!("../resources/ai-ko.txt")
    } else {
        include_str!("../resources/ai-en.txt")
    };
    file.lines()
        .find_map(|line| {
            line.split_once('=')
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v)
        })
        .expect("compiled AI resource")
}
/// Renders the commands available inside an interactive AI chat session.
///
/// Keep this output provider-independent: asking for help must not start a
/// provider call, refresh models, or change the current session.
pub(super) fn help(ko: bool) -> String {
    [
        "help_header",
        "help_help",
        "help_status",
        "help_provider",
        "help_model",
        "help_mode",
        "help_fast",
        "help_agent",
        "help_cli",
        "help_quit",
    ]
    .into_iter()
    .map(|key| text(ko, key))
    .collect::<Vec<_>>()
    .join("\n")
}
fn emit(value: Value) {
    println!("{value}");
}

#[cfg(any(windows, test))]
fn ensure_mail_access_with(
    interactive: bool,
    mut ready: impl FnMut() -> Result<bool, ErrorCode>,
    configure: impl FnOnce() -> Result<(), ErrorCode>,
) -> Result<(), ErrorCode> {
    if ready()? {
        return Ok(());
    }
    if !interactive {
        return Err(ErrorCode::HumanStepRequired);
    }
    configure()?;
    if ready()? {
        Ok(())
    } else {
        Err(ErrorCode::ApprovalRequired)
    }
}

fn ensure_mail_access(provider: Provider, ko: bool, no_input: bool) -> Result<(), ErrorCode> {
    #[cfg(windows)]
    {
        use hantong_contracts::registration::{
            AiProvider, RegistrationCommand, RegistrationProgress,
        };
        let provider = match provider {
            Provider::Codex => AiProvider::Codex,
            Provider::Claude => AiProvider::Claude,
        };
        super::core_start::ensure_started()?;
        let ready = || match hantong_ipc_client::verify_ai(provider) {
            Ok(()) => Ok(true),
            Err(ErrorCode::ClientNotRegistered | ErrorCode::AuthRequired) => Ok(false),
            Err(error) => Err(error),
        };
        ensure_mail_access_with(
            !no_input && io::stdin().is_terminal() && io::stdout().is_terminal(),
            ready,
            || {
                println!("{}", text(ko, "mail_setup"));
                let receipt = super::terminal_workflows::registration(
                    RegistrationCommand::AiRequest(provider),
                    if ko { super::Lang::Ko } else { super::Lang::En },
                )?;
                match receipt.state() {
                    RegistrationProgress::Ready => Ok(()),
                    RegistrationProgress::Denied => Err(ErrorCode::ApprovalDenied),
                    RegistrationProgress::Expired => Err(ErrorCode::ApprovalExpired),
                    _ => Err(ErrorCode::ApprovalRequired),
                }
            },
        )
    }
    #[cfg(not(windows))]
    {
        let _ = (provider, ko, no_input);
        Err(ErrorCode::CapabilityUnsupported)
    }
}
fn ensure_switch_mail_access(provider: Provider, ko: bool, no_input: bool) -> Result<(), AiError> {
    ensure_mail_access(provider, ko, no_input).map_err(|error| match error {
        ErrorCode::HumanStepRequired => AiError::HumanStepRequired,
        ErrorCode::ApprovalDenied => AiError::Cancelled,
        ErrorCode::AuthRequired => AiError::LoginRequired,
        _ => AiError::AgentUnavailable,
    })
}
fn failure(e: AiError, ko: bool, json_mode: bool) -> i32 {
    let code = match e {
        AiError::InvalidInput => ErrorCode::InvalidInput,
        AiError::ProviderMissing
        | AiError::SubscriptionUnsupported
        | AiError::Unsupported
        | AiError::SandboxUnavailable
        | AiError::AgentUnavailable => ErrorCode::CapabilityUnsupported,
        AiError::LoginRequired => ErrorCode::AuthRequired,
        AiError::Timeout => ErrorCode::Offline,
        AiError::Cancelled => ErrorCode::ApprovalDenied,
        AiError::HumanStepRequired => ErrorCode::HumanStepRequired,
        _ => ErrorCode::InternalError,
    };
    if json_mode {
        emit(
            json!({"schema_version":1,"ok":false,"error":{"code":code.as_str(),"message":e.code(),"retryable":false}}),
        );
    } else {
        let key = match e {
            AiError::ProviderMissing => "missing",
            AiError::SandboxUnavailable => "sandbox_unavailable",
            AiError::SubscriptionUnsupported => "subscription",
            AiError::LoginRequired => "login",
            AiError::InvalidInput => "usage",
            _ => "failed",
        };
        eprintln!("{} ({})", text(ko, key), e.code());
    }
    code.exit_code()
}
/// Called after the parent parses common language/JSON/no-input flags.
pub(super) fn try_run(
    command: &[String],
    ko: bool,
    json_mode: bool,
    no_input: bool,
) -> Option<i32> {
    if command.first().map(String::as_str) != Some("ai") {
        return None;
    }
    Some(match command.get(1).map(String::as_str) {
        Some("help") if command.len() == 2 => {
            if json_mode {
                emit(json!({"schema_version":1,"ok":true,"help":help(ko)}));
            } else {
                println!("{}", help(ko));
            }
            0
        }
        Some("providers") if command.len() == 2 => {
            let values:Vec<_>=[Provider::Codex,Provider::Claude].into_iter().map(|p|json!({"provider":p.name(),"installed":hantong_ai::available(p),"browser_login":true,"billing":if p==Provider::Claude{"console_api"}else{"managed_account"},"automatic_mail_tools":true,"bulk_mail_upload":false})).collect();
            if json_mode {
                emit(json!({"schema_version":1,"ok":true,"providers":values}));
            } else {
                for value in values {
                    println!(
                        "{}: {}",
                        value["provider"].as_str().unwrap(),
                        value["installed"]
                    );
                }
            }
            0
        }
        Some(action @ ("login" | "status" | "chat")) => {
            let Some(provider) = command.get(2).and_then(|v| Provider::parse(v).ok()) else {
                return Some(failure(AiError::InvalidInput, ko, json_mode));
            };
            if action == "login" {
                if command.len() != 3 {
                    return Some(failure(AiError::InvalidInput, ko, json_mode));
                }
                if no_input || (json_mode && provider == Provider::Claude) {
                    if json_mode {
                        emit(
                            json!({"schema_version":1,"ok":false,"error":{"code":"HUMAN_STEP_REQUIRED","message":"HUMAN_STEP_REQUIRED","retryable":false}}),
                        );
                    } else {
                        eprintln!("{}", text(ko, "human"));
                    }
                    ErrorCode::HumanStepRequired.exit_code()
                } else {
                    if !json_mode {
                        eprintln!("{}", text(ko, "browser"));
                    }
                    match hantong_ai::login(provider) {
                        Ok(status) => {
                            if json_mode {
                                emit(
                                    json!({"schema_version":1,"ok":true,"provider":provider.name(),"authenticated":status.authenticated,"mode":status.mode}),
                                );
                            } else {
                                println!("{}", text(ko, "ready"));
                            }
                            0
                        }
                        Err(e) => failure(e, ko, json_mode),
                    }
                }
            } else if action == "status" {
                if command.len() != 3 {
                    return Some(failure(AiError::InvalidInput, ko, json_mode));
                }
                match hantong_ai::status(provider) {
                    Ok(status) => {
                        emit(
                            json!({"schema_version":1,"ok":true,"provider":provider.name(),"authenticated":status.authenticated,"mode":status.mode}),
                        );
                        0
                    }
                    Err(e) => failure(e, ko, json_mode),
                }
            } else {
                let prompt = match command.get(3).map(String::as_str) {
                    None => None,
                    Some("--prompt") if command.len() == 5 => command.get(4),
                    _ => return Some(failure(AiError::InvalidInput, ko, json_mode)),
                };
                if prompt.is_none()
                    && (no_input
                        || json_mode
                        || !io::stdin().is_terminal()
                        || !io::stdout().is_terminal())
                {
                    eprintln!("{}", text(ko, "human"));
                    return Some(ErrorCode::HumanStepRequired.exit_code());
                }
                if let Err(error) = ensure_mail_access(provider, ko, no_input || json_mode) {
                    if json_mode {
                        emit(
                            json!({"schema_version":1,"ok":false,"error":{"code":error.as_str(),"message":error.as_str(),"retryable":false}}),
                        );
                    } else {
                        eprintln!("{} ({})", text(ko, "mail_setup_failed"), error.as_str());
                    }
                    return Some(error.exit_code());
                }
                // Claude OAuth belongs to the official interactive Claude Code
                // process. Keep prompt-bearing/headless requests on the existing
                // embedded path, where Session::start still rejects OAuth.
                let interactive = !json_mode
                    && !no_input
                    && io::stdin().is_terminal()
                    && io::stdout().is_terminal();
                let native_auth = if provider == Provider::Claude && prompt.is_none() && interactive
                {
                    hantong_ai::status(provider).ok()
                } else {
                    None
                };
                if use_native_claude(
                    provider,
                    prompt.is_some(),
                    interactive,
                    native_auth.as_ref().and_then(|auth| auth.mode.as_deref()),
                ) {
                    println!("{}", text(ko, "claude_native"));
                    return Some(match hantong_ai::run_claude_interactive() {
                        Ok(()) => 0,
                        Err(error) => failure(error, ko, false),
                    });
                }
                let mut session = match Session::start(provider) {
                    Ok(v) => v,
                    Err(e) => return Some(failure(e, ko, json_mode)),
                };
                if let Some(prompt) = prompt {
                    if prompt.starts_with('/') {
                        return Some(
                            match command_result(&mut session, prompt, ko, no_input || json_mode) {
                                Ok(value) => {
                                    emit(value);
                                    0
                                }
                                Err(e) => failure(e, ko, json_mode),
                            },
                        );
                    }
                    let mut reply = String::new();
                    match session.ask(prompt, |delta| {
                        if json_mode {
                            if reply.len() + delta.len() <= 1024 * 1024 {
                                reply.push_str(delta);
                            }
                        } else {
                            print!("{delta}");
                            let _ = io::stdout().flush();
                        }
                    }) {
                        Ok(()) => {
                            if json_mode {
                                emit(
                                    json!({"schema_version":1,"ok":true,"provider":provider.name(),"text":reply}),
                                );
                            } else {
                                println!();
                            }
                            0
                        }
                        Err(e) => failure(e, ko, json_mode),
                    }
                } else {
                    println!("{}\n{}", text(ko, "title"), text(ko, "notice"));
                    loop {
                        print!("{}> ", text(ko, "prompt"));
                        if io::stdout().flush().is_err() {
                            break;
                        }
                        let mut line = String::new();
                        let result = io::stdin().lock().take(32770).read_line(&mut line);
                        match result {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {}
                        }
                        if line.len() > 32768 {
                            eprintln!("AI_SIZE_LIMIT");
                            continue;
                        }
                        let prompt = line.trim();
                        if prompt == "/quit" {
                            break;
                        }
                        if prompt.starts_with('/') {
                            match slash(&mut session, prompt, ko) {
                                Ok(Slash::Continue) => continue,
                                Ok(Slash::Exit) => break,
                                Err(e) => {
                                    failure(e, ko, false);
                                    continue;
                                }
                            }
                        }
                        if prompt.is_empty() {
                            continue;
                        }
                        print!("{}> ", text(ko, "reply"));
                        match session.ask(prompt, |delta| {
                            print!("{delta}");
                            let _ = io::stdout().flush();
                        }) {
                            Ok(()) => println!(),
                            Err(e) => {
                                println!();
                                failure(e, ko, false);
                                break;
                            }
                        }
                    }
                    0
                }
            }
        }
        _ => failure(AiError::InvalidInput, ko, json_mode),
    })
}

/// Accept the short forms used while the onboarding menu is still active.
/// They are routed directly to this process; no shell or child command is
/// involved, and the normal AI mail-consent checks remain in `try_run`.
pub(super) fn is_terminal_command(command: &str) -> bool {
    matches!(
        command,
        "ai codex"
            | "ai claude"
            | ":ai codex"
            | ":ai claude"
            | "/ai codex"
            | "/ai claude"
            | ":agent"
            | "/agent"
    )
}

pub(super) fn run_terminal_command(command: &str, lang: crate::Lang) -> Result<(), ErrorCode> {
    let ko = matches!(lang, crate::Lang::Ko);
    let provider = match command {
        "ai codex" | ":ai codex" | "/ai codex" => Provider::Codex,
        "ai claude" | ":ai claude" | "/ai claude" => Provider::Claude,
        ":agent" | "/agent" => {
            let labels = vec!["openai (Codex)".to_owned(), "Claude".to_owned()];
            let picked = hantong_platform::interactive_select(&labels, 0)
                .map_err(|_| ErrorCode::HumanStepRequired)?
                .ok_or(ErrorCode::ApprovalDenied)?;
            match picked {
                0 => Provider::Codex,
                1 => Provider::Claude,
                _ => return Err(ErrorCode::InvalidInput),
            }
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    let command = vec![
        "ai".to_owned(),
        "chat".to_owned(),
        provider.name().to_owned(),
    ];
    let code = try_run(&command, ko, false, false).ok_or(ErrorCode::InvalidInput)?;
    terminal_exit_result(code)
}

fn use_native_claude(
    provider: Provider,
    has_prompt: bool,
    interactive: bool,
    mode: Option<&str>,
) -> bool {
    provider == Provider::Claude
        && !has_prompt
        && interactive
        && matches!(mode, Some("oauth_token" | "claude.ai"))
}

#[cfg(test)]
#[test]
fn ai07_native_oauth_handoff_never_accepts_headless_or_forwarded_prompts() {
    for mode in ["oauth_token", "claude.ai"] {
        assert!(use_native_claude(Provider::Claude, false, true, Some(mode)));
        assert!(!use_native_claude(Provider::Claude, true, true, Some(mode)));
        assert!(!use_native_claude(
            Provider::Claude,
            false,
            false,
            Some(mode)
        ));
        assert!(!use_native_claude(Provider::Codex, false, true, Some(mode)));
    }
    for mode in [
        None,
        Some("api_key"),
        Some("api_key_helper"),
        Some("third_party"),
        Some("unknown"),
    ] {
        assert!(!use_native_claude(Provider::Claude, false, true, mode));
    }
}

fn terminal_exit_result(code: i32) -> Result<(), ErrorCode> {
    if code == 0 {
        return Ok(());
    }
    Err(ErrorCode::ALL
        .iter()
        .copied()
        .find(|error| error.exit_code() == code)
        .unwrap_or(ErrorCode::InternalError))
}

#[cfg(test)]
#[test]
fn cli17_ai_terminal_preserves_stable_failure_codes() {
    assert_eq!(terminal_exit_result(0), Ok(()));
    for error in ErrorCode::ALL {
        assert_eq!(terminal_exit_result(error.exit_code()), Err(error));
    }
    assert_eq!(terminal_exit_result(255), Err(ErrorCode::InternalError));
    assert_eq!(
        terminal_exit_result(ErrorCode::CapabilityUnsupported.exit_code()),
        Err(ErrorCode::CapabilityUnsupported)
    );
}
enum Slash {
    Continue,
    Exit,
}
fn command_result(
    session: &mut Session,
    line: &str,
    ko: bool,
    no_input: bool,
) -> Result<Value, AiError> {
    let words: Vec<_> = line.split_whitespace().collect();
    if words.len() > 2 {
        return Err(AiError::InvalidInput);
    }
    let arg = words.get(1).copied();
    match words.first().copied() {
        Some("/provider" | "/provide") => {
            if let Some(value) = arg
                && value != "list"
            {
                let provider = Provider::parse(value)?;
                if provider != session.provider() {
                    ensure_switch_mail_access(provider, ko, no_input)?;
                    *session = Session::start(provider)?;
                }
            }
            Ok(
                json!({"schema_version":1,"ok":true,"provider":session.provider().name(),"options":["openai","codex","claude"],"usage":"/provider PROVIDER"}),
            )
        }
        Some("/model") => {
            if let Some(id) = arg
                && id != "list"
            {
                session.select_model(id)?;
            } else {
                session.refresh_models()?;
            }
            let options: Vec<_> = session
                .models()
                .iter()
                .map(|m| json!({"id":m.id,"label":m.label,"efforts":m.efforts,"fast":m.fast}))
                .collect();
            Ok(
                json!({"schema_version":1,"ok":true,"model":session.current_model().map(|m|&m.id),"options":options,"usage":"/model ID"}),
            )
        }
        Some("/mode") => {
            if let Some(mode) = arg {
                session.select_mode(mode)?;
            }
            let options = mode_options(session.current_model());
            Ok(
                json!({"schema_version":1,"ok":true,"effort":session.preferences().effort,"options":options,"usage":"/mode MODE"}),
            )
        }
        Some("/fast") => {
            if let Some(value) = arg {
                session.select_fast(match value {
                    "on" => true,
                    "off" => false,
                    _ => return Err(AiError::InvalidInput),
                })?;
            }
            Ok(
                json!({"schema_version":1,"ok":true,"fast_requested":session.preferences().fast,"supported":session.current_model().is_some_and(|m|m.fast)}),
            )
        }
        Some("/agent") => {
            if let Some(value) = arg {
                session.set_agent(match value {
                    "on" => true,
                    "off" => false,
                    _ => return Err(AiError::InvalidInput),
                })?;
            }
            Ok(json!({"schema_version":1,"ok":true,"agent":session.agent_enabled()}))
        }
        Some("/status") if arg.is_none() => Ok(
            json!({"schema_version":1,"ok":true,"provider":session.provider().name(),"model":session.current_model().map(|m|&m.id),"effort":session.preferences().effort,"fast_requested":session.preferences().fast,"agent":session.agent_enabled()}),
        ),
        Some("/help") if arg.is_none() => Ok(json!({"schema_version":1,"ok":true,"help":help(ko)})),
        _ => Err(AiError::InvalidInput),
    }
}
fn choose(labels: &[String], initial: usize) -> Result<Option<usize>, AiError> {
    match hantong_platform::interactive_select(labels, initial) {
        Ok(v) => Ok(v),
        Err(_) => {
            for (i, label) in labels.iter().enumerate() {
                println!("{}: {label}", i + 1);
            }
            Ok(None)
        }
    }
}
type Chooser<'a> = dyn FnMut(&[String], usize) -> Result<Option<usize>, AiError> + 'a;
fn provider_pick(
    current: Provider,
    arg: Option<&str>,
    chooser: &mut Chooser<'_>,
) -> Result<Option<Provider>, AiError> {
    if let Some(arg) = arg {
        return Provider::parse(arg).map(Some);
    }
    let picked = chooser(
        &["openai (Codex)".into(), "claude".into()],
        usize::from(current == Provider::Claude),
    )?;
    picked
        .map(|i| match i {
            0 => Ok(Provider::Codex),
            1 => Ok(Provider::Claude),
            _ => Err(AiError::InvalidInput),
        })
        .transpose()
}
fn mode_options(model: Option<&hantong_ai::Model>) -> Vec<&'static str> {
    ["light", "med", "high", "ultra-high"]
        .into_iter()
        .filter(|mode| model.is_some_and(|m| m.effort_for_mode(mode).is_ok()))
        .collect()
}
fn mode_pick(
    model: Option<&hantong_ai::Model>,
    current: Option<&str>,
    arg: Option<&str>,
    chooser: &mut Chooser<'_>,
) -> Result<Option<String>, AiError> {
    let options = mode_options(model);
    if let Some(arg) = arg {
        if !options.contains(&arg) {
            return Err(AiError::Unsupported);
        }
        return Ok(Some(arg.into()));
    }
    if options.is_empty() {
        return Err(AiError::Unsupported);
    }
    let initial = options
        .iter()
        .position(|m| model.and_then(|model| model.effort_for_mode(m).ok()) == current)
        .unwrap_or(0);
    let picked = chooser(
        &options.iter().map(|s| (*s).into()).collect::<Vec<String>>(),
        initial,
    )?;
    picked
        .map(|i| {
            options
                .get(i)
                .map(|s| (*s).into())
                .ok_or(AiError::InvalidInput)
        })
        .transpose()
}
fn show_status(session: &Session) {
    println!(
        "{}",
        json!({"provider":session.provider().name(),"model":session.current_model().map(|m|m.id.as_str()),"effort":session.preferences().effort,"fast_requested":session.preferences().fast,"agent":session.agent_enabled()})
    );
}
fn slash(session: &mut Session, line: &str, ko: bool) -> Result<Slash, AiError> {
    let parts: Vec<_> = line.split_whitespace().collect();
    if parts.len() > 2 {
        return Err(AiError::InvalidInput);
    }
    let arg = parts.get(1).copied();
    match parts.first().copied() {
        Some("/help") if arg.is_none() => println!("{}", help(ko)),
        Some("/status") if arg.is_none() => show_status(session),
        Some("/cli") if arg.is_none() => return Ok(Slash::Exit),
        Some("/tui") if arg.is_none() => {
            let exe = std::env::current_exe()
                .map_err(|_| AiError::Io)?
                .parent()
                .ok_or(AiError::Io)?
                .join(if cfg!(windows) {
                    "hantong-t.exe"
                } else {
                    "hantong-t"
                });
            if !exe.is_file() {
                return Err(AiError::ProviderMissing);
            }
            std::process::Command::new(exe)
                .status()
                .map_err(|_| AiError::Io)?;
            return Ok(Slash::Exit);
        }
        Some("/agent") => {
            match arg {
                Some("on") => session.set_agent(true)?,
                Some("off") => session.set_agent(false)?,
                None => {}
                _ => return Err(AiError::InvalidInput),
            }
            show_status(session);
        }
        Some("/fast") => {
            match arg {
                Some("on") => session.select_fast(true)?,
                Some("off") => session.select_fast(false)?,
                None => {}
                _ => return Err(AiError::InvalidInput),
            }
            show_status(session);
        }
        Some("/provider" | "/provide") => {
            if arg == Some("list") {
                println!("openai / codex, claude");
                return Ok(Slash::Continue);
            }
            let provider = provider_pick(session.provider(), arg, &mut choose)?;
            if let Some(provider) = provider
                && provider != session.provider()
            {
                ensure_switch_mail_access(provider, ko, false)?;
                let next = Session::start(provider)?;
                *session = next;
                println!("{}", text(ko, "clean"));
            }
            show_status(session);
        }
        Some("/model") => {
            if arg == Some("list") {
                session.refresh_models()?;
                for m in session.models() {
                    println!("{}  {}", m.id, m.label);
                }
                return Ok(Slash::Continue);
            }
            let id = if let Some(id) = arg {
                Some(id.to_owned())
            } else {
                session.refresh_models()?;
                let labels: Vec<_> = session
                    .models()
                    .iter()
                    .map(|m| format!("{}  {}", m.id, m.label))
                    .collect();
                let initial = session
                    .current_model()
                    .and_then(|m| session.models().iter().position(|v| v.id == m.id))
                    .unwrap_or(0);
                choose(&labels, initial)?.map(|i| session.models()[i].id.clone())
            };
            if let Some(id) = id {
                session.select_model(&id)?;
            }
            show_status(session);
        }
        Some("/mode") => {
            let mode = mode_pick(
                session.current_model(),
                session.preferences().effort.as_deref(),
                arg,
                &mut choose,
            )?;
            if let Some(mode) = mode {
                session.select_mode(&mode)?;
            }
            show_status(session);
        }
        _ => return Err(AiError::InvalidInput),
    }
    Ok(Slash::Continue)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ai07_automatic_mail_setup_does_not_register_again_or_escalate_headless() {
        assert_eq!(
            ensure_mail_access_with(false, || Ok(true), || panic!("already configured")),
            Ok(())
        );
        assert_eq!(
            ensure_mail_access_with(false, || Ok(false), || panic!("headless cannot configure")),
            Err(ErrorCode::HumanStepRequired)
        );
        assert_eq!(
            ensure_mail_access_with(
                true,
                || Err(ErrorCode::ClientSuspended),
                || panic!("revocation cannot be repaired")
            ),
            Err(ErrorCode::ClientSuspended)
        );
        let mut calls = 0;
        assert_eq!(
            ensure_mail_access_with(
                true,
                || {
                    calls += 1;
                    Ok(calls == 2)
                },
                || Ok(())
            ),
            Ok(())
        );
        assert_eq!(calls, 2);
        assert_eq!(
            ensure_mail_access_with(true, || Ok(false), || Err(ErrorCode::ApprovalDenied)),
            Err(ErrorCode::ApprovalDenied)
        );
    }
    #[test]
    fn cli16_provider_arrow_chooser_and_escape_preserve_preferences() {
        let current = Provider::Codex;
        let mut arrow = |labels: &[String], initial| {
            assert_eq!(labels.len(), 2);
            let mut state = hantong_platform::Selection::new(labels.len(), initial).unwrap();
            state.key(hantong_platform::SelectionKey::Up);
            Ok(state.key(hantong_platform::SelectionKey::Enter).unwrap())
        };
        assert_eq!(
            provider_pick(current, None, &mut arrow),
            Ok(Some(Provider::Claude))
        );
        let mut escape = |_: &[String], _| Ok(None);
        assert_eq!(provider_pick(current, None, &mut escape), Ok(None));
        assert_eq!(current, Provider::Codex);
        assert_eq!(
            provider_pick(current, Some("openai"), &mut escape),
            Ok(Some(Provider::Codex))
        );
    }
    #[test]
    fn cli16_mode_chooser_filters_capabilities_and_no_input_list_never_waits() {
        let model = hantong_ai::Model {
            id: "synthetic".into(),
            label: "Synthetic".into(),
            efforts: vec!["low".into(), "high".into()],
            fast: false,
            default: true,
        };
        assert_eq!(mode_options(Some(&model)), vec!["light", "high"]);
        let mut cancel = |labels: &[String], initial| {
            assert_eq!(labels, &["light", "high"]);
            assert_eq!(initial, 1);
            Ok(None)
        };
        assert_eq!(
            mode_pick(Some(&model), Some("high"), None, &mut cancel),
            Ok(None)
        );
        let mut forbidden = |_: &[String], _| {
            panic!("Unsupported or explicit selections must not read console input")
        };
        assert_eq!(
            mode_pick(
                Some(&model),
                Some("high"),
                Some("ultra-high"),
                &mut forbidden
            ),
            Err(AiError::Unsupported)
        );
        assert_eq!(
            mode_pick(Some(&model), Some("high"), Some("light"), &mut forbidden),
            Ok(Some("light".into()))
        );
        // Listing uses this same pure capability filter, with no chooser parameter.
        let list = mode_options(Some(&model));
        assert_eq!(list.len(), 2);
    }
    #[test]
    fn ai07_unknown_provider_and_no_input_login_do_not_spawn_provider() {
        assert_eq!(
            try_run(
                &["ai".into(), "login".into(), "unknown".into()],
                false,
                true,
                false
            ),
            Some(2)
        );
        assert_eq!(
            try_run(
                &["ai".into(), "login".into(), "codex".into()],
                false,
                true,
                true
            ),
            Some(24)
        );
    }
    #[test]
    fn cli16_bilingual_ai_resources_have_exact_same_keys() {
        let keys = |s: &str| {
            s.lines()
                .map(|l| l.split_once('=').unwrap().0.to_owned())
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(
            keys(include_str!("../resources/ai-en.txt")),
            keys(include_str!("../resources/ai-ko.txt"))
        );
    }

    #[test]
    fn ai07_help_is_localized_and_does_not_need_a_provider() {
        let english = help(false);
        let korean = help(true);
        assert!(english.contains("/provider PROVIDER"));
        assert!(english.contains("/provide is an alias"));
        assert!(english.contains("/model [list|ID]"));
        assert!(english.contains("/quit"));
        assert!(korean.contains("/provider PROVIDER"));
        assert!(korean.contains("/provide"));
        assert!(korean.contains("/model [list|ID]"));
        assert!(korean.contains("/quit"));
        assert_ne!(english, korean);

        assert_eq!(
            try_run(&["ai".into(), "help".into()], false, false, true),
            Some(0)
        );
        assert_eq!(
            try_run(&["ai".into(), "help".into()], true, true, true),
            Some(0)
        );
    }
}
