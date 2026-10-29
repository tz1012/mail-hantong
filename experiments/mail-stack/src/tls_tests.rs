//! Deterministic STARTTLS-only probe experiment.
//!
//! These tests use a Tokio in-memory duplex stream and Rustls' published test
//! CA.  They do not exercise the implicit-TLS TCP wrapper (Lettre's injected
//! stream seam is intentionally classified as plaintext until STARTTLS).

use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use lettre::transport::smtp::{
    authentication::{Credentials, Mechanism},
    client::{AsyncSmtpConnection, Certificate, CertificateStore, TlsParameters},
    extension::ClientId,
};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    time::timeout,
};
use tokio_rustls::TlsAcceptor;

use crate::LettreTokioStream;

const CA: &[u8] = include_bytes!("../tests/fixtures/rustls-rsa-2048/ca.cert");
const CHAIN: &[u8] = include_bytes!("../tests/fixtures/rustls-rsa-2048/end.fullchain");
const KEY: &[u8] = include_bytes!("../tests/fixtures/rustls-rsa-2048/end.key");

fn tls_parameters(domain: &str, trusted: bool) -> TlsParameters {
    let mut builder =
        TlsParameters::builder(domain.to_owned()).certificate_store(CertificateStore::None);
    if trusted {
        builder = builder.add_root_certificate(Certificate::from_pem(CA).unwrap());
    }
    builder.build_rustls().unwrap()
}

fn server_config() -> Arc<ServerConfig> {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(CHAIN)
        .collect::<Result<_, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(KEY).unwrap();
    Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap(),
    )
}

async fn line<R: tokio::io::AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.read_until(b'\n', &mut out).await?;
    Ok(out)
}

async fn finish_server<T>(mut task: tokio::task::JoinHandle<T>) {
    match timeout(Duration::from_secs(1), &mut task).await {
        Ok(result) => {
            let _ = result.expect("fake server must not panic");
        }
        Err(_) => {
            task.abort();
            assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        }
    }
}

async fn fake_starttls(
    mut server: tokio::io::DuplexStream,
    advertise: bool,
    transcript: Arc<Mutex<Vec<Vec<u8>>>>,
) -> io::Result<bool> {
    server.write_all(b"220 mail-hantong test\r\n").await?;
    let mut reader = BufReader::new(server);
    let ehlo = line(&mut reader).await?;
    transcript.lock().unwrap().push(ehlo);
    if advertise {
        reader
            .get_mut()
            .write_all(b"250-test\r\n250-STARTTLS\r\n250 AUTH PLAIN\r\n")
            .await?;
    } else {
        reader
            .get_mut()
            .write_all(b"250-test\r\n250 AUTH PLAIN\r\n")
            .await?;
    }
    let command = line(&mut reader).await?;
    transcript.lock().unwrap().push(command.clone());
    if !command.eq_ignore_ascii_case(b"STARTTLS\r\n") {
        return Ok(false);
    }
    reader
        .get_mut()
        .write_all(b"220 2.0.0 Ready to start TLS\r\n")
        .await?;
    let stream = reader.into_inner();
    let mut tls = TlsAcceptor::from(server_config())
        .accept(stream)
        .await
        .map_err(io::Error::other)?;
    let mut tls_reader = BufReader::new(&mut tls);
    let ehlo = line(&mut tls_reader).await?;
    transcript.lock().unwrap().push(ehlo);
    tls_reader
        .get_mut()
        .write_all(b"250-test\r\n250 AUTH PLAIN\r\n")
        .await?;
    let auth = line(&mut tls_reader).await?;
    transcript.lock().unwrap().push(auth.clone());
    let authenticated = auth.starts_with(b"AUTH PLAIN");
    if authenticated {
        tls_reader
            .get_mut()
            .write_all(b"235 2.7.0 Authentication successful\r\n")
            .await?;
    }
    let quit = line(&mut tls_reader).await?;
    transcript.lock().unwrap().push(quit);
    tls_reader.get_mut().write_all(b"221 2.0.0 Bye\r\n").await?;
    Ok(authenticated)
}

async fn fake_starttls_report(
    mut server: tokio::io::DuplexStream,
    mode: ServerMode,
    transcript: Arc<Mutex<Vec<Vec<u8>>>>,
) -> io::Result<()> {
    if mode == ServerMode::Stall {
        tokio::time::sleep(Duration::from_secs(5)).await;
        return Ok(());
    }
    if mode == ServerMode::Oversized {
        server.write_all(&vec![b'X'; 256 * 1024 + 1]).await?;
        return Ok(());
    }
    server.write_all(b"220 mail-hantong test\r\n").await?;
    let mut reader = BufReader::new(server);
    let ehlo = line(&mut reader).await?;
    transcript.lock().unwrap().push(ehlo);
    reader
        .get_mut()
        .write_all(b"250-test\r\n250-STARTTLS\r\n250 AUTH PLAIN\r\n")
        .await?;
    let command = line(&mut reader).await?;
    transcript.lock().unwrap().push(command.clone());
    if !command.eq_ignore_ascii_case(b"STARTTLS\r\n") {
        return Ok(());
    }
    reader
        .get_mut()
        .write_all(b"220 2.0.0 Ready to start TLS\r\n")
        .await?;
    if mode == ServerMode::Malformed {
        reader
            .get_mut()
            .write_all(b"not a TLS handshake\r\n")
            .await?;
        return Ok(());
    }
    let stream = reader.into_inner();
    let mut tls = TlsAcceptor::from(server_config())
        .accept(stream)
        .await
        .map_err(io::Error::other)?;
    let mut tls_reader = BufReader::new(&mut tls);
    let ehlo = line(&mut tls_reader).await?;
    transcript.lock().unwrap().push(ehlo);
    tls_reader
        .get_mut()
        .write_all(b"250-test\r\n250 AUTH PLAIN\r\n")
        .await?;
    let auth = line(&mut tls_reader).await?;
    transcript.lock().unwrap().push(auth.clone());
    if auth.starts_with(b"AUTH PLAIN") {
        tls_reader
            .get_mut()
            .write_all(b"235 2.7.0 Authentication successful\r\n")
            .await?;
    }
    let quit = line(&mut tls_reader).await?;
    transcript.lock().unwrap().push(quit);
    tls_reader.get_mut().write_all(b"221 2.0.0 Bye\r\n").await?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerMode {
    Stall,
    Oversized,
    Malformed,
}

async fn connect(
    advertise: bool,
    trusted: bool,
    domain: &str,
) -> (Result<bool, String>, Vec<Vec<u8>>, usize) {
    let (client, server) = tokio::io::duplex(8192);
    let transcript = Arc::new(Mutex::new(Vec::new()));
    let server_task = tokio::spawn(fake_starttls(server, advertise, Arc::clone(&transcript)));
    let credential_accesses = Arc::new(AtomicUsize::new(0));
    let stream = LettreTokioStream::new(client, "127.0.0.1:2525".parse().unwrap());
    let result = timeout(Duration::from_secs(3), async {
        let mut conn = AsyncSmtpConnection::connect_with_transport(
            Box::new(stream),
            &ClientId::Domain("client.test".into()),
        )
        .await
        .map_err(|e| e.to_string())?;
        let before = conn.is_encrypted();
        if !conn.can_starttls() {
            return Ok(false);
        }
        conn.starttls(
            tls_parameters(domain, trusted),
            &ClientId::Domain("client.test".into()),
        )
        .await
        .map_err(|e| e.to_string())?;
        if !conn.is_encrypted() {
            return Ok(false);
        }
        credential_accesses.fetch_add(1, Ordering::SeqCst);
        let _ = conn
            .auth(
                &[Mechanism::Plain],
                &Credentials::new("user".into(), "secret".into()),
            )
            .await
            .map_err(|e| e.to_string())?;
        conn.quit().await.map_err(|e| e.to_string())?;
        Ok(!before)
    })
    .await
    .map_err(|_| "deadline".to_owned())
    .and_then(|v| v);
    finish_server(server_task).await;
    let transcript = transcript.lock().unwrap().clone();
    (
        result,
        transcript,
        credential_accesses.load(Ordering::SeqCst),
    )
}

async fn failure_case(mode: ServerMode) -> (Result<bool, String>, Vec<Vec<u8>>, usize) {
    let (client, server) = tokio::io::duplex(8192);
    let transcript = Arc::new(Mutex::new(Vec::new()));
    let server_task = tokio::spawn(fake_starttls_report(server, mode, Arc::clone(&transcript)));
    let credential_accesses = Arc::new(AtomicUsize::new(0));
    let stream = LettreTokioStream::new(client, "127.0.0.1:2525".parse().unwrap());
    let result = timeout(Duration::from_secs(3), async {
        let mut conn = AsyncSmtpConnection::connect_with_transport(
            Box::new(stream),
            &ClientId::Domain("client.test".into()),
        )
        .await
        .map_err(|e| e.to_string())?;
        if !conn.can_starttls() {
            return Ok(false);
        }
        conn.starttls(
            tls_parameters("localhost", true),
            &ClientId::Domain("client.test".into()),
        )
        .await
        .map_err(|e| e.to_string())?;
        credential_accesses.fetch_add(1, Ordering::SeqCst);
        let _ = conn
            .auth(
                &[Mechanism::Plain],
                &Credentials::new("user".into(), "secret".into()),
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
    .map_err(|_| "deadline".to_owned())
    .and_then(|v| v);
    finish_server(server_task).await;
    (
        result,
        transcript.lock().unwrap().clone(),
        credential_accesses.load(Ordering::SeqCst),
    )
}

#[tokio::test(flavor = "current_thread")]
async fn starttls_verifies_before_authentication() {
    let (result, transcript, accesses) = connect(true, true, "localhost").await;
    assert!(result.unwrap());
    assert_eq!(accesses, 1);
    assert_eq!(transcript.len(), 5);
    assert_eq!(transcript[0], b"EHLO client.test\r\n");
    assert_eq!(transcript[1], b"STARTTLS\r\n");
    assert_eq!(transcript[2], b"EHLO client.test\r\n");
    assert_eq!(transcript[3], b"AUTH PLAIN AHVzZXIAc2VjcmV0\r\n");
    assert_eq!(transcript[4], b"QUIT\r\n");
}

#[tokio::test(flavor = "current_thread")]
async fn missing_starttls_never_authenticates() {
    let (result, transcript, accesses) = connect(false, true, "localhost").await;
    assert!(result.is_ok() && !result.unwrap());
    assert_eq!(accesses, 0);
    assert!(
        transcript
            .iter()
            .all(|command| !command.starts_with(b"AUTH "))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hostname_mismatch_and_untrusted_root_fail_before_authentication() {
    let (bad_host, host_transcript, host_accesses) = connect(true, true, "wrong.invalid").await;
    assert!(bad_host.is_err());
    assert_ne!(bad_host, Err("deadline".to_owned()));
    assert_eq!(host_accesses, 0);
    assert!(
        host_transcript
            .iter()
            .all(|command| !command.starts_with(b"AUTH "))
    );
    let (bad_root, root_transcript, root_accesses) = connect(true, false, "localhost").await;
    assert!(bad_root.is_err());
    assert_ne!(bad_root, Err("deadline".to_owned()));
    assert_eq!(root_accesses, 0);
    assert!(
        root_transcript
            .iter()
            .all(|command| !command.starts_with(b"AUTH "))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_handshake_never_authenticates() {
    let (result, transcript, accesses) = failure_case(ServerMode::Malformed).await;
    assert!(result.is_err());
    assert_eq!(accesses, 0);
    assert!(
        transcript
            .iter()
            .all(|command| !command.starts_with(b"AUTH "))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_greeting_is_bounded_and_never_authenticates() {
    let (result, transcript, accesses) = failure_case(ServerMode::Oversized).await;
    assert!(result.is_err());
    assert_eq!(accesses, 0);
    assert!(
        transcript
            .iter()
            .all(|command| !command.starts_with(b"AUTH "))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn stalled_server_hits_outer_deadline_and_never_authenticates() {
    let (result, transcript, accesses) = failure_case(ServerMode::Stall).await;
    assert_eq!(result, Err("deadline".to_owned()));
    assert_eq!(accesses, 0);
    assert!(
        transcript
            .iter()
            .all(|command| !command.starts_with(b"AUTH "))
    );
}
