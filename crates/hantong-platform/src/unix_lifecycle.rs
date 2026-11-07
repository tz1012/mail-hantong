//! Narrow Unix lifecycle control used after a verified self-update.
//!
//! The caller supplies only a path returned by the release verifier.  No
//! arbitrary PID, executable, socket, signer, or secret is accepted here.

use hantong_contracts::{ErrorCode, unix_auth::UnixPeerContextV2};
use std::path::Path;

/// Detach a Core only after protected startup and storage recovery have
/// completed. The process keeps no terminal handles after this point.
#[cfg(unix)]
#[allow(unsafe_code)]
pub fn detach_background_process() -> Result<(), ErrorCode> {
    if unsafe { libc::setsid() } < 0 {
        return Err(ErrorCode::CoreUnavailable);
    }
    let null = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let fd = std::os::fd::AsRawFd::as_raw_fd(&null);
    for target in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        if unsafe { libc::dup2(fd, target) } < 0 {
            return Err(ErrorCode::CoreUnavailable);
        }
    }
    Ok(())
}

/// Stop the peer represented by an authenticated Unix peer context after
/// checking that it is the verified installed Core image.  Linux uses pidfd;
/// macOS deliberately has no equivalent implementation in this release.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub fn stop_verified_core(
    peer: &UnixPeerContextV2,
    verified_image: &Path,
) -> Result<(), ErrorCode> {
    #[cfg(not(test))]
    {
        let asset = crate::linux_identity::current_asset()?;
        if asset != "bin/mail-hantong-core" {
            return Err(ErrorCode::ScopeDenied);
        }
        let root = crate::linux_identity::current_release_root()?;
        let expected = root.join(asset);
        if verified_image != expected {
            return Err(ErrorCode::ScopeDenied);
        }
    }
    #[cfg(test)]
    let expected = std::fs::canonicalize(verified_image).map_err(|_| ErrorCode::CoreUnavailable)?;
    #[cfg(not(test))]
    let expected = verified_image.to_path_buf();
    #[cfg(test)]
    let current =
        std::fs::canonicalize(std::env::current_exe().map_err(|_| ErrorCode::CoreUnavailable)?)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
    #[cfg(not(test))]
    let current = crate::linux_identity::current_release_root()?.join("bin/mail-hantong-core");
    if expected != current || peer.effective_uid != unsafe { libc::geteuid() } {
        return Err(ErrorCode::ScopeDenied);
    }
    match crate::unix_peer::peer_matches_current_image(peer) {
        Ok(true) => {}
        Ok(false) => return Err(ErrorCode::ScopeDenied),
        Err(ErrorCode::AuthRequired)
            if matches!(
                std::fs::symlink_metadata(format!("/proc/{}/exe", peer.peer_pid)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound
            ) =>
        {
            return Ok(());
        }
        Err(error) => return Err(error),
    }
    let pid = libc::pid_t::try_from(peer.peer_pid).map_err(|_| ErrorCode::InvalidInput)?;
    let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if pidfd < 0 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ESRCH) => Ok(()),
            Some(libc::EPERM | libc::EACCES) => Err(ErrorCode::ScopeDenied),
            _ => Err(ErrorCode::CoreUnavailable),
        };
    }
    let result = (|| {
        match crate::unix_peer::peer_matches_current_image(peer) {
            Ok(true) => {}
            Ok(false) => return Err(ErrorCode::ScopeDenied),
            Err(ErrorCode::AuthRequired)
                if matches!(
                    std::fs::symlink_metadata(format!("/proc/{}/exe", peer.peer_pid)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound
                ) =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
        let sent = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                pidfd,
                libc::SIGTERM,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if sent < 0 {
            return match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ESRCH) => Ok(()),
                Some(libc::EPERM | libc::EACCES) => Err(ErrorCode::ScopeDenied),
                _ => Err(ErrorCode::CoreUnavailable),
            };
        }
        let mut pollfd = libc::pollfd {
            fd: pidfd as libc::c_int,
            events: libc::POLLIN,
            revents: 0,
        };
        let waited = unsafe { libc::poll(&mut pollfd, 1, 5_000) };
        if waited < 0 {
            return Err(ErrorCode::CoreUnavailable);
        }
        if waited == 0 {
            return Err(ErrorCode::Conflict);
        }
        Ok(())
    })();
    unsafe {
        libc::close(pidfd as libc::c_int);
    }
    result
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use hantong_contracts::unix_auth::{UnixPeerContextV2, UnixPlatform};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::process::Command;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn cli07_forged_process_instance_is_rejected_before_pidfd_signal() {
        let mut peer = crate::unix_peer::current_peer_context().expect("current peer");
        peer.process_instance[0] ^= 1;
        let image = std::env::current_exe().expect("test image");
        assert_eq!(
            stop_verified_core(&peer, &image),
            Err(ErrorCode::ScopeDenied)
        );
    }

    #[test]
    fn cli07_already_exited_peer_is_a_noop() {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn short-lived child");
        let pid = child.id();
        let _ = child.wait();
        let current = crate::unix_peer::current_peer_context().expect("current peer");
        let peer = UnixPeerContextV2::new(
            UnixPlatform::Linux,
            current.effective_uid,
            current.effective_gid,
            pid,
            1,
            [7; 32],
        )
        .expect("peer shape");
        let image = std::env::current_exe().expect("test image");
        assert_eq!(stop_verified_core(&peer, &image), Ok(()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cli07_pidfd_stops_same_image_child_only() {
        if std::env::var_os("HANTONG_LIFECYCLE_ISOLATED").is_none() {
            let root = std::env::temp_dir().join(format!(
                "hantong-lifecycle-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos()
            ));
            std::fs::create_dir(&root).expect("runtime root");
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                .expect("private runtime root");
            let status = Command::new(std::env::current_exe().expect("test executable"))
                .arg("--exact")
                .arg("unix_lifecycle::tests::cli07_pidfd_stops_same_image_child_only")
                .arg("--nocapture")
                .env("HANTONG_LIFECYCLE_ISOLATED", "1")
                .env("XDG_RUNTIME_DIR", &root)
                .status()
                .expect("isolated test");
            let _ = std::fs::remove_dir_all(root);
            assert!(status.success());
            return;
        }
        let server = crate::unix_transport::UnixLocalServer::bind().expect("bind");
        let socket = server.socket_path().to_owned();
        let mut child = ChildGuard(
            Command::new(std::env::current_exe().expect("test executable"))
                .arg("--exact")
                .arg("unix_lifecycle::tests::cli07_same_image_child_helper")
                .arg("--nocapture")
                .env("HANTONG_LIFECYCLE_CHILD", "1")
                .env("HANTONG_TEST_SOCKET", &socket)
                .spawn()
                .expect("same-image child"),
        );
        let connection = tokio::time::timeout(Duration::from_secs(5), server.accept())
            .await
            .expect("child accept timeout")
            .expect("accept child");
        let peer = connection.peer_context().clone();
        let image = std::env::current_exe().expect("test image");
        assert_eq!(stop_verified_core(&peer, &image), Ok(()));
        let status = child.0.wait().expect("child exit");
        assert!(!status.success());
        drop(connection);
        drop(server);
    }

    #[test]
    fn cli07_same_image_child_helper() {
        if std::env::var_os("HANTONG_LIFECYCLE_CHILD").is_none() {
            return;
        }
        let path = std::env::var_os("HANTONG_TEST_SOCKET").expect("socket path");
        let _stream = UnixStream::connect(path).expect("connect parent");
        std::thread::sleep(Duration::from_secs(30));
    }
}

#[cfg(target_os = "macos")]
pub fn stop_verified_core(
    _peer: &UnixPeerContextV2,
    _verified_image: &Path,
) -> Result<(), ErrorCode> {
    Err(ErrorCode::HumanStepRequired)
}
