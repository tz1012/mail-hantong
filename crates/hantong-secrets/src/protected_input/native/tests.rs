use super::*;
use windows_sys::Win32::Foundation::HWND;

struct Fixture(HWND);
impl Fixture {
    fn new() -> Self {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPED,
                0,
                0,
                400,
                400,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        assert_eq!(unsafe { IsWindowVisible(hwnd) }, 0);
        Self(hwnd)
    }
    fn child(&self, id: i32) -> HWND {
        unsafe { GetDlgItem(self.0, id) }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
}
fn plan() -> ImapPasswordCapturePlan {
    let config = hantong_domain::mail_connection::MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"fixture","imap":{"settings_version":1,"host":"imap.example","port":993,"tls":"implicit_tls","login_name":"fixture","authentication":"app_password"}}"#).unwrap();
    ImapPasswordCapturePlan::new("fixture", config.imap(), [1; 32], [2; 32]).unwrap()
}
fn set_text(hwnd: HWND, value: &str) {
    let text: Vec<u16> = value.encode_utf16().chain([0]).collect();
    assert_ne!(unsafe { SetWindowTextW(hwnd, text.as_ptr()) }, 0);
}

#[test]
fn inst05_native_rejects_non_root_before_creating_password_child() {
    let root = Fixture::new();
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let nested = unsafe {
        CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            WS_CHILD,
            0,
            0,
            50,
            50,
            root.0,
            70usize as _,
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!nested.is_null());
    let outcome = plan().attach_to_parent(nested as isize, 71);
    // Keep the red run safe even if the old implementation created a child:
    // destroy the fixture-owned child while its subclass state is still alive.
    if outcome.is_ok() {
        unsafe {
            DestroyWindow(GetDlgItem(nested, 71));
        }
    }
    assert!(matches!(outcome, Err(SecretError::InvalidInputWindow)));
}

#[test]
fn inst05_native_rejects_duplicate_control_id_without_replacing_original() {
    let root = Fixture::new();
    let original = plan().attach_to_parent(root.0 as isize, 72).unwrap();
    let hwnd = root.child(72);
    let outcome = plan().attach_to_parent(root.0 as isize, 72);
    assert!(matches!(outcome, Err(SecretError::InvalidInputWindow)));
    assert!(original.matches_child_hwnd(hwnd as isize));
}

#[test]
fn inst05_native_capture_decodes_surrogate_pairs_and_destroys_control() {
    let root = Fixture::new();
    let input = plan().attach_to_parent(root.0 as isize, 73).unwrap();
    let hwnd = root.child(73);
    set_text(hwnd, " synthetic-한글-😀 ");
    let pending = input.capture().unwrap();
    assert_eq!(pending.secret.as_slice(), " synthetic-한글-😀 ".as_bytes());
    assert_eq!(unsafe { IsWindow(hwnd) }, 0);
}

#[test]
fn inst05_native_enable_is_idempotent_and_uses_actual_enabled_state() {
    let root = Fixture::new();
    let input = plan().attach_to_parent(root.0 as isize, 74).unwrap();
    input.set_enabled(true).unwrap();
    input.set_enabled(true).unwrap();
    input.set_enabled(false).unwrap();
    input.set_enabled(false).unwrap();
    input.set_enabled(true).unwrap();
}

#[test]
fn inst05_parent_destroy_releases_hook_and_reused_handle_is_never_touched() {
    let mut root = Fixture::new();
    let mut input = plan().attach_to_parent(root.0 as isize, 75).unwrap();
    let lifetime = Rc::downgrade(&input.native.lifetime);
    assert_eq!(Rc::strong_count(&input.native.lifetime), 2);
    assert_ne!(unsafe { DestroyWindow(root.0) }, 0);
    root.0 = std::ptr::null_mut();
    assert!(!input.native.lifetime.alive.get());
    assert_eq!(Rc::strong_count(&input.native.lifetime), 1);
    let replacement = Fixture::new();
    // Deterministically simulate Windows reusing the numeric handle.
    input.native.hwnd = replacement.0;
    assert!(!input.matches_child_hwnd(replacement.0 as isize));
    assert_eq!(
        input.set_enabled(false),
        Err(SecretError::InvalidInputWindow)
    );
    drop(input);
    assert!(lifetime.upgrade().is_none());
    assert_ne!(unsafe { IsWindow(replacement.0) }, 0);
    assert_ne!(unsafe { IsWindowEnabled(replacement.0) }, 0);
}

#[test]
fn inst05_changed_style_or_parent_rejects_capture_but_destroys_owned_child() {
    for reparent in [false, true] {
        let root = Fixture::new();
        let another = Fixture::new();
        let input = plan().attach_to_parent(root.0 as isize, 76).unwrap();
        let hwnd = root.child(76);
        let lifetime = Rc::downgrade(&input.native.lifetime);
        set_text(hwnd, "synthetic-fixture");
        if reparent {
            assert_eq!(unsafe { SetParent(hwnd, another.0) }, root.0);
        } else {
            let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
            unsafe {
                SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(ES_PASSWORD as isize));
            }
        }
        assert!(matches!(
            input.capture(),
            Err(SecretError::InvalidInputWindow)
        ));
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
        assert!(lifetime.upgrade().is_none());
    }
}

#[test]
fn inst05_invalid_password_or_expired_plan_always_destroys_owned_child() {
    for value in [
        String::new(),
        "control\tfixture".into(),
        "a".repeat(1025),
        "한".repeat(342),
    ] {
        let root = Fixture::new();
        let input = plan().attach_to_parent(root.0 as isize, 77).unwrap();
        let hwnd = root.child(77);
        set_text(hwnd, &value);
        assert!(matches!(
            input.capture(),
            Err(SecretError::InvalidCredential)
        ));
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
    }
    let root = Fixture::new();
    let mut input = plan().attach_to_parent(root.0 as isize, 77).unwrap();
    let hwnd = root.child(77);
    set_text(hwnd, "synthetic-expired");
    input.plan.as_mut().unwrap().started = Instant::now() - INPUT_TTL;
    assert!(matches!(input.capture(), Err(SecretError::InputExpired)));
    assert_eq!(unsafe { IsWindow(hwnd) }, 0);
}

#[test]
fn inst05_cancel_drop_and_maximum_utf8_capture_clean_up() {
    let root = Fixture::new();
    for cancel in [false, true] {
        let input = plan().attach_to_parent(root.0 as isize, 78).unwrap();
        let hwnd = root.child(78);
        set_text(hwnd, "synthetic-cancel");
        if cancel {
            input.cancel().unwrap();
        } else {
            drop(input);
        }
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
    }
    for value in ["a".repeat(1024), "😀".repeat(256)] {
        let input = plan().attach_to_parent(root.0 as isize, 78).unwrap();
        let hwnd = root.child(78);
        set_text(hwnd, &value);
        let pending = input.capture().unwrap();
        assert_eq!(pending.secret.as_slice(), value.as_bytes());
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
    }
}

#[test]
fn inst05_decode_rejects_unpaired_surrogates_and_preserves_exact_unicode() {
    for value in [&[0xd800][..], &[0xdc00], &[97, 0xd800, 98], &[97, 0], &[10]] {
        assert!(matches!(
            decode_password(value),
            Err(SecretError::InvalidCredential)
        ));
    }
    let value: Vec<u16> = " 한글😀 ".encode_utf16().collect();
    assert_eq!(
        decode_password(&value).unwrap().as_slice(),
        " 한글😀 ".as_bytes()
    );
}

#[test]
fn inst05_wrong_thread_or_expired_plan_cannot_attach_to_root() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (finish, done) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let fixture = Fixture::new();
        tx.send(fixture.0 as isize).unwrap();
        done.recv().unwrap();
    });
    let foreign_thread = rx.recv().unwrap();
    let result = plan().attach_to_parent(foreign_thread, 79);
    finish.send(()).unwrap();
    thread.join().unwrap();
    assert!(matches!(result, Err(SecretError::InvalidInputWindow)));
    let root = Fixture::new();
    let mut expired = plan();
    expired.started = Instant::now() - INPUT_TTL;
    assert!(matches!(
        expired.attach_to_parent(root.0 as isize, 79),
        Err(SecretError::InputExpired)
    ));
    assert!(root.child(79).is_null());
    assert!(matches!(
        plan().attach_to_parent(0, 79),
        Err(SecretError::InvalidInputWindow)
    ));
    assert!(matches!(
        plan().attach_to_parent(root.0 as isize, 0),
        Err(SecretError::InvalidInputWindow)
    ));
}

#[test]
fn inst05_font_bounds_and_limit_changes_are_validated_without_reading_plaintext() {
    use windows_sys::Win32::Graphics::Gdi::{DEFAULT_GUI_FONT, GetStockObject};
    let root = Fixture::new();
    let input = plan().attach_to_parent(root.0 as isize, 80).unwrap();
    let hwnd = root.child(80);
    input.set_bounds(20, 30, 160, 24).unwrap();
    assert_eq!(
        input.set_bounds(0, 0, 0, 24),
        Err(SecretError::InvalidInputWindow)
    );
    let font = unsafe { GetStockObject(DEFAULT_GUI_FONT) };
    input.set_font(font as isize).unwrap();
    assert_eq!(
        unsafe { SendMessageW(hwnd, WM_GETFONT, 0, 0) },
        font as isize
    );
    assert_eq!(input.set_font(0), Err(SecretError::InvalidInputWindow));
    unsafe {
        SendMessageW(hwnd, EM_SETLIMITTEXT, 2048, 0);
    }
    assert!(matches!(
        input.capture(),
        Err(SecretError::InvalidInputWindow)
    ));
    assert_eq!(unsafe { IsWindow(hwnd) }, 0);
}

#[test]
fn inst05_hidden_or_changed_scroll_style_cannot_capture() {
    for removed in [WS_VISIBLE, ES_AUTOHSCROLL as u32] {
        let root = Fixture::new();
        let input = plan().attach_to_parent(root.0 as isize, 82).unwrap();
        let hwnd = root.child(82);
        set_text(hwnd, "synthetic-style");
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
        unsafe {
            SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(removed as isize));
        }
        assert!(matches!(
            input.capture(),
            Err(SecretError::InvalidInputWindow)
        ));
        assert_eq!(unsafe { IsWindow(hwnd) }, 0);
    }
}

#[test]
fn inst05_invalid_native_length_is_window_error_and_never_returns_pending() {
    unsafe extern "system" fn bad_length(
        hwnd: HWND,
        message: u32,
        w: usize,
        l: isize,
        _: usize,
        _: usize,
    ) -> isize {
        if message == WM_GETTEXTLENGTH {
            -1
        } else {
            unsafe { DefSubclassProc(hwnd, message, w, l) }
        }
    }
    let root = Fixture::new();
    let input = plan().attach_to_parent(root.0 as isize, 83).unwrap();
    let hwnd = root.child(83);
    assert_ne!(
        unsafe { SetWindowSubclass(hwnd, Some(bad_length), 1234, 0) },
        0
    );
    assert!(matches!(
        input.capture(),
        Err(SecretError::InvalidInputWindow)
    ));
    assert_eq!(unsafe { IsWindow(hwnd) }, 0);
}

fn pending(root: &Fixture) -> PendingImapCredential {
    let input = plan().attach_to_parent(root.0 as isize, 81).unwrap();
    set_text(root.child(81), "synthetic-pending");
    input.capture().unwrap()
}

#[test]
fn acc03_pending_target_and_expiry_mismatch_preserve_existing_dpapi_ciphertext() {
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "hantong-protected-pending-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&path).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    let _cleanup = Cleanup(path.clone());
    let store = SecretStore::open(path.join("keys")).unwrap();
    let root = Fixture::new();
    let expected = plan();
    let handle = store
        .store_imap_credential(
            "fixture",
            expected.endpoint(),
            b"synthetic-existing".as_slice(),
        )
        .unwrap();
    let before = fs::read(&handle.path).unwrap();
    assert_eq!(
        store.store_pending_imap_credential(pending(&root), "other", expected.endpoint()),
        Err(SecretError::CredentialMismatch)
    );
    assert_eq!(fs::read(&handle.path).unwrap(), before);
    let other = hantong_domain::mail_connection::MailConnectionConfig::from_json(br#"{"schema_version":1,"account_id":"fixture","imap":{"settings_version":2,"host":"other.example","port":993,"tls":"implicit_tls","login_name":"fixture","authentication":"app_password"}}"#).unwrap();
    assert_eq!(
        store.store_pending_imap_credential(pending(&root), "fixture", other.imap()),
        Err(SecretError::CredentialMismatch)
    );
    assert_eq!(fs::read(&handle.path).unwrap(), before);
    let mut expired = pending(&root);
    expired.captured_at = Instant::now() - INPUT_TTL;
    assert_eq!(
        store.store_pending_imap_credential(expired, "fixture", expected.endpoint()),
        Err(SecretError::InputExpired)
    );
    assert_eq!(fs::read(&handle.path).unwrap(), before);
    store
        .store_pending_imap_credential(pending(&root), "fixture", expected.endpoint())
        .unwrap();
    store
        .verify_imap_credential("fixture", expected.endpoint())
        .unwrap();
    let clear = store
        .unseal_imap_credential("fixture", expected.endpoint())
        .unwrap();
    assert_eq!(clear.text().unwrap(), "synthetic-pending");
}
