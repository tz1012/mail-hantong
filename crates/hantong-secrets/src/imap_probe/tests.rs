use super::{
    windows::{ImapTrashOutcome, Source, run_connected},
    *,
};
use crate::{SecretError, TerminalSecretBinding};
use hantong_domain::mail_connection::MailConnectionConfig;
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
type Failure = MailProbeError;
const CA: &[u8] = include_bytes!("../../tests/fixtures/tls/ca.cert");
const CHAIN: &[u8] = include_bytes!("../../tests/fixtures/tls/end.fullchain");
const KEY: &[u8] = include_bytes!("../../tests/fixtures/tls/end.key");
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn fixture() -> (Cleanup, SecretStore) {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "hantong-imap-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    #[cfg(target_os = "linux")]
    std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
    #[cfg(target_os = "linux")]
    let store = crate::test_headless_store(&root).unwrap();
    #[cfg(windows)]
    let store = SecretStore::open(&root).unwrap();
    (Cleanup(root), store)
}
fn endpoint(implicit: bool, host: &str, auth: &str, version: u64) -> ImapEndpointConfig {
    let value = serde_json::json!({"schema_version":1,"account_id":"acct","imap":{"settings_version":version,"host":host,"port":993,"tls":if implicit {"implicit_tls"} else {"starttls_required"},"login_name":"user","authentication":auth}});
    MailConnectionConfig::from_json(&serde_json::to_vec(&value).unwrap())
        .unwrap()
        .imap()
        .clone()
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
    NoRevision,
    NoAuth,
    StaleCredential,
    MissingCredential,
    EarlyAuthOk,
    NonEmptyChallenge,
    ExtraChallenge,
    AuthInterleaved,
    AuthFlood,
    ChangedCapabilities,
    RejectPostAuthCapability,
    Rev2,
    DualRevision,
    OauthInitial,
    Trash,
}
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
    if tag == "h5" && mode == Mode::RejectPostAuthCapability {
        return io.get_mut().write_all(b"h5 NO private failure\r\n").await;
    }
    let auth = match mode {
        Mode::Login => "AUTH=LOGIN",
        Mode::Oauth | Mode::OauthInitial => "AUTH=XOAUTH2",
        Mode::NoAuth => "AUTH=CRAM-MD5",
        _ => "AUTH=PLAIN",
    };
    let initial = if matches!(mode, Mode::InitialResponse | Mode::OauthInitial) {
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
    let revision = match mode {
        Mode::NoRevision => "UNKNOWN",
        Mode::Rev2 => "IMAP4rev2",
        Mode::DualRevision => "IMAP4rev1 IMAP4rev2",
        _ => "IMAP4rev1",
    };
    let features = if mode == Mode::Trash {
        " IDLE UIDPLUS MOVE"
    } else if matches!(mode, Mode::Rev2 | Mode::DualRevision)
        || mode == Mode::ChangedCapabilities && tag == "h5"
    {
        ""
    } else {
        " IDLE UIDPLUS"
    };
    io.get_mut().write_all(format!("* CAPABILITY {revision} {auth}{initial}{starttls}{features}\r\n{tag} OK capabilities\r\n").as_bytes()).await
}

fn trash_request(
    uidvalidity: u32,
    uid: u32,
) -> (
    hantong_contracts::imap_trash::ImapTrashRequest,
    hantong_contracts::uid_page::UidMailboxIdentity,
) {
    use hantong_contracts::{
        imap_trash::ImapTrashRequest,
        uid_page::{UidMailboxEncoding, UidMailboxIdentity},
    };
    let source = UidMailboxIdentity::new("494e424f58", UidMailboxEncoding::Utf8).unwrap();
    let destination = UidMailboxIdentity::new("5472617368", UidMailboxEncoding::Utf8).unwrap();
    let request = ImapTrashRequest::new(
        "acct",
        source,
        7,
        uidvalidity,
        uid,
        "trash-operation",
        "trash-idempotency",
        [7; 32],
    )
    .unwrap();
    (request, destination)
}

async fn run_trash_script(
    request: &hantong_contracts::imap_trash::ImapTrashRequest,
    destination: &hantong_contracts::uid_page::UidMailboxIdentity,
    script: Script,
) -> (Result<ImapTrashOutcome, Failure>, Vec<Vec<u8>>) {
    let (_cleanup, store) = fixture();
    let config = endpoint(true, "localhost", "app_password", 7);
    store
        .store_imap_credential("acct", &config, &b"secret"[..])
        .unwrap();
    let (left, right) = tokio::io::duplex(8192);
    let log = Arc::new(Mutex::new(Vec::new()));
    let accesses = AtomicUsize::new(0);
    let mut task = tokio::spawn(server_with_operations(
        right,
        true,
        Mode::Trash,
        log.clone(),
        None,
        Some(script),
    ));
    let result = timeout(
        Duration::from_secs(3),
        super::windows::run_test_trash(
            &store,
            "acct",
            &config,
            left,
            configs(true).0,
            request,
            destination,
            &std::sync::atomic::AtomicBool::new(false),
            Some(&accesses),
        ),
    )
    .await
    .unwrap_or(Err(Failure::Timeout));
    let joined = timeout(Duration::from_millis(300), &mut task).await;
    match joined {
        Ok(joined) => {
            let joined = joined.expect("IMAP trash fixture must not panic");
            if let Err(error) = joined {
                assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
            }
        }
        Err(_) => {
            task.abort();
            let _ = task.await;
            panic!("IMAP trash fixture timed out");
        }
    }
    assert_eq!(accesses.load(Ordering::SeqCst), 1);
    (result, log.lock().unwrap().clone())
}

#[tokio::test]
async fn sec08_held_fresh_imap_credential_probes_without_replacing_stored_value() {
    let (_cleanup, store) = fixture();
    let config = endpoint(true, "localhost", "app_password", 7);
    store
        .store_imap_credential("acct", &config, &b"stored-value"[..])
        .unwrap();
    let fresh =
        crate::mail_credentials::BoundCredential::from_plain("acct", &config, b"secret").unwrap();
    let (left, right) = tokio::io::duplex(8192);
    let log = Arc::new(Mutex::new(Vec::new()));
    let task = tokio::spawn(server(right, true, Mode::Good, log));
    let result = timeout(
        Duration::from_secs(3),
        run_connected(
            Source {
                store: &store,
                account: "acct",
                endpoint: &config,
                held_credential: Some(fresh),
                accesses: None,
            },
            left,
            configs(true).0,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result.settings_version(), 7);
    let stored = store.unseal_imap_credential("acct", &config).unwrap();
    assert_eq!(stored.text().unwrap(), "stored-value");
    task.await.unwrap().unwrap();
}

fn terminal_imap_binding(endpoint: &ImapEndpointConfig) -> TerminalSecretBinding {
    let request_kind = *b"terminal-imap-v1";
    let endpoint_bytes = serde_json::to_vec(endpoint).unwrap();
    let mut canonical = Vec::new();
    for value in [
        b"terminal-endpoint-v1".as_slice(),
        request_kind.as_slice(),
        b"acct".as_slice(),
        endpoint_bytes.as_slice(),
    ] {
        canonical.extend_from_slice(&(value.len() as u32).to_le_bytes());
        canonical.extend_from_slice(value);
    }
    TerminalSecretBinding::new(
        request_kind,
        [21; 32],
        [22; 32],
        crate::google::crypto::sha256(b"acct").unwrap(),
        crate::google::crypto::sha256(&canonical).unwrap(),
        endpoint.settings_version(),
        [25; 32],
        [26; 32],
        [27; 32],
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
            + 60_000,
    )
    .unwrap()
}

#[tokio::test]
async fn sec08_terminal_imap_probe_consumes_fresh_input_without_replacing_stored_value() {
    let (_cleanup, store) = fixture();
    let config = endpoint(true, "localhost", "app_password", 7);
    store
        .store_imap_credential("acct", &config, &b"stored-value"[..])
        .unwrap();
    let binding = terminal_imap_binding(&config);
    let ticket = store.issue_terminal_secret_ticket(binding).unwrap();
    let handle = store
        .stage_terminal_secret(&ticket, &b"secret"[..])
        .unwrap();
    let (left, right) = tokio::io::duplex(8192);
    let log = Arc::new(Mutex::new(Vec::new()));
    let task = tokio::spawn(server(right, true, Mode::Good, log));
    let witness = timeout(
        Duration::from_secs(3),
        store.consume_terminal_imap_probe_with_stream(
            ticket.clone(),
            handle.clone(),
            binding,
            "acct",
            &config,
            (left, configs(true).0),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(witness.request_id(), &[21; 32]);
    assert_eq!(witness.account_id(), "acct");
    assert_eq!(witness.receipt().settings_version(), 7);
    assert_eq!(
        store
            .unseal_imap_credential("acct", &config)
            .unwrap()
            .text()
            .unwrap(),
        "stored-value"
    );
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn sec08_terminal_imap_probe_rejects_mismatch_and_replay() {
    let (_cleanup, store) = fixture();
    let config = endpoint(true, "localhost", "app_password", 7);
    let binding = terminal_imap_binding(&config);
    let ticket = store.issue_terminal_secret_ticket(binding).unwrap();
    let handle = store
        .stage_terminal_secret(&ticket, &b"secret"[..])
        .unwrap();
    let changed = endpoint(false, "localhost", "app_password", 8);
    let changed_binding = terminal_imap_binding(&changed);
    let (left, _right) = tokio::io::duplex(8192);
    let mismatch = store
        .consume_terminal_imap_probe_with_stream(
            ticket.clone(),
            handle.clone(),
            changed_binding,
            "acct",
            &changed,
            (left, configs(true).0),
        )
        .await;
    assert!(matches!(
        mismatch,
        Err(MailProbeError::Credential(SecretError::CredentialMismatch))
    ));

    let (left, right) = tokio::io::duplex(8192);
    let log = Arc::new(Mutex::new(Vec::new()));
    let task = tokio::spawn(server(right, true, Mode::Good, log));
    store
        .consume_terminal_imap_probe_with_stream(
            ticket.clone(),
            handle.clone(),
            binding,
            "acct",
            &config,
            (left, configs(true).0),
        )
        .await
        .unwrap();
    task.await.unwrap().unwrap();
    let (left, _right) = tokio::io::duplex(8192);
    let replay = store
        .consume_terminal_imap_probe_with_stream(
            ticket,
            handle,
            binding,
            "acct",
            &config,
            (left, configs(true).0),
        )
        .await;
    assert!(matches!(
        replay,
        Err(MailProbeError::Credential(SecretError::CredentialMismatch))
    ));
}

#[test]
fn sec08_terminal_imap_probe_cancel_is_one_shot_and_preserves_stored_value() {
    let (_cleanup, store) = fixture();
    let config = endpoint(true, "localhost", "app_password", 7);
    store
        .store_imap_credential("acct", &config, &b"stored-value"[..])
        .unwrap();
    let binding = terminal_imap_binding(&config);
    let ticket = store.issue_terminal_secret_ticket(binding).unwrap();
    let handle = store
        .stage_terminal_secret(&ticket, &b"secret"[..])
        .unwrap();
    store
        .cancel_terminal_secret(&ticket, handle.opaque_id())
        .unwrap();
    assert!(matches!(
        store.cancel_terminal_secret(&ticket, handle.opaque_id()),
        Err(SecretError::NotFound)
    ));
    assert_eq!(
        store
            .unseal_imap_credential("acct", &config)
            .unwrap()
            .text()
            .unwrap(),
        "stored-value"
    );
}

fn trash_script(uidvalidity: u32, move_reply: &[u8]) -> Script {
    vec![
        (
            b"h6 SELECT INBOX\r\n".to_vec(),
            format!(
                "* OK [UIDVALIDITY {uidvalidity}] selected\r\n* OK [UIDNEXT 99] next\r\nh6 OK [READ-WRITE] selected\r\n"
            )
            .into_bytes(),
        ),
        (
            b"h7 UID MOVE 17 Trash\r\n".to_vec(),
            move_reply.to_vec(),
        ),
    ]
}

fn assert_no_legacy_mutation_commands(transcript: &[Vec<u8>]) {
    for line in transcript {
        for token in ["COPY", "STORE", "EXPUNGE", "CLOSE"] {
            assert!(
                !line
                    .windows(token.len())
                    .any(|window| window == token.as_bytes()),
                "unexpected legacy mutation command in transcript: {:?}",
                line
            );
        }
    }
}

#[tokio::test]
async fn sec03_authenticated_imap_trash_pipeline_accepts_one_uid_move() {
    let (request, destination) = trash_request(42, 17);
    let (result, transcript) = run_trash_script(
        &request,
        &destination,
        trash_script(42, b"h7 OK [COPYUID 7 17 99] moved\r\n"),
    )
    .await;
    assert_eq!(result, Ok(ImapTrashOutcome::Accepted));
    assert_no_legacy_mutation_commands(&transcript);
}

#[tokio::test]
async fn sec03_authenticated_imap_trash_pipeline_rejects_uidvalidity_before_move() {
    let (request, destination) = trash_request(42, 17);
    let (result, transcript) = run_trash_script(
        &request,
        &destination,
        vec![
            (
                b"h6 SELECT INBOX\r\n".to_vec(),
                b"* OK [UIDVALIDITY 43] selected\r\n* OK [UIDNEXT 99] next\r\nh6 OK [READ-WRITE] selected\r\n".to_vec(),
            ),
        ],
    )
    .await;
    assert_eq!(result, Ok(ImapTrashOutcome::Rejected));
    assert!(
        !transcript
            .iter()
            .any(|line| line.starts_with(b"h7 UID MOVE"))
    );
    assert_no_legacy_mutation_commands(&transcript);
}

#[tokio::test]
async fn sec03_authenticated_imap_trash_pipeline_maps_postwrite_no_to_unknown() {
    let (request, destination) = trash_request(42, 17);
    let (result, transcript) = run_trash_script(
        &request,
        &destination,
        trash_script(42, b"h7 NO [TRYCREATE] not moved\r\n"),
    )
    .await;
    assert_eq!(result, Ok(ImapTrashOutcome::Unknown));
    assert_eq!(
        transcript
            .iter()
            .filter(|line| line.starts_with(b"h7 UID MOVE"))
            .count(),
        1
    );
    assert_no_legacy_mutation_commands(&transcript);
}

#[tokio::test]
async fn sec03_authenticated_imap_trash_pipeline_maps_postwrite_disconnect_to_unknown() {
    let (request, destination) = trash_request(42, 17);
    let (result, transcript) =
        run_trash_script(&request, &destination, trash_script(42, TEST_DISCONNECT)).await;
    assert_eq!(result, Ok(ImapTrashOutcome::Unknown));
    assert_eq!(
        transcript
            .iter()
            .filter(|line| line.starts_with(b"h7 UID MOVE"))
            .count(),
        1
    );
    assert_no_legacy_mutation_commands(&transcript);
}
async fn server(
    stream: DuplexStream,
    implicit: bool,
    mode: Mode,
    log: Transcript,
) -> io::Result<()> {
    server_with_list(stream, implicit, mode, log, None).await
}
async fn server_with_list(
    stream: DuplexStream,
    implicit: bool,
    mode: Mode,
    log: Transcript,
    list: Option<Vec<u8>>,
) -> io::Result<()> {
    server_with_operations(stream, implicit, mode, log, list, None).await
}
type Script = Vec<(Vec<u8>, Vec<u8>)>;
const TEST_DISCONNECT: &[u8] = b"\0hantong-test-disconnect\0";
async fn server_with_operations(
    mut stream: DuplexStream,
    implicit: bool,
    mode: Mode,
    log: Transcript,
    list: Option<Vec<u8>>,
    script: Option<Script>,
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
    if matches!(mode, Mode::EarlyAuthOk | Mode::NonEmptyChallenge) {
        wire.get_mut()
            .write_all(if mode == Mode::EarlyAuthOk {
                b"h4 OK fake success\r\n"
            } else {
                b"+ ZmFrZQ==\r\n"
            })
            .await?;
        assert!(read_command(&mut wire, &log).await?.is_empty());
        return Ok(());
    }
    if mode == Mode::AuthInterleaved {
        wire.get_mut()
            .write_all(b"* OK notice\r\n* CAPABILITY IMAP4rev1 AUTH=LOGIN\r\n")
            .await?;
    }
    if mode == Mode::AuthFlood {
        for _ in 0..31 {
            wire.get_mut().write_all(b"* OK notice\r\n").await?;
        }
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
        Mode::OauthInitial => assert_eq!(
            auth,
            b"h4 AUTHENTICATE XOAUTH2 dXNlcj11c2VyAWF1dGg9QmVhcmVyIHRva2VuAQE=\r\n"
        ),
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
    if mode == Mode::ExtraChallenge {
        wire.get_mut().write_all(b"+ \r\n").await?;
        assert!(read_command(&mut wire, &log).await?.is_empty());
        return Ok(());
    }
    if mode == Mode::AuthInterleaved {
        wire.get_mut()
            .write_all(b"* CAPABILITY IMAP4rev1 MOVE CONDSTORE\r\n")
            .await?;
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
    if let Some(script) = script {
        for (expected, reply) in script {
            let command = read_command(&mut wire, &log).await?;
            if command.is_empty() {
                return Ok(());
            }
            assert_eq!(command, expected);
            if reply == TEST_DISCONNECT {
                // A post-write disconnect is represented by an explicit
                // test-only marker. Empty replies retain their legacy meaning
                // of withholding a response so timeout tests remain valid.
                return Ok(());
            }
            wire.get_mut().write_all(&reply).await?;
            if let Err(error) = wire.get_mut().flush().await {
                if error.kind() == io::ErrorKind::UnexpectedEof {
                    return Ok(());
                }
                return Err(error);
            }
        }
        assert!(read_command(&mut wire, &log).await?.is_empty());
        return Ok(());
    }
    let mut logout = read_command(&mut wire, &log).await?;
    if mode == Mode::RejectPostAuthCapability {
        assert!(logout.is_empty());
        return Ok(());
    }
    let logout_tag = if let Some(list) = list {
        if logout.is_empty() {
            return Ok(());
        }
        assert_eq!(logout, b"h6 LIST \"\" *\r\n");
        wire.get_mut().write_all(&list).await?;
        wire.get_mut().flush().await?;
        logout = read_command(&mut wire, &log).await?;
        if logout.is_empty() {
            return Ok(());
        }
        "h7"
    } else {
        "h6"
    };
    assert_eq!(logout, format!("{logout_tag} LOGOUT\r\n").as_bytes());
    wire.get_mut()
        .write_all(
            if mode == Mode::MissingBye {
                format!("{logout_tag} OK bye\r\n")
            } else {
                format!("* BYE fixture\r\n{logout_tag} OK bye\r\n")
            }
            .as_bytes(),
        )
        .await
}

#[path = "discovery_tests.rs"]
mod discovery;
#[path = "uid_page_tests.rs"]
mod uid_page;
async fn run(
    implicit: bool,
    mode: Mode,
    host: &str,
    trusted: bool,
) -> (Result<ImapProbeReceipt, Failure>, usize, Vec<Vec<u8>>) {
    let (_cleanup, store) = fixture();
    let config = endpoint(
        implicit,
        host,
        if matches!(mode, Mode::Oauth | Mode::OauthInitial) {
            "oauth2"
        } else {
            "app_password"
        },
        7,
    );
    let credential: &[u8] = if matches!(mode, Mode::Oauth | Mode::OauthInitial) {
        b"token"
    } else {
        b"secret"
    };
    if mode != Mode::MissingCredential {
        let stored = if mode == Mode::StaleCredential {
            endpoint(implicit, host, "app_password", 6)
        } else {
            config.clone()
        };
        store
            .store_imap_credential("acct", &stored, credential)
            .unwrap();
    }
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
        run_connected(
            Source {
                store: &store,
                held_credential: None,
                account: "acct",
                endpoint: &config,
                accesses: Some(&accesses),
            },
            left,
            configs(trusted).0,
        ),
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
async fn ext03_imap_tls_modes_and_auth_mechanisms_roundtrip() {
    for implicit in [true, false] {
        for mode in [
            Mode::Good,
            Mode::Login,
            Mode::Oauth,
            Mode::InitialResponse,
            Mode::OauthInitial,
            Mode::AuthInterleaved,
        ] {
            let (result, accesses, log) = run(implicit, mode, "localhost", true).await;
            let receipt = result.unwrap();
            assert!(receipt.capabilities().idle());
            assert_eq!(receipt.settings_version(), 7);
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
async fn ext03_imap_untrusted_tls_or_hostname_never_accesses_credentials() {
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
async fn ext03_imap_starttls_refusal_and_plaintext_injection_never_authenticate() {
    for mode in [Mode::NoStarttls, Mode::RejectTls, Mode::InjectTls] {
        let (result, accesses, log) = run(false, mode, "localhost", true).await;
        assert_eq!(result, Err(Failure::Tls));
        assert_eq!(accesses, 0);
        assert!(!log.iter().any(|line| line.starts_with(b"h4")));
    }
}
#[tokio::test]
async fn ext03_imap_preauth_tag_literal_flood_and_stall_fail_before_credentials() {
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
async fn ext03_imap_wrong_auth_tag_rejection_or_missing_bye_never_reports_success() {
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

#[tokio::test]
async fn ext03_imap_missing_revision_or_mechanism_prevents_unseal() {
    for implicit in [true, false] {
        for mode in [Mode::NoRevision, Mode::NoAuth] {
            let (result, accesses, log) = run(implicit, mode, "localhost", true).await;
            assert_eq!(
                result,
                Err(if mode == Mode::NoAuth {
                    Failure::MechanismUnsupported
                } else {
                    Failure::Protocol
                })
            );
            assert_eq!(accesses, 0);
            assert!(!log.iter().any(|line| line.starts_with(b"h4")));
        }
    }
}
#[tokio::test]
async fn sec01_imap_bound_missing_or_stale_credentials_never_authenticate() {
    for mode in [Mode::MissingCredential, Mode::StaleCredential] {
        let (result, accesses, log) = run(true, mode, "localhost", true).await;
        assert_eq!(
            result,
            Err(Failure::Credential(if mode == Mode::MissingCredential {
                crate::SecretError::NotFound
            } else {
                crate::SecretError::CredentialMismatch
            }))
        );
        assert_eq!(accesses, 1);
        assert!(!log.iter().any(|line| line.starts_with(b"h4")));
    }
}
#[tokio::test]
async fn ext03_imap_auth_flow_rejects_early_success_challenge_injection_and_cumulative_flood() {
    for mode in [
        Mode::EarlyAuthOk,
        Mode::NonEmptyChallenge,
        Mode::ExtraChallenge,
        Mode::AuthFlood,
    ] {
        let (result, accesses, log) = run(true, mode, "localhost", true).await;
        assert_eq!(
            result,
            Err(if mode == Mode::EarlyAuthOk {
                Failure::Authentication
            } else {
                Failure::Protocol
            })
        );
        assert_eq!(accesses, 1);
        assert_eq!(log.iter().filter(|s| s.starts_with(b"h4 ")).count(), 1);
        assert!(!log.iter().any(|line| line.starts_with(b"h5")));
        if matches!(mode, Mode::EarlyAuthOk | Mode::NonEmptyChallenge) {
            assert!(!log.iter().any(|line| line.starts_with(b"AHVz")));
        }
    }
}
#[tokio::test]
async fn ext05_imap_snapshot_uses_post_auth_capabilities_and_revision_mode() {
    for mode in [
        Mode::ChangedCapabilities,
        Mode::Rev2,
        Mode::DualRevision,
        Mode::AuthInterleaved,
    ] {
        let (result, _, _) = run(true, mode, "localhost", true).await;
        let receipt = result.unwrap();
        let caps = receipt.capabilities();
        assert_eq!(
            caps.idle(),
            matches!(mode, Mode::Rev2 | Mode::AuthInterleaved)
        );
        assert_eq!(
            caps.uidplus(),
            matches!(mode, Mode::Rev2 | Mode::AuthInterleaved)
        );
        assert_eq!(caps.move_messages(), mode == Mode::Rev2);
        assert!(!caps.condstore() && !caps.qresync() && !caps.special_use());
    }
    let (result, accesses, _) = run(true, Mode::RejectPostAuthCapability, "localhost", true).await;
    assert_eq!(result, Err(Failure::Protocol));
    assert_eq!(accesses, 1);
}
#[test]
fn sec08_imap_library_logging_is_compiled_out_in_debug_and_release() {
    assert_eq!(log::STATIC_MAX_LEVEL, log::LevelFilter::Off);
}
#[tokio::test(start_paused = true)]
async fn ext03_imap_whole_probe_deadline_drops_pending_transport() {
    struct Dropped(Arc<AtomicUsize>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let drops = Arc::new(AtomicUsize::new(0));
    let guard = Dropped(drops.clone());
    let started = tokio::time::Instant::now();
    let result: Result<(), Failure> = windows::with_deadline(async {
        let _held = guard;
        std::future::pending().await
    })
    .await;
    assert_eq!(result, Err(Failure::Timeout));
    assert_eq!(started.elapsed(), Duration::from_secs(30));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
