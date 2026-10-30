#![forbid(unsafe_code)]
#![cfg_attr(windows, windows_subsystem = "windows")]

use hantong_contracts::ErrorCode;
use hantong_desktop::{CoreStatus, Language, ShellCallbacks, ShellOptions, Theme};
use std::{
    process::ExitCode,
    sync::{Arc, Mutex},
};

mod bridge;
mod header_display;
pub(crate) use bridge::GuiBridge;

#[derive(Debug, PartialEq)]
enum Action {
    Help,
    Version,
    Open(ShellOptions),
}

fn parse(args: &[String]) -> Result<Action, ErrorCode> {
    if args == ["--help"] || args == ["-h"] {
        return Ok(Action::Help);
    }
    if args == ["--version"] || args == ["-V"] {
        return Ok(Action::Version);
    }
    let mut options = ShellOptions::default();
    let mut language = false;
    let mut theme = false;
    let mut arguments = args.iter();
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or(ErrorCode::InvalidInput)?;
        match flag.as_str() {
            "--lang" if !language => {
                options.language = Some(match value.as_str() {
                    "en" => Language::English,
                    "ko" => Language::Korean,
                    _ => return Err(ErrorCode::InvalidInput),
                });
                language = true;
            }
            "--theme" if !theme => {
                options.theme = match value.as_str() {
                    "light" => Theme::Light,
                    "dark" => Theme::Dark,
                    "system" => Theme::System,
                    _ => return Err(ErrorCode::InvalidInput),
                };
                theme = true;
            }
            _ => return Err(ErrorCode::InvalidInput),
        }
    }
    Ok(Action::Open(options))
}

fn probe() -> CoreStatus {
    match hantong_ipc_client::core_status() {
        Ok(value) if value.transport_ready => CoreStatus::Available {
            mail_ready: value.mail_ready,
        },
        Ok(_) => CoreStatus::Unavailable(ErrorCode::CoreUnavailable),
        Err(error) => CoreStatus::Unavailable(error),
    }
}

fn execute(
    args: &[String],
    open: impl FnOnce(ShellOptions) -> Result<(), ErrorCode>,
) -> Result<Option<String>, ErrorCode> {
    match parse(args)? {
        Action::Help => Ok(Some("mail-hantong\nUsage: mail-hantong [--lang en|ko] [--theme light|dark|system]\n       mail-hantong --help | --version\n".into())),
        Action::Version => Ok(Some(format!("mail-hantong {}\n", env!("CARGO_PKG_VERSION")))),
        Action::Open(options) => { open(options)?; Ok(None) }
    }
}

fn main() -> ExitCode {
    let args = std::env::args_os()
        .skip(1)
        .map(|value| value.into_string().map_err(|_| ErrorCode::InvalidInput))
        .collect::<Result<Vec<_>, _>>();
    match args.and_then(|args| {
        execute(&args, |options| {
            let bridge = Arc::new(Mutex::new(GuiBridge::new()));
            let action_bridge = Arc::clone(&bridge);
            let callbacks = ShellCallbacks {
                action: Some(Arc::new(move |action| {
                    action_bridge
                        .lock()
                        .map_err(|_| ErrorCode::InternalError)?
                        .action(action)
                })),
            };
            hantong_desktop::run_with_callbacks(options, Arc::new(probe), callbacks)
        })
    }) {
        Ok(Some(output)) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", error.as_str());
            ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(1))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).into()).collect()
    }
    #[test]
    fn ops07_ops08_startup_options_are_explicit_and_invalid_arguments_have_no_effect() {
        for language in ["en", "ko"] {
            for theme in ["light", "dark", "system"] {
                assert!(matches!(
                    parse(&args(&["--lang", language, "--theme", theme])),
                    Ok(Action::Open(_))
                ));
            }
        }
        assert_eq!(parse(&[]), Ok(Action::Open(ShellOptions::default())));
        for invalid in [
            vec!["--lang"],
            vec!["--lang", "jp"],
            vec!["--theme", "auto"],
            vec!["--theme", "dark", "--theme", "light"],
            vec!["--lang", "en", "--lang", "ko"],
            vec!["--profile", "private-value"],
            vec!["--help", "--lang", "ko"],
        ] {
            assert_eq!(
                execute(&args(&invalid), |_| panic!(
                    "invalid input must not open a window or connect"
                )),
                Err(ErrorCode::InvalidInput)
            );
        }
    }
    #[test]
    fn ops07_help_and_version_have_no_window_ipc_or_profile_side_effects() {
        for option in ["--help", "-h", "--version", "-V"] {
            let result = execute(&args(&[option]), |_| {
                panic!("metadata command must not open a window or connect")
            })
            .unwrap()
            .unwrap();
            assert!(result.starts_with("mail-hantong"));
        }
    }
    #[test]
    fn ops08_valid_options_reach_the_native_boundary_once() {
        let called = std::cell::Cell::new(0);
        assert_eq!(
            execute(&args(&["--lang", "ko", "--theme", "dark"]), |options| {
                called.set(called.get() + 1);
                assert_eq!(options.language, Some(Language::Korean));
                assert_eq!(options.theme, Theme::Dark);
                Ok(())
            }),
            Ok(None)
        );
        assert_eq!(called.get(), 1);
    }
}
