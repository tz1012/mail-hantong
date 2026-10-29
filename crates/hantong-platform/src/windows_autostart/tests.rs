use super::*;
use hantong_contracts::platform::{AutoStart, AutoStartState, PlatformError};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use windows_sys::Win32::System::Registry::*;

static NEXT: AtomicU64 = AtomicU64::new(0);

struct TestKey {
    path: String,
}
impl TestKey {
    fn new() -> Self {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let nonce = u32::from_le_bytes(crate::random_nonce().unwrap()[..4].try_into().unwrap());
        let path = format!(
            "Software\\mail-hantong\\tests\\autostart-{}-{}-{}",
            std::process::id(),
            id,
            nonce
        );
        create_key(&path);
        Self { path }
    }
    fn auto(&self) -> WindowsAutoStart {
        let value = WindowsAutoStart::command_for_path(Path::new(
            r"C:\Program Files\mail-hantong\mail-hantong-core.exe",
        ))
        .unwrap();
        WindowsAutoStart {
            value,
            run_path: wide(&self.path),
        }
    }
}
impl Drop for TestKey {
    fn drop(&mut self) {
        let name = wide(&self.path);
        unsafe {
            // This is the exact uniquely-owned test key, never a parent tree.
            let status = RegDeleteKeyW(HKEY_CURRENT_USER, name.as_ptr());
            if !std::thread::panicking() {
                assert_eq!(status, 0, "owned test key cleanup failed");
            }
        }
    }
}

fn create_key(path: &str) {
    let mut key = std::ptr::null_mut();
    let name = wide(path);
    let mut disposition = 0u32;
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            name.as_ptr(),
            0,
            std::ptr::null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_READ | KEY_WRITE,
            std::ptr::null(),
            &mut key,
            &mut disposition,
        )
    };
    let _owned = Key(key);
    assert_eq!(status, 0);
    assert_eq!(disposition, REG_CREATED_NEW_KEY);
}

fn set_raw(path: &str, kind: u32, bytes: &[u8]) {
    set_named_raw(path, VALUE_NAME, kind, bytes);
}

fn set_named_raw(path: &str, value: &str, kind: u32, bytes: &[u8]) {
    let mut key = std::ptr::null_mut();
    let name = wide(path);
    let value_name = wide(value);
    assert_eq!(
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                name.as_ptr(),
                0,
                KEY_READ | KEY_WRITE,
                &mut key,
            )
        },
        0
    );
    let _owned = Key(key);
    assert_eq!(
        unsafe {
            RegSetValueExW(
                key,
                value_name.as_ptr(),
                0,
                kind,
                bytes.as_ptr(),
                bytes.len() as u32,
            )
        },
        0
    );
}

fn read_raw(path: &str) -> (u32, Vec<u8>) {
    read_named_raw(path, VALUE_NAME)
}

fn read_named_raw(path: &str, value: &str) -> (u32, Vec<u8>) {
    let mut key = std::ptr::null_mut();
    let name = wide(path);
    let value_name = wide(value);
    assert_eq!(
        unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, name.as_ptr(), 0, KEY_READ, &mut key) },
        0
    );
    let _owned = Key(key);
    let mut kind = 0;
    let mut size = 0;
    assert_eq!(
        unsafe {
            RegQueryValueExW(
                key,
                value_name.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                std::ptr::null_mut(),
                &mut size,
            )
        },
        0
    );
    let mut bytes = vec![0; size as usize];
    assert_eq!(
        unsafe {
            RegQueryValueExW(
                key,
                value_name.as_ptr(),
                std::ptr::null_mut(),
                &mut kind,
                bytes.as_mut_ptr(),
                &mut size,
            )
        },
        0
    );
    bytes.truncate(size as usize);
    (kind, bytes)
}

fn utf16_bytes(value: &[u16]) -> Vec<u8> {
    value.iter().flat_map(|unit| unit.to_le_bytes()).collect()
}

#[test]
fn inst13_absent_and_missing_key_are_disabled_and_idempotent() {
    let fixture = TestKey::new();
    let mut auto = fixture.auto();
    assert_eq!(auto.state(), Ok(AutoStartState::Disabled));
    assert_eq!(auto.set_enabled(false), Ok(()));
    assert_eq!(auto.set_enabled(true), Ok(()));
    assert_eq!(auto.state(), Ok(AutoStartState::Enabled));
    assert_eq!(auto.set_enabled(true), Ok(()));
    assert_eq!(auto.set_enabled(false), Ok(()));
    assert_eq!(auto.state(), Ok(AutoStartState::Disabled));
    assert_eq!(auto.set_enabled(false), Ok(()));
}

#[test]
fn inst13_owned_value_is_exact_reg_sz_and_roundtrips() {
    let fixture = TestKey::new();
    let mut auto = fixture.auto();
    auto.set_enabled(true).unwrap();
    let (kind, bytes) = read_raw(&fixture.path);
    assert_eq!(kind, REG_SZ);
    assert_eq!(bytes, utf16_bytes(&auto.value));
    assert_eq!(auto.state(), Ok(AutoStartState::Enabled));
}

#[test]
fn inst13_missing_key_disable_is_idempotent_and_enable_recreates_it() {
    let fixture = TestKey::new();
    let mut auto = fixture.auto();
    let name = wide(&fixture.path);
    assert_eq!(
        unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, name.as_ptr()) },
        0
    );
    assert_eq!(auto.state(), Ok(AutoStartState::Disabled));
    assert_eq!(auto.set_enabled(false), Ok(()));
    assert_eq!(auto.set_enabled(true), Ok(()));
    assert_eq!(auto.state(), Ok(AutoStartState::Enabled));
}

#[test]
fn inst13_foreign_value_is_conflict_and_never_overwritten_or_deleted() {
    let fixture = TestKey::new();
    let mut auto = fixture.auto();
    let foreign = utf16_bytes(&wide(r#""C:\Other\mail-hantong-core.exe" --supervise"#));
    set_raw(&fixture.path, REG_SZ, &foreign);
    assert_eq!(auto.state(), Ok(AutoStartState::OwnershipConflict));
    assert_eq!(auto.set_enabled(true), Err(PlatformError::Denied));
    assert_eq!(auto.set_enabled(false), Err(PlatformError::Denied));
    assert_eq!(read_raw(&fixture.path), (REG_SZ, foreign));
}

#[test]
fn inst13_sibling_value_survives_owned_value_changes() {
    let fixture = TestKey::new();
    let mut auto = fixture.auto();
    set_named_raw(
        &fixture.path,
        "other-value",
        REG_SZ,
        &utf16_bytes(&wide("keep")),
    );
    auto.set_enabled(true).unwrap();
    auto.set_enabled(false).unwrap();
    assert_eq!(
        read_named_raw(&fixture.path, "other-value"),
        (REG_SZ, utf16_bytes(&wide("keep")))
    );
}

#[test]
fn inst13_malformed_values_are_conflicts_and_preserved() {
    let fixture = TestKey::new();
    let cases = [
        (REG_DWORD, vec![1, 0, 0, 0]),
        (REG_SZ, vec![b'a']),
        (REG_SZ, utf16_bytes(&[b'a' as u16, b'b' as u16])),
        (REG_SZ, utf16_bytes(&[b'a' as u16, 0, b'b' as u16, 0])),
        (REG_SZ, utf16_bytes(&[0xd800, 0])),
        (REG_SZ, vec![b'a'; 524]),
    ];
    for (kind, bytes) in cases {
        set_raw(&fixture.path, kind, &bytes);
        // Compare the value actually retained by Windows, which may append a
        // terminator for a malformed REG_SZ fixture during native storage/query.
        let retained = read_raw(&fixture.path);
        let mut auto = fixture.auto();
        assert_eq!(auto.state(), Ok(AutoStartState::OwnershipConflict));
        assert_eq!(auto.set_enabled(true), Err(PlatformError::Denied));
        assert_eq!(auto.set_enabled(false), Err(PlatformError::Denied));
        assert_eq!(read_raw(&fixture.path), retained);
    }
}

#[test]
fn inst13_path_validation_rejects_unsafe_forms_and_accepts_spaces_korean() {
    for path in [
        PathBuf::from(r"relative\mail-hantong-core.exe"),
        PathBuf::from(r"\\server\share\mail-hantong-core.exe"),
        PathBuf::from(r"\\?\C:\mail-hantong-core.exe"),
        PathBuf::from(r#"C:\mail-hantong-core.exe" --bad"#),
        PathBuf::from("C:\\bad\0name\\mail-hantong-core.exe"),
        PathBuf::from("C:\\bad\nname\\mail-hantong-core.exe"),
        PathBuf::from(r"C:\%PATH%\mail-hantong-core.exe"),
    ] {
        assert_eq!(
            WindowsAutoStart::command_for_path(&path),
            Err(PlatformError::Denied)
        );
    }
    let valid = Path::new(r"C:\Program Files\한통\mail-hantong-core.exe");
    assert!(WindowsAutoStart::command_for_path(valid).is_ok());
    let fixed = format!("\"{}\" --supervise", r"C:\\mail-hantong-core.exe");
    let border = format!(
        r"C:\{}\mail-hantong-core.exe",
        "a".repeat(260 - fixed.encode_utf16().count())
    );
    assert_eq!(
        WindowsAutoStart::command_for_path(Path::new(&border))
            .unwrap()
            .len(),
        261
    );
    let mut longer = border.clone();
    let marker = r"\mail-hantong-core.exe";
    let marker_start = longer.rfind(marker).unwrap();
    longer.insert(marker_start, 'a');
    assert_eq!(
        WindowsAutoStart::command_for_path(Path::new(&longer)),
        Err(PlatformError::Denied)
    );
}

#[test]
fn inst13_registry_query_error_is_not_reported_as_disabled() {
    let fixture = TestKey::new();
    assert_eq!(
        fixture.auto().state_for_key(&Key(std::ptr::null_mut())),
        Err(PlatformError::Denied)
    );
}

#[test]
fn inst13_idempotent_state_needs_no_registry_write_access() {
    use windows_sys::Win32::Security::{
        Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1},
        DACL_SECURITY_INFORMATION,
    };
    struct Descriptor(*mut std::ffi::c_void);
    impl Drop for Descriptor {
        fn drop(&mut self) {
            unsafe {
                windows_sys::Win32::Foundation::LocalFree(self.0);
            }
        }
    }
    for enabled in [false, true] {
        let fixture = TestKey::new();
        let mut auto = fixture.auto();
        if enabled {
            auto.set_enabled(true).unwrap();
        }
        let mut raw = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    wide(&fixture.path).as_ptr(),
                    0,
                    KEY_ALL_ACCESS,
                    &mut raw,
                )
            },
            0
        );
        let key = Key(raw);
        let sid = crate::current_peer_context().unwrap();
        let sddl = wide(&format!(
            "D:P(D;;0x2;;;{})(A;;KA;;;{})",
            sid.user_sid(),
            sid.user_sid()
        ));
        let mut raw_descriptor = std::ptr::null_mut();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut raw_descriptor,
                    std::ptr::null_mut(),
                )
            },
            0
        );
        let descriptor = Descriptor(raw_descriptor);
        assert_eq!(
            unsafe { RegSetKeySecurity(key.0, DACL_SECURITY_INFORMATION, descriptor.0) },
            0
        );
        assert!(matches!(auto.open(true, false), Err(PlatformError::Denied)));
        assert_eq!(auto.set_enabled(enabled), Ok(()));
        assert_eq!(auto.set_enabled(!enabled), Err(PlatformError::Denied));
        drop(key);
    }
}
