//! Bounded Unix Google HTTP transport.
//!
//! The public transport trait is synchronous, so Unix I/O is serialized through
//! one dedicated worker.  A caller that times out abandons its reply receiver;
//! the worker nevertheless finishes the in-flight operation before admitting a
//! second request.  This prevents DNS or TLS work from becoming an unbounded
//! thread/queue resource.

use super::{Host, Request, Response, unix::parse_response};
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
        .map_err(|_| ErrorCode::Offline)?
        .with_no_client_auth();
    // DNS is intentionally awaited to completion.  Dropping a lookup future
    // does not prove the underlying resolver has stopped; the worker remains
    // occupied until this completes, while the caller may already time out.
    let addresses = lookup_host((host, 443))
        .await
        .map_err(|_| ErrorCode::Offline)?;
    let mut stream = None;
    for address in addresses.take(8) {
        check(&request)?;
        if let Ok(Ok(connected)) = timeout(remaining(&request)?, TcpStream::connect(address)).await
        {
            stream = Some(connected);
            break;
        }
    }
    let stream = stream.ok_or(ErrorCode::Offline)?;
    check(&request)?;
    let server_name = ServerName::try_from(host.to_owned()).map_err(|_| ErrorCode::Offline)?;
    let connector = TlsConnector::from(Arc::new(config));
    let mut stream = timeout(remaining(&request)?, connector.connect(server_name, stream))
        .await
        .map_err(|_| ErrorCode::Offline)?
        .map_err(|_| ErrorCode::Offline)?;
    check(&request)?;
    let wire = request_bytes(&request)?;
    timeout(remaining(&request)?, stream.write_all(&wire.0))
        .await
        .map_err(|_| ErrorCode::Offline)?
        .map_err(|_| ErrorCode::Offline)?;
    let cap = (16usize * 1024)
        .checked_add(request.limit)
        .and_then(|n| n.checked_add(MAX_RESPONSE_OVERHEAD))
        .ok_or(ErrorCode::QuotaExceeded)?;
    let mut raw = SensitiveBuffer(Vec::with_capacity(cap.min(16 * 1024)));
    let mut chunk = SensitiveBuffer(vec![0; 8192]);
    loop {
        check(&request)?;
        let read = timeout(remaining(&request)?, stream.read(&mut chunk.0))
            .await
            .map_err(|_| ErrorCode::Offline)?
            .map_err(|_| ErrorCode::Offline)?;
        if read == 0 {
            break;
        }
        if raw.0.len() > cap.saturating_sub(read) {
            return Err(ErrorCode::QuotaExceeded);
        }
        raw.0.extend_from_slice(&chunk.0[..read]);
    }
    let (status, body) = parse_response(&raw.0, request.limit)?;
    Ok(Response { status, body })
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
