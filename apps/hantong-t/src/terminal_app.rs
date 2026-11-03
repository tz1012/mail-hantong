//! Full-screen presentation over the existing CLI/Core boundary.
use super::*;
use crate::terminal_view::{Frame, Hit, Theme, body_line_count, hit_test, render, text};
use hantong_platform::{MouseKind, TerminalEvent, TerminalKey, TerminalSession};

struct State {
    accounts: Value,
    folders: Value,
    messages: Value,
    account: Option<Value>,
    folder: Option<Value>,
    screen: u8,
    selected: usize,
    body: String,
    offset: usize,
    theme: Theme,
    help: bool,
    status: String,
    demo: bool,
    width: u16,
    lang: Lang,
    preferences: hantong_platform::ui_preferences::Preferences,
    unified: Option<crate::unified::Unified>,
    expanded: bool,
}
impl State {
    fn new(theme: Theme, demo: bool) -> Self {
        Self {
            accounts: serde_json::json!({"items":[]}),
            folders: serde_json::json!({"items":[]}),
            messages: serde_json::json!({"items":[]}),
            account: None,
            folder: None,
            screen: 0,
            selected: 0,
            body: String::new(),
            offset: 0,
            theme,
            help: false,
            status: String::new(),
            demo,
            width: 120,
            lang: Lang::En,
            preferences: Default::default(),
            unified: None,
            expanded: false,
        }
    }
    fn body_limit(&self) -> usize {
        if self.expanded {
            crate::terminal_view::expanded_body_line_count(&self.body, self.width)
        } else {
            body_line_count(&self.body, self.width)
        }
    }
    fn display_account(&self, item: &Value, index: usize) -> String {
        let label = account_label(item, index);
        item.get("account_id")
            .and_then(Value::as_str)
            .and_then(|id| self.preferences.alias(id))
            .map(|alias| format!("{alias} / {label}"))
            .unwrap_or(label)
    }
    fn selected_account(&self) -> Option<&Value> {
        if self.screen >= 2
            && let Some(unified) = &self.unified
        {
            return unified.selected(self.selected).ok().map(|v| v.0);
        }
        if self.screen == 0 {
            rows(&self.accounts).get(self.selected)
        } else {
            self.account.as_ref()
        }
    }
    fn source_state(&self) -> Result<State, String> {
        let (account, folder, page, item) = self
            .unified
            .as_ref()
            .ok_or("Unified source unavailable")?
            .selected(self.selected)?;
        let mut state = State::new(self.theme, self.demo);
        state.lang = self.lang;
        state.account = Some(account.clone());
        state.folder = Some(folder.clone());
        state.messages = page.clone();
        state.screen = 2;
        state.selected = rows(page)
            .iter()
            .position(|v| v == item)
            .ok_or("Message source changed")?;
        Ok(state)
    }
    fn open_unified(&mut self, runner: &Runner) {
        let unified = crate::unified::load(
            &self.accounts,
            self.preferences.aliases(),
            self.demo,
            &mut |args| runner.call(args),
        );
        self.status = if unified.failures.is_empty() {
            text(self.lang == Lang::Ko, "unified_limit").into()
        } else {
            format!(
                "{}: {}",
                text(self.lang == Lang::Ko, "partial"),
                unified
                    .failures
                    .iter()
                    .map(|(tag, error)| format!("[{tag}] {error}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        };
        self.messages = unified.page.clone();
        self.unified = Some(unified);
        self.screen = 2;
        self.selected = 0;
        self.body.clear();
        self.offset = 0;
        self.expanded = false;
    }
    fn page(&self) -> &Value {
        match self.screen {
            0 => &self.accounts,
            1 => &self.folders,
            _ => &self.messages,
        }
    }
    fn account_id(&self) -> Result<&str, String> {
        self.account
            .as_ref()
            .and_then(|v| v.get("account_id"))
            .and_then(Value::as_str)
            .ok_or_else(|| text(self.lang == Lang::Ko, "select_account").into())
    }
    fn gmail(&self) -> bool {
        self.account
            .as_ref()
            .and_then(|v| v.get("provider"))
            .and_then(Value::as_str)
            == Some("gmail")
    }
    fn labels(&self, value: &Value) -> Vec<String> {
        rows(value)
            .iter()
            .enumerate()
            .map(|(index, item)| {
                if item.get("provider").is_some() {
                    self.display_account(item, index)
                } else {
                    projected_label(value, item)
                }
            })
            .collect()
    }
    fn draw(&self, terminal: &TerminalSession, lang: Lang) -> io::Result<()> {
        let (width, height) = terminal.size();
        let accounts = self.labels(&self.accounts);
        let folders = self.labels(&self.folders);
        let list = self.labels(self.page());
        let account = if self.unified.is_some() && self.screen == 2 {
            text(lang == Lang::Ko, "unified").to_owned()
        } else {
            self.account
                .as_ref()
                .map(|v| self.display_account(v, 0))
                .unwrap_or_default()
        };
        let folder = self.folder.as_ref().map(item_label).unwrap_or_default();
        let frame = Frame {
            lang_ko: lang == Lang::Ko,
            width,
            height,
            theme: self.theme,
            screen: if self.expanded { 4 } else { self.screen },
            selected: self.selected,
            account: &account,
            folder: &folder,
            accounts: &accounts,
            folders: &folders,
            rows: &list,
            body: &self.body,
            body_offset: self.offset,
            status: &self.status,
            help: self.help,
        };
        print!("{}", render(&frame));
        io::stdout().flush()
    }
    fn move_selection(&mut self, delta: isize) {
        if self.screen == 3 {
            self.offset = self
                .offset
                .saturating_add_signed(delta)
                .min(self.body_limit().saturating_sub(1));
        } else {
            self.select_index(self.selected.saturating_add_signed(delta));
        }
    }
    fn select_index(&mut self, index: usize) {
        let next = index.min(rows(self.page()).len().saturating_sub(1));
        if self.selected != next && self.screen >= 2 {
            self.body.clear();
            self.offset = 0;
        }
        self.selected = next;
    }
    fn back(&mut self) {
        if self.expanded {
            self.expanded = false;
            return;
        }
        if self.help {
            self.help = false;
            return;
        }
        match self.screen {
            3 => self.screen = 2,
            2 => {
                if self.unified.is_some() {
                    self.screen = 0;
                    self.selected = 0;
                    return;
                }
                self.screen = 1;
                self.selected = rows(&self.folders)
                    .iter()
                    .position(|v| Some(v) == self.folder.as_ref())
                    .unwrap_or(0);
            }
            1 => {
                self.screen = 0;
                self.selected = rows(&self.accounts)
                    .iter()
                    .position(|v| Some(v) == self.account.as_ref())
                    .unwrap_or(0);
            }
            _ => {}
        }
    }
    fn open(&mut self, runner: &Runner) -> Result<(), String> {
        if self.unified.is_some() && self.screen == 2 {
            let mut source = self.source_state()?;
            source.open(runner)?;
            self.account = source.account;
            self.folder = source.folder;
            self.body = source.body;
            self.screen = 3;
            self.offset = 0;
            return Ok(());
        }

        if self.screen == 3 {
            return Ok(());
        }
        let item = rows(self.page())
            .get(self.selected)
            .cloned()
            .ok_or_else(|| text(runner.lang == Lang::Ko, "empty").to_owned())?;
        match self.screen {
            0 => {
                self.unified = None;
                self.expanded = false;
                self.account = Some(item);
                self.folders = serde_json::json!({"items":[]});
                self.screen = 1;
                self.selected = 0;
                self.folder = None;
                self.messages = serde_json::json!({"items":[]});
                self.body.clear();
                self.folders = if self.demo {
                    demo_folders()
                } else {
                    load_folders(runner, self.account_id()?, self.gmail())?
                };
                self.screen = 1;
                self.selected = 0;
            }
            1 => {
                self.folder = Some(item);
                self.messages = serde_json::json!({"items":[]});
                self.body.clear();
                self.screen = 2;
                self.selected = 0;
                self.messages = if self.demo {
                    demo_messages()
                } else {
                    self.load_messages(runner)?
                };
                self.body.clear();
                self.screen = 2;
                self.selected = 0;
            }
            _ => {
                self.body.clear();
                self.offset = 0;
                self.body = if self.demo {
                    format!("Message {:02}\n{}", self.selected + 1, demo_body())
                } else {
                    let id = self.account_id()?;
                    let message = field(
                        &item,
                        if self.gmail() {
                            "id"
                        } else {
                            "internal_message_id"
                        },
                    )
                    .ok_or_else(|| text(self.lang == Lang::Ko, "message_identity").to_owned())?;
                    let value = if self.gmail() {
                        runner.call(&[
                            "gmail",
                            "read",
                            "--account",
                            id,
                            "--message-id",
                            &message,
                        ])?
                    } else {
                        runner.call(&["read", "--account", id, "--message-id", &message])?
                    };
                    let mut body = value
                        .pointer(if self.gmail() { "/body/text" } else { "/text" })
                        .and_then(Value::as_str)
                        .ok_or_else(|| text(self.lang == Lang::Ko, "body_unavailable").to_owned())?
                        .to_owned();
                    if value.get("truncated").and_then(Value::as_bool) == Some(true) {
                        body.push_str("\n\n");
                        body.push_str(text(self.lang == Lang::Ko, "body_truncated"));
                    }
                    body
                };
                self.offset = 0;
                self.screen = 3;
            }
        }
        Ok(())
    }
    fn load_messages(&self, runner: &Runner) -> Result<Value, String> {
        let id = self.account_id()?;
        let folder = self
            .folder
            .as_ref()
            .ok_or_else(|| text(self.lang == Lang::Ko, "select_folder").to_owned())?;
        if self.gmail() {
            let label = field(folder, "id")
                .ok_or_else(|| text(self.lang == Lang::Ko, "folder_identity").to_owned())?;
            runner
                .call(&[
                    "gmail",
                    "list",
                    "--account",
                    id,
                    "--label-id",
                    &label,
                    "--limit",
                    "32",
                ])
                .map(google_items)
        } else {
            let wire = field(folder, "wire_hex")
                .ok_or_else(|| text(self.lang == Lang::Ko, "folder_identity").to_owned())?;
            let encoding = field(folder, "encoding")
                .ok_or_else(|| text(self.lang == Lang::Ko, "folder_encoding").to_owned())?;
            runner.call(&[
                "search",
                "--account",
                id,
                "--wire-hex",
                &wire,
                "--encoding",
                &encoding,
                "--limit",
                "32",
            ])
        }
    }
    fn refresh(&mut self, runner: &Runner) -> Result<(), String> {
        if self.unified.is_some() && self.screen >= 2 {
            self.open_unified(runner);
            return Ok(());
        }

        if self.demo {
            return Ok(());
        }
        if self.screen == 0 {
            self.accounts = runner.call(&["accounts", "list"])?;
            return Ok(());
        }
        let id = self.account_id()?.to_owned();
        let gmail = self.gmail();
        if self.screen == 1 && !gmail {
            let generation = self
                .folders
                .pointer("/metadata/generation")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let operation = refresh_operation_id("folders");
            refresh_imap_folders_with(
                &id,
                generation,
                &operation,
                &operation,
                &mut |args| runner.call(args),
                &mut thread::sleep,
            )?;
        } else if gmail {
            let folder = if self.screen == 1 {
                rows(&self.folders).get(self.selected)
            } else {
                self.folder.as_ref()
            };
            let label = folder
                .and_then(|v| v.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("INBOX");
            refresh_google_with(
                &id,
                label,
                Duration::from_secs(600),
                &mut |args| runner.call(args),
                &mut thread::sleep,
            )?;
        } else {
            let folder = self
                .folder
                .as_ref()
                .ok_or_else(|| text(self.lang == Lang::Ko, "select_folder").to_owned())?;
            let wire = field(folder, "wire_hex")
                .ok_or_else(|| text(self.lang == Lang::Ko, "folder_identity").to_owned())?;
            let encoding = field(folder, "encoding")
                .ok_or_else(|| text(self.lang == Lang::Ko, "folder_encoding").to_owned())?;
            refresh_imap_initial_preview_with(
                &id,
                &wire,
                &encoding,
                &refresh_operation_id("recent"),
                Duration::from_secs(300),
                &mut |args| runner.call(args),
                &mut thread::sleep,
            )?;
        }
        if self.screen == 1 {
            self.folders = load_folders(runner, &id, gmail)?;
        } else {
            self.messages = self.load_messages(runner)?;
            self.body.clear();
            self.screen = 2;
            self.offset = 0;
        }
        self.selected = self.selected.min(rows(self.page()).len().saturating_sub(1));
        Ok(())
    }
    fn trash(&self, runner: &Runner) -> Result<(), String> {
        if self.unified.is_some() {
            return self.source_state()?.trash(runner);
        }

        if self.screen < 2 {
            return Err(text(runner.lang == Lang::Ko, "select_message").into());
        }
        let id = self.account_id()?;
        let gmail = self.gmail();
        let mut item = rows(&self.messages)
            .get(self.selected)
            .cloned()
            .ok_or_else(|| text(self.lang == Lang::Ko, "select_message").to_owned())?;
        let folder = self
            .folder
            .as_ref()
            .ok_or_else(|| text(self.lang == Lang::Ko, "select_folder").to_owned())?;
        let identity = if gmail {
            (
                field(folder, "id")
                    .ok_or_else(|| text(self.lang == Lang::Ko, "folder_identity").to_owned())?,
                "gmail".to_owned(),
            )
        } else {
            (
                field(folder, "wire_hex")
                    .ok_or_else(|| text(self.lang == Lang::Ko, "folder_identity").to_owned())?,
                field(folder, "encoding")
                    .ok_or_else(|| text(self.lang == Lang::Ko, "folder_encoding").to_owned())?,
            )
        };
        if !gmail {
            let message = field(&item, "internal_message_id")
                .ok_or_else(|| text(self.lang == Lang::Ko, "message_identity").to_owned())?;
            let located = runner.call(&["read", "--account", id, "--message-id", &message])?;
            if located.get("account_id").and_then(Value::as_str) != Some(id)
                || located.get("mailbox_wire_hex").and_then(Value::as_str)
                    != Some(identity.0.as_str())
                || located.get("mailbox_encoding").and_then(Value::as_str)
                    != Some(identity.1.as_str())
                || located.get("uid").and_then(Value::as_u64)
                    != item.get("uid").and_then(Value::as_u64)
            {
                return Err(text(self.lang == Lang::Ko, "message_changed").into());
            }
            let inspection = runner.call(&[
                "sync",
                "inspect",
                "--account",
                id,
                "--wire-hex",
                &identity.0,
                "--encoding",
                &identity.1,
            ])?;
            if inspection.get("account_id").and_then(Value::as_str) != Some(id)
                || inspection.get("wire_hex").and_then(Value::as_str) != Some(identity.0.as_str())
                || inspection.get("encoding").and_then(Value::as_str) != Some(identity.1.as_str())
            {
                return Err(text(self.lang == Lang::Ko, "folder_changed").into());
            }
            item["uidvalidity"] = located["uidvalidity"].clone();
            item["catalog_generation"] = inspection["catalog_generation"].clone();
        }
        runner.interactive_owned(&selected_trash_args(
            id,
            gmail,
            Some(&identity),
            &self.messages,
            &item,
        )?)
    }
}
fn account_label(item: &Value, index: usize) -> String {
    [
        "primary_address",
        "address",
        "display_name",
        "name",
        "login",
    ]
    .iter()
    .find_map(|key| {
        item.get(key)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
    })
    .map(sanitize_single_line)
    .unwrap_or_else(|| format!("Account {}", index + 1))
}
fn load_folders(runner: &Runner, id: &str, gmail: bool) -> Result<Value, String> {
    if gmail {
        runner
            .call(&["gmail", "labels", "--account", id])
            .map(google_items)
            .map(|value| inbox_first(value, true))
    } else {
        runner
            .call(&["folders", "list", "--account", id])
            .map(|value| inbox_first(value, false))
    }
}
fn demo_accounts() -> Value {
    serde_json::json!({"items":[{"account_id":"demo","primary_address":"reader@example.invalid","provider":"imap"},{"account_id":"demo2","primary_address":"work@example.invalid","provider":"gmail"}]})
}
fn demo_folders() -> Value {
    serde_json::json!({"items":[{"name":"Inbox","wire_hex":"494e424f58","encoding":"modified_utf7"},{"name":"Sent"},{"name":"Trash"}]})
}
fn demo_messages() -> Value {
    serde_json::json!({"items":(0..24).map(|i|serde_json::json!({"subject":format!("Welcome to HANTONG / terminal letter {:02}",i+1),"sender":"friend@example.invalid"})).collect::<Vec<_>>()})
}
fn demo_body() -> String {
    (1..=60).map(|i|format!("{i:02}  한통 통신 터미널 / Welcome to HANTONG\n     Keyboard and mouse, one connected workspace.\n")).collect()
}

pub(super) fn run(args: &[String]) -> io::Result<()> {
    let ko = args.windows(2).any(|v| v == ["--lang", "ko"]);
    let lang = if ko { Lang::Ko } else { Lang::En };
    let theme = match args.iter().position(|v| v == "--theme") {
        Some(i) => args
            .get(i + 1)
            .and_then(|v| Theme::parse(v))
            .ok_or_else(|| io::Error::other(text(ko, "invalid_theme")))?,
        None => Theme::Blue,
    };
    let demo = args.iter().any(|v| v == "--demo");
    let runner = Runner {
        program: if demo {
            std::path::PathBuf::new()
        } else {
            Runner::sibling().map_err(io::Error::other)?
        },
        lang,
    };
    let mut state = State::new(theme, demo);
    state.lang = lang;
    if !demo {
        match hantong_platform::ui_preferences::load() {
            Ok(p) => state.preferences = p,
            Err(e) => state.status = e,
        }
    }

    let mut session = Some(TerminalSession::enter().map_err(|e| io::Error::other(e.as_str()))?);
    state.status = text(ko, "loading").into();
    state.draw(session.as_ref().unwrap(), lang)?;
    match if demo {
        Ok(demo_accounts())
    } else {
        runner.call(&["accounts", "list"])
    } {
        Ok(v) => {
            state.accounts = v;
            state.status = text(ko, "ready").into();
        }
        Err(e) => state.status = e,
    }
    if demo {
        state.account = rows(&state.accounts).first().cloned();
        state.folders = demo_folders();
        state.folder = rows(&state.folders).first().cloned();
        state.messages = demo_messages();
        state.body = demo_body();
        state.screen = 2;
    }
    if let Some(index) = args.iter().position(|v| v == "--account")
        && let Some(id) = args.get(index + 1)
    {
        if let Some(index) = rows(&state.accounts)
            .iter()
            .position(|v| v.get("account_id").and_then(Value::as_str) == Some(id))
        {
            state.screen = 0;
            state.selected = index;
            if let Err(e) = state.open(&runner) {
                state.status = e;
            }
        } else {
            state.status = text(ko, "select_account").into();
        }
    }
    loop {
        if session.is_none() {
            session = Some(TerminalSession::enter().map_err(|e| io::Error::other(e.as_str()))?);
        }
        let terminal = session.as_mut().unwrap();
        state.width = terminal.size().0;
        state.draw(terminal, lang)?;
        let event = loop {
            let event = terminal
                .read_event()
                .map_err(|e| io::Error::other(e.as_str()))?;
            if event != TerminalEvent::Tick {
                break event;
            }
        };
        let key = match event {
            TerminalEvent::Tick | TerminalEvent::Resize => continue,
            TerminalEvent::Key(key) => key,
            TerminalEvent::Mouse { column, row, kind } => {
                let (width, height) = terminal.size();
                let hit = hit_test(
                    width,
                    height,
                    column,
                    row,
                    if state.expanded { 4 } else { state.screen },
                    state.selected,
                    rows(state.page()).len(),
                );
                if matches!(kind, MouseKind::ScrollUp | MouseKind::ScrollDown) {
                    if hit == Hit::Body {
                        state.offset = state
                            .offset
                            .saturating_add_signed(if kind == MouseKind::ScrollUp { -3 } else { 3 })
                            .min(state.body_limit().saturating_sub(1));
                    } else {
                        state.move_selection(if kind == MouseKind::ScrollUp { -3 } else { 3 });
                    }
                    continue;
                }
                match hit {
                    Hit::Row(index) => {
                        if state.screen == 3 {
                            state.screen = 2;
                        }
                        state.select_index(index);
                        if kind == MouseKind::DoubleClick {
                            TerminalKey::Enter
                        } else {
                            continue;
                        }
                    }
                    Hit::Account(index) => {
                        if index >= rows(&state.accounts).len() {
                            continue;
                        }
                        state.screen = 0;
                        state.selected = index;
                        if kind == MouseKind::DoubleClick {
                            TerminalKey::Enter
                        } else {
                            continue;
                        }
                    }
                    Hit::Folder(index) => {
                        if state.unified.is_some() {
                            continue;
                        }
                        if index >= rows(&state.folders).len() {
                            continue;
                        }
                        state.screen = 1;
                        state.selected = index;
                        if kind == MouseKind::DoubleClick {
                            TerminalKey::Enter
                        } else {
                            continue;
                        }
                    }
                    Hit::Body => {
                        if !state.body.is_empty() {
                            state.screen = 3;
                        }
                        continue;
                    }
                    Hit::Theme => TerminalKey::Char('F'),
                    Hit::Compose => TerminalKey::Char('c'),
                    Hit::Refresh => TerminalKey::Char('r'),
                    Hit::Back => TerminalKey::Escape,
                    Hit::Help => TerminalKey::Char('?'),
                    Hit::None => continue,
                }
            }
        };
        match key {
            TerminalKey::Quit | TerminalKey::Char('q') => break,
            TerminalKey::Char('F' | 'f') => state.theme = state.theme.next(),
            TerminalKey::Char('?') => state.help = !state.help,
            TerminalKey::Char('u') => {
                state.status = text(ko, "loading").into();
                state.draw(session.as_ref().unwrap(), lang)?;
                state.open_unified(&runner);
            }
            TerminalKey::Char('v') if !state.body.is_empty() => {
                state.expanded = !state.expanded;
                state.screen = 3;
                state.offset = 0;
            }

            TerminalKey::Up | TerminalKey::Char('k') => state.move_selection(-1),
            TerminalKey::Down | TerminalKey::Char('j') => state.move_selection(1),
            TerminalKey::Home => {
                if state.screen == 3 {
                    state.offset = 0
                } else {
                    state.select_index(0)
                }
            }
            TerminalKey::End => {
                if state.screen == 3 {
                    state.offset = state.body_limit().saturating_sub(1)
                } else {
                    state.select_index(rows(state.page()).len().saturating_sub(1))
                }
            }
            TerminalKey::PageUp | TerminalKey::PageDown => {
                state.offset = state
                    .offset
                    .saturating_add_signed(if key == TerminalKey::PageUp { -10 } else { 10 })
                    .min(state.body_limit().saturating_sub(1));
            }
            TerminalKey::Left | TerminalKey::Escape | TerminalKey::Char('h') => state.back(),
            TerminalKey::Tab | TerminalKey::BackTab => {
                if state.unified.is_some() && state.screen >= 2 {
                    state.screen = if state.screen == 2 && !state.body.is_empty() {
                        3
                    } else {
                        2
                    };
                    state.expanded = false;
                    continue;
                }
                if state.selected != 0 {
                    state.body.clear();
                    state.offset = 0;
                }
                state.expanded = false;
                state.screen = if key == TerminalKey::BackTab {
                    state.screen.saturating_sub(1)
                } else {
                    match state.screen {
                        0 if state.account.is_some() => 1,
                        1 if state.folder.is_some() => 2,
                        2 if !state.body.is_empty() => 3,
                        _ => 0,
                    }
                };
                state.selected = 0;
            }
            TerminalKey::Enter | TerminalKey::Right | TerminalKey::Char('r') => {
                state.status = text(ko, "loading").into();
                state.draw(session.as_ref().unwrap(), lang)?;
                let result = if key == TerminalKey::Char('r') {
                    state.refresh(&runner)
                } else {
                    state.open(&runner)
                };
                state.status = result.err().unwrap_or_else(|| text(ko, "ready").into());
            }
            TerminalKey::Char('c' | 't' | ':' | 'a') => {
                if demo {
                    state.status = text(ko, "demo").into();
                    continue;
                }
                // Restore cooked modes and the original screen before any Core-owned prompt.
                drop(session.take());
                let result = match key {
                    TerminalKey::Char('c') if state.screen == 0 => {
                        Err(text(ko, "select_account").into())
                    }
                    TerminalKey::Char('c') => state
                        .selected_account()
                        .and_then(|v| v.get("account_id"))
                        .and_then(Value::as_str)
                        .ok_or_else(|| text(ko, "select_account").to_owned())
                        .and_then(|id| runner.interactive(&["compose", "--account", id])),
                    TerminalKey::Char('a') => {
                        let id = state
                            .selected_account()
                            .and_then(|v| v.get("account_id"))
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                        if let Some(id) = id {
                            println!("{}", text(ko, "alias_prompt"));
                            io::stdout().flush()?;
                            if let Some(alias) = bounded_command(io::stdin().lock()) {
                                if alias == "/cancel" {
                                    Ok(())
                                } else {
                                    match hantong_platform::ui_preferences::set(
                                        &state.preferences,
                                        &id,
                                        &alias,
                                    ) {
                                        Ok(p) => {
                                            state.preferences = p;
                                            if let Some(unified) = &mut state.unified {
                                                unified.retag(state.preferences.aliases());
                                                state.messages = unified.page.clone();
                                            }
                                            Ok(())
                                        }
                                        Err(e) => {
                                            if let Ok(p) = hantong_platform::ui_preferences::load()
                                            {
                                                state.preferences = p;
                                            }
                                            Err(e)
                                        }
                                    }
                                }
                            } else {
                                Ok(())
                            }
                        } else {
                            Err(text(ko, "select_account").into())
                        }
                    }
                    TerminalKey::Char('t') => state.trash(&runner),
                    _ => {
                        print!("hantong > ");
                        io::stdout().flush()?;
                        if let Some(command) = bounded_command(io::stdin().lock()) {
                            match google_login_args(&command) {
                                Ok(Some(args)) => runner.interactive_owned(&args),
                                Err(e) => Err(e),
                                Ok(None) => match command.as_str() {
                                    "ai codex" => runner.interactive(&["ai", "chat", "codex"]),
                                    "ai claude" => runner.interactive(&["ai", "chat", "claude"]),
                                    "ai login codex" => {
                                        runner.interactive(&["ai", "login", "codex"])
                                    }
                                    "ai login claude" => {
                                        runner.interactive(&["ai", "login", "claude"])
                                    }
                                    _ => Err(text(ko, "command_help").into()),
                                },
                            }
                        } else {
                            Ok(())
                        }
                    }
                };
                state.status = result.err().unwrap_or_else(|| text(ko, "ready").into());
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tui01_unified_aliases_never_replace_the_selected_source_identity() {
        let runner = Runner {
            program: std::path::PathBuf::new(),
            lang: Lang::En,
        };
        let mut state = State::new(Theme::Bbs16, true);
        state.accounts = demo_accounts();
        state.preferences.set("demo", "Shared tag").unwrap();
        state.preferences.set("demo2", "Shared tag").unwrap();
        state.open_unified(&runner);
        for index in 0..rows(&state.messages).len() {
            state.selected = index;
            let (account, folder, page, item) =
                state.unified.as_ref().unwrap().selected(index).unwrap();
            let source = state.source_state().unwrap();
            assert_eq!(source.account.as_ref(), Some(account));
            assert_eq!(source.folder.as_ref(), Some(folder));
            assert_eq!(&source.messages, page);
            assert_eq!(&rows(&source.messages)[source.selected], item);
        }
        state.back();
        assert_eq!(state.screen, 0);
    }
    #[test]
    fn tui01_account_labels_never_expose_internal_id() {
        assert_eq!(
            account_label(&serde_json::json!({"account_id":"private_internal"}), 2),
            "Account 3"
        );
        assert_eq!(
            account_label(
                &serde_json::json!({"primary_address":"name@example.invalid","account_id":"private"}),
                0
            ),
            "name@example.invalid"
        );
    }
    #[test]
    fn tui01_demo_navigation_is_bounded_without_a_cli() {
        let runner = Runner {
            program: std::path::PathBuf::new(),
            lang: Lang::En,
        };
        let mut s = State::new(Theme::Blue, true);
        s.accounts = demo_accounts();
        s.open(&runner).unwrap();
        assert_eq!(s.screen, 1);
        s.open(&runner).unwrap();
        assert_eq!(s.screen, 2);
        s.move_selection(999);
        assert_eq!(s.selected, 23);
        s.open(&runner).unwrap();
        assert_eq!(s.screen, 3);
        s.move_selection(999);
        assert!(s.offset < body_line_count(&s.body, s.width));
        s.back();
        assert_eq!(s.screen, 2);
        assert_eq!(s.selected, 23);
        assert_eq!(s.theme, Theme::Blue);
    }

    #[test]
    fn tui01_failed_account_switch_never_keeps_previous_mail_data() {
        let runner = Runner {
            program: std::path::PathBuf::new(),
            lang: Lang::En,
        };
        let mut s = State::new(Theme::Amber, false);
        s.accounts = demo_accounts();
        s.account = rows(&s.accounts).first().cloned();
        s.folders = demo_folders();
        s.messages = demo_messages();
        s.body = "old account body".into();
        s.selected = 1;
        assert!(s.open(&runner).is_err());
        assert!(rows(&s.folders).is_empty());
        assert!(rows(&s.messages).is_empty());
        assert!(s.body.is_empty());
        assert_eq!(s.screen, 1);
        assert_eq!(s.account_id().unwrap(), "demo2");
        assert_eq!(s.theme, Theme::Amber);
    }
}
