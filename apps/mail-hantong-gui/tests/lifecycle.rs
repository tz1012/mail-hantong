use std::process::Command;

#[test]
fn ops07_gui_help_and_version_return_without_a_native_window_or_connection() {
    for option in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_mail-hantong"))
            .arg(option)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .starts_with("mail-hantong")
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn sec08_gui_argument_errors_do_not_echo_private_values() {
    let output = Command::new(env!("CARGO_BIN_EXE_mail-hantong"))
        .args(["--profile", "private@example.invalid"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap().trim(),
        "INVALID_INPUT"
    );
}

#[cfg(not(windows))]
#[test]
fn ops07_gui_has_no_non_windows_native_fallback() {
    let output = Command::new(env!("CARGO_BIN_EXE_mail-hantong"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(28));
    assert!(output.stdout.is_empty());
}
