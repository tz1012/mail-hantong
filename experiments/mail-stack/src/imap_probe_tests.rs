//! In-memory, codec-only probe. No io-imap high-level session/send coroutine.
use crate::{
    BoundedAsyncStream,
    imap_gate::{GateMode, ReplyKind, StrictResponseGate},
};
use io_imap::{
    codec::{AuthenticateDataCodec, CommandCodec, encode::Encoder},
    types::{
        auth::{AuthMechanism, AuthenticateData},
        command::{Command, CommandBody},
        response::StatusKind,
    },
};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject},
};
use std::{
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{
        AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
        DuplexStream,
    },
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

const CA: &[u8] = include_bytes!("../tests/fixtures/rustls-rsa-2048/ca.cert");
const CHAIN: &[u8] = include_bytes!("../tests/fixtures/rustls-rsa-2048/end.fullchain");
const KEY: &[u8] = include_bytes!("../tests/fixtures/rustls-rsa-2048/end.key");
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    Protocol,
    Tls,
    Authentication,
    Unsupported,
    Timeout,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Good,
    Login,
    Oauth,
    InitialResponse,
    Preauth,
    WrongCapTag,
    WrongAuthTag,
    NoStarttls,
    RejectTls,
    InjectTls,
    RejectAuth,
    Literal,
    MissingBye,
    Flood,
    Stall,
}
trait Wire: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Wire for T {}
type Stream = Box<dyn Wire>;
type Transcript = Arc<Mutex<Vec<Vec<u8>>>>;

fn configs(trusted: bool) -> (Arc<ClientConfig>, TlsAcceptor) {
    let mut roots = RootCertStore::empty();
    if trusted {
        for c in CertificateDer::pem_slice_iter(CA) {
            roots.add(c.unwrap()).unwrap();
        }
    }
    let client = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let certs = CertificateDer::pem_slice_iter(CHAIN)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let server = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, PrivateKeyDer::from_pem_slice(KEY).unwrap())
        .unwrap();
    (Arc::new(client), TlsAcceptor::from(Arc::new(server)))
}
async fn frame<S: AsyncRead + Unpin>(
    io: &mut S,
    mode: GateMode,
    tag: Option<&str>,
) -> Result<ReplyKind, Failure> {
    let mut gate = StrictResponseGate::new(mode, tag).map_err(|_| Failure::Protocol)?;
    loop {
        let mut byte = [0];
        if io.read(&mut byte).await.map_err(|_| Failure::Protocol)? == 0 {
            return Err(Failure::Protocol);
        }
        let result = gate.feed(&byte).map_err(|_| Failure::Protocol)?;
        assert_eq!(result.consumed, 1);
        if let Some(frame) = result.frame {
            return Ok(frame.into_kind());
        }
    }
}
async fn write_command<S: AsyncWrite + Unpin>(
    io: &mut S,
    tag: &str,
    body: CommandBody<'_>,
) -> Result<(), Failure> {
    let command = Command {
        tag: tag.try_into().map_err(|_| Failure::Protocol)?,
        body,
    };
    let bytes = CommandCodec::new().encode(&command).dump();
    if bytes.len() > 32768 {
        return Err(Failure::Protocol);
    }
    io.write_all(&bytes).await.map_err(|_| Failure::Protocol)?;
    io.flush().await.map_err(|_| Failure::Protocol)
}
async fn finish<S: AsyncRead + Unpin>(
    io: &mut S,
    mode: GateMode,
    tag: &str,
) -> Result<Vec<String>, Failure> {
    let mut caps = None;
    let mut bye = false;
    for _ in 0..32 {
        match frame(io, mode, Some(tag)).await? {
            ReplyKind::Capability(values) if caps.is_none() => caps = Some(values),
            ReplyKind::Bye if mode == GateMode::Logout && !bye => bye = true,
            ReplyKind::UntaggedStatus(_) => {}
            ReplyKind::Tagged(StatusKind::Ok) => {
                if mode == GateMode::Capability && caps.is_none()
                    || mode == GateMode::Logout && !bye
                {
                    return Err(Failure::Protocol);
                }
                return Ok(caps.unwrap_or_default());
            }
            ReplyKind::Tagged(_) => {
                return Err(if mode == GateMode::StartTls {
                    Failure::Tls
                } else {
                    Failure::Authentication
                });
            }
            _ => return Err(Failure::Protocol),
        }
    }
    Err(Failure::Protocol)
}
async fn capabilities<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    tag: &str,
) -> Result<Vec<String>, Failure> {
    write_command(io, tag, CommandBody::Capability).await?;
    finish(io, GateMode::Capability, tag).await
}
async fn client(
    stream: DuplexStream,
    implicit: bool,
    mode: Mode,
    host: &str,
    trusted: bool,
    accesses: Arc<AtomicUsize>,
) -> Result<Vec<String>, Failure> {
    // Prefetch is BELOW the TLS boundary, including on the STARTTLS branch.
    let mut wire: Stream = Box::new(BufReader::new(BoundedAsyncStream::new(stream)));
    if !implicit {
        if !matches!(
            frame(&mut wire, GateMode::Greeting, None).await?,
            ReplyKind::GreetingOk
        ) {
            return Err(Failure::Protocol);
        }
        let caps = capabilities(&mut wire, "h1").await?;
        if !caps.iter().any(|s| s == "STARTTLS") {
            return Err(Failure::Tls);
        }
        write_command(&mut wire, "h2", CommandBody::StartTLS).await?;
        finish(&mut wire, GateMode::StartTls, "h2").await?;
    }
    let (config, _) = configs(trusted);
    let tls = TlsConnector::from(config)
        .connect(
            ServerName::try_from(host.to_owned()).map_err(|_| Failure::Tls)?,
            wire,
        )
        .await
        .map_err(|_| Failure::Tls)?;
    let mut io = BufReader::new(BoundedAsyncStream::new(tls));
    if implicit
        && !matches!(
            frame(&mut io, GateMode::Greeting, None).await?,
            ReplyKind::GreetingOk
        )
    {
        return Err(Failure::Protocol);
    }
    let caps = capabilities(&mut io, "h3").await?;
    let mechanism = if mode == Mode::Oauth {
        AuthMechanism::XOAuth2
    } else if caps.iter().any(|s| s == "AUTH=PLAIN") {
        AuthMechanism::Plain
    } else {
        AuthMechanism::Login
    };
    if !caps.iter().any(|s| s == &format!("AUTH={mechanism}")) {
        return Err(Failure::Unsupported);
    }
    accesses.fetch_add(1, Ordering::SeqCst); // prototype credential read; product uses DPAPI here
    let initial = mechanism != AuthMechanism::Login && caps.iter().any(|s| s == "SASL-IR");
    let payload = if mechanism == AuthMechanism::XOAuth2 {
        b"user=user\x01auth=Bearer token\x01\x01".to_vec()
    } else {
        b"\0user\0secret".to_vec()
    };
    let initial_response = initial.then(|| {
        io_imap::types::secret::Secret::new(std::borrow::Cow::Borrowed(payload.as_slice()))
    });
    write_command(
        &mut io,
        "h4",
        CommandBody::Authenticate {
            mechanism: mechanism.clone(),
            initial_response,
        },
    )
    .await?;
    let responses: Vec<&[u8]> = if initial {
        vec![]
    } else if mechanism == AuthMechanism::Login {
        vec![b"user", b"secret"]
    } else {
        vec![&payload]
    };
    for data in responses {
        if !matches!(
            frame(&mut io, GateMode::Authentication, Some("h4")).await?,
            ReplyKind::Continuation
        ) {
            return Err(Failure::Authentication);
        }
        let encoded = AuthenticateDataCodec::new()
            .encode(&AuthenticateData::r#continue(data))
            .dump();
        io.write_all(&encoded)
            .await
            .map_err(|_| Failure::Protocol)?;
        io.flush().await.map_err(|_| Failure::Protocol)?;
    }
    finish(&mut io, GateMode::Authentication, "h4").await?;
    let final_caps = capabilities(&mut io, "h5").await?;
    write_command(&mut io, "h6", CommandBody::Logout).await?;
    finish(&mut io, GateMode::Logout, "h6").await?;
    Ok(final_caps)
}
async fn read_command<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut BufReader<S>,
    log: &Transcript,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    io.read_until(b'\n', &mut bytes).await?;
    if !bytes.is_empty() {
        log.lock().unwrap().push(bytes.clone());
    }
    Ok(bytes)
}
async fn cap_reply<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut BufReader<S>,
    tag: &str,
    mode: Mode,
) -> io::Result<()> {
    if mode == Mode::Literal {
        return io
            .get_mut()
            .write_all(b"* CAPABILITY {4294967295}\r\n")
            .await;
    }
    let auth = match mode {
        Mode::Login => "AUTH=LOGIN",
        Mode::Oauth => "AUTH=XOAUTH2",
        _ => "AUTH=PLAIN",
    };
    let initial = if mode == Mode::InitialResponse {
        " SASL-IR"
    } else {
        ""
    };
    let starttls = if mode == Mode::NoStarttls {
        ""
    } else {
        " STARTTLS"
    };
    if mode == Mode::Flood {
        for _ in 0..40 {
            io.get_mut().write_all(b"* OK harmless status\r\n").await?;
        }
    }
    let tag = if mode == Mode::WrongCapTag {
        "wrong"
    } else {
        tag
    };
    io.get_mut().write_all(format!("* CAPABILITY IMAP4rev1 {auth}{initial}{starttls} IDLE UIDPLUS\r\n{tag} OK capabilities\r\n").as_bytes()).await
}
async fn server(
    mut stream: DuplexStream,
    implicit: bool,
    mode: Mode,
    log: Transcript,
) -> io::Result<()> {
    if mode == Mode::Stall {
        tokio::time::sleep(Duration::from_secs(20)).await;
        return Ok(());
    }
    let mut wire = BufReader::new(stream);
    if !implicit {
        wire.get_mut().write_all(b"* OK fixture\r\n").await?;
        assert_eq!(read_command(&mut wire, &log).await?, b"h1 CAPABILITY\r\n");
        cap_reply(&mut wire, "h1", mode).await?;
        let command = read_command(&mut wire, &log).await?;
        if command.is_empty() {
            return Ok(());
        }
        assert_eq!(command, b"h2 STARTTLS\r\n");
        if mode == Mode::RejectTls {
            wire.get_mut().write_all(b"h2 NO TLS disabled\r\n").await?;
            return Ok(());
        }
        let reply: &[u8] = if mode == Mode::InjectTls {
            b"h2 OK ready\r\n* CAPABILITY IMAP4rev1 AUTH=PLAIN\r\n"
        } else {
            b"h2 OK ready\r\n"
        };
        wire.get_mut().write_all(reply).await?;
    }
    assert!(wire.buffer().is_empty());
    stream = wire.into_inner();
    let (_, acceptor) = configs(true);
    let tls = acceptor.accept(stream).await?;
    let mut wire = BufReader::new(tls);
    if implicit {
        wire.get_mut()
            .write_all(if mode == Mode::Preauth {
                b"* PREAUTH fixture\r\n"
            } else {
                b"* OK fixture\r\n"
            })
            .await?;
    }
    let command = read_command(&mut wire, &log).await?;
    if command.is_empty() {
        return Ok(());
    }
    assert_eq!(command, b"h3 CAPABILITY\r\n");
    cap_reply(&mut wire, "h3", mode).await?;
    let auth = read_command(&mut wire, &log).await?;
    if auth.is_empty() {
        return Ok(());
    }
    match mode {
        Mode::Login => {
            assert_eq!(auth, b"h4 AUTHENTICATE LOGIN\r\n");
            wire.get_mut().write_all(b"+ VXNlcm5hbWU6\r\n").await?;
            assert_eq!(read_command(&mut wire, &log).await?, b"dXNlcg==\r\n");
            wire.get_mut().write_all(b"+ UGFzc3dvcmQ6\r\n").await?;
            assert_eq!(read_command(&mut wire, &log).await?, b"c2VjcmV0\r\n");
        }
        Mode::InitialResponse => assert_eq!(auth, b"h4 AUTHENTICATE PLAIN AHVzZXIAc2VjcmV0\r\n"),
        _ => {
            let expected: &[u8] = if mode == Mode::Oauth {
                b"h4 AUTHENTICATE XOAUTH2\r\n"
            } else {
                b"h4 AUTHENTICATE PLAIN\r\n"
            };
            assert_eq!(auth, expected);
            wire.get_mut().write_all(b"+ \r\n").await?;
            let expected: &[u8] = if mode == Mode::Oauth {
                b"dXNlcj11c2VyAWF1dGg9QmVhcmVyIHRva2VuAQE=\r\n"
            } else {
                b"AHVzZXIAc2VjcmV0\r\n"
            };
            assert_eq!(read_command(&mut wire, &log).await?, expected);
        }
    }
    if mode == Mode::RejectAuth {
        wire.get_mut()
            .write_all(b"h4 NO private failure\r\n")
            .await?;
        return Ok(());
    }
    wire.get_mut()
        .write_all(if mode == Mode::WrongAuthTag {
            b"wrong OK authenticated\r\n"
        } else {
            b"h4 OK authenticated\r\n"
        })
        .await?;
    let cmd = read_command(&mut wire, &log).await?;
    if cmd.is_empty() {
        return Ok(());
    }
    assert_eq!(cmd, b"h5 CAPABILITY\r\n");
    cap_reply(&mut wire, "h5", mode).await?;
    assert_eq!(read_command(&mut wire, &log).await?, b"h6 LOGOUT\r\n");
    wire.get_mut()
        .write_all(if mode == Mode::MissingBye {
            b"h6 OK bye\r\n"
        } else {
            b"* BYE fixture\r\nh6 OK bye\r\n"
        })
        .await
}
async fn run(
    implicit: bool,
    mode: Mode,
    host: &str,
    trusted: bool,
) -> (Result<Vec<String>, Failure>, usize, Vec<Vec<u8>>) {
    let (left, right) = tokio::io::duplex(8192);
    let log = Arc::new(Mutex::new(Vec::new()));
    let accesses = Arc::new(AtomicUsize::new(0));
    let mut task = tokio::spawn(server(right, implicit, mode, log.clone()));
    let duration = if mode == Mode::Stall {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(3)
    };
    let result = timeout(
        duration,
        client(left, implicit, mode, host, trusted, accesses.clone()),
    )
    .await
    .unwrap_or(Err(Failure::Timeout));
    match timeout(Duration::from_millis(300), &mut task).await {
        Ok(joined) => {
            let _ = joined.expect("server must not panic");
        }
        Err(_) => {
            task.abort();
            assert!(matches!(task.await,Err(e) if e.is_cancelled()));
        }
    }
    let transcript = log.lock().unwrap().clone();
    assert!(
        transcript
            .iter()
            .all(|line| !line.windows(6).any(|part| part == b"APPEND")
                && !line.windows(7).any(|part| part == b"EXPUNGE"))
    );
    (result, accesses.load(Ordering::SeqCst), transcript)
}
#[tokio::test]
async fn strict_imap_tls_modes_and_auth_mechanisms_roundtrip() {
    for implicit in [true, false] {
        for mode in [Mode::Good, Mode::Login, Mode::Oauth, Mode::InitialResponse] {
            let (result, accesses, log) = run(implicit, mode, "localhost", true).await;
            let caps = result.unwrap();
            assert!(caps.iter().any(|s| s == "IDLE"));
            assert_eq!(accesses, 1);
            assert_eq!(
                log.iter()
                    .filter(|line| line.starts_with(b"h4 AUTHENTICATE"))
                    .count(),
                1
            );
            assert_eq!(log.last().unwrap(), b"h6 LOGOUT\r\n");
        }
    }
}
#[tokio::test]
async fn strict_imap_untrusted_tls_or_hostname_never_accesses_credentials() {
    for implicit in [true, false] {
        for (host, trusted) in [("wrong.invalid", true), ("localhost", false)] {
            let (result, accesses, log) = run(implicit, Mode::Good, host, trusted).await;
            assert_eq!(result, Err(Failure::Tls));
            assert_eq!(accesses, 0);
            assert!(!log.iter().any(|line| line.starts_with(b"h4")));
        }
    }
}
#[tokio::test]
async fn strict_imap_starttls_refusal_and_plaintext_injection_never_authenticate() {
    for mode in [Mode::NoStarttls, Mode::RejectTls, Mode::InjectTls] {
        let (result, accesses, log) = run(false, mode, "localhost", true).await;
        assert_eq!(result, Err(Failure::Tls));
        assert_eq!(accesses, 0);
        assert!(!log.iter().any(|line| line.starts_with(b"h4")));
    }
}
#[tokio::test]
async fn strict_imap_preauth_tag_literal_flood_and_stall_fail_before_credentials() {
    for mode in [
        Mode::Preauth,
        Mode::WrongCapTag,
        Mode::Literal,
        Mode::Flood,
        Mode::Stall,
    ] {
        let (result, accesses, log) = run(true, mode, "localhost", true).await;
        assert_eq!(
            result,
            Err(if mode == Mode::Stall {
                Failure::Timeout
            } else {
                Failure::Protocol
            })
        );
        assert_eq!(accesses, 0);
        assert!(!log.iter().any(|line| line.starts_with(b"h4")));
    }
}
#[tokio::test]
async fn strict_imap_wrong_auth_tag_rejection_or_missing_bye_never_reports_success() {
    for mode in [Mode::WrongAuthTag, Mode::RejectAuth, Mode::MissingBye] {
        let (result, accesses, log) = run(true, mode, "localhost", true).await;
        assert_eq!(
            result,
            Err(if mode == Mode::RejectAuth {
                Failure::Authentication
            } else {
                Failure::Protocol
            })
        );
        assert_eq!(accesses, 1);
        assert_eq!(log.iter().filter(|line| line.starts_with(b"h4")).count(), 1);
    }
}
