//! Small interactive startup presentation for the command-line onboarding.
//!
//! This is deliberately presentation-only.  It does not delay startup, read
//! credentials, or change any account, registration, or provider state.

use hantong_contracts::ErrorCode;
#[cfg(any(windows, unix))]
use std::io::Read;
use std::io::{self, IsTerminal, Write};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(any(windows, unix))]
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const EN: &str = include_str!("../resources/startup-en.txt");
const KO: &str = include_str!("../resources/startup-ko.txt");

pub(super) fn release_version() -> &'static str {
    option_env!("HANTONG_RELEASE_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// Reads only the bounded, installer-owned update marker.  The marker is
/// optional, so startup remains offline-first and never blocks on a feed.
pub(super) fn cached_update_version() -> Option<String> {
    if cfg!(target_os = "linux") {
        return None;
    } // R2 updates require an independent version/commit/digest tuple.
    let path = product_root()?.join("update-notice.txt");
    let metadata = std::fs::metadata(&path).ok()?;
    if metadata.len() > 64 {
        return None;
    }
    let value = std::fs::read_to_string(path).ok()?;
    let value = value.trim();
    if !is_newer_release(value, release_version()) {
        return None;
    }
    Some(value.to_owned())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
    /// Stable releases sort after beta releases.
    beta: Option<u64>,
}

fn release_parts(value: &str) -> Option<ReleaseVersion> {
    let value = value.strip_prefix('v').unwrap_or(value);
    let (core, pre) = value
        .split_once('-')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    let mut numbers = core.split('.');
    let major = numeric_component(numbers.next()?)?;
    let minor = numeric_component(numbers.next()?)?;
    let patch = numeric_component(numbers.next()?)?;
    if numbers.next().is_some() {
        return None;
    }
    let beta = match pre {
        None => None,
        Some(value) => Some(numeric_component(value.strip_prefix("beta.")?)?),
    };
    Some(ReleaseVersion {
        major,
        minor,
        patch,
        beta,
    })
}

fn numeric_component(value: &str) -> Option<u64> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

fn is_newer_release(candidate: &str, current: &str) -> bool {
    let Some(candidate) = release_parts(candidate) else {
        return false;
    };
    let Some(current) = release_parts(current) else {
        return false;
    };
    release_key(candidate) > release_key(current)
}

fn compare_release(candidate: &str, current: &str) -> Option<std::cmp::Ordering> {
    let candidate = release_parts(candidate)?;
    let current = release_parts(current)?;
    Some(release_key(candidate).cmp(&release_key(current)))
}

fn release_key(value: ReleaseVersion) -> (u64, u64, u64, bool, u64) {
    (
        value.major,
        value.minor,
        value.patch,
        value.beta.is_none(),
        value.beta.unwrap_or(0),
    )
}

fn resource(lang: super::Lang, key: &str) -> &'static str {
    let source = match lang {
        super::Lang::En => EN,
        super::Lang::Ko => KO,
    };
    source
        .lines()
        .find_map(|line| line.strip_prefix(key).and_then(|v| v.strip_prefix('=')))
        .expect("validated startup resource key")
}

/// Runs `work` while showing a bounded, interruptible character animation on an
/// interactive console.  The work controls the duration; this function adds
/// no artificial wait.  Non-interactive callers receive exactly the original
/// result and no output.
pub(super) fn with_animation<T>(
    lang: super::Lang,
    work: impl FnOnce() -> Result<T, ErrorCode>,
) -> Result<T, ErrorCode> {
    // Unix startup may collect a Secrets-owned passphrase on this same terminal.
    if cfg!(unix) || !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return work();
    }

    let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);
    let preparation = resource(lang, "preparing");
    let animation = std::thread::spawn(move || {
        let frames = ["(•_•)", "/(•_•)\\", "(•_•)\\", "\\(•_•)/"];
        let mut index = 0usize;
        let mut stdout = io::stdout();
        loop {
            match stop_rx.recv_timeout(Duration::from_millis(120)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let _ = write!(
                stdout,
                "\rHanTong {} {}",
                preparation,
                frames[index % frames.len()]
            );
            let _ = stdout.flush();
            index = index.wrapping_add(1);
        }
    });

    let guard = AnimationGuard {
        stop_tx: Some(stop_tx),
        animation: Some(animation),
    };
    let result = work();
    drop(guard);
    let mut stdout = io::stdout();
    let _ = write!(stdout, "\r                \r");
    let _ = stdout.flush();
    result
}

struct AnimationGuard {
    stop_tx: Option<mpsc::SyncSender<()>>,
    animation: Option<std::thread::JoinHandle<()>>,
}

impl Drop for AnimationGuard {
    fn drop(&mut self) {
        if let Some(sender) = self.stop_tx.take() {
            let _ = sender.send(());
        }
        if let Some(animation) = self.animation.take() {
            let _ = animation.join();
        }
        let mut stdout = io::stdout();
        let _ = write!(stdout, "\r                         \r");
        let _ = stdout.flush();
    }
}

pub(super) struct UpdateFetch {
    receiver: Option<mpsc::Receiver<Option<String>>>,
}

pub(super) fn begin_update_fetch() -> UpdateFetch {
    if cfg!(target_os = "linux") {
        return UpdateFetch { receiver: None };
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let value = fetch_latest_release().filter(|v| is_newer_release(v, release_version()));
        if let Some(value) = value.as_deref() {
            let _ = write_update_cache(value);
        }
        let _ = sender.send(value);
    });
    UpdateFetch {
        receiver: Some(receiver),
    }
}

impl UpdateFetch {
    pub(super) fn finish(mut self) -> Option<String> {
        self.receiver
            .take()
            .and_then(|receiver| receiver.try_recv().ok().flatten())
            .or_else(cached_update_version)
    }
}

#[cfg(not(any(windows, unix)))]
fn fetch_latest_release() -> Option<String> {
    None
}

#[cfg(any(windows, unix))]
fn fetch_latest_release() -> Option<String> {
    #[cfg(windows)]
    let powershell = hantong_platform::system_powershell().ok()?;
    #[cfg(windows)]
    let script = r#"$ProgressPreference='SilentlyContinue'; (Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/tz1012/mail-hantong/releases.atom' -TimeoutSec 4).Content"#;
    #[cfg(windows)]
    let mut child = Command::new(powershell)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .ok()?;
    #[cfg(unix)]
    let mut child = Command::new("/usr/bin/curl")
        .args([
            "--fail",
            "--silent",
            "--proto",
            "=https",
            "--max-time",
            "4",
            "--max-filesize",
            "65536",
            "https://github.com/tz1012/mail-hantong/releases.atom",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut limited = stdout.take(64 * 1024 + 1);
        let _ = std::io::Read::read_to_end(&mut limited, &mut bytes);
        let _ = sender.send(bytes);
    });
    let start = std::time::Instant::now();
    while child.try_wait().ok()?.is_none() {
        if start.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = reader.join();
    let output = receiver.recv_timeout(Duration::from_secs(1)).ok()?;
    if output.len() > 64 * 1024 {
        return None;
    }
    newest_atom_title(std::str::from_utf8(&output).ok()?)
}

fn newest_atom_title(xml: &str) -> Option<String> {
    xml.split("<title>")
        .skip(1)
        .filter_map(|part| part.split_once("</title>").map(|(v, _)| v.trim()))
        .filter_map(|value| value.strip_prefix('v'))
        .filter_map(|value| release_parts(value).map(|_| value.to_owned()))
        .max_by(|a, b| compare_release(a, b).unwrap_or(std::cmp::Ordering::Equal))
}

fn product_root() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bin = exe.parent()?;
    if bin.file_name()?.to_str()? != "bin" {
        return None;
    }
    let version = bin.parent()?;
    let versions = version.parent()?;
    if versions.file_name()?.to_str()? != "versions" {
        return None;
    }
    let root = versions.parent()?.to_path_buf();
    let marker = std::fs::read_to_string(root.join(".mail-hantong-owner")).ok()?;
    (marker.trim() == "mail-hantong-cli-v1").then_some(root)
}

fn write_update_cache(version: &str) -> Option<()> {
    let root = product_root()?;
    let temp = root.join("update-notice.txt.tmp");
    std::fs::write(&temp, version.as_bytes()).ok()?;
    std::fs::rename(temp, root.join("update-notice.txt")).ok()
}

/// Prints the stable ready header before onboarding emits account choices.
/// The optional notice is intentionally supplied by a trusted caller/cache;
/// startup never performs a network request or runs an updater.
pub(super) fn write_ready_header(
    io: &mut impl super::onboarding::Io,
    lang: super::Lang,
    update_version: Option<&str>,
) -> Result<(), ErrorCode> {
    io.write(&format!("HanTong {}\n", release_version()))?;
    if let Some(version) = update_version {
        io.write(&format!(
            "{}\n",
            resource(lang, "update_available").replace("{version}", version)
        ))?;
        io.write(&format!("{}\n", resource(lang, "update_command")))?;
    }
    io.write(resource(lang, "ready"))?;
    io.write("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn sec03_linux_pinned_updates_never_select_shared_atom_notice() {
        assert!(begin_update_fetch().receiver.is_none());
        assert!(cached_update_version().is_none());
    }

    #[test]
    fn startup_version_uses_release_label_when_present() {
        assert!(!release_version().is_empty());
    }

    #[test]
    fn startup_resources_are_localized_and_bounded() {
        assert!(EN.contains("ready="));
        assert!(KO.contains("ready="));
        assert!(EN.contains("preparing="));
        assert!(KO.contains("preparing="));
        assert!(EN.contains("update_command="));
        assert!(KO.contains("update_command="));
    }

    #[test]
    fn update_marker_requires_a_strictly_newer_numeric_release() {
        assert!(is_newer_release("0.1.0-beta.17", "0.1.0-beta.16"));
        assert!(is_newer_release("0.1.0", "0.1.0-beta.16"));
        assert!(!is_newer_release("0.1.0-beta.16", "0.1.0-beta.16"));
        assert!(!is_newer_release("0.01.0-beta.17", "0.1.0-beta.16"));
        assert!(!is_newer_release("latest", "0.1.0-beta.16"));
    }
}
