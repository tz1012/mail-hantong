//! Explicit provider login and freeform chat frontend; no implicit mail context.
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
fn emit(value: Value) {
    println!("{value}");
}
fn failure(e: AiError, ko: bool, json_mode: bool) -> i32 {
    let code = match e {
        AiError::InvalidInput => ErrorCode::InvalidInput,
        AiError::ProviderMissing
        | AiError::SubscriptionUnsupported
        | AiError::Unsupported
        | AiError::AgentUnavailable => ErrorCode::CapabilityUnsupported,
        AiError::LoginRequired => ErrorCode::AuthRequired,
        AiError::Timeout => ErrorCode::Offline,
        AiError::Cancelled => ErrorCode::ApprovalDenied,
        _ => ErrorCode::InternalError,
    };
    if json_mode {
        emit(
            json!({"schema_version":1,"ok":false,"error":{"code":code.as_str(),"message":e.code(),"retryable":false}}),
        );
    } else {
        let key = match e {
            AiError::ProviderMissing => "missing",
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
        Some("providers") if command.len() == 2 => {
            let values:Vec<_>=[Provider::Codex,Provider::Claude].into_iter().map(|p|json!({"provider":p.name(),"installed":hantong_ai::available(p),"browser_login":true,"billing":if p==Provider::Claude{"console_api"}else{"managed_account"},"automatic_mail_context":false})).collect();
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
                let mut session = match Session::start(provider) {
                    Ok(v) => v,
                    Err(e) => return Some(failure(e, ko, json_mode)),
                };
                if let Some(prompt) = prompt {
                    if prompt.starts_with('/') {
                        return Some(match command_result(&mut session, prompt, ko) {
                            Ok(value) => {
                                emit(value);
                                0
                            }
                            Err(e) => failure(e, ko, json_mode),
                        });
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
enum Slash {
    Continue,
    Exit,
}
fn command_result(session: &mut Session, line: &str, ko: bool) -> Result<Value, AiError> {
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
        Some("/help") if arg.is_none() => {
            Ok(json!({"schema_version":1,"ok":true,"help":text(ko,"commands")}))
        }
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
        Some("/help") if arg.is_none() => println!("{}", text(ko, "commands")),
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
}
