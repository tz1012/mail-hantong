//! Fixed Linux activation and pinned-update entrypoint. Network addresses and
//! release identities are selected by the platform verifier, never by a URL.

#![cfg(target_os = "linux")]

use hantong_contracts::ErrorCode;
use std::ffi::OsString;
use std::process::{Child, Stdio};
use std::time::Duration;

const MODE: &str = "linux-update-v2";
const BOOTSTRAP: &str = "bootstrap";
const START_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Clone, Debug, Eq, PartialEq)]
enum CommandMode {
    Bootstrap,
    ResumeActivation,
    Update {
        version: String,
        commit: String,
        descriptor_sha: String,
        resume: bool,
    },
}

fn parse_mode(args: &[OsString]) -> Result<CommandMode, ErrorCode> {
    if args.first().and_then(|s| s.to_str()) != Some(MODE) {
        return Err(ErrorCode::InvalidInput);
    }
    if args.len() == 2 && args[1] == BOOTSTRAP {
        return Ok(CommandMode::Bootstrap);
    }
    if args.len() == 2 && args[1] == "resume-activation" {
        return Ok(CommandMode::ResumeActivation);
    }
    if args.len() == 5 && (args[1] == "update" || args[1] == "resume") {
        let fields = args[2..]
            .iter()
            .map(|s| s.to_str().ok_or(ErrorCode::InvalidInput))
            .collect::<Result<Vec<_>, _>>()?;
        if !hantong_platform::linux_identity::valid_version(fields[0])
            || fields[1].len() != 40
            || fields[2].len() != 64
            || fields[1..].iter().any(|s| {
                !s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        return Ok(CommandMode::Update {
            version: fields[0].to_owned(),
            commit: fields[1].to_owned(),
            descriptor_sha: fields[2].to_owned(),
            resume: args[1] == "resume",
        });
    }
    Err(ErrorCode::InvalidInput)
}

pub(crate) fn run(args: &[OsString]) -> Result<(), ErrorCode> {
    match parse_mode(args)? {
        CommandMode::Bootstrap => run_bootstrap(false),
        CommandMode::ResumeActivation => run_bootstrap(true),
        CommandMode::Update {
            version,
            commit,
            descriptor_sha,
            resume,
        } => run_update(&version, &commit, &descriptor_sha, resume),
    }
}

fn wait_owned(child: &mut Child, seconds: u64) -> Result<(), ErrorCode> {
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(ErrorCode::CoreUnavailable)
                };
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ErrorCode::CoreUnavailable);
            }
        }
    }
}
fn run_update(
    version: &str,
    commit: &str,
    descriptor_sha: &str,
    resume: bool,
) -> Result<(), ErrorCode> {
    let update = if resume {
        hantong_platform::linux_pinned_update::resume(version, commit, descriptor_sha)?
    } else {
        hantong_platform::linux_pinned_update::stage(version, commit, descriptor_sha)?
    };
    // Seal and bind the candidate before stopping the old Core. Its own context
    // performs locked activation and readiness against the new release identity.
    let mut candidate = update.core_command()?;
    let mut stop = hantong_platform::unix_release::verified_core_command("mail-hantong-core")?;
    let mut stopping = stop
        .arg("unix-stop")
        .stdin(Stdio::null())
        .spawn()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    wait_owned(&mut stopping, 45)?;
    let mut activating = candidate
        .arg(MODE)
        .arg(if resume {
            "resume-activation"
        } else {
            BOOTSTRAP
        })
        .stdin(Stdio::inherit())
        .spawn()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    wait_owned(&mut activating, 60)
}

fn run_bootstrap(resume: bool) -> Result<(), ErrorCode> {
    // This call performs the one-time offline pointer activation while holding
    // the lifecycle/database locks. It does not grant scopes or touch mail.
    if (if resume {
        hantong_platform::linux_install::activate_resume()?
    } else {
        hantong_platform::linux_install::activate_bootstrap()?
    }) == hantong_platform::linux_install::Activation::DurabilityUncertain
    {
        eprintln!(
            "hantong_linux_update stage=activation_durability kind=uncertain code=CORE_UNAVAILABLE"
        );
        return Err(ErrorCode::CoreUnavailable);
    }

    let mut command = hantong_platform::unix_release::verified_core_command("mail-hantong-core")?;
    let mut child = command
        .arg("run")
        .arg("--background")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    wait_for_ready(&mut child)
}

fn wait_for_ready(child: &mut Child) -> Result<(), ErrorCode> {
    let mut child_reaped = false;
    let result = (|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        runtime.block_on(async {
            let deadline = tokio::time::Instant::now() + START_TIMEOUT;
            loop {
                if let Some(status) = child.try_wait().map_err(|_| ErrorCode::CoreUnavailable)? {
                    child_reaped = true;
                    let _ = status;
                    return Err(ErrorCode::CoreUnavailable);
                }
                if let Ok(connection) = hantong_platform::unix_transport::connect().await {
                    if let Ok(readiness) =
                        hantong_ipc_client::core_status_on_connection(connection).await
                        && readiness.transport_ready
                    {
                        return Ok(());
                    }
                }
                if tokio::time::Instant::now() >= deadline {
                    return Err(ErrorCode::CoreUnavailable);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
    })();
    if result.is_err() && !child_reaped {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn cli07_bootstrap_parser_accepts_only_fixed_mode() {
        assert_eq!(
            parse_mode(&args(&[MODE, BOOTSTRAP])),
            Ok(CommandMode::Bootstrap)
        );
    }

    #[test]
    fn cli07_bootstrap_parser_rejects_paths_pids_and_extra_arguments() {
        for args in [
            args(&[MODE]),
            args(&[MODE, BOOTSTRAP, "https://example.invalid"]),
            args(&[MODE, BOOTSTRAP, "123"]),
            args(&["update.sh", BOOTSTRAP]),
        ] {
            assert_eq!(parse_mode(&args), Err(ErrorCode::InvalidInput));
        }
    }
}
