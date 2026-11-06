//! Start only the sibling Core covered by the installed release manifest.
use hantong_contracts::{ErrorCode, terminal_review::TerminalReviewKind, wire_hex};
#[cfg(test)]
use std::process::Command;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

pub(super) fn ensure_started() -> Result<(), ErrorCode> {
    match hantong_ipc_client::core_status() {
        Ok(value) if value.transport_ready => return Ok(()),
        Err(ErrorCode::CoreUnavailable) => {}
        Ok(_) => return Err(ErrorCode::CoreUnavailable),
        Err(error) => return Err(error),
    }
    let mut command = hantong_platform::unix_release::verified_core_command("hantong")?;
    // Core/Secrets own any protected TTY input. The CLI never reads credentials.
    let child = command
        .arg("run")
        .arg("--background")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    wait_for_readiness(
        child,
        hantong_ipc_client::core_status,
        Duration::from_secs(300),
    )
}

fn wait_for_readiness(
    child: Child,
    mut status: impl FnMut() -> Result<hantong_ipc_client::Readiness, ErrorCode>,
    timeout: Duration,
) -> Result<(), ErrorCode> {
    let mut owned = OwnedChild(Some(child));
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(exit) = owned
            .0
            .as_mut()
            .ok_or(ErrorCode::CoreUnavailable)?
            .try_wait()
            .map_err(|_| ErrorCode::CoreUnavailable)?
        {
            return Err(exit_error(exit.code()));
        }
        match status() {
            Ok(value) if value.transport_ready => {
                owned.0.take();
                return Ok(());
            }
            Ok(_) | Err(ErrorCode::CoreUnavailable) => {}
            Err(error) => return Err(error),
        }
        if Instant::now() >= deadline {
            return Err(ErrorCode::CoreUnavailable);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(super) fn review(
    kind: TerminalReviewKind,
    request_id: [u8; 32],
    account: Option<&str>,
    hash: Option<[u8; 32]>,
    lang: crate::Lang,
) -> Result<(), ErrorCode> {
    let mut command = hantong_platform::unix_release::verified_core_command("hantong")?;
    command.args([
        "terminal-review",
        "--kind",
        kind.as_str(),
        "--request-id",
        &wire_hex::encode(&request_id),
        "--lang",
        if lang == crate::Lang::Ko { "ko" } else { "en" },
    ]);
    if let Some(account) = account {
        command.args(["--account-id", account]);
    }
    if let Some(hash) = hash {
        command.args(["--payload-hash", &wire_hex::encode(&hash)]);
    }
    let result = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if result.success() {
        Ok(())
    } else {
        Err(exit_error(result.code()))
    }
}

fn exit_error(code: Option<i32>) -> ErrorCode {
    ErrorCode::ALL
        .into_iter()
        .find(|error| Some(error.exit_code()) == code)
        .unwrap_or(ErrorCode::CoreUnavailable)
}

struct OwnedChild(Option<Child>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inst01_unix_startup_keeps_protected_input_failure_code() {
        assert_eq!(
            exit_error(Some(ErrorCode::HumanStepRequired.exit_code())),
            ErrorCode::HumanStepRequired
        );
        assert_eq!(exit_error(None), ErrorCode::CoreUnavailable);
    }

    #[test]
    fn inst01_unix_failed_start_reaps_only_its_child_without_retry() {
        let child = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let mut polls = 0;
        let result = wait_for_readiness(
            child,
            || {
                polls += 1;
                Err(ErrorCode::CoreUnavailable)
            },
            Duration::from_secs(2),
        );
        assert!(result.is_err());
        assert!(polls < 10);
    }
}
