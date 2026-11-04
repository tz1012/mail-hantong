//! Kernel-observed local Unix transport v2.

#![cfg(unix)]
#![allow(unsafe_code)]

use crate::unix_paths::{UnixDirectoryGuard, runtime_socket_dir, socket_path};
use crate::unix_peer::{observe_peer, peer_matches_current_image};
use hantong_contracts::{ErrorCode, unix_auth::UnixPeerContextV2};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{UnixListener, UnixStream};

const SOCKET_NAME: &str = "core-v2.sock";
const SOCKET_MODE: u32 = 0o600;

pub struct UnixLocalServer {
    listener: UnixListener,
    socket_path: PathBuf,
    socket_identity: (u64, u64),
    _guard: UnixDirectoryGuard,
    _lock: std::fs::File,
}

pub struct UnixLocalConnection {
    stream: UnixStream,
    peer: UnixPeerContextV2,
    _guard: Option<UnixDirectoryGuard>,
}

impl UnixLocalServer {
    pub fn bind() -> Result<Self, ErrorCode> {
        let dir = runtime_socket_dir()?;
        let path = socket_path()?;
        Self::bind_at(&dir, &path)
    }

    fn bind_at(dir: &Path, path: &Path) -> Result<Self, ErrorCode> {
        if path.parent() != Some(dir) {
            return Err(ErrorCode::InvalidInput);
        }
        let guard = UnixDirectoryGuard::prepare(dir)?;
        let lock = crate::unix_socket::open_socket_lock(path)?;
        match std::fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_socket() => {
                if metadata.uid() != unsafe { libc::geteuid() } {
                    return Err(ErrorCode::ScopeDenied);
                }
                std::fs::remove_file(path).map_err(|_| ErrorCode::Conflict)?;
            }
            Ok(_) => return Err(ErrorCode::ScopeDenied),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(ErrorCode::CoreUnavailable),
        }
        let listener =
            std::os::unix::net::UnixListener::bind(path).map_err(|_| ErrorCode::Conflict)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        let before = std::fs::symlink_metadata(path).map_err(|_| ErrorCode::CoreUnavailable)?;
        if !before.file_type().is_socket() || before.uid() != unsafe { libc::geteuid() } {
            return Err(ErrorCode::ScopeDenied);
        }
        let name = std::ffi::CString::new(SOCKET_NAME).map_err(|_| ErrorCode::InvalidInput)?;
        if unsafe {
            libc::fchmodat(
                guard.directory_fd()?.as_raw_fd(),
                name.as_ptr(),
                SOCKET_MODE,
                0,
            )
        } != 0
        {
            return Err(ErrorCode::ScopeDenied);
        }
        let metadata = std::fs::symlink_metadata(path).map_err(|_| ErrorCode::CoreUnavailable)?;
        if !metadata.file_type().is_socket()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o777 != SOCKET_MODE
            || (metadata.dev(), metadata.ino()) != (before.dev(), before.ino())
        {
            return Err(ErrorCode::ScopeDenied);
        }
        let socket_identity = (metadata.dev(), metadata.ino());
        let listener = UnixListener::from_std(listener).map_err(|_| ErrorCode::CoreUnavailable)?;
        Ok(Self {
            listener,
            socket_path: path.to_owned(),
            socket_identity,
            _guard: guard,
            _lock: lock,
        })
    }

    pub async fn accept(&self) -> Result<UnixLocalConnection, ErrorCode> {
        let (stream, _) = self
            .listener
            .accept()
            .await
            .map_err(|_| ErrorCode::CoreUnavailable)?;
        let peer = observe_peer(stream.as_raw_fd())?;
        if peer.effective_uid != unsafe { libc::geteuid() } {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(UnixLocalConnection {
            stream,
            peer,
            _guard: None,
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

pub async fn connect() -> Result<UnixLocalConnection, ErrorCode> {
    let path = socket_path()?;
    crate::unix_socket::validate_socket_endpoint(&path)?;
    let parent = path.parent().ok_or(ErrorCode::InvalidInput)?;
    let guard = UnixDirectoryGuard::open(parent, true)?;
    let before = std::fs::symlink_metadata(&path).map_err(|_| ErrorCode::CoreUnavailable)?;
    let stream = UnixStream::connect(&path)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let after = std::fs::symlink_metadata(&path).map_err(|_| ErrorCode::CoreUnavailable)?;
    if (before.dev(), before.ino()) != (after.dev(), after.ino()) {
        return Err(ErrorCode::Conflict);
    }
    let peer = observe_peer(stream.as_raw_fd())?;
    if peer.effective_uid != unsafe { libc::geteuid() } {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(UnixLocalConnection {
        stream,
        peer,
        _guard: Some(guard),
    })
}

impl UnixLocalConnection {
    pub fn peer_context(&self) -> &UnixPeerContextV2 {
        &self.peer
    }

    pub fn peer_matches_current_image(&self) -> Result<bool, ErrorCode> {
        peer_matches_current_image(&self.peer)
    }
}

impl AsyncRead for UnixLocalConnection {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for UnixLocalConnection {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        bytes: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.get_mut().stream).poll_write(cx, bytes)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

impl Drop for UnixLocalServer {
    fn drop(&mut self) {
        let Ok(metadata) = std::fs::symlink_metadata(&self.socket_path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o777 == 0o600
            && (metadata.dev(), metadata.ino()) == self.socket_identity
        {
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hantong-unix-transport-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir(&path).expect("temp directory");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
            .expect("private temp directory");
        path
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sec03_second_bind_is_rejected_while_lock_is_held() {
        let dir = temp_dir("bind");
        let path = dir.join(SOCKET_NAME);
        let first = UnixLocalServer::bind_at(&dir, &path).expect("first bind");
        assert!(matches!(
            UnixLocalServer::bind_at(&dir, &path),
            Err(ErrorCode::Conflict)
        ));
        drop(first);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sec03_stale_owned_socket_is_replaced_under_lock() {
        let dir = temp_dir("stale");
        let path = dir.join(SOCKET_NAME);
        let stale = std::os::unix::net::UnixListener::bind(&path).expect("stale socket");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("socket mode");
        drop(stale);
        let server = UnixLocalServer::bind_at(&dir, &path).expect("replace stale socket");
        let metadata = std::fs::symlink_metadata(&path).expect("new socket");
        assert!(metadata.file_type().is_socket());
        assert_eq!(metadata.mode() & 0o777, 0o600);
        drop(server);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sec03_bind_accept_roundtrip_observes_same_uid_peer() {
        let dir = temp_dir("roundtrip");
        let path = dir.join(SOCKET_NAME);
        let server = UnixLocalServer::bind_at(&dir, &path).expect("bind");
        let connecting = UnixStream::connect(&path);
        let accepting = server.accept();
        let (client, accepted) = tokio::join!(connecting, accepting);
        let mut client = client.expect("connect");
        let mut accepted = accepted.expect("accept");
        assert_eq!(accepted.peer_context().effective_uid, unsafe {
            libc::geteuid()
        });
        assert!(accepted.peer_matches_current_image().expect("image check"));
        client.write_all(b"ping").await.expect("write");
        let mut buffer = [0u8; 4];
        accepted.read_exact(&mut buffer).await.expect("read");
        assert_eq!(&buffer, b"ping");
        drop(server);
        let _ = std::fs::remove_dir_all(dir);
    }
}
