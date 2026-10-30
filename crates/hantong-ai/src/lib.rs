#![forbid(unsafe_code)]
//! Explicit user chat only; provider-owned authentication, no automatic mail context.
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod agent;
mod catalog;
pub use catalog::{Model, Preferences, effort_for_mode};
const MAX_FRAME: usize = 1_048_576;
const MAX_STREAM: usize = 8 * 1024 * 1024;
const MAX_PROMPT: usize = 32 * 1024;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provider {
    Codex,
    Claude,
}
impl Provider {
    pub fn parse(s: &str) -> Result<Self, AiError> {
        match s {
            "codex" | "openai" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            _ => Err(AiError::InvalidInput),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AiError {
    InvalidInput,
    ProviderMissing,
    Io,
    Protocol,
    Timeout,
    TooLarge,
    LoginRequired,
    SubscriptionUnsupported,
    ProviderFailed,
    Cancelled,
    BrowserFailed,
    Unsupported,
    AgentUnavailable,
}
impl AiError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidInput => "AI_INVALID_INPUT",
            Self::ProviderMissing => "AI_PROVIDER_MISSING",
            Self::Io => "AI_IO_ERROR",
            Self::Protocol => "AI_PROTOCOL_ERROR",
            Self::Timeout => "AI_TIMEOUT",
            Self::TooLarge => "AI_SIZE_LIMIT",
            Self::LoginRequired => "AI_LOGIN_REQUIRED",
            Self::SubscriptionUnsupported => "CLAUDE_SUBSCRIPTION_UNSUPPORTED",
            Self::ProviderFailed => "AI_PROVIDER_FAILED",
            Self::Cancelled => "AI_CANCELLED",
            Self::BrowserFailed => "AI_BROWSER_FAILED",
            Self::Unsupported => "AI_SETTING_UNSUPPORTED",
            Self::AgentUnavailable => "AI_AGENT_NOT_REGISTERED_OR_CONSENT_MISSING",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthStatus {
    pub authenticated: bool,
    pub mode: Option<String>,
}
pub fn available(p: Provider) -> bool {
    resolve(p).is_ok()
}
fn resolve(p: Provider) -> Result<PathBuf, AiError> {
    let path = env::var_os("PATH").ok_or(AiError::ProviderMissing)?;
    for directory in env::split_paths(&path).filter(|v| v.is_absolute()) {
        let binary = directory.join(if cfg!(windows) {
            format!("{}.exe", p.name())
        } else {
            p.name().to_owned()
        });
        if binary.is_file() {
            return Ok(binary);
        }
        #[cfg(windows)]
        {
            let candidates:Vec<PathBuf>=match p{
    Provider::Codex=>vec![directory.join("node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe"),directory.join("node_modules/@openai/codex/vendor/x86_64-pc-windows-msvc/codex/codex.exe")],
    Provider::Claude=>vec![directory.join("node_modules/@anthropic-ai/claude-code/bin/claude.exe"),directory.join("node_modules/@anthropic-ai/claude-code/node_modules/@anthropic-ai/claude-code-win32-x64/claude.exe")],};
            for binary in candidates {
                if binary.is_file() {
                    return Ok(binary);
                }
            }
        }
    }
    Err(AiError::ProviderMissing)
}
struct Directory(PathBuf);
impl Directory {
    fn new() -> Result<Self, AiError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| AiError::Io)?
            .as_nanos();
        let p = env::temp_dir().join(format!(
            "mail-hantong-ai-{}-{clock}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).map_err(|_| AiError::Io)?;
        Ok(Self(p))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}
enum Frame {
    Json(Value),
    End,
    Error(AiError),
}
type PendingWrite = (Vec<u8>, mpsc::SyncSender<Result<(), AiError>>);
struct Process {
    pending: std::collections::VecDeque<Value>,
    child: Child,
    input: Option<mpsc::SyncSender<PendingWrite>>,
    rx: mpsc::Receiver<Frame>,
    _cwd: Directory,
}
impl Process {
    fn spawn(program: &Path, args: &[&str]) -> Result<Self, AiError> {
        let cwd = Directory::new()?;
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(&cwd.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|_| AiError::ProviderMissing)?;
        let mut stdin = child.stdin.take().ok_or(AiError::Io)?;
        let (writes, receiver) = mpsc::sync_channel::<PendingWrite>(1);
        thread::spawn(move || {
            while let Ok((bytes, reply)) = receiver.recv() {
                let result = stdin
                    .write_all(&bytes)
                    .and_then(|_| stdin.flush())
                    .map_err(|_| AiError::Io);
                let failed = result.is_err();
                let _ = reply.send(result);
                if failed {
                    break;
                }
            }
        });
        let input = Some(writes);
        let stdout = child.stdout.take().ok_or(AiError::Io)?;
        let stderr = child.stderr.take().ok_or(AiError::Io)?;
        let (tx, rx) = mpsc::sync_channel(16);
        let errors = tx.clone();
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut total = 0;
            loop {
                let mut line = Vec::new();
                let n = match Read::take(&mut reader, (MAX_FRAME + 1) as u64)
                    .read_until(b'\n', &mut line)
                {
                    Ok(n) => n,
                    Err(_) => {
                        let _ = tx.send(Frame::Error(AiError::Io));
                        return;
                    }
                };
                if n == 0 {
                    let _ = tx.send(Frame::End);
                    return;
                }
                total += n;
                if n > MAX_FRAME || total > MAX_STREAM {
                    let _ = tx.send(Frame::Error(AiError::TooLarge));
                    return;
                }
                match serde_json::from_slice(&line) {
                    Ok(v) => {
                        if tx.send(Frame::Json(v)).is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(Frame::Error(AiError::Protocol));
                        return;
                    }
                }
            }
        });
        thread::spawn(move || {
            let mut reader = stderr;
            let mut total = 0;
            let mut buf = [0; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        total += n;
                        if total > 65536 {
                            let _ = errors.send(Frame::Error(AiError::TooLarge));
                            return;
                        }
                    }
                }
            }
        });
        Ok(Self {
            child,
            input,
            rx,
            _cwd: cwd,
            pending: std::collections::VecDeque::new(),
        })
    }
    fn write(&mut self, bytes: Vec<u8>) -> Result<(), AiError> {
        self.write_with_timeout(bytes, Duration::from_secs(10))
    }
    fn write_with_timeout(&mut self, bytes: Vec<u8>, timeout: Duration) -> Result<(), AiError> {
        if bytes.len() > MAX_FRAME {
            return Err(AiError::TooLarge);
        }
        let (tx, rx) = mpsc::sync_channel(1);
        self.input
            .as_ref()
            .ok_or(AiError::Io)?
            .try_send((bytes, tx))
            .map_err(|_| AiError::Io)?;
        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => {
                let _ = self.child.kill();
                let _ = self.child.wait();
                Err(AiError::Timeout)
            }
        }
    }
    fn send(&mut self, v: &Value) -> Result<(), AiError> {
        let mut data = serde_json::to_vec(v).map_err(|_| AiError::Protocol)?;
        data.push(b'\n');
        self.write(data)
    }
    fn receive(&mut self, deadline: Instant) -> Result<Option<Value>, AiError> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        self.raw_receive(deadline)
    }
    fn raw_receive(&mut self, deadline: Instant) -> Result<Option<Value>, AiError> {
        let wait = deadline
            .checked_duration_since(Instant::now())
            .ok_or(AiError::Timeout)?;
        match self.rx.recv_timeout(wait) {
            Ok(Frame::Json(v)) => Ok(Some(v)),
            Ok(Frame::End) => Ok(None),
            Ok(Frame::Error(e)) => Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AiError::Timeout),
            Err(_) => Err(AiError::Io),
        }
    }
    fn wait_turn_result(&mut self, id: u64, deadline: Instant) -> Result<Value, AiError> {
        self.wait_response(id, deadline, true)
    }
    fn wait_result(&mut self, id: u64, deadline: Instant) -> Result<Value, AiError> {
        self.wait_response(id, deadline, false)
    }
    fn wait_response(
        &mut self,
        id: u64,
        deadline: Instant,
        keep_tools: bool,
    ) -> Result<Value, AiError> {
        let mut deferred = std::collections::VecDeque::new();
        for _ in 0..4096 {
            let value = if let Some(value) = self.pending.pop_front() {
                value
            } else {
                self.raw_receive(deadline)?.ok_or(AiError::ProviderFailed)?
            };
            if value.get("id") == Some(&json!(id)) && value.get("method").is_none() {
                self.pending = deferred;
                if value.get("error").is_some() {
                    return Err(AiError::ProviderFailed);
                }
                return value.get("result").cloned().ok_or(AiError::Protocol);
            }
            if keep_tools && value["method"] == "item/tool/call" && value.get("id").is_some() {
                if deferred.len() >= 256 {
                    return Err(AiError::TooLarge);
                }
                deferred.push_back(value);
                continue;
            }
            self.refuse(&value)?;
            if value.get("method").is_some() && value.get("id").is_none() {
                if deferred.len() >= 256 {
                    return Err(AiError::TooLarge);
                }
                deferred.push_back(value);
            }
        }
        Err(AiError::TooLarge)
    }
    fn refuse(&mut self, v: &Value) -> Result<(), AiError> {
        if v.get("method").is_some() && v.get("id").is_some() {
            self.send(&json!({"id":v["id"],"error":{"code":-32601,"message":"Tools and permission requests are disabled in freeform chat"}}))?;
        }
        Ok(())
    }
    fn close_input(&mut self) {
        self.input.take();
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
const CODEX_ARGS: &[&str] = &[
    "app-server",
    "-c",
    "features.shell_tool=false",
    "-c",
    "web_search=\"disabled\"",
    "-c",
    "apps._default.enabled=false",
    "-c",
    "mcp_servers={}",
    "-c",
    "plugins={}",
];
fn codex() -> Result<Process, AiError> {
    let mut p = Process::spawn(&resolve(Provider::Codex)?, CODEX_ARGS)?;
    initialize(&mut p)?;
    Ok(p)
}
fn initialize(p: &mut Process) -> Result<(), AiError> {
    p.send(&json!({"method":"initialize","id":1,"params":{"clientInfo":{"name":"mail-hantong","title":"mail-hantong","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}}))?;
    p.wait_result(1, Instant::now() + Duration::from_secs(20))?;
    p.send(&json!({"method":"initialized","params":{}}))
}
fn codex_status(p: &mut Process) -> Result<AuthStatus, AiError> {
    p.send(&json!({"method":"account/read","id":2,"params":{"refreshToken":false}}))?;
    let result = p.wait_result(2, Instant::now() + Duration::from_secs(20))?;
    let mode = result["account"]["type"]
        .as_str()
        .filter(|v| matches!(*v, "chatgpt" | "apiKey" | "amazonBedrock"))
        .map(str::to_owned);
    Ok(AuthStatus {
        authenticated: mode.is_some(),
        mode,
    })
}
fn claude_status_process(mut p: Process) -> Result<AuthStatus, AiError> {
    p.close_input();
    let result = p
        .receive(Instant::now() + Duration::from_secs(20))?
        .ok_or(AiError::ProviderFailed)?;
    let mode = result["authMethod"]
        .as_str()
        .filter(|v| {
            matches!(
                *v,
                "none" | "claude.ai" | "oauth_token" | "api_key" | "api_key_helper" | "third_party"
            )
        })
        .map(str::to_owned);
    let authenticated = result["loggedIn"]
        .as_bool()
        .unwrap_or_else(|| mode.as_deref().is_some_and(|s| s != "none"));
    Ok(AuthStatus {
        authenticated,
        mode,
    })
}
pub fn status(provider: Provider) -> Result<AuthStatus, AiError> {
    match provider {
        Provider::Codex => codex_status(&mut codex()?),
        Provider::Claude => {
            claude_status_process(Process::spawn(&resolve(provider)?, &["auth", "status"])?)
        }
    }
}
pub fn valid_auth_url(url: &str) -> bool {
    url.len() <= 8192
        && !url
            .chars()
            .any(|c| c.is_control() || matches!(c, '"' | '\'' | '\\' | ' '))
        && ["https://chatgpt.com/", "https://auth.openai.com/"]
            .iter()
            .any(|host| url.starts_with(host))
}
fn browser(url: &str) -> Result<(), AiError> {
    if !valid_auth_url(url) {
        return Err(AiError::BrowserFailed);
    }
    #[cfg(windows)]
    {
        let system = env::var_os("SystemRoot").ok_or(AiError::BrowserFailed)?;
        Command::new(PathBuf::from(system).join("System32/rundll32.exe"))
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .map_err(|_| AiError::BrowserFailed)?;
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("/usr/bin/open")
            .arg(url)
            .spawn()
            .map_err(|_| AiError::BrowserFailed)?;
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|_| AiError::BrowserFailed)?;
    }
    Ok(())
}
pub fn login(provider: Provider) -> Result<AuthStatus, AiError> {
    match provider {
        Provider::Codex => {
            let mut p = codex()?;
            codex_login(&mut p, browser)?;
            codex_status(&mut p)
        }
        Provider::Claude => {
            let mut child = Command::new(resolve(provider)?)
                .args(["auth", "login", "--console"])
                .spawn()
                .map_err(|_| AiError::ProviderMissing)?;
            let deadline = Instant::now() + Duration::from_secs(300);
            loop {
                if let Some(exit) = child.try_wait().map_err(|_| AiError::Io)? {
                    if !exit.success() {
                        return Err(AiError::ProviderFailed);
                    }
                    break;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AiError::Timeout);
                }
                thread::sleep(Duration::from_millis(50));
            }
            status(provider)
        }
    }
}
fn codex_login(
    p: &mut Process,
    mut open: impl FnMut(&str) -> Result<(), AiError>,
) -> Result<(), AiError> {
    p.send(&json!({"method":"account/login/start","id":3,"params":{"type":"chatgpt","useHostedLoginSuccessPage":true,"appBrand":"codex"}}))?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let result = p.wait_result(3, deadline)?;
    let id = result["loginId"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or(AiError::Protocol)?
        .to_owned();
    let url = result["authUrl"]
        .as_str()
        .filter(|s| valid_auth_url(s))
        .ok_or(AiError::BrowserFailed)?;
    open(url)?;
    for _ in 0..4096 {
        let event = p.receive(deadline)?.ok_or(AiError::ProviderFailed)?;
        if event["method"] == "account/login/completed" && event["params"]["loginId"] == id {
            return if event["params"]["success"] == true {
                Ok(())
            } else {
                Err(AiError::Cancelled)
            };
        }
        p.refuse(&event)?;
    }
    Err(AiError::TooLarge)
}
pub struct Session {
    provider: Provider,
    process: Option<Process>,
    thread_id: Option<String>,
    next: u64,
    history: String,
    models: Vec<Model>,
    preferences: Preferences,
    agent: Option<agent::Agent>,
}
impl Session {
    pub fn start(provider: Provider) -> Result<Self, AiError> {
        let auth = status(provider)?;
        if !auth.authenticated {
            return Err(AiError::LoginRequired);
        }
        if provider == Provider::Claude
            && !matches!(
                auth.mode.as_deref(),
                Some("api_key" | "api_key_helper" | "third_party")
            )
        {
            return Err(AiError::SubscriptionUnsupported);
        }
        let mut session = Self {
            provider,
            process: None,
            thread_id: None,
            next: 10,
            history: String::new(),
            models: Vec::new(),
            preferences: Preferences::default(),
            agent: None,
        };
        session.refresh_models()?;
        session.preferences.model = session.current_model().map(|m| m.id.clone());
        session.start_thread()?;
        Ok(session)
    }
    pub fn provider(&self) -> Provider {
        self.provider
    }
    pub fn models(&self) -> &[Model] {
        &self.models
    }
    pub fn preferences(&self) -> &Preferences {
        &self.preferences
    }
    pub fn agent_enabled(&self) -> bool {
        self.agent.is_some()
    }
    pub fn current_model(&self) -> Option<&Model> {
        self.preferences
            .model
            .as_ref()
            .and_then(|id| self.models.iter().find(|m| &m.id == id))
            .or_else(|| self.models.iter().find(|m| m.default))
            .or_else(|| self.models.first())
    }
    pub fn refresh_models(&mut self) -> Result<(), AiError> {
        self.models = if self.provider == Provider::Codex {
            let mut p = codex()?;
            catalog::codex_models(&mut p, &mut self.next)?
        } else {
            catalog::claude_models()?
        };
        Ok(())
    }
    pub fn select_model(&mut self, id: &str) -> Result<(), AiError> {
        if !self.models.iter().any(|m| m.id == id) {
            return Err(AiError::Unsupported);
        }
        self.preferences.model = Some(id.into());
        self.preferences.effort = None;
        self.preferences.fast = false;
        Ok(())
    }
    pub fn select_mode(&mut self, mode: &str) -> Result<(), AiError> {
        let effort = self
            .current_model()
            .ok_or(AiError::Unsupported)?
            .effort_for_mode(mode)?;
        self.preferences.effort = Some(effort.into());
        Ok(())
    }
    pub fn select_fast(&mut self, on: bool) -> Result<(), AiError> {
        if on && !self.current_model().is_some_and(|m| m.fast) {
            return Err(AiError::Unsupported);
        }
        self.preferences.fast = on;
        Ok(())
    }
    pub fn set_agent(&mut self, on: bool) -> Result<(), AiError> {
        if on == self.agent_enabled() {
            return Ok(());
        }
        let old = self.agent.take();
        if on {
            self.agent = Some(agent::Agent::connect()?);
        }
        if let Err(e) = self.start_thread() {
            self.agent = old;
            return Err(e);
        }
        self.history.clear();
        Ok(())
    }
    fn start_thread(&mut self) -> Result<(), AiError> {
        if self.provider != Provider::Codex {
            return Ok(());
        }
        let mut p = codex()?;
        let cwd = p._cwd.0.to_str().ok_or(AiError::Io)?;
        let mut params = json!({"cwd":cwd,"ephemeral":true,"approvalPolicy":"onRequest","sandbox":"readOnly","serviceTier":"default","config":{"features":{"shell_tool":false},"mcp_servers":{},"plugins":{},"apps":{"_default":{"enabled":false}},"web_search":"disabled"}});
        if let Some(model) = &self.preferences.model {
            params["model"] = json!(model);
        }
        if let Some(agent) = &self.agent {
            params["dynamicTools"] = json!(agent.tools);
        }
        p.send(&json!({"method":"thread/start","id":4,"params":params}))?;
        let result = p.wait_result(4, Instant::now() + Duration::from_secs(30))?;
        let id = result["thread"]["id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .ok_or(AiError::Protocol)?;
        self.thread_id = Some(id.into());
        self.process = Some(p);
        Ok(())
    }
    pub fn ask(&mut self, prompt: &str, mut text: impl FnMut(&str)) -> Result<(), AiError> {
        if prompt.trim().is_empty() || prompt.len() > MAX_PROMPT || prompt.contains('\0') {
            return Err(AiError::InvalidInput);
        }
        match self.provider {
            Provider::Codex => {
                let p = self.process.as_mut().ok_or(AiError::Io)?;
                let thread_id = self.thread_id.as_ref().ok_or(AiError::Protocol)?;
                let request = self.next;
                self.next += 1;
                let mut params = json!({"threadId":thread_id,"input":[{"type":"text","text":prompt}],"approvalPolicy":"onRequest","serviceTier":if self.preferences.fast{"priority"}else{"default"},"sandboxPolicy":{"type":"readOnly","access":{"type":"restricted","includePlatformDefaults":false,"readableRoots":[p._cwd.0]}}});
                if let Some(model) = &self.preferences.model {
                    params["model"] = json!(model);
                }
                if let Some(effort) = &self.preferences.effort {
                    params["effort"] = json!(effort);
                }
                p.send(&json!({"method":"turn/start","id":request,"params":params}))?;
                let deadline = Instant::now() + Duration::from_secs(180);
                let reply = p.wait_turn_result(request, deadline)?;
                let turn = reply["turn"]["id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or(AiError::Protocol)?
                    .to_owned();
                for _ in 0..16384 {
                    let event = p.receive(deadline)?.ok_or(AiError::ProviderFailed)?;
                    if event["method"] == "item/tool/call"
                        && event.get("id").is_some()
                        && event["params"]["threadId"] == *thread_id
                        && event["params"]["turnId"] == turn
                        && event["params"]["namespace"] == "hantong"
                    {
                        let result = if let Some(agent) = self.agent.as_mut() {
                            let tool = event["params"]["tool"].as_str().ok_or(AiError::Protocol)?;
                            agent.call(tool, event["params"]["arguments"].clone())
                        } else {
                            Err(AiError::AgentUnavailable)
                        };
                        let output = match result {
                            Ok(v) => {
                                json!({"success":v["isError"]==false,"contentItems":[{"type":"inputText","text":v.to_string()}]})
                            }
                            Err(e) => {
                                json!({"success":false,"contentItems":[{"type":"inputText","text":e.code()}]})
                            }
                        };
                        p.send(&json!({"id":event["id"],"result":output}))?;
                        continue;
                    }
                    p.refuse(&event)?;
                    if event["params"]["threadId"] != *thread_id {
                        continue;
                    }
                    if event["method"] == "item/agentMessage/delta"
                        && event["params"]["turnId"] == turn
                    {
                        if let Some(delta) = event["params"]["delta"].as_str() {
                            text(&terminal_text(delta));
                        }
                    } else if event["method"] == "turn/completed"
                        && event["params"]["turn"]["id"] == turn
                    {
                        return if event["params"]["turn"]["status"] == "completed" {
                            Ok(())
                        } else {
                            Err(AiError::ProviderFailed)
                        };
                    }
                }
                Err(AiError::TooLarge)
            }
            Provider::Claude => {
                let mcp = self.agent.as_ref().map(agent::Agent::claude_config);
                let mut args = catalog::claude_arguments(&self.preferences, mcp.as_ref());
                args.extend(["--input-format".into(), "stream-json".into()]);
                let refs: Vec<_> = args.iter().map(String::as_str).collect();
                let mut p = Process::spawn(&resolve(Provider::Claude)?, &refs)?;
                let initialized = catalog::claude_initialize(&mut p)?;
                if self.preferences.fast && initialized["fast_mode_state"] != "on" {
                    return Err(AiError::Unsupported);
                }
                if self.agent.is_some() {
                    catalog::claude_mcp_ready(&mut p)?;
                }
                let input = format!("{}\nUser: {prompt}\nAssistant:", self.history);
                if input.len() > MAX_PROMPT * 4 {
                    return Err(AiError::TooLarge);
                }
                p.send(&json!({"type":"user","session_id":"","message":{"role":"user","content":input},"parent_tool_use_id":null,"client_composed":true}))?;
                p.close_input();
                let mut answer = String::new();
                let deadline = Instant::now() + Duration::from_secs(180);
                for _ in 0..16384 {
                    let event = p.receive(deadline)?.ok_or(AiError::ProviderFailed)?;
                    if event["type"] == "control_request" {
                        p.send(&json!({"type":"control_response","response":{"subtype":"error","request_id":event["request_id"],"error":"Generic permission requests are disabled"}}))?;
                        continue;
                    }
                    if event["type"] == "stream_event"
                        && event["event"]["delta"]["type"] == "text_delta"
                    {
                        if let Some(delta) = event["event"]["delta"]["text"].as_str() {
                            let delta = terminal_text(delta);
                            answer.push_str(&delta);
                            text(&delta);
                        }
                    } else if event["type"] == "result" {
                        if event["is_error"] == true {
                            return Err(AiError::ProviderFailed);
                        }
                        if answer.is_empty()
                            && let Some(result) = event["result"].as_str()
                        {
                            answer = terminal_text(result);
                            text(&answer);
                        }
                        if self.history.len() + prompt.len() + answer.len() < MAX_PROMPT * 3 {
                            self.history
                                .push_str(&format!("\nUser: {prompt}\nAssistant: {answer}"));
                        } else {
                            self.history.clear();
                        }
                        return Ok(());
                    }
                }
                Err(AiError::TooLarge)
            }
        }
    }
}
pub fn terminal_text(text: &str) -> String {
    let mut out = String::new();
    let mut escape = 0_u8;
    for ch in text.chars() {
        match escape {
            1 => {
                escape = if ch == '[' {
                    2
                } else if ch == ']' {
                    3
                } else {
                    0
                };
                continue;
            }
            2 => {
                if ('@'..='~').contains(&ch) {
                    escape = 0;
                }
                continue;
            }
            3 => {
                if ch == '\u{7}' {
                    escape = 0;
                } else if ch == '\u{1b}' {
                    escape = 4;
                }
                continue;
            }
            4 => {
                escape = if ch == '\\' { 0 } else { 3 };
                continue;
            }
            _ => {}
        }
        if ch == '\u{1b}' {
            escape = 1;
        } else if (!ch.is_control() || matches!(ch, '\n' | '\t'))
            && !matches!(ch,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        {
            out.push(ch);
        }
        if out.len() > 65536 {
            break;
        }
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ai07_claude_agent_requires_exact_connected_runtime_mcp_server() {
        let mut good = fake("claude_models");
        catalog::claude_initialize(&mut good).unwrap();
        assert_eq!(catalog::claude_mcp_ready(&mut good), Ok(()));
        let mut absent = fake("claude_mcp_missing");
        catalog::claude_initialize(&mut absent).unwrap();
        assert_eq!(
            catalog::claude_mcp_ready(&mut absent),
            Err(AiError::AgentUnavailable)
        );
    }
    fn synthetic_session(mode: &str) -> Session {
        let mut process = fake(mode);
        initialize(&mut process).unwrap();
        let mut next = 10;
        let models = catalog::codex_models(&mut process, &mut next).unwrap();
        Session {
            provider: Provider::Codex,
            process: Some(process),
            thread_id: Some("T".into()),
            next,
            history: String::new(),
            models,
            preferences: Preferences::default(),
            agent: None,
        }
    }
    #[test]
    fn ai07_runtime_catalog_gates_effort_fast_and_actual_turn_fields() {
        let mut s = synthetic_session("settings");
        s.select_model("fake-model").unwrap();
        s.select_mode("high").unwrap();
        s.select_fast(true).unwrap();
        let before = s.preferences().clone();
        assert_eq!(s.select_mode("med"), Err(AiError::Unsupported));
        assert_eq!(s.preferences(), &before);
        assert_eq!(s.select_model("invented"), Err(AiError::Unsupported));
        s.ask("synthetic request", |_| {}).unwrap();
        s.select_model("limited-model").unwrap();
        assert_eq!(s.select_fast(true), Err(AiError::Unsupported));
        assert_eq!(s.select_mode("ultra-high"), Err(AiError::Unsupported));
        assert!(!s.preferences().fast);
    }
    #[test]
    fn cli16_ultra_high_uses_advertised_xhigh_or_max_without_inventing_support() {
        let mut model = Model {
            id: "synthetic".into(),
            label: "synthetic".into(),
            efforts: vec!["low".into(), "max".into()],
            fast: false,
            default: true,
        };
        assert_eq!(model.effort_for_mode("ultra-high"), Ok("max"));
        assert_eq!(model.effort_for_mode("high"), Err(AiError::Unsupported));
        model.efforts.push("xhigh".into());
        assert_eq!(model.effort_for_mode("ultra-high"), Ok("xhigh"));
        assert_eq!(
            model.effort_for_mode("invented"),
            Err(AiError::InvalidInput)
        );
    }
    #[test]
    fn ai07_claude_initialize_catalog_does_not_send_a_prompt() {
        let mut p = fake("claude_models");
        let r = catalog::claude_initialize(&mut p).unwrap();
        let models = catalog::parse_models(&r["models"], Provider::Claude).unwrap();
        assert_eq!(models[0].id, "fake-claude");
        assert_eq!(models[0].efforts, vec!["low", "high"]);
        assert!(models[0].fast);
        let prefs = Preferences {
            model: Some("fake-claude".into()),
            effort: Some("high".into()),
            fast: true,
        };
        let args = catalog::claude_arguments(&prefs, None);
        assert!(args.windows(2).any(|p| p == ["--effort", "high"]));
        assert!(args.iter().any(|a| a.contains("\"fastMode\":true")));
    }
    #[test]
    fn ai07_dynamic_tool_racing_turn_start_is_forwarded_only_to_real_mcp_bridge() {
        let mut s = synthetic_session("agent");
        s.agent = Some(agent::Agent::synthetic());
        let mut out = String::new();
        s.ask("synthetic mail query", |text| out.push_str(text))
            .unwrap();
        assert_eq!(out, "hello world");
    }
    #[test]
    fn ai07_auth_url_cannot_launch_other_schemes_or_hosts() {
        for s in [
            "file:///x",
            "https://chatgpt.com.evil/x",
            "https://chatgpt.com@evil/x",
            "https://chatgpt.com/\ncmd",
        ] {
            assert!(!valid_auth_url(s));
        }
        assert!(valid_auth_url(
            "https://auth.openai.com/oauth/authorize?state=123"
        ));
    }
    #[test]
    fn sec08_provider_text_cannot_inject_terminal_or_bidi() {
        assert_eq!(
            terminal_text("hi\u{1b}[2J world\u{1b}]0;title\u{7}\u{202e}"),
            "hi world"
        );
    }
    #[test]
    fn ai07_no_model_or_default_mail_context_in_provider_arguments() {
        assert!(!CODEX_ARGS.join(" ").contains("--model"));
        assert!(CODEX_ARGS.contains(&"features.shell_tool=false"));
        assert!(CODEX_ARGS.contains(&"mcp_servers={}"));
    }
    pub(super) fn fake(mode: &str) -> Process {
        let python = std::env::var_os("HANTONG_TEST_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("python"));
        let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake_provider.py");
        Process::spawn(&python, &["-u", file.to_str().unwrap(), mode])
            .expect("Python test runtime (never a real AI provider)")
    }
    #[test]
    fn ai07_managed_browser_login_keeps_racing_completion_and_does_not_expose_tokens() {
        let mut process = fake("codex");
        initialize(&mut process).unwrap();
        let mut opened = false;
        codex_login(&mut process, |url| {
            assert!(valid_auth_url(url));
            opened = true;
            Ok(())
        })
        .unwrap();
        assert!(opened);
        let state = codex_status(&mut process).unwrap();
        assert_eq!(state.mode.as_deref(), Some("chatgpt"));
        assert!(!format!("{state:?}").contains("do-not-display"));
    }
    #[test]
    fn ai07_stream_matches_thread_and_turn_and_refuses_tool_requests() {
        let mut process = fake("codex");
        initialize(&mut process).unwrap();
        let mut session = Session {
            provider: Provider::Codex,
            process: Some(process),
            thread_id: Some("T".into()),
            next: 10,
            history: String::new(),
            models: Vec::new(),
            preferences: Preferences::default(),
            agent: None,
        };
        let mut response = String::new();
        session
            .ask("synthetic prompt", |delta| response.push_str(delta))
            .unwrap();
        assert_eq!(response, "hello world");
    }
    #[test]
    fn sec08_subprocess_frame_limits_timeouts_and_invalid_json_are_stable() {
        let mut process = fake("oversize");
        assert!(matches!(
            process.receive(Instant::now() + Duration::from_secs(5)),
            Err(AiError::TooLarge)
        ));
        drop(process);
        let mut process = fake("timeout");
        assert_eq!(
            process
                .receive(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .unwrap()["ready"],
            true
        );
        let began = Instant::now();
        assert!(matches!(
            process.receive(Instant::now() + Duration::from_millis(30)),
            Err(AiError::Timeout)
        ));
        drop(process);
        assert!(began.elapsed() < Duration::from_secs(3));
        let mut process = fake("invalid");
        let error = process
            .receive(Instant::now() + Duration::from_secs(5))
            .unwrap_err();
        assert_eq!(error, AiError::Protocol);
        assert!(!format!("{error:?}").contains("secret_test_marker"));
    }
    #[test]
    fn ai07_claude_status_only_returns_whitelisted_mode() {
        let state = claude_status_process(fake("claude_status")).unwrap();
        assert_eq!(state.mode.as_deref(), Some("api_key"));
        assert!(!format!("{state:?}").contains("do-not-display"));
    }
    #[test]
    fn sec08_blocked_stdin_is_bounded_and_provider_is_reaped() {
        let mut process = fake("no_read");
        assert_eq!(
            process
                .receive(Instant::now() + Duration::from_secs(5))
                .unwrap()
                .unwrap()["ready"],
            true
        );
        assert_eq!(
            process.write_with_timeout(vec![b'x'; 512 * 1024], Duration::from_millis(40)),
            Err(AiError::Timeout)
        );
        assert!(process.child.try_wait().unwrap().is_some());
    }
}
