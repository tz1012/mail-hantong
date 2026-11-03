//! User-driven Claude Code sessions with the HanTong MCP bridge attached.
//!
//! This is deliberately a thin process handoff. OAuth is supported only by
//! the provider's own interactive executable; HanTong never implements the
//! OAuth protocol or sends a prompt through a programmatic session.

use super::{AiError, Directory, Provider, agent::Agent, resolve, status};
use serde_json::Value;
use std::{
    io::IsTerminal,
    process::{Command, Stdio},
};

fn interactive_gate(stdin_tty: bool, stdout_tty: bool) -> Result<(), AiError> {
    if stdin_tty && stdout_tty {
        Ok(())
    } else {
        Err(AiError::HumanStepRequired)
    }
}

fn native_args(mcp_config: &Value) -> Vec<String> {
    vec![
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        mcp_config.to_string(),
        "--tools".into(),
        "".into(),
        "--safe-mode".into(),
        "--restricted".into(),
        "--setting-sources".into(),
        "".into(),
    ]
}

/// Hand control to the unmodified official Claude Code binary.
///
/// The caller must have completed HanTong's same-terminal mail registration
/// first. The provider status check and MCP preflight happen before the child
/// is launched, while the child owns the actual OAuth/session interaction.
pub fn run_claude_interactive() -> Result<(), AiError> {
    interactive_gate(
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    )?;

    let auth = status(Provider::Claude)?;
    if !auth.authenticated {
        return Err(AiError::LoginRequired);
    }

    let agent = Agent::connect(Provider::Claude)?;
    let args = native_args(&agent.claude_config());
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let cwd = Directory::new()?;
    let mut child = Command::new(resolve(Provider::Claude)?)
        .args(refs)
        .current_dir(&cwd.0)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| AiError::ProviderMissing)?;

    match child.wait() {
        Ok(status) if status.success() => Ok(()),
        Ok(_) => Err(AiError::ProviderFailed),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(AiError::Io)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai_native_claude_gate_rejects_noninteractive_without_provider_calls() {
        assert_eq!(
            interactive_gate(false, true),
            Err(AiError::HumanStepRequired)
        );
        assert_eq!(
            interactive_gate(true, false),
            Err(AiError::HumanStepRequired)
        );
    }

    #[test]
    fn ai_native_claude_argv_is_restricted_and_prompt_free() {
        let config = serde_json::json!({"mcpServers":{"hantong":{"command":"hantong-mcp"}}});
        let args = native_args(&config);
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--strict-mcp-config" && w[1] == "--mcp-config")
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--tools" && w[1].is_empty())
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--setting-sources" && w[1].is_empty())
        );
        assert!(args.contains(&"--safe-mode".to_owned()));
        assert!(args.contains(&"--restricted".to_owned()));
        // This flag is print-mode-only; never advertise it for native sessions.
        assert!(!args.contains(&"--no-session-persistence".to_owned()));
        for forbidden in [
            "--allowedTools",
            "--prompt",
            "-p",
            "--input-format",
            "--output-format",
        ] {
            assert!(!args.iter().any(|arg| arg == forbidden));
        }
        assert_eq!(args[2], config.to_string());
    }
}
