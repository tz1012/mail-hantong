//! First-run, interactive CLI onboarding.
//!
//! This module owns only the human-facing orchestration.  Authentication,
//! OAuth, and mailbox operations remain in the Google and IPC modules.

use hantong_contracts::{ErrorCode, google_setup::GoogleLocalProfile};
use std::io::{self, BufRead, Read, Write};

use super::Lang;

const EN: &str = include_str!("../resources/onboarding-en.txt");
const KO: &str = include_str!("../resources/onboarding-ko.txt");

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AccountChoice {
    id: String,
    label: String,
}

pub(super) trait Io {
    fn write(&mut self, text: &str) -> Result<(), ErrorCode>;
    fn read_line(&mut self) -> Result<Option<String>, ErrorCode>;
    fn read_text_line(&mut self, _max_bytes: usize) -> Result<Option<String>, ErrorCode> {
        self.read_line()
    }
}

pub(super) trait Effects {
    fn connect_google(
        &mut self,
        lang: Lang,
        profile: GoogleLocalProfile,
    ) -> Result<String, ErrorCode>;
    fn continue_existing(&mut self, account: &str, lang: Lang) -> Result<(), ErrorCode>;
    fn enable_send(&mut self, account: &str, lang: Lang) -> Result<(), ErrorCode>;
    fn compose(&mut self, account: &str, lang: Lang, io: &mut dyn Io) -> Result<(), ErrorCode>;
    fn inbox(&mut self, account: &str, lang: Lang) -> Result<(), ErrorCode>;
}

struct Stdio {
    output: io::Stdout,
}

impl Stdio {
    fn new() -> Self {
        Self {
            output: io::stdout(),
        }
    }
}

impl Io for Stdio {
    fn write(&mut self, text: &str) -> Result<(), ErrorCode> {
        self.output
            .write_all(text.as_bytes())
            .and_then(|_| self.output.flush())
            .map_err(|_| ErrorCode::CoreUnavailable)
    }

    fn read_line(&mut self) -> Result<Option<String>, ErrorCode> {
        let mut bytes = Vec::new();
        let mut input = io::stdin().lock();
        let count = input
            .by_ref()
            .take(257)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        if count == 0 {
            return Ok(None);
        }
        decode_line(&bytes)
    }

    fn read_text_line(&mut self, max_bytes: usize) -> Result<Option<String>, ErrorCode> {
        let mut bytes = Vec::new();
        let mut input = io::stdin().lock();
        let count = input
            .by_ref()
            .take((max_bytes + 3) as u64)
            .read_until(b'\n', &mut bytes)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        if count == 0 {
            return Ok(None);
        }
        decode_text_line(&bytes, max_bytes).map(Some)
    }
}

fn decode_text_line(bytes: &[u8], max_bytes: usize) -> Result<String, ErrorCode> {
    let text = std::str::from_utf8(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let text = text.strip_suffix('\r').unwrap_or(text);
    if text.len() > max_bytes {
        return Err(ErrorCode::QuotaExceeded);
    }
    Ok(text.to_owned())
}

fn decode_line(bytes: &[u8]) -> Result<Option<String>, ErrorCode> {
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() > 256 {
        return Err(ErrorCode::InvalidInput);
    }
    let line = std::str::from_utf8(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    Ok(Some(line.trim().to_owned()))
}

struct RealEffects;

impl Effects for RealEffects {
    fn connect_google(
        &mut self,
        lang: Lang,
        profile: GoogleLocalProfile,
    ) -> Result<String, ErrorCode> {
        super::google::connect_guided(lang, profile)
    }

    fn continue_existing(&mut self, account: &str, lang: Lang) -> Result<(), ErrorCode> {
        super::google::continue_guided(account, lang)
    }

    fn enable_send(&mut self, account: &str, lang: Lang) -> Result<(), ErrorCode> {
        super::google::enable_send_guided(account, lang)
    }

    fn compose(&mut self, account: &str, lang: Lang, io: &mut dyn Io) -> Result<(), ErrorCode> {
        super::guided_compose::run(account, lang, io)
    }

    fn inbox(&mut self, account: &str, lang: Lang) -> Result<(), ErrorCode> {
        super::google::inbox_guided(account, lang)
    }
}

fn resource(lang: Lang, key: &str) -> &'static str {
    let source = match lang {
        Lang::En => EN,
        Lang::Ko => KO,
    };
    source
        .lines()
        .find_map(|line| line.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
        .expect("validated onboarding resource key")
}

fn say(io: &mut impl Io, lang: Lang, key: &str) -> Result<(), ErrorCode> {
    io.write(resource(lang, key))?;
    io.write("\n")
}

fn choose(io: &mut impl Io, lang: Lang, help_keys: &[&str]) -> Result<Option<String>, ErrorCode> {
    loop {
        let Some(value) = io.read_line()? else {
            return Ok(None);
        };
        if value == "/help" {
            say(io, lang, "help_header")?;
            for key in help_keys {
                say(io, lang, key)?;
            }
            say(io, lang, "help_prompt")?;
            continue;
        }
        return Ok(Some(value));
    }
}

fn connect_account(
    lang: Lang,
    io: &mut impl Io,
    effects: &mut impl Effects,
) -> Result<String, ErrorCode> {
    say(io, lang, "connecting")?;
    let account = effects.connect_google(lang, GoogleLocalProfile::PersonalReadSend)?;
    say(io, lang, "connected")?;
    say(io, lang, "features_after_connection")?;
    Ok(account)
}

fn work_menu(
    lang: Lang,
    account: &str,
    io: &mut impl Io,
    effects: &mut impl Effects,
) -> Result<(), ErrorCode> {
    let mut current_account = account.to_owned();
    loop {
        say(io, lang, "work_menu")?;
        match choose(
            io,
            lang,
            &[
                "help_work_inbox",
                "help_work_connect",
                "help_work_exit",
                "help_work_enable_send",
                "help_work_compose",
            ],
        )?
        .as_deref()
        {
            None | Some("3") => return Ok(()),
            Some("1") => effects.inbox(&current_account, lang)?,
            Some("2") => {
                current_account = connect_account(lang, io, effects)?;
            }
            Some("4") => effects.enable_send(&current_account, lang)?,
            Some("5") => effects.compose(&current_account, lang, io)?,
            Some(_) => {
                say(io, lang, "invalid_choice")?;
                continue;
            }
        }
    }
}

fn connect_prompt(
    lang: Lang,
    io: &mut impl Io,
    effects: &mut impl Effects,
) -> Result<(), ErrorCode> {
    say(io, lang, "no_accounts")?;
    loop {
        say(io, lang, "connect_or_exit")?;
        match choose(io, lang, &["help_connect", "help_exit"])?.as_deref() {
            Some("1") => {
                let account = connect_account(lang, io, effects)?;
                return work_menu(lang, &account, io, effects);
            }
            None | Some("2") => return Ok(()),
            Some(_) => {
                say(io, lang, "invalid_choice")?;
            }
        }
    }
}

fn select_existing(
    lang: Lang,
    accounts: &[AccountChoice],
    io: &mut impl Io,
    effects: &mut impl Effects,
) -> Result<(), ErrorCode> {
    loop {
        for (index, account) in accounts.iter().enumerate() {
            io.write(&format!(
                "{} {}: {}\n",
                resource(lang, "account"),
                index + 1,
                account.label
            ))?;
        }
        say(io, lang, "add_or_exit")?;
        match choose(
            io,
            lang,
            &[
                "help_account_number",
                "help_account_add",
                "help_account_exit",
            ],
        )?
        .as_deref()
        {
            None | Some("0") => return Ok(()),
            Some("add") => {
                let account = connect_account(lang, io, effects)?;
                return work_menu(lang, &account, io, effects);
            }
            Some(value) => {
                let Ok(index) = value.parse::<usize>() else {
                    say(io, lang, "invalid_choice")?;
                    continue;
                };
                let Some(account) = index.checked_sub(1).and_then(|index| accounts.get(index))
                else {
                    say(io, lang, "invalid_choice")?;
                    continue;
                };
                // Verify the existing read grant without starting OAuth here.
                effects.continue_existing(&account.id, lang)?;
                return work_menu(lang, &account.id, io, effects);
            }
        }
    }
}

pub(super) fn run(lang: Lang) -> Result<(), ErrorCode> {
    let accounts = match hantong_ipc_client::accounts::list_default_accounts(false) {
        Ok(response) => response
            .items()
            .iter()
            .filter(|item| item.provider() == "gmail" && item.status() == "ready")
            .map(|item| AccountChoice {
                id: item.account_id().to_owned(),
                label: super::safe_human(
                    item.display_name()
                        .or(item.primary_address())
                        .unwrap_or("Gmail account"),
                ),
            })
            .collect::<Vec<_>>(),
        Err(ErrorCode::ClientNotRegistered) => Vec::new(),
        Err(code) => return Err(code),
    };
    let mut io = Stdio::new();
    let mut effects = RealEffects;
    run_flow(lang, &accounts, &mut io, &mut effects)
}

pub(super) fn run_flow(
    lang: Lang,
    accounts: &[AccountChoice],
    io: &mut impl Io,
    effects: &mut impl Effects,
) -> Result<(), ErrorCode> {
    say(io, lang, "title")?;
    if accounts.is_empty() {
        return connect_prompt(lang, io, effects);
    }
    select_existing(lang, accounts, io, effects)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestIo {
        input: Vec<Option<String>>,
        cursor: usize,
        output: String,
    }
    impl TestIo {
        fn new(input: &[&str]) -> Self {
            Self {
                input: input.iter().map(|v| Some((*v).into())).collect(),
                cursor: 0,
                output: String::new(),
            }
        }
    }
    impl Io for TestIo {
        fn write(&mut self, text: &str) -> Result<(), ErrorCode> {
            self.output.push_str(text);
            Ok(())
        }
        fn read_line(&mut self) -> Result<Option<String>, ErrorCode> {
            let value = self.input.get(self.cursor).cloned().unwrap_or(None);
            self.cursor += 1;
            Ok(value)
        }
    }

    struct TestEffects {
        connected: Result<String, ErrorCode>,
        inbox_calls: usize,
        enable_send_calls: usize,
        compose_calls: usize,
        connect_calls: usize,
        continue_calls: usize,
        profiles: Vec<GoogleLocalProfile>,
    }
    impl Effects for TestEffects {
        fn connect_google(
            &mut self,
            _: Lang,
            profile: GoogleLocalProfile,
        ) -> Result<String, ErrorCode> {
            self.connect_calls += 1;
            self.profiles.push(profile);
            self.connected.clone()
        }
        fn continue_existing(&mut self, _: &str, _: Lang) -> Result<(), ErrorCode> {
            self.continue_calls += 1;
            Ok(())
        }
        fn enable_send(&mut self, _: &str, _: Lang) -> Result<(), ErrorCode> {
            self.enable_send_calls += 1;
            Ok(())
        }
        fn compose(&mut self, _: &str, _: Lang, _: &mut dyn Io) -> Result<(), ErrorCode> {
            self.compose_calls += 1;
            Ok(())
        }
        fn inbox(&mut self, _: &str, _: Lang) -> Result<(), ErrorCode> {
            self.inbox_calls += 1;
            Ok(())
        }
    }

    fn account() -> AccountChoice {
        AccountChoice {
            id: "google_fixture".into(),
            label: "person@example.test".into(),
        }
    }

    #[test]
    fn acc02_cli04_connection_denied_stops_before_inbox() {
        let mut io = TestIo::new(&["1", "1"]);
        let mut effects = TestEffects {
            connected: Err(ErrorCode::ApprovalDenied),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(
            run_flow(Lang::En, &[], &mut io, &mut effects),
            Err(ErrorCode::ApprovalDenied)
        );
        assert_eq!(effects.inbox_calls, 0);
    }

    #[test]
    fn cli04_exit_before_connection_has_no_effect() {
        let mut io = TestIo::new(&["2"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(run_flow(Lang::En, &[], &mut io, &mut effects), Ok(()));
        assert_eq!(effects.connect_calls, 0);
        assert!(!io.output.contains(resource(Lang::En, "connecting")));
    }

    #[test]
    fn cli04_connect_opens_google_directly_with_read_only_then_shows_features() {
        let mut io = TestIo::new(&["1", "3"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(run_flow(Lang::En, &[], &mut io, &mut effects), Ok(()));
        assert_eq!(effects.profiles, vec![GoogleLocalProfile::PersonalReadSend]);
        assert_eq!(effects.enable_send_calls, 0);
        assert_eq!(effects.connect_calls, 1);
        let connected = io.output.find(resource(Lang::En, "connected")).unwrap();
        let features = io
            .output
            .find(resource(Lang::En, "features_after_connection"))
            .unwrap();
        assert!(connected < features);
        assert!(!io.output.contains("Choose features:"));
        assert_eq!(effects.inbox_calls, 0);
    }

    #[test]
    fn acc02_cli04_add_account_paths_connect_directly_without_send_or_inbox() {
        for input in [&["add", "3"][..], &["1", "2", "3"][..]] {
            let mut io = TestIo::new(input);
            let mut effects = TestEffects {
                connected: Ok("google_new".into()),
                inbox_calls: 0,
                enable_send_calls: 0,
                compose_calls: 0,
                connect_calls: 0,
                continue_calls: 0,
                profiles: Vec::new(),
            };
            assert_eq!(
                run_flow(Lang::En, &[account()], &mut io, &mut effects),
                Ok(())
            );
            assert_eq!(effects.profiles, vec![GoogleLocalProfile::PersonalReadSend]);
            assert_eq!(effects.connect_calls, 1);
            assert_eq!(effects.enable_send_calls, 0);
            assert_eq!(effects.inbox_calls, 0);
            assert!(!io.output.contains("Choose features:"));
        }
    }

    #[test]
    fn acc02_existing_ready_account_verifies_without_starting_oauth() {
        let mut io = TestIo::new(&["1", "3"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(
            run_flow(Lang::En, &[account()], &mut io, &mut effects),
            Ok(())
        );
        assert_eq!(effects.connect_calls, 0);
        assert_eq!(effects.continue_calls, 1);
    }

    #[test]
    fn cli04_explicit_inbox_selection_only_calls_inbox() {
        let mut io = TestIo::new(&["1", "1", "3"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(
            run_flow(Lang::En, &[account()], &mut io, &mut effects),
            Ok(())
        );
        assert_eq!(effects.inbox_calls, 1);
        assert_eq!(effects.connect_calls, 0);
        assert_eq!(effects.continue_calls, 1);
    }

    #[test]
    fn beta_enable_send_is_explicit_and_does_not_send() {
        let mut io = TestIo::new(&["1", "4", "3"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(
            run_flow(Lang::En, &[account()], &mut io, &mut effects),
            Ok(())
        );
        assert_eq!(effects.enable_send_calls, 1);
        assert_eq!(effects.inbox_calls, 0);
        assert_eq!(effects.connect_calls, 0);
    }

    #[test]
    fn beta_compose_is_explicit_and_does_not_send_from_menu() {
        let mut io = TestIo::new(&["1", "5", "3"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(
            run_flow(Lang::En, &[account()], &mut io, &mut effects),
            Ok(())
        );
        assert_eq!(effects.compose_calls, 1);
        assert_eq!(effects.inbox_calls, 0);
        assert_eq!(effects.enable_send_calls, 0);
    }

    #[test]
    fn cli04_eof_cancels_without_effects() {
        let mut io = TestIo::new(&[]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(run_flow(Lang::En, &[], &mut io, &mut effects), Ok(()));
        assert_eq!(effects.connect_calls, 0);
    }

    #[test]
    fn cli04_input_is_bounded_to_256_bytes() {
        assert!(decode_line(&vec![b'x'; 256]).is_ok());
        assert_eq!(decode_line(&vec![b'x'; 257]), Err(ErrorCode::InvalidInput));
    }

    #[test]
    fn cli04_compose_text_preserves_spaces_and_bounds_full_paragraphs() {
        assert_eq!(
            decode_text_line(b"  indented  \r\n", 20).unwrap(),
            "  indented  "
        );
        let paragraph = "한".repeat(1000);
        assert_eq!(
            decode_text_line(paragraph.as_bytes(), 3000).unwrap(),
            paragraph
        );
        assert_eq!(
            decode_text_line(paragraph.as_bytes(), 2999),
            Err(ErrorCode::QuotaExceeded)
        );
        assert_eq!(decode_text_line(b"\xff", 10), Err(ErrorCode::InvalidInput));
    }

    #[test]
    fn acc02_cli04_zero_alias_does_not_select_an_account() {
        let mut io = TestIo::new(&["00", "0"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(
            run_flow(Lang::En, &[account()], &mut io, &mut effects),
            Ok(())
        );
        assert_eq!(effects.continue_calls, 0);
        assert_eq!(effects.connect_calls, 0);
        assert_eq!(effects.inbox_calls, 0);
        assert!(io.output.contains(resource(Lang::En, "invalid_choice")));
    }

    #[test]
    fn cli04_help_is_local_and_keeps_the_current_menu_open() {
        let mut io = TestIo::new(&["1", "/help", "3"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(run_flow(Lang::En, &[], &mut io, &mut effects), Ok(()));
        assert_eq!(effects.connect_calls, 1);
        assert_eq!(effects.profiles, vec![GoogleLocalProfile::PersonalReadSend]);
        assert!(io.output.contains("Available choices:"));
        assert!(io.output.contains("4) Enable composing and sending"));
    }

    #[test]
    fn cli04_help_is_localized_for_the_basic_menu() {
        let mut io = TestIo::new(&["/help", "/help", "2"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(run_flow(Lang::Ko, &[], &mut io, &mut effects), Ok(()));
        assert_eq!(effects.connect_calls, 0);
        assert!(io.output.contains("사용 가능한 선택:"));
        assert!(io.output.contains("1) Google 계정 연결 —"));
        assert_eq!(io.output.matches("사용 가능한 선택:").count(), 2);
        assert_eq!(effects.inbox_calls, 0);
        assert_eq!(effects.compose_calls, 0);
        assert_eq!(effects.enable_send_calls, 0);
        assert_eq!(effects.continue_calls, 0);
    }

    #[test]
    fn cli04_account_help_uses_zero_to_exit_without_effects() {
        let mut io = TestIo::new(&["/help", "0"]);
        let mut effects = TestEffects {
            connected: Ok("google_new".into()),
            inbox_calls: 0,
            enable_send_calls: 0,
            compose_calls: 0,
            connect_calls: 0,
            continue_calls: 0,
            profiles: Vec::new(),
        };
        assert_eq!(
            run_flow(Lang::En, &[account()], &mut io, &mut effects),
            Ok(())
        );
        assert!(io.output.contains("0) Exit —"));
        assert!(!io.output.contains("2) Exit —"));
        assert_eq!(effects.connect_calls, 0);
        assert_eq!(effects.continue_calls, 0);
        assert_eq!(effects.inbox_calls, 0);
        assert_eq!(effects.compose_calls, 0);
        assert_eq!(effects.enable_send_calls, 0);
    }
}
