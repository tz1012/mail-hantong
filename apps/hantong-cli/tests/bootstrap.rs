use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_hantong"))
        .args(args)
        .output()
        .expect("CLI process")
}

#[test]
fn cli16_help_uses_english_by_default_and_korean_after_all_flags() {
    let en = cli(&["--help"]);
    assert!(String::from_utf8_lossy(&en.stdout).contains("Usage:"));
    assert!(String::from_utf8_lossy(&en.stdout).contains("clients verify"));
    let ko = cli(&["--help", "--lang", "ko"]);
    assert!(String::from_utf8_lossy(&ko.stdout).contains("사용법:"));
    let ko2 = cli(&["--lang", "ko", "--help"]);
    assert!(String::from_utf8_lossy(&ko2.stdout).contains("사용법:"));
}

#[test]
fn cli01_reconcile_help_and_invalid_commands_do_not_load_profiles() {
    for lang in ["en", "ko"] {
        let output = cli(&["--lang", lang, "--help"]);
        assert_eq!(output.status.code(), Some(0));
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("sync reconcile"));
        assert!(text.contains("sync reconcile-status"));
    }
    for args in [
        vec!["--json", "sync", "reconcile", "--account", "acct"],
        vec![
            "--json",
            "sync",
            "reconcile-status",
            "--account",
            "acct",
            "--execution-id",
            "00",
        ],
        vec![
            "--json",
            "sync",
            "reconcile",
            "--account",
            "acct",
            "--wire-hex",
            "494e424f58",
            "--encoding",
            "utf8",
            "--expected-catalog-generation",
            "1",
            "--expected-cursor-version",
            "0",
            "--operation-id",
            "op",
            "--idempotency-key",
            "key",
        ],
    ] {
        let output = cli(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stdout).contains("INVALID_INPUT"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn cli01_uid_invalid_commands_exit_before_any_profile_access() {
    for args in [
        vec!["--json", "sync", "page", "--account", "acct"],
        vec![
            "--json",
            "sync",
            "cursor",
            "--account",
            "acct",
            "--wire-hex",
            "AB",
            "--encoding",
            "utf8",
        ],
        vec![
            "--json",
            "sync",
            "status",
            "--account",
            "acct",
            "--execution-id",
            "00",
        ],
    ] {
        let output = cli(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stdout).contains("INVALID_INPUT"));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn cli15_verify_rejects_paths_before_loading_any_profile() {
    for selector in ["other", "../default", "D:/fixture/default.json"] {
        let output = cli(&[
            "--json",
            "--no-input",
            "--client",
            selector,
            "clients",
            "verify",
        ]);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stdout).contains("\"code\":\"INVALID_INPUT\""));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn cli16_version_uses_package_version() {
    let output = cli(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        format!("hantong {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn cli02_json_error_has_stable_fields_and_stdout_only() {
    let output = cli(&["--json", "send", "execute"]);
    assert_eq!(output.status.code(), Some(22));
    let text = String::from_utf8_lossy(&output.stdout);
    for field in [
        "schema_version",
        "ok",
        "error",
        "items",
        "next_cursor",
        "account_results",
        "completeness",
        "warnings",
        "observed_at",
    ] {
        assert!(text.contains(&format!("\"{field}\"")));
    }
    assert!(output.stderr.is_empty());
}

#[test]
fn cli04_no_input_does_not_wait_or_echo_untrusted_input() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hantong"))
        .args(["--no-input", "send", "execute"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("CLI process");
    let sentinel = "fixture-secret-do-not-echo-한통";
    if let Err(error) = child
        .stdin
        .as_mut()
        .expect("piped stdin")
        .write_all(sentinel.as_bytes())
    {
        assert_eq!(error.kind(), ErrorKind::BrokenPipe);
    }
    let start = Instant::now();
    while child.try_wait().expect("poll").is_none() {
        if start.elapsed() >= Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("--no-input waited for stdin");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().expect("output");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(22));
    assert!(!combined.contains(sentinel));
}

#[test]
fn cli15_client_name_cannot_authenticate_and_unknown_options_are_invalid() {
    let spoof = cli(&["--client", "admin", "accounts", "list"]);
    assert_eq!(spoof.status.code(), Some(2));
    // Protected command rejection is covered without loading a real profile.
    let unknown = cli(&["--mystery"]);
    assert_eq!(unknown.status.code(), Some(2));
    let missing = cli(&["--client", "--json", "accounts", "list"]);
    assert_eq!(missing.status.code(), Some(2));
}
