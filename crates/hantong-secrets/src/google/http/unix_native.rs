//! Bounded Unix Google HTTP transport.
//!
//! The public transport trait is synchronous, so Unix I/O is serialized through
//! one dedicated worker.  A caller that times out abandons its reply receiver;
//! the worker nevertheless finishes the in-flight operation before admitting a
//! second request.  This prevents DNS or TLS work from becoming an unbounded
//! thread/queue resource.

use super::super::diagnostics::{
    self, TransportFailureKind as FailureKind, TransportStage as Stage,
};
use super::{
    Host, Request, Response,
    unix::{framed_response, parse_response},
};
use crate::SensitiveBuffer;
use hantong_contracts::ErrorCode;
use rustls::{ClientConfig, pki_types::ServerName};
use rustls_platform_verifier::BuilderVerifierExt;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, SyncSender, sync_channel},
};
use std::thread;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, lookup_host};
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

const MAX_RESPONSE_OVERHEAD: usize = 64 * 1024;

struct OwnedRequest {
    host: Host,
    path: SensitiveBuffer,
    post: Option<SensitiveBuffer>,
    bearer: Option<SensitiveBuffer>,
    limit: usize,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
}

struct Job {
    request: OwnedRequest,
    reply: SyncSender<Result<Response, ErrorCode>>,
}

struct Worker {
    tx: SyncSender<Job>,
    busy: Arc<AtomicBool>,
}

static WORKER: OnceLock<Result<Worker, ErrorCode>> = OnceLock::new();

pub(super) fn request(request: Request<'_>) -> Result<Response, ErrorCode> {
    super::validate(&request)?;
    if request.cancel.load(Ordering::Acquire) {
        return Err(ErrorCode::Offline);
    }
    let worker = WORKER.get_or_init(start_worker).as_ref().map_err(|e| *e)?;
    if worker
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(ErrorCode::Offline);
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let owned = OwnedRequest {
        host: request.host,
        path: SensitiveBuffer(request.path.as_bytes().to_vec()),
        post: request.post.map(|body| SensitiveBuffer(body.to_vec())),
        bearer: request
            .bearer
            .map(|bearer| SensitiveBuffer(bearer.as_bytes().to_vec())),
        limit: request.limit,
        deadline: request.deadline,
        cancel: Arc::clone(&cancel),
    };
    let (reply_tx, reply_rx) = sync_channel(1);
    if worker
        .tx
        .send(Job {
            request: owned,
            reply: reply_tx,
        })
        .is_err()
    {
        worker.busy.store(false, Ordering::Release);
        return Err(ErrorCode::Offline);
    }
    loop {
        match reply_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => {
                if request.cancel.load(Ordering::Acquire) || Instant::now() >= request.deadline {
                    cancel.store(true, Ordering::Release);
                    return Err(ErrorCode::Offline);
                }
                return result;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if request.cancel.load(Ordering::Acquire)
                    || cancel.load(Ordering::Relaxed)
                    || Instant::now() >= request.deadline
                {
                    cancel.store(true, Ordering::Release);
                    return Err(ErrorCode::Offline);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err(ErrorCode::Offline),
        }
    }
}

fn start_worker() -> Result<Worker, ErrorCode> {
    let (tx, rx) = sync_channel(1);
    let busy = Arc::new(AtomicBool::new(false));
    let thread_busy = Arc::clone(&busy);
    thread::Builder::new()
        .name("hantong-unix-http".to_owned())
        .spawn(move || worker_loop(rx, thread_busy))
        .map_err(|_| ErrorCode::Offline)?;
    Ok(Worker { tx, busy })
}

fn worker_loop(rx: Receiver<Job>, busy: Arc<AtomicBool>) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .max_blocking_threads(1)
        .build()
    else {
        busy.store(false, Ordering::Release);
        return;
    };
    while let Ok(job) = rx.recv() {
        let result = runtime.block_on(run(job.request));
        let _ = job.reply.send(result);
        busy.store(false, Ordering::Release);
    }
}

async fn run(request: OwnedRequest) -> Result<Response, ErrorCode> {
    check(&request)?;
    let host = request.host.name();
    let config = ClientConfig::builder()
        .with_platform_verifier()
        .map_err(|_| failure(&request, Stage::Tls, FailureKind::Tls, ErrorCode::Offline))?
        .with_no_client_auth();
    // DNS is intentionally awaited to completion.  Dropping a lookup future
    // does not prove the underlying resolver has stopped; the worker remains
    // occupied until this completes, while the caller may already time out.
    let addresses = lookup_host((host, 443))
        .await
        .map_err(|e| io_failure(&request, Stage::Dns, &e))?;
    let mut stream = None;
    let mut connect_kind = FailureKind::Io;
    for address in addresses.take(8) {
        check(&request)?;
        match timeout(remaining(&request)?, TcpStream::connect(address)).await {
            Ok(Ok(connected)) => {
                stream = Some(connected);
                break;
            }
            Ok(Err(_)) => connect_kind = FailureKind::Io,
            Err(_) => connect_kind = FailureKind::Timeout,
        }
    }
    let stream = stream
        .ok_or_else(|| failure(&request, Stage::Connect, connect_kind, ErrorCode::Offline))?;
    check(&request)?;
    let server_name = ServerName::try_from(host.to_owned()).map_err(|_| ErrorCode::Offline)?;
    let connector = TlsConnector::from(Arc::new(config));
    let mut stream = timeout(remaining(&request)?, connector.connect(server_name, stream))
        .await
        .map_err(|_| {
            failure(
                &request,
                Stage::Tls,
                FailureKind::Timeout,
                ErrorCode::Offline,
            )
        })?
        .map_err(|_| failure(&request, Stage::Tls, FailureKind::Tls, ErrorCode::Offline))?;
    check(&request)?;
    let wire = request_bytes(&request)?;
    timeout(remaining(&request)?, stream.write_all(&wire.0))
        .await
        .map_err(|_| {
            failure(
                &request,
                Stage::Write,
                FailureKind::Timeout,
                ErrorCode::Offline,
            )
        })?
        .map_err(|e| io_failure(&request, Stage::Write, &e))?;
    read_response(&mut stream, &request).await
}

async fn read_response(
    stream: &mut (impl AsyncRead + Unpin),
    request: &OwnedRequest,
) -> Result<Response, ErrorCode> {
    let cap = (16usize * 1024)
        .checked_add(request.limit)
        .and_then(|n| n.checked_add(MAX_RESPONSE_OVERHEAD))
        .ok_or(ErrorCode::QuotaExceeded)?;
    let mut raw = SensitiveBuffer(Vec::with_capacity(cap.min(16 * 1024)));
    let mut chunk = SensitiveBuffer(vec![0; 8192]);
    loop {
        check(request)?;
        let read = timeout(remaining(request)?, stream.read(&mut chunk.0))
            .await
            .map_err(|_| {
                failure(
                    request,
                    Stage::Read,
                    FailureKind::Timeout,
                    ErrorCode::Offline,
                )
            })?
            .map_err(|e| io_failure(request, Stage::Read, &e))?;
        if read == 0 {
            break;
        }
        if raw.0.len() > cap.saturating_sub(read) {
            return Err(failure(
                request,
                Stage::Framing,
                FailureKind::Quota,
                ErrorCode::QuotaExceeded,
            ));
        }
        raw.0.extend_from_slice(&chunk.0[..read]);
        // HTTP framing, not TLS shutdown, determines completion. A peer may
        // omit close_notify after the full Content-Length/chunked response.
        if let Some((status, body)) = framed_response(&raw.0, request.limit, false)
            .map_err(|error| framing_failure(request, error))?
        {
            check(request)?;
            return Ok(Response { status, body });
        }
    }
    let (status, body) =
        parse_response(&raw.0, request.limit).map_err(|error| framing_failure(request, error))?;
    check(request)?;
    Ok(Response { status, body })
}

fn failure(request: &OwnedRequest, stage: Stage, kind: FailureKind, error: ErrorCode) -> ErrorCode {
    diagnostics::transport_failure(request.host, stage, kind, error);
    error
}

fn io_failure(request: &OwnedRequest, stage: Stage, error: &std::io::Error) -> ErrorCode {
    let kind = if error.kind() == std::io::ErrorKind::UnexpectedEof {
        FailureKind::UnexpectedEof
    } else {
        FailureKind::Io
    };
    failure(request, stage, kind, ErrorCode::Offline)
}

fn framing_failure(request: &OwnedRequest, error: ErrorCode) -> ErrorCode {
    let kind = if error == ErrorCode::QuotaExceeded {
        FailureKind::Quota
    } else {
        FailureKind::InvalidFrame
    };
    failure(request, Stage::Framing, kind, error)
}

fn request_bytes(request: &OwnedRequest) -> Result<SensitiveBuffer, ErrorCode> {
    let path = std::str::from_utf8(&request.path.0).map_err(|_| ErrorCode::InvalidInput)?;
    let mut out = Vec::with_capacity(256 + request.post.as_ref().map_or(0, |b| b.0.len()));
    out.extend_from_slice(if request.post.is_some() {
        b"POST "
    } else {
        b"GET "
    });
    out.extend_from_slice(path.as_bytes());
    out.extend_from_slice(b" HTTP/1.1\r\nHost: ");
    out.extend_from_slice(request.host.name().as_bytes());
    out.extend_from_slice(b"\r\nConnection: close\r\nAccept-Encoding: identity\r\n");
    if let Some(bearer) = &request.bearer {
        out.extend_from_slice(b"Authorization: Bearer ");
        out.extend_from_slice(&bearer.0);
        out.extend_from_slice(b"\r\n");
    }
    if let Some(post) = &request.post {
        let content_type = if request.host == Host::Token {
            b"application/x-www-form-urlencoded".as_slice()
        } else {
            b"application/json".as_slice()
        };
        out.extend_from_slice(b"Content-Type: ");
        out.extend_from_slice(content_type);
        out.extend_from_slice(b"\r\nContent-Length: ");
        out.extend_from_slice(post.0.len().to_string().as_bytes());
        out.extend_from_slice(b"\r\n\r\n");
        out.extend_from_slice(&post.0);
    } else {
        out.extend_from_slice(b"\r\n");
    }
    Ok(SensitiveBuffer(out))
}

fn check(request: &OwnedRequest) -> Result<(), ErrorCode> {
    if request.cancel.load(Ordering::Acquire) || Instant::now() >= request.deadline {
        Err(ErrorCode::Offline)
    } else {
        Ok(())
    }
}

fn remaining(request: &OwnedRequest) -> Result<Duration, ErrorCode> {
    check(request)?;
    Ok(request
        .deadline
        .saturating_duration_since(Instant::now())
        .max(Duration::from_millis(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        pin::Pin,
        task::{Context, Poll},
    };
    use tokio::io::ReadBuf;

    // Models rustls returning data first, then an error for missing close_notify.
    // No certificate bypass or external network is involved.
    struct Reply {
        bytes: Vec<u8>,
        position: usize,
        fragment: usize,
        terminal: Option<io::ErrorKind>,
        terminal_reads: usize,
    }

    impl AsyncRead for Reply {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            out: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.position < self.bytes.len() {
                let end = self
                    .bytes
                    .len()
                    .min(self.position + self.fragment.min(out.remaining()));
                out.put_slice(&self.bytes[self.position..end]);
                self.position = end;
                Poll::Ready(Ok(()))
            } else {
                self.terminal_reads += 1;
                Poll::Ready(
                    self.terminal
                        .map_or(Ok(()), |kind| Err(io::Error::from(kind))),
                )
            }
        }
    }

    fn request_fixture() -> OwnedRequest {
        OwnedRequest {
            host: Host::Token,
            path: SensitiveBuffer(b"/token".to_vec()),
            post: None,
            bearer: None,
            limit: 1024,
            deadline: Instant::now() + Duration::from_secs(5),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn reply(bytes: &[u8], fragment: usize, terminal: Option<io::ErrorKind>) -> Reply {
        Reply {
            bytes: bytes.to_vec(),
            position: 0,
            fragment,
            terminal,
            terminal_reads: 0,
        }
    }

    async fn tls_reply(frame: &[u8]) -> tokio_rustls::client::TlsStream<tokio::io::DuplexStream> {
        use rustls::{
            RootCertStore, ServerConfig,
            pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
        };
        const CA: &[u8] = include_bytes!("../../../tests/fixtures/tls/ca.cert");
        const CHAIN: &[u8] = include_bytes!("../../../tests/fixtures/tls/end.fullchain");
        const KEY: &[u8] = include_bytes!("../../../tests/fixtures/tls/end.key");
        let mut roots = RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(CA) {
            roots.add(cert.unwrap()).unwrap();
        }
        let client = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let server = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                CertificateDer::pem_slice_iter(CHAIN)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap(),
                PrivateKeyDer::from_pem_slice(KEY).unwrap(),
            )
            .unwrap();
        let (client_io, server_io) = tokio::io::duplex(8192);
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
        let frame = frame.to_vec();
        let server = tokio::spawn(async move {
            let mut stream = acceptor.accept(server_io).await.unwrap();
            stream.write_all(&frame).await.unwrap();
            stream.flush().await.unwrap();
            // Intentionally drop without TLS shutdown/close_notify.
        });
        let stream = TlsConnector::from(Arc::new(client))
            .connect(ServerName::try_from("localhost").unwrap(), client_io)
            .await
            .unwrap();
        server.await.unwrap();
        stream
    }

    #[tokio::test]
    async fn acc01_unix_http_verified_tls_reproduces_eof_and_accepts_only_complete_frames() {
        for (frame, complete) in [
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nmail".as_slice(),
                true,
            ),
            (
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nmail\r\n0\r\n\r\n"
                    .as_slice(),
                true,
            ),
            (
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nmai".as_slice(),
                false,
            ),
            (
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nmail\r\n".as_slice(),
                false,
            ),
        ] {
            let mut stream = tls_reply(frame).await;
            let mut bytes = Vec::new();
            assert_eq!(
                stream.read_to_end(&mut bytes).await.unwrap_err().kind(),
                io::ErrorKind::UnexpectedEof
            );
            assert_eq!(bytes, frame);
            let mut stream = tls_reply(frame).await;
            let result = read_response(&mut stream, &request_fixture()).await;
            if complete {
                assert_eq!(result.unwrap().body.0, b"mail");
            } else {
                assert!(matches!(result, Err(ErrorCode::Offline)));
            }
        }
    }

    #[tokio::test]
    async fn acc01_unix_http_complete_response_does_not_require_tls_close_notify() {
        for frame in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nmail".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nmail\r\n0\r\n\r\n"
                .as_slice(),
        ] {
            for fragment in [1, 7, 8192] {
                let mut stream = reply(frame, fragment, Some(io::ErrorKind::UnexpectedEof));
                let result = read_response(&mut stream, &request_fixture())
                    .await
                    .unwrap();
                assert_eq!(result.status, 200);
                assert_eq!(result.body.0, b"mail");
                assert_eq!(stream.terminal_reads, 0);
            }
        }
    }

    #[tokio::test]
    async fn sec03_unix_http_incomplete_response_never_ignores_eof() {
        for frame in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nmai".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nmail\r\n".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nmail\r\n0\r\n".as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n".as_slice(),
            b"HTTP/1.1 200 OK\r\n\r\n".as_slice(),
        ] {
            for terminal in [
                Some(io::ErrorKind::UnexpectedEof),
                Some(io::ErrorKind::ConnectionReset),
            ] {
                let mut stream = reply(frame, 7, terminal);
                assert!(matches!(
                    read_response(&mut stream, &request_fixture()).await,
                    Err(ErrorCode::Offline)
                ));
            }
        }
    }

    #[tokio::test]
    async fn sec03_unix_http_clean_close_still_rejects_truncated_frames() {
        for frame in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nmai".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nmail\r\n".as_slice(),
        ] {
            let mut stream = reply(frame, 7, None);
            assert!(matches!(
                read_response(&mut stream, &request_fixture()).await,
                Err(ErrorCode::Offline)
            ));
        }
    }

    #[tokio::test]
    async fn acc01_unix_http_preserves_google_error_status_after_complete_body() {
        for (status, expected) in [
            (400, ErrorCode::AuthRequired),
            (401, ErrorCode::AuthRequired),
            (403, ErrorCode::ScopeDenied),
            (429, ErrorCode::RateLimited),
            (500, ErrorCode::Offline),
        ] {
            let frame = format!("HTTP/1.1 {status} Error\r\nContent-Length: 2\r\n\r\n{{}}");
            let mut stream = reply(frame.as_bytes(), 7, Some(io::ErrorKind::UnexpectedEof));
            let result = read_response(&mut stream, &request_fixture())
                .await
                .unwrap();
            assert_eq!(result.status, status);
            assert_eq!(super::super::success(result.status), Err(expected));
            assert_eq!(stream.terminal_reads, 0);
        }
    }

    #[tokio::test]
    async fn sec03_unix_http_rejects_ambiguous_oversized_and_trailing_data() {
        for frame in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\na".as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n1\r\na\r\n0\r\n\r\n".as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\naa".as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 1025\r\n\r\n".as_slice(),
        ] {
            let mut stream = reply(frame, 8192, Some(io::ErrorKind::UnexpectedEof));
            assert!(read_response(&mut stream, &request_fixture()).await.is_err());
        }
    }
}
