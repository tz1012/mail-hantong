use std::process::Command;

#[test]
fn cli16_core_help_and_version_do_not_start_transport() {
    for args in [
        vec!["--help"],
        vec!["--version"],
        vec!["--lang", "ko", "--help"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mail-hantong-core"))
            .args(&args)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(!output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        if args.contains(&"ko") {
            assert!(text.contains("사용법"));
        }
    }
}

#[test]
fn sec08_invalid_core_arguments_are_not_reflected() {
    let output = Command::new(env!("CARGO_BIN_EXE_mail-hantong-core"))
        .arg("private@example.invalid")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(text.contains("INVALID_INPUT"));
    assert!(!text.contains("private"));
}

#[cfg(not(windows))]
#[test]
fn sec03_no_non_windows_product_transport_fallback() {
    let output = Command::new(env!("CARGO_BIN_EXE_mail-hantong-core"))
        .arg("run")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(28));
    assert!(output.stdout.is_empty());
}
