use super::{
    windows::{Source, TlsSettings, run_connected},
    *,
};
use hantong_domain::mail_connection::{MailConnectionConfig, TlsMode};
use lettre::transport::smtp::client::{Certificate, CertificateStore, TlsParameters};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use std::{
    fs, io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, DuplexStream},
    time::timeout,
};
use tokio_rustls::TlsAcceptor;
const CA: &[u8] = include_bytes!("../../tests/fixtures/tls/ca.cert");
const CHAIN: &[u8] = include_bytes!("../../tests/fixtures/tls/end.fullchain");
const KEY: &[u8] = include_bytes!("../../tests/fixtures/tls/end.key");
type Transcript = Arc<Mutex<Vec<Vec<u8>>>>;
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn fixture() -> (Cleanup, SecretStore) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "hantong-smtp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let store = SecretStore::open(&root).unwrap();
    (Cleanup(root), store)
}
fn endpoint(tls: TlsMode, host: &str, auth: &str, version: u64) -> SmtpEndpointConfig {
    let tls = match tls {
        TlsMode::ImplicitTls => "implicit_tls",
        TlsMode::StarttlsRequired => "starttls_required",
    };
    let value = serde_json::json!({"schema_version":1,"account_id":"acct", "imap":{
        "settings_version":1,"host":"localhost","port":993,"tls":"implicit_tls","login_name":"user","authentication":"app_password"
    },"smtp":{"settings_version":version,"host":host,"port":465,"tls":tls,"login_name":"user","authentication":auth}});
    MailConnectionConfig::from_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .smtp()
        .unwrap()
        .clone()
}
fn tls_settings(host: &str, trusted: bool) -> TlsSettings {
    let mut roots = RootCertStore::empty();
    let mut starttls =
        TlsParameters::builder(host.to_owned()).certificate_store(CertificateStore::None);
    if trusted {
        for cert in CertificateDer::pem_slice_iter(CA) {
            roots.add(cert.unwrap()).unwrap();
        }
        starttls = starttls.add_root_certificate(Certificate::from_pem(CA).unwrap());
    }
    TlsSettings {
        implicit: Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        ),
        starttls: starttls.build_rustls().unwrap(),
    }
}
fn acceptor() -> TlsAcceptor {
    let certs = CertificateDer::pem_slice_iter(CHAIN)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(KEY).unwrap();
    TlsAcceptor::from(Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap(),
    ))
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum Scenario {
    Good,
    NoStarttls,
    RefuseStarttls,
    MalformedTls,
    Stall,
    Oversize,
    NoAuth,
    Login,
    Oauth,
    RejectAuth,
    BadAuthCode,
    RejectQuit,
    BadQuitCode,
    InjectBeforeTls,
}
async fn command<S: AsyncRead + AsyncWrite + Unpin>(
    reader: &mut BufReader<S>,
    log: &Transcript,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.read_until(b'\n', &mut bytes).await?;
    assert!(bytes.len() <= 32 * 1024, "fixture command bound");
    if !bytes.is_empty() {
        log.lock().unwrap().push(bytes.clone());
    }
    Ok(bytes)
}
async fn conversation<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    greeting: bool,
    mode: Scenario,
    log: Transcript,
) -> io::Result<()> {
    let mut reader = BufReader::new(stream);
    if mode == Scenario::Oversize {
        reader
            .get_mut()
            .write_all(&vec![b'X'; 256 * 1024 + 1])
            .await?;
        return Ok(());
    }
    if greeting {
        reader.get_mut().write_all(b"220 fixture\r\n").await?;
    }
    assert_eq!(
        command(&mut reader, &log).await?,
        b"EHLO mail-hantong.invalid\r\n"
    );
    let caps: &[u8] = match mode {
        Scenario::NoAuth => b"250-fixture\r\n250 SIZE 1024\r\n",
        Scenario::Login => b"250-fixture\r\n250 AUTH LOGIN\r\n",
        Scenario::Oauth => b"250-fixture\r\n250 AUTH XOAUTH2\r\n",
        _ => b"250-fixture\r\n250 AUTH PLAIN LOGIN XOAUTH2\r\n",
    };
    reader.get_mut().write_all(caps).await?;
    let auth = command(&mut reader, &log).await?;
    if auth.is_empty() {
        return Ok(());
    }
    if mode == Scenario::Login {
        assert_eq!(auth, b"AUTH LOGIN\r\n");
        reader.get_mut().write_all(b"334 VXNlcm5hbWU6\r\n").await?;
        assert_eq!(command(&mut reader, &log).await?, b"dXNlcg==\r\n");
        reader.get_mut().write_all(b"334 UGFzc3dvcmQ6\r\n").await?;
        assert_eq!(command(&mut reader, &log).await?, b"c2VjcmV0\r\n");
    } else if mode == Scenario::Oauth {
        assert_eq!(
            auth,
            b"AUTH XOAUTH2 dXNlcj11c2VyAWF1dGg9QmVhcmVyIHRva2VuAQE=\r\n"
        );
    } else {
        assert_eq!(auth, b"AUTH PLAIN AHVzZXIAc2VjcmV0\r\n");
    }
    let reply: &[u8] = match mode {
        Scenario::RejectAuth => b"535 private fixture response\r\n",
        Scenario::BadAuthCode => b"250 not authentication success\r\n",
        _ => b"235 authenticated\r\n",
    };
    reader.get_mut().write_all(reply).await?;
    let quit = command(&mut reader, &log).await?;
    if quit.is_empty() {
        return Ok(());
    }
    assert_eq!(quit, b"QUIT\r\n");
    let reply: &[u8] = match mode {
        Scenario::RejectQuit => b"500 private fixture response\r\n",
        Scenario::BadQuitCode => b"250 not quit success\r\n",
        _ => b"221 bye\r\n",
    };
    reader.get_mut().write_all(reply).await?;
    let _ = command(&mut reader, &log).await?;
    Ok(())
}
async fn server(
    mut stream: DuplexStream,
    tls: TlsMode,
    mode: Scenario,
    log: Transcript,
) -> io::Result<()> {
    if mode == Scenario::Stall {
        tokio::time::sleep(Duration::from_secs(20)).await;
        return Ok(());
    }
    if tls == TlsMode::StarttlsRequired {
        stream.write_all(b"220 fixture\r\n").await?;
        let mut reader = BufReader::new(stream);
        assert_eq!(
            command(&mut reader, &log).await?,
            b"EHLO mail-hantong.invalid\r\n"
        );
        let caps: &[u8] = if mode == Scenario::NoStarttls {
            b"250 AUTH PLAIN\r\n"
        } else {
            b"250-STARTTLS\r\n250 AUTH PLAIN\r\n"
        };
        reader.get_mut().write_all(caps).await?;
        let cmd = command(&mut reader, &log).await?;
        if cmd.is_empty() {
            return Ok(());
        }
        assert_eq!(cmd, b"STARTTLS\r\n");
        if mode == Scenario::RefuseStarttls {
            reader
                .get_mut()
                .write_all(b"454 TLS unavailable\r\n")
                .await?;
            let quit = command(&mut reader, &log).await?;
            if quit == b"QUIT\r\n" {
                reader.get_mut().write_all(b"221 bye\r\n").await?;
            }
            return Ok(());
        }
        if mode == Scenario::InjectBeforeTls {
            reader.get_mut().write_all(b"220 ready\r\n250-fixture\r\n250 AUTH PLAIN\r\n235 forged success\r\n221 forged quit\r\n").await?;
        } else {
            reader.get_mut().write_all(b"220 ready\r\n").await?;
        }
        assert!(reader.buffer().is_empty());
        stream = reader.into_inner();
    }
    if mode == Scenario::MalformedTls {
        stream.write_all(b"not TLS\r\n").await?;
        return Ok(());
    }
    let secure = acceptor().accept(stream).await?;
    conversation(secure, tls == TlsMode::ImplicitTls, mode, log).await
}
struct Outcome {
    result: Result<SmtpProbeReceipt, MailProbeError>,
    accesses: usize,
    transcript: Vec<Vec<u8>>,
}
async fn run(
    tls: TlsMode,
    mode: Scenario,
    host: &str,
    trusted: bool,
    saved_version: Option<u64>,
) -> Outcome {
    let (_cleanup, store) = fixture();
    let auth = if mode == Scenario::Oauth {
        "oauth2"
    } else {
        "app_password"
    };
    let endpoint = endpoint(tls, host, auth, 1);
    if let Some(version) = saved_version {
        let saved = self::endpoint(tls, host, auth, version);
        let secret: &[u8] = if mode == Scenario::Oauth {
            b"token"
        } else {
            b"secret"
        };
        store.store_smtp_credential("acct", &saved, secret).unwrap();
    }
    let accesses = AtomicUsize::new(0);
    let (client, peer) = tokio::io::duplex(8192);
    let transcript = Arc::new(Mutex::new(Vec::new()));
    let mut task = tokio::spawn(server(peer, tls, mode, transcript.clone()));
    let deadline = if mode == Scenario::Stall {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(3)
    };
    let result = timeout(
        deadline,
        run_connected(
            Source {
                store: &store,
                account: "acct",
                endpoint: &endpoint,
                accesses: Some(&accesses),
            },
            client,
            "127.0.0.1:2465".parse().unwrap(),
            tls_settings(host, trusted),
        ),
    )
    .await
    .unwrap_or(Err(MailProbeError::Timeout));
    match timeout(Duration::from_millis(300), &mut task).await {
        Ok(joined) => {
            let _ = joined.expect("fake server must not panic");
        }
        Err(_) => {
            task.abort();
            assert!(matches!(task.await, Err(e) if e.is_cancelled()));
        }
    }
    let transcript = transcript.lock().unwrap().clone();
    assert!(
        transcript.iter().all(|s| !s.starts_with(b"MAIL ")
            && !s.starts_with(b"RCPT ")
            && !s.starts_with(b"DATA"))
    );
    Outcome {
        result,
        accesses: accesses.load(Ordering::SeqCst),
        transcript,
    }
}
#[tokio::test]
async fn ext03_m201_tls_modes_verify_before_bound_unseal_and_auth() {
    for tls in [TlsMode::ImplicitTls, TlsMode::StarttlsRequired] {
        let out = run(tls, Scenario::Good, "localhost", true, Some(1)).await;
        assert_eq!(out.result.unwrap().settings_version(), 1);
        assert_eq!(out.accesses, 1);
        let expected: Vec<&[u8]> = if tls == TlsMode::ImplicitTls {
            vec![
                b"EHLO mail-hantong.invalid\r\n",
                b"AUTH PLAIN AHVzZXIAc2VjcmV0\r\n",
                b"QUIT\r\n",
            ]
        } else {
            vec![
                b"EHLO mail-hantong.invalid\r\n",
                b"STARTTLS\r\n",
                b"EHLO mail-hantong.invalid\r\n",
                b"AUTH PLAIN AHVzZXIAc2VjcmV0\r\n",
                b"QUIT\r\n",
            ]
        };
        assert_eq!(out.transcript, expected);
    }
}
#[tokio::test]
async fn ext03_m201_invalid_tls_never_unseals_or_authenticates() {
    for tls in [TlsMode::ImplicitTls, TlsMode::StarttlsRequired] {
        for (mode, host, trusted) in [
            (Scenario::Good, "wrong.invalid", true),
            (Scenario::Good, "localhost", false),
            (Scenario::MalformedTls, "localhost", true),
        ] {
            let out = run(tls, mode, host, trusted, Some(1)).await;
            assert_eq!(out.result, Err(MailProbeError::Tls));
            assert_eq!(out.accesses, 0);
            assert!(!out.transcript.iter().any(|s| s.starts_with(b"AUTH ")));
        }
    }
}
#[tokio::test]
async fn ext03_m201_starttls_is_required_without_plaintext_fallback() {
    for mode in [Scenario::NoStarttls, Scenario::RefuseStarttls] {
        let out = run(TlsMode::StarttlsRequired, mode, "localhost", true, Some(1)).await;
        assert_eq!(out.result, Err(MailProbeError::Tls));
        assert_eq!(out.accesses, 0);
        assert!(!out.transcript.iter().any(|s| s.starts_with(b"AUTH ")));
    }
}
#[tokio::test]
async fn sec01_m201_bound_envelope_and_mechanism_checked_before_auth() {
    for (mode, version, expected, reads) in [
        (
            Scenario::NoAuth,
            Some(1),
            MailProbeError::MechanismUnsupported,
            0,
        ),
        (
            Scenario::Good,
            None,
            MailProbeError::Credential(SecretError::NotFound),
            1,
        ),
        (
            Scenario::Good,
            Some(2),
            MailProbeError::Credential(SecretError::CredentialMismatch),
            1,
        ),
    ] {
        let out = run(TlsMode::ImplicitTls, mode, "localhost", true, version).await;
        assert_eq!(out.result, Err(expected));
        assert_eq!(out.accesses, reads);
        assert!(!out.transcript.iter().any(|s| s.starts_with(b"AUTH ")));
    }
}
#[tokio::test]
async fn ext01_m201_login_and_xoauth2_use_one_selected_mechanism() {
    for mode in [Scenario::Login, Scenario::Oauth] {
        let out = run(TlsMode::ImplicitTls, mode, "localhost", true, Some(1)).await;
        assert!(out.result.is_ok());
        assert_eq!(out.accesses, 1);
        assert_eq!(
            out.transcript
                .iter()
                .filter(|s| s.starts_with(b"AUTH "))
                .count(),
            1
        );
    }
}
#[tokio::test]
async fn ext03_m201_auth_and_quit_responses_must_have_exact_success_codes() {
    for (mode, expected) in [
        (Scenario::RejectAuth, MailProbeError::Authentication),
        (Scenario::BadAuthCode, MailProbeError::Authentication),
        (Scenario::RejectQuit, MailProbeError::Protocol),
        (Scenario::BadQuitCode, MailProbeError::Protocol),
    ] {
        let out = run(TlsMode::ImplicitTls, mode, "localhost", true, Some(1)).await;
        assert_eq!(out.result, Err(expected));
        assert_eq!(out.accesses, 1);
        assert_eq!(
            out.transcript
                .iter()
                .filter(|s| s.starts_with(b"AUTH "))
                .count(),
            1
        );
        assert!(
            out.transcript
                .iter()
                .filter(|s| s.starts_with(b"QUIT"))
                .count()
                <= 1
        );
    }
}
#[tokio::test]
async fn ext03_m201_stall_and_oversized_reply_never_unseal() {
    for tls in [TlsMode::ImplicitTls, TlsMode::StarttlsRequired] {
        for mode in [Scenario::Stall, Scenario::Oversize] {
            let out = run(tls, mode, "localhost", true, Some(1)).await;
            assert!(out.result.is_err());
            assert_eq!(out.accesses, 0);
            if mode == Scenario::Stall {
                assert_eq!(out.result, Err(MailProbeError::Timeout));
            }
            assert!(!out.transcript.iter().any(|s| s.starts_with(b"AUTH ")));
        }
    }
}
#[tokio::test]
async fn ext03_m201_invalid_account_and_errors_are_static() {
    let (_cleanup, store) = fixture();
    assert_eq!(
        store
            .probe_smtp(
                "bad account",
                &endpoint(TlsMode::ImplicitTls, "localhost", "app_password", 1)
            )
            .await,
        Err(MailProbeError::InvalidAccount)
    );
    assert_eq!(
        MailProbeError::Authentication.to_string(),
        "mail authentication failed"
    );
    assert_eq!(
        MailProbeError::Credential(SecretError::CredentialMismatch).to_string(),
        "mail credential unavailable for these settings"
    );
}

#[tokio::test]
async fn ext03_m201_pre_tls_injected_replies_never_cross_upgrade_boundary() {
    let out = run(
        TlsMode::StarttlsRequired,
        Scenario::InjectBeforeTls,
        "localhost",
        true,
        Some(1),
    )
    .await;
    assert_eq!(out.result, Err(MailProbeError::Tls));
    assert_eq!(out.accesses, 0);
    assert!(!out.transcript.iter().any(|s| s.starts_with(b"AUTH ")));
}

#[tokio::test(start_paused = true)]
async fn ext03_m201_real_probe_deadline_cancels_and_drops_pending_work() {
    struct Dropped(Arc<AtomicUsize>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicUsize::new(0));
    let count = dropped.clone();
    let start = tokio::time::Instant::now();
    let result: Result<(), MailProbeError> = windows::with_deadline(async move {
        let _guard = Dropped(count);
        std::future::pending().await
    })
    .await;
    assert_eq!(result, Err(MailProbeError::Timeout));
    assert_eq!(tokio::time::Instant::now() - start, Duration::from_secs(30));
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}
