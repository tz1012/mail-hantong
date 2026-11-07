use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
#[path = "../../../tests/Fakes/linux_sealed_product_fixture.rs"]
mod linux_product_fixture;
macro_rules! core_command {
    ($fixture:ident,$command:ident) => {
        #[cfg(target_os = "linux")]
        let $fixture = linux_product_fixture::ProductFixture::new(
            "mail-hantong-core",
            env!("CARGO_BIN_EXE_mail-hantong-core"),
        );
        #[cfg(target_os = "linux")]
        let mut $command = $fixture.command();
        #[cfg(not(target_os = "linux"))]
        let mut $command = Command::new(env!("CARGO_BIN_EXE_mail-hantong-core"));
    };
}

fn bounded_output(mut command: Command) -> Output {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            // Reap this exact child even if it exited between the last poll
            // and kill. A timeout always fails the test.
            let _ = child.kill();
            let _ = child.wait();
            panic!("Core lifecycle command exceeded its deadline");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn cli16_core_help_and_version_do_not_start_transport() {
    for args in [
        vec!["--help"],
        vec!["--version"],
        vec!["--lang", "ko", "--help"],
    ] {
        core_command!(fixture, command);
        command.args(&args);
        let output = bounded_output(command);
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
    core_command!(fixture, command);
    command.arg("private@example.invalid");
    let output = bounded_output(command);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(text.contains("INVALID_INPUT"));
    assert!(!text.contains("private"));
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
#[test]
fn sec03_no_non_windows_product_transport_fallback() {
    core_command!(fixture, command);
    command.arg("run");
    let output = bounded_output(command);
    assert_eq!(output.status.code(), Some(28));
    assert!(output.stdout.is_empty());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sec03_native_unix_relay_rejects_missing_frame_without_fallback() {
    // An empty frame is rejected before path discovery, vault access or any
    // broker connection. Native startup itself is covered by the mandatory
    // isolated installed-product gates, never an unbounded local daemon.
    core_command!(fixture, command);
    command.arg("unix-relay");
    let output = bounded_output(command);
    assert_eq!(output.status.code(), Some(25));
    assert!(output.stdout.is_empty());
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(text.contains("CORE_UNAVAILABLE"));
    assert!(!text.contains("CAPABILITY_UNSUPPORTED"));
}

#[cfg(target_os = "linux")]
#[test]
fn sec03_linux_run_rejects_unsafe_storage_root_before_initialization() {
    core_command!(fixture, command);
    command
        .arg("run")
        .env("XDG_DATA_HOME", "relative-not-authorized")
        .env("XDG_CONFIG_HOME", "relative-not-authorized");
    let output = bounded_output(command);
    // Path discovery classifies an unusable XDG root as unsupported; the
    // directories stage distinguishes it from an absent native runtime.
    assert_eq!(output.status.code(), Some(28));
    assert!(output.stdout.is_empty());
    let text = String::from_utf8(output.stderr).unwrap();
    assert!(
        text.contains(
            "hantong_startup stage=directories outcome=failed code=CAPABILITY_UNSUPPORTED"
        )
    );
    assert!(!text.contains("relative-not-authorized"));
}
