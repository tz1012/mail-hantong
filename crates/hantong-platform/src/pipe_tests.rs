use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::time::{Duration, timeout};
use windows_sys::Win32::Security::{ACCESS_ALLOWED_ACE, GetAce, GetSecurityDescriptorDacl};

fn endpoint() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        r"\\.\pipe\mail-hantong-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[test]
fn sec03_descriptor_has_only_explicit_logon_data_rights() {
    let (_owner, attrs) = security_attributes().unwrap();
    let (mut present, mut defaulted, mut acl) = (0, 0, ptr::null_mut());
    unsafe {
        assert_ne!(
            GetSecurityDescriptorDacl(
                attrs.lpSecurityDescriptor,
                &mut present,
                &mut acl,
                &mut defaulted
            ),
            0
        );
        assert_ne!(present, 0);
        assert!(!acl.is_null());
        assert_eq!((*acl).AceCount, 1);
        let mut raw = ptr::null_mut();
        assert_ne!(GetAce(acl, 0, &mut raw), 0);
        let ace = &*raw.cast::<ACCESS_ALLOWED_ACE>();
        assert_eq!(ace.Header.AceType, 0);
        assert_eq!(ace.Mask, 0x100083);
        assert_eq!(ace.Mask & 4, 0);
    }
}

#[tokio::test]
async fn sec03_second_bind_fails_closed_and_drop_releases_name() {
    let name = endpoint();
    let first = LocalPipeServer::bind_endpoint(&name).unwrap();
    assert!(matches!(
        LocalPipeServer::bind_endpoint(&name),
        Err(ErrorCode::Conflict)
    ));
    drop(first);
    assert!(LocalPipeServer::bind_endpoint(&name).is_ok());
}

#[tokio::test]
async fn cli06_real_pipe_roundtrip_and_reconnect() {
    timeout(Duration::from_secs(5), async {
        let name = endpoint();
        let mut server = LocalPipeServer::bind_endpoint(&name).unwrap();
        for n in 41..61 {
            let accept = async {
                let mut stream = server.accept().await.unwrap();
                assert_eq!(
                    stream.peer_context().unwrap(),
                    current_peer_context().unwrap()
                );
                assert_eq!(stream.read_u8().await.unwrap(), n);
                stream.write_u8(n + 1).await.unwrap();
                assert_eq!(stream.read_u8().await.unwrap(), 0x06);
            };
            let send = async {
                let mut stream = connect_endpoint(&name)
                    .await
                    .unwrap_or_else(|e| panic!("iteration {n}: {e:?}"));
                stream.write_u8(n).await.unwrap();
                assert_eq!(stream.read_u8().await.unwrap(), n + 1);
                stream.write_u8(0x06).await.unwrap();
            };
            tokio::join!(accept, send);
        }
    })
    .await
    .unwrap();
}

fn raw_client(name: &str) -> NamedPipeClient {
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            CLIENT_RIGHTS,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT,
            ptr::null_mut(),
        )
    };
    assert!(handle != INVALID_HANDLE_VALUE && !handle.is_null());
    unsafe { NamedPipeClient::from_raw_handle(handle as _) }.unwrap()
}

fn assert_not_impersonating() {
    let mut handle = ptr::null_mut();
    let result = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut handle) };
    if result != 0 {
        unsafe {
            CloseHandle(handle);
        }
    }
    assert_eq!(result, 0, "thread must revert before returning");
}

#[tokio::test]
async fn sec03_peer_user_and_logon_mismatch_deny_and_revert() {
    for mismatch in ["user", "logon"] {
        let name = endpoint();
        let mut server = LocalPipeServer::bind_endpoint(&name).unwrap();
        if mismatch == "user" {
            server.expected.user = "S-1-0-0".into();
        } else {
            server.expected.logon = "S-1-0-0".into();
        }
        let (accepted, client) = timeout(Duration::from_secs(3), async {
            tokio::join!(server.accept(), connect_endpoint(&name))
        })
        .await
        .unwrap();
        assert!(client.is_ok());
        assert!(matches!(accepted, Err(ErrorCode::ScopeDenied)));
        drop(accepted);
        assert_not_impersonating();
    }
}

#[tokio::test]
async fn sec03_cancelled_accept_discards_old_preface_and_bytes() {
    let name = endpoint();
    let mut server = LocalPipeServer::bind_endpoint(&name).unwrap();
    let mut old = raw_client(&name);
    assert!(
        timeout(Duration::from_millis(30), server.accept())
            .await
            .is_err()
    );
    // The cancelled accept guard closed the old pipe; its bytes cannot become
    // the next client's preface or JSON frame.
    let _ = old.write_all(b"\x01old-untrusted-payload").await;
    drop(old);
    let (accepted, client) = timeout(Duration::from_secs(3), async {
        tokio::join!(server.accept(), connect_endpoint(&name))
    })
    .await
    .unwrap();
    assert!(accepted.is_ok());
    assert!(client.is_ok());
    assert_not_impersonating();
}

#[tokio::test]
async fn sec03_invalid_preface_does_not_poison_next_connection() {
    let name = endpoint();
    let mut server = LocalPipeServer::bind_endpoint(&name).unwrap();
    let mut bad = raw_client(&name);
    bad.write_u8(0xff).await.unwrap();
    assert!(matches!(server.accept().await, Err(ErrorCode::ScopeDenied)));
    drop(bad);
    let (accepted, client) = timeout(Duration::from_secs(3), async {
        tokio::join!(server.accept(), connect_endpoint(&name))
    })
    .await
    .unwrap();
    assert!(accepted.is_ok());
    assert!(client.is_ok());
    assert_not_impersonating();
}

#[tokio::test]
async fn sec03_instance_guard_survives_between_connections() {
    let name = endpoint();
    let mut server = LocalPipeServer::bind_endpoint(&name).unwrap();
    let (accepted, client) = tokio::join!(server.accept(), connect_endpoint(&name));
    drop(accepted.unwrap());
    drop(client.unwrap());
    assert!(server.inner.is_none());
    assert!(matches!(
        LocalPipeServer::bind_endpoint(&name),
        Err(ErrorCode::Conflict)
    ));
}

#[tokio::test]
async fn sec03_generic_write_does_not_allow_new_instances() {
    let name = endpoint();
    let _server = LocalPipeServer::bind_endpoint(&name).unwrap();
    let result = tokio::net::windows::named_pipe::ClientOptions::new().open(&name);
    assert!(result.is_err(), "GENERIC_WRITE must be denied");
}
