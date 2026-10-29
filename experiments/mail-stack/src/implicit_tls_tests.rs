//! In-memory implicit-TLS SMTP proof. This is an experiment only: no TCP,
//! account, credential store, or production authentication path is involved.

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
    client::AsyncSmtpConnection,
};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    time::{sleep, timeout},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::{BoundedAsyncStream, LettreTokioStream};

const CA: &[u8] = include_bytes!("../tests/fixtures/rustls-rsa-2048/ca.cert");
const CHAIN: &[u8] = include_bytes!("../tests/fixtures/rustls-rsa-2048/end.fullchain");
const KEY: &[u8] = include_bytes!("../tests/fixtures/rustls-rsa-2048/end.key");
const FIXED_USER: &str = "fixture-user";
const FIXED_PASSWORD: &str = "fixture-password";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ServerMode {
    Success,
    RejectAuth,
    MissingMechanism,
    StallHandshake,
    StallGreeting,
    OversizedGreeting,
    UnterminatedGreeting,
}

struct Outcome {
    client: Result<(), String>,
    transcript: Vec<Vec<u8>>,
    credential_accesses: usize,
    server: Result<(), String>,
}

fn server_config() -> Arc<ServerConfig> {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(CHAIN)
        .collect::<Result<_, _>>()
        .expect("fixture certificate");
    let key = PrivateKeyDer::from_pem_slice(KEY).expect("fixture key");
    Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .expect("fixture server config"),
    )
}

fn client_config(trusted: bool) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    if trusted {
        let ca = CertificateDer::pem_slice_iter(CA)
            .collect::<Result<Vec<_>, _>>()
            .expect("fixture CA");
        for cert in ca {
            roots.add(cert).expect("fixture CA root");
        }
    }
    Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

async fn server(
    stream: tokio::io::DuplexStream,
    mode: ServerMode,
    transcript: Arc<Mutex<Vec<Vec<u8>>>>,
) -> io::Result<()> {
    if mode == ServerMode::StallHandshake {
        sleep(Duration::from_secs(5)).await;
        return Ok(());
    }
    let mut tls = TlsAcceptor::from(server_config())
        .accept(stream)
        .await
        .map_err(io::Error::other)?;
    if mode == ServerMode::StallGreeting {
        sleep(Duration::from_secs(5)).await;
        return Ok(());
    }
    if mode == ServerMode::OversizedGreeting {
        tls.write_all(&vec![b'X'; 256 * 1024 + 1]).await?;
        return Ok(());
    }
    if mode == ServerMode::UnterminatedGreeting {
        tls.write_all(b"220 implicit tls fixture").await?;
        return Ok(());
    }

    tls.write_all(b"220 mail-hantong implicit tls\r\n").await?;
    let mut reader = BufReader::new(tls);
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line).await?;
    transcript
        .lock()
        .map_err(|_| io::Error::other("transcript poisoned"))?
        .push(line);
    let capabilities: &[u8] = if mode == ServerMode::MissingMechanism {
        b"250-test\r\n250 SIZE 1024\r\n"
    } else {
        b"250-test\r\n250 AUTH PLAIN LOGIN\r\n"
    };
    reader.get_mut().write_all(capabilities).await?;

    line = Vec::new();
    reader.read_until(b'\n', &mut line).await?;
    transcript
        .lock()
        .map_err(|_| io::Error::other("transcript poisoned"))?
        .push(line.clone());
    if !line.starts_with(b"AUTH PLAIN") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected AUTH PLAIN",
        ));
    }
    if mode == ServerMode::RejectAuth {
        reader
            .get_mut()
            .write_all(b"535 5.7.8 rejected\r\n")
            .await?;
        return Ok(());
    }
    reader
        .get_mut()
        .write_all(b"235 2.7.0 authenticated\r\n")
        .await?;
    line = Vec::new();
    reader.read_until(b'\n', &mut line).await?;
    transcript
        .lock()
        .map_err(|_| io::Error::other("transcript poisoned"))?
        .push(line);
    reader.get_mut().write_all(b"221 2.0.0 bye\r\n").await
}

async fn run(mode: ServerMode, hostname: &str, trusted: bool) -> Outcome {
    let (client, server_stream) = tokio::io::duplex(8192);
    let transcript = Arc::new(Mutex::new(Vec::new()));
    let credential_accesses = Arc::new(AtomicUsize::new(0));
    let server_task = tokio::spawn(server(server_stream, mode, Arc::clone(&transcript)));

    let deadline = if matches!(mode, ServerMode::StallHandshake | ServerMode::StallGreeting) {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(3)
    };
    let client_result = timeout(deadline, async {
        let connector = TlsConnector::from(client_config(trusted));
        let server_name = ServerName::try_from(hostname.to_owned())
            .map_err(|_| "invalid server name".to_owned())?;
        let bounded = BoundedAsyncStream::new(client);
        let tls = connector
            .connect(server_name, bounded)
            .await
            .map_err(|error| format!("TLS: {error}"))?;
        let stream = LettreTokioStream::new(tls, "127.0.0.1:2465".parse().expect("peer"));
        let mut connection = AsyncSmtpConnection::connect_with_transport(
            Box::new(stream),
            &lettre::transport::smtp::extension::ClientId::Domain("client.test".into()),
        )
        .await
        .map_err(|error| format!("SMTP: {error}"))?;
        if connection.is_encrypted() {
            return Err("lettre transport unexpectedly reported encryption".to_owned());
        }
        if !connection
            .server_info()
            .supports_auth_mechanism(Mechanism::Plain)
        {
            return Err("mechanism unsupported".to_owned());
        }
        credential_accesses.fetch_add(1, Ordering::SeqCst);
        connection
            .auth(
                &[Mechanism::Plain],
                &Credentials::new(FIXED_USER.to_owned(), FIXED_PASSWORD.to_owned()),
            )
            .await
            .map_err(|error| format!("AUTH: {error}"))?;
        connection
            .quit()
            .await
            .map(|_| ())
            .map_err(|error| format!("QUIT: {error}"))
    })
    .await
    .map_err(|_| "deadline".to_owned())
    .and_then(|value| value);

    let mut server_task = server_task;
    let server_result = match timeout(Duration::from_secs(1), &mut server_task).await {
        Ok(joined) => joined.map_err(|error| format!("server join: {error}")),
        Err(_) => {
            server_task.abort();
            assert!(matches!(server_task.await, Err(error) if error.is_cancelled()));
            Err("server join deadline".to_owned())
        }
    };
    let transcript = transcript
        .lock()
        .expect("transcript must not be poisoned")
        .clone();
    Outcome {
        client: client_result,
        transcript,
        credential_accesses: credential_accesses.load(Ordering::SeqCst),
        server: server_result.and_then(|value| value.map_err(|error| error.to_string())),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn implicit_tls_verifies_before_single_auth_and_quit() {
    let outcome = run(ServerMode::Success, "localhost", true).await;
    assert!(outcome.client.is_ok(), "client: {:?}", outcome.client);
    assert!(outcome.server.is_ok(), "server: {:?}", outcome.server);
    assert_eq!(outcome.credential_accesses, 1);
    assert_eq!(outcome.transcript.len(), 3);
    assert_eq!(outcome.transcript[0], b"EHLO client.test\r\n");
    assert_eq!(
        outcome.transcript[1],
        b"AUTH PLAIN AGZpeHR1cmUtdXNlcgBmaXh0dXJlLXBhc3N3b3Jk\r\n"
    );
    assert_eq!(outcome.transcript[2], b"QUIT\r\n");
}

#[tokio::test(flavor = "current_thread")]
async fn implicit_tls_bad_hostname_and_untrusted_root_never_access_credentials() {
    for (hostname, trusted) in [("wrong.invalid", true), ("localhost", false)] {
        let outcome = run(ServerMode::Success, hostname, trusted).await;
        assert!(outcome.client.as_ref().unwrap_err().starts_with("TLS:"));
        assert_eq!(outcome.credential_accesses, 0);
        assert!(
            outcome
                .transcript
                .iter()
                .all(|line| !line.starts_with(b"AUTH "))
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn implicit_tls_stall_and_oversized_or_unterminated_greeting_are_bounded() {
    for mode in [
        ServerMode::StallHandshake,
        ServerMode::StallGreeting,
        ServerMode::OversizedGreeting,
        ServerMode::UnterminatedGreeting,
    ] {
        let outcome = run(mode, "localhost", true).await;
        assert!(outcome.client.is_err());
        if matches!(mode, ServerMode::StallHandshake | ServerMode::StallGreeting) {
            assert_eq!(outcome.client, Err("deadline".to_owned()));
        } else {
            assert!(outcome.client.as_ref().unwrap_err().starts_with("SMTP:"));
        }
        assert_eq!(outcome.credential_accesses, 0);
        assert!(
            outcome
                .transcript
                .iter()
                .all(|line| !line.starts_with(b"AUTH "))
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn implicit_tls_auth_rejection_has_one_auth_and_no_retry_or_mail_commands() {
    let outcome = run(ServerMode::RejectAuth, "localhost", true).await;
    assert!(outcome.client.as_ref().unwrap_err().starts_with("AUTH:"));
    assert_eq!(outcome.credential_accesses, 1);
    assert_eq!(
        outcome
            .transcript
            .iter()
            .filter(|line| line.starts_with(b"AUTH "))
            .count(),
        1
    );
    assert!(outcome.transcript.iter().all(|line| {
        !line.starts_with(b"MAIL ") && !line.starts_with(b"RCPT ") && !line.starts_with(b"DATA")
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn implicit_tls_missing_mechanism_never_accesses_credentials() {
    let outcome = run(ServerMode::MissingMechanism, "localhost", true).await;
    assert_eq!(outcome.client, Err("mechanism unsupported".to_owned()));
    assert_eq!(outcome.credential_accesses, 0);
    assert!(
        outcome
            .transcript
            .iter()
            .all(|line| !line.starts_with(b"AUTH "))
    );
}
