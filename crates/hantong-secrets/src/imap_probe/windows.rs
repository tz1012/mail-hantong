use super::ImapUidPageError;
use super::list_gate::{ListGate, ListReply, RawMailbox};
use super::{
    gate::{GateMode, ReplyKind, StrictResponseGate},
    *,
};
use crate::{SensitiveBuffer, mail_transport::BoundedAsyncStream};
use hantong_domain::imap_reconcile::{ImapUidReconcilePage, ImapUidReconcileRequest};
use hantong_domain::imap_recovery::{ImapUidRecoveryPage, ImapUidRecoveryRequest};
use hantong_domain::imap_sync::{ImapUidPage, ImapUidPageRequest};
use hantong_domain::mail_connection::{Authentication, TlsMode};
use hantong_domain::mailboxes::{
    ImapMailboxDescriptor, ImapMailboxName, ImapMailboxSnapshot, MailboxChildren, MailboxEncoding,
    SpecialUseRole,
};
use io_imap::{
    codec::{AuthenticateDataCodec, CommandCodec, encode::Encoder},
    types::{
        auth::{AuthMechanism, AuthenticateData},
        command::{Command, CommandBody},
        response::StatusKind,
        secret::Secret,
    },
};
use rustls::{ClientConfig, pki_types::ServerName};
use rustls_platform_verifier::BuilderVerifierExt;
use std::{borrow::Cow, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    net::{TcpStream, lookup_host},
    time::timeout,
};
use tokio_rustls::TlsConnector;

#[path = "uid_page_driver.rs"]
mod uid_page_driver;

pub(super) struct Source<'a> {
    pub(super) store: &'a SecretStore,
    pub(super) account: &'a str,
    pub(super) endpoint: &'a ImapEndpointConfig,
    #[cfg(test)]
    pub(super) accesses: Option<&'a std::sync::atomic::AtomicUsize>,
}
impl Source<'_> {
    fn credential(&self) -> Result<crate::mail_credentials::BoundCredential, MailProbeError> {
        #[cfg(test)]
        if let Some(counter) = self.accesses {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        self.store
            .unseal_imap_credential(self.account, self.endpoint)
            .map_err(MailProbeError::Credential)
    }
}
pub(super) async fn with_deadline<T>(
    future: impl std::future::Future<Output = Result<T, MailProbeError>>,
) -> Result<T, MailProbeError> {
    timeout(Duration::from_secs(30), future)
        .await
        .map_err(|_| MailProbeError::Timeout)?
}
pub(super) async fn probe(
    store: &SecretStore,
    account: &str,
    endpoint: &ImapEndpointConfig,
) -> Result<ImapProbeReceipt, MailProbeError> {
    match connect(store, account, endpoint, ReadOperation::Probe).await? {
        ReadReceipt::Probe(receipt) => Ok(receipt),
        _ => Err(MailProbeError::Protocol),
    }
}
#[derive(Clone, Copy)]
enum ReadOperation<'a> {
    Probe,
    Mailboxes,
    UidObservation(&'a ImapUidObservationRequest),
    UidPage(&'a ImapUidPageRequest),
    UidReconcile(&'a ImapUidReconcileRequest),
    UidRecovery(&'a ImapUidRecoveryRequest),
}
enum ReadReceipt {
    Probe(ImapProbeReceipt),
    Mailboxes(ImapMailboxSnapshot),
    UidObservation(Result<ImapUidObservation, ImapUidPageError>),
    UidPage(Result<ImapUidPage, ImapUidPageError>),
    UidReconcile(Result<ImapUidReconcilePage, ImapUidPageError>),
    UidRecovery(Result<ImapUidRecoveryPage, ImapUidPageError>),
}
async fn connect(
    store: &SecretStore,
    account: &str,
    endpoint: &ImapEndpointConfig,
    operation: ReadOperation<'_>,
) -> Result<ReadReceipt, MailProbeError> {
    with_deadline(async {
        let tls = Arc::new(
            ClientConfig::builder()
                .with_platform_verifier()
                .map_err(|_| MailProbeError::Tls)?
                .with_no_client_auth(),
        );
        let addresses = lookup_host((endpoint.host(), endpoint.port()))
            .await
            .map_err(|_| MailProbeError::Connection)?;
        let mut stream = None;
        for address in addresses.take(8) {
            if let Ok(Ok(connected)) =
                timeout(Duration::from_secs(8), TcpStream::connect(address)).await
            {
                stream = Some(connected);
                break;
            }
        }
        let stream = stream.ok_or(MailProbeError::Connection)?;
        stream
            .set_nodelay(true)
            .map_err(|_| MailProbeError::Connection)?;
        run_operation(
            Source {
                store,
                account,
                endpoint,
                #[cfg(test)]
                accesses: None,
            },
            stream,
            tls,
            operation,
        )
        .await
    })
    .await
}

pub(super) async fn discover(
    store: &SecretStore,
    account: &str,
    endpoint: &ImapEndpointConfig,
) -> Result<ImapMailboxSnapshot, MailProbeError> {
    match connect(store, account, endpoint, ReadOperation::Mailboxes).await? {
        ReadReceipt::Mailboxes(receipt) => Ok(receipt),
        _ => Err(MailProbeError::Protocol),
    }
}

pub(super) async fn fetch_uid_observation(
    store: &SecretStore,
    request: &ImapUidObservationRequest,
    endpoint: &ImapEndpointConfig,
) -> Result<ImapUidObservation, ImapUidPageError> {
    match connect(
        store,
        request.account_id(),
        endpoint,
        ReadOperation::UidObservation(request),
    )
    .await?
    {
        ReadReceipt::UidObservation(observation) => observation,
        _ => Err(MailProbeError::Protocol.into()),
    }
}

#[cfg(test)]
pub(super) async fn run_uid_observation_connected<S>(
    source: Source<'_>,
    stream: S,
    tls: Arc<ClientConfig>,
    request: &ImapUidObservationRequest,
) -> Result<ImapUidObservation, ImapUidPageError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    match with_deadline(run_operation(
        source,
        stream,
        tls,
        ReadOperation::UidObservation(request),
    ))
    .await?
    {
        ReadReceipt::UidObservation(observation) => observation,
        _ => Err(MailProbeError::Protocol.into()),
    }
}

pub(super) async fn fetch_uid_page(
    store: &SecretStore,
    request: &ImapUidPageRequest,
    endpoint: &ImapEndpointConfig,
) -> Result<ImapUidPage, ImapUidPageError> {
    match connect(
        store,
        request.account_id(),
        endpoint,
        ReadOperation::UidPage(request),
    )
    .await?
    {
        ReadReceipt::UidPage(page) => page,
        _ => Err(MailProbeError::Protocol.into()),
    }
}

pub(super) async fn fetch_uid_reconcile_page(
    store: &SecretStore,
    request: &ImapUidReconcileRequest,
    endpoint: &ImapEndpointConfig,
) -> Result<ImapUidReconcilePage, ImapUidPageError> {
    match connect(
        store,
        request.account_id(),
        endpoint,
        ReadOperation::UidReconcile(request),
    )
    .await?
    {
        ReadReceipt::UidReconcile(page) => page,
        _ => Err(MailProbeError::Protocol.into()),
    }
}

pub(super) async fn fetch_uid_recovery_page(
    store: &SecretStore,
    request: &ImapUidRecoveryRequest,
    endpoint: &ImapEndpointConfig,
) -> Result<ImapUidRecoveryPage, ImapUidPageError> {
    match connect(
        store,
        request.account_id(),
        endpoint,
        ReadOperation::UidRecovery(request),
    )
    .await?
    {
        ReadReceipt::UidRecovery(page) => page,
        _ => Err(MailProbeError::Protocol.into()),
    }
}

#[cfg(test)]
pub(super) async fn run_uid_recovery_connected<S>(
    source: Source<'_>,
    stream: S,
    tls: Arc<ClientConfig>,
    request: &ImapUidRecoveryRequest,
) -> Result<ImapUidRecoveryPage, ImapUidPageError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    match with_deadline(run_operation(
        source,
        stream,
        tls,
        ReadOperation::UidRecovery(request),
    ))
    .await?
    {
        ReadReceipt::UidRecovery(page) => page,
        _ => Err(MailProbeError::Protocol.into()),
    }
}

#[cfg(test)]
pub(super) async fn run_uid_reconcile_connected<S>(
    source: Source<'_>,
    stream: S,
    tls: Arc<ClientConfig>,
    request: &ImapUidReconcileRequest,
) -> Result<ImapUidReconcilePage, ImapUidPageError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    match with_deadline(run_operation(
        source,
        stream,
        tls,
        ReadOperation::UidReconcile(request),
    ))
    .await?
    {
        ReadReceipt::UidReconcile(page) => page,
        _ => Err(MailProbeError::Protocol.into()),
    }
}

#[cfg(test)]
pub(super) async fn run_uid_connected<S>(
    source: Source<'_>,
    stream: S,
    tls: Arc<ClientConfig>,
    request: &ImapUidPageRequest,
) -> Result<ImapUidPage, ImapUidPageError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    match with_deadline(run_operation(
        source,
        stream,
        tls,
        ReadOperation::UidPage(request),
    ))
    .await?
    {
        ReadReceipt::UidPage(page) => page,
        _ => Err(MailProbeError::Protocol.into()),
    }
}

#[cfg(test)]
pub(super) async fn run_discovery_connected<S>(
    source: Source<'_>,
    stream: S,
    tls: Arc<ClientConfig>,
) -> Result<hantong_domain::mailboxes::ImapMailboxSnapshot, MailProbeError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    match run_operation(source, stream, tls, ReadOperation::Mailboxes).await? {
        ReadReceipt::Mailboxes(receipt) => Ok(receipt),
        _ => Err(MailProbeError::Protocol),
    }
}

// No public/raw command/session API. All command constructors below are fixed.
struct Protocol<S> {
    io: S,
    written: usize,
    remaining: usize,
}
impl<S: AsyncRead + AsyncWrite + Unpin> Protocol<S> {
    async fn byte(&mut self) -> Result<u8, MailProbeError> {
        if self.remaining == 0 {
            return Err(MailProbeError::Protocol);
        }
        let mut byte = [0];
        if self
            .io
            .read(&mut byte)
            .await
            .map_err(|_| MailProbeError::Protocol)?
            != 1
        {
            return Err(MailProbeError::Protocol);
        }
        self.remaining -= 1;
        Ok(byte[0])
    }
    async fn frame(
        &mut self,
        mode: GateMode,
        tag: Option<&str>,
        remaining: &mut usize,
    ) -> Result<ReplyKind, MailProbeError> {
        if *remaining == 0 {
            return Err(MailProbeError::Protocol);
        }
        *remaining -= 1;
        let mut gate = StrictResponseGate::new(mode, tag).map_err(|_| MailProbeError::Protocol)?;
        loop {
            let byte = [self.byte().await?];
            let parsed = gate.feed(&byte).map_err(|_| MailProbeError::Protocol)?;
            if parsed.consumed != 1 {
                return Err(MailProbeError::Protocol);
            }
            if let Some(frame) = parsed.frame {
                return Ok(frame.into_kind());
            }
        }
    }
    async fn write(&mut self, bytes: SensitiveBuffer) -> Result<(), MailProbeError> {
        if bytes.0.len() > 32 * 1024
            || bytes.0.len() > (64 * 1024usize).saturating_sub(self.written)
        {
            return Err(MailProbeError::Protocol);
        }
        self.written += bytes.0.len();
        self.io
            .write_all(bytes.as_slice())
            .await
            .map_err(|_| MailProbeError::Protocol)?;
        self.io.flush().await.map_err(|_| MailProbeError::Protocol)
    }
    async fn command(&mut self, tag: &str, body: CommandBody<'_>) -> Result<(), MailProbeError> {
        let command = Command {
            tag: tag.try_into().map_err(|_| MailProbeError::Protocol)?,
            body,
        };
        // All callers use fixed tags/bodies and bounded credential inputs. Codec's
        // internal base64 copies are not guaranteed to be wiped; this final one is.
        self.write(SensitiveBuffer(CommandCodec::new().encode(&command).dump()))
            .await
    }
    async fn finish(
        &mut self,
        mode: GateMode,
        tag: &str,
        remaining: &mut usize,
    ) -> Result<Vec<String>, MailProbeError> {
        let mut capabilities = None;
        let mut bye = false;
        loop {
            match self.frame(mode, Some(tag), remaining).await? {
                ReplyKind::Capability(values)
                    if mode == GateMode::Capability && capabilities.is_none() =>
                {
                    capabilities = Some(values)
                }
                ReplyKind::Capability(_) if mode == GateMode::Authentication => {}
                ReplyKind::UntaggedStatus(_) => {}
                ReplyKind::Bye if mode == GateMode::Logout && !bye => bye = true,
                ReplyKind::Tagged(StatusKind::Ok) => {
                    if (mode == GateMode::Capability && capabilities.is_none())
                        || (mode == GateMode::Logout && !bye)
                    {
                        return Err(MailProbeError::Protocol);
                    }
                    return Ok(capabilities.unwrap_or_default());
                }
                ReplyKind::Tagged(_) => {
                    return Err(match mode {
                        GateMode::StartTls => MailProbeError::Tls,
                        GateMode::Authentication => MailProbeError::Authentication,
                        _ => MailProbeError::Protocol,
                    });
                }
                _ => return Err(MailProbeError::Protocol),
            }
        }
    }
    async fn capabilities(&mut self, tag: &str) -> Result<Vec<String>, MailProbeError> {
        self.command(tag, CommandBody::Capability).await?;
        let caps = self.finish(GateMode::Capability, tag, &mut 32).await?;
        if !has(&caps, "IMAP4REV1") && !has(&caps, "IMAP4REV2") {
            return Err(MailProbeError::Protocol);
        }
        Ok(caps)
    }
    async fn mailboxes(
        &mut self,
        encoding: MailboxEncoding,
    ) -> Result<Vec<ImapMailboxDescriptor>, MailProbeError> {
        self.command(
            "h6",
            CommandBody::list("", "*").map_err(|_| MailProbeError::Protocol)?,
        )
        .await?;
        let mut mailboxes = Vec::new();
        for _ in 0..1056 {
            let mut gate =
                ListGate::with_limit("h6", self.remaining).map_err(|_| MailProbeError::Protocol)?;
            let reply = loop {
                let feed = gate
                    .feed(&[self.byte().await?])
                    .map_err(|_| MailProbeError::Protocol)?;
                if feed.consumed != 1 {
                    return Err(MailProbeError::Protocol);
                }
                if let Some(reply) = feed.reply {
                    break reply;
                }
            };
            match reply {
                ListReply::Mailbox(raw) => {
                    if mailboxes.len() == 1024 {
                        return Err(MailProbeError::Protocol);
                    }
                    mailboxes.push(descriptor(raw, encoding)?);
                }
                ListReply::Status(StatusKind::Ok) => {}
                ListReply::Status(StatusKind::No | StatusKind::Bad) => {
                    return Err(MailProbeError::Protocol);
                }
                ListReply::Tagged(StatusKind::Ok) => return Ok(mailboxes),
                ListReply::Tagged(_) => return Err(MailProbeError::Protocol),
            }
        }
        Err(MailProbeError::Protocol)
    }
    async fn challenge(
        &mut self,
        empty_required: bool,
        remaining: &mut usize,
    ) -> Result<(), MailProbeError> {
        loop {
            match self
                .frame(GateMode::Authentication, Some("h4"), remaining)
                .await?
            {
                ReplyKind::Continuation { empty } if empty || !empty_required => return Ok(()),
                ReplyKind::UntaggedStatus(_) | ReplyKind::Capability(_) => {}
                ReplyKind::Tagged(_) => return Err(MailProbeError::Authentication),
                _ => return Err(MailProbeError::Protocol),
            }
        }
    }
    async fn authenticate(
        &mut self,
        source: &Source<'_>,
        caps: &[String],
    ) -> Result<(), MailProbeError> {
        let mechanism = match source.endpoint.authentication() {
            Authentication::Oauth2 if has(caps, "AUTH=XOAUTH2") => AuthMechanism::XOAuth2,
            Authentication::AppPassword if has(caps, "AUTH=PLAIN") => AuthMechanism::Plain,
            Authentication::AppPassword if has(caps, "AUTH=LOGIN") => AuthMechanism::Login,
            _ => return Err(MailProbeError::MechanismUnsupported),
        };
        // Only called after hostname-verified TLS and fresh CAPABILITY.
        let secret = source.credential()?;
        let password = secret
            .text()
            .map_err(MailProbeError::Credential)?
            .as_bytes();
        let login = source.endpoint.login_name().as_bytes();
        if login.len() > 1280 || password.len() > 16384 {
            return Err(MailProbeError::Protocol);
        }
        let mut payload = SensitiveBuffer(Vec::with_capacity(login.len() + password.len() + 20));
        if mechanism == AuthMechanism::XOAuth2 {
            payload.0.extend_from_slice(b"user=");
            payload.0.extend_from_slice(login);
            payload.0.extend_from_slice(b"\x01auth=Bearer ");
            payload.0.extend_from_slice(password);
            payload.0.extend_from_slice(b"\x01\x01");
        } else if mechanism == AuthMechanism::Plain {
            payload.0.push(0);
            payload.0.extend_from_slice(login);
            payload.0.push(0);
            payload.0.extend_from_slice(password);
        }
        let initial = mechanism != AuthMechanism::Login && has(caps, "SASL-IR");
        self.command(
            "h4",
            CommandBody::Authenticate {
                mechanism: mechanism.clone(),
                initial_response: initial.then(|| Secret::new(Cow::Borrowed(payload.as_slice()))),
            },
        )
        .await?;
        let mut remaining = 32;
        if !initial {
            let responses: &[&[u8]] = if mechanism == AuthMechanism::Login {
                &[login, password]
            } else {
                &[payload.as_slice()]
            };
            for data in responses {
                self.challenge(mechanism != AuthMechanism::Login, &mut remaining)
                    .await?;
                self.write(SensitiveBuffer(
                    AuthenticateDataCodec::new()
                        .encode(&AuthenticateData::r#continue(*data))
                        .dump(),
                ))
                .await?;
            }
        }
        self.finish(GateMode::Authentication, "h4", &mut remaining)
            .await?;
        Ok(())
    }
}
fn descriptor(
    raw: RawMailbox,
    encoding: MailboxEncoding,
) -> Result<ImapMailboxDescriptor, MailProbeError> {
    let has = |name: &str| raw.attributes.iter().any(|attr| attr == name);
    let children = match (has("haschildren"), has("hasnochildren")) {
        (true, false) => MailboxChildren::HasChildren,
        (false, true) => MailboxChildren::HasNoChildren,
        _ => MailboxChildren::Unknown,
    };
    let roles = [
        ("all", SpecialUseRole::All),
        ("archive", SpecialUseRole::Archive),
        ("drafts", SpecialUseRole::Drafts),
        ("flagged", SpecialUseRole::Flagged),
        ("junk", SpecialUseRole::Junk),
        ("sent", SpecialUseRole::Sent),
        ("trash", SpecialUseRole::Trash),
    ]
    .into_iter()
    .filter(|(name, _)| has(name))
    .map(|(_, role)| role)
    .collect();
    ImapMailboxDescriptor::new(
        ImapMailboxName::from_wire(&raw.name, encoding).map_err(|_| MailProbeError::Protocol)?,
        raw.delimiter,
        has("noselect"),
        has("nonexistent"),
        children,
        roles,
    )
    .map_err(|_| MailProbeError::Protocol)
}
fn has(caps: &[String], name: &str) -> bool {
    caps.iter().any(|s| s == name)
}
fn snapshot(caps: &[String]) -> ImapCapabilities {
    use hantong_domain::capabilities::ImapFeature;
    let rev2 = has(caps, "IMAP4REV2") && !has(caps, "IMAP4REV1");
    ImapCapabilities::from_features(
        [
            (ImapFeature::Idle, rev2 || has(caps, "IDLE")),
            (ImapFeature::Move, rev2 || has(caps, "MOVE")),
            (ImapFeature::Uidplus, rev2 || has(caps, "UIDPLUS")),
            (ImapFeature::Condstore, has(caps, "CONDSTORE")),
            (ImapFeature::Qresync, has(caps, "QRESYNC")),
            (ImapFeature::SpecialUse, has(caps, "SPECIAL-USE")),
        ]
        .into_iter()
        .filter_map(|(feature, enabled)| enabled.then_some(feature)),
    )
}
#[cfg(test)]
pub(super) async fn run_connected<S>(
    source: Source<'_>,
    stream: S,
    tls: Arc<ClientConfig>,
) -> Result<ImapProbeReceipt, MailProbeError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    match run_operation(source, stream, tls, ReadOperation::Probe).await? {
        ReadReceipt::Probe(receipt) => Ok(receipt),
        _ => Err(MailProbeError::Protocol),
    }
}
async fn run_operation<S>(
    source: Source<'_>,
    stream: S,
    tls: Arc<ClientConfig>,
    operation: ReadOperation<'_>,
) -> Result<ReadReceipt, MailProbeError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    validate_account(source.account).map_err(|_| MailProbeError::InvalidAccount)?;
    if let ReadOperation::UidObservation(request) = operation
        && (source.account != request.account_id()
            || source.endpoint.settings_version() != request.endpoint_version())
    {
        return Ok(ReadReceipt::UidObservation(Err(
            ImapUidPageError::CursorInvalid,
        )));
    }
    if let ReadOperation::UidPage(request) = operation
        && (source.account != request.account_id()
            || source.endpoint.settings_version() != request.endpoint_version())
    {
        return Ok(ReadReceipt::UidPage(Err(ImapUidPageError::CursorInvalid)));
    }
    if let ReadOperation::UidReconcile(request) = operation
        && (source.account != request.account_id()
            || source.endpoint.settings_version() != request.endpoint_version())
    {
        return Ok(ReadReceipt::UidReconcile(Err(
            ImapUidPageError::CursorInvalid,
        )));
    }
    if let ReadOperation::UidRecovery(request) = operation
        && (source.account != request.account_id()
            || source.endpoint.settings_version() != request.endpoint_version())
    {
        return Ok(ReadReceipt::UidRecovery(Err(
            ImapUidPageError::CursorInvalid,
        )));
    }
    let (wire_limit, plaintext_limit) = match operation {
        ReadOperation::Probe => (256 * 1024, 256 * 1024),
        ReadOperation::Mailboxes => (4 * 1024 * 1024, 2 * 1024 * 1024),
        ReadOperation::UidObservation(_)
        | ReadOperation::UidPage(_)
        | ReadOperation::UidReconcile(_)
        | ReadOperation::UidRecovery(_) => (16 * 1024 * 1024, 8 * 1024 * 1024),
    };
    // Prefetch remains BELOW TLS, including bytes following STARTTLS OK.
    let mut plain = Protocol {
        io: BufReader::new(BoundedAsyncStream::with_limit(stream, wire_limit)),
        written: 0,
        remaining: wire_limit,
    };
    if source.endpoint.tls() == TlsMode::StarttlsRequired {
        if !matches!(
            plain.frame(GateMode::Greeting, None, &mut 1).await?,
            ReplyKind::GreetingOk
        ) {
            return Err(MailProbeError::Protocol);
        }
        if !has(&plain.capabilities("h1").await?, "STARTTLS") {
            return Err(MailProbeError::Tls);
        }
        plain.command("h2", CommandBody::StartTLS).await?;
        plain.finish(GateMode::StartTls, "h2", &mut 32).await?;
    }
    let plain_consumed = wire_limit - plain.remaining;
    let secure = TlsConnector::from(tls)
        .connect(
            ServerName::try_from(source.endpoint.host().to_owned())
                .map_err(|_| MailProbeError::Tls)?,
            plain.io,
        )
        .await
        .map_err(|_| MailProbeError::Tls)?;
    let mut verified = Protocol {
        io: BufReader::new(BoundedAsyncStream::with_limit(secure, plaintext_limit)),
        written: plain.written,
        remaining: if matches!(
            operation,
            ReadOperation::UidObservation(_)
                | ReadOperation::UidPage(_)
                | ReadOperation::UidReconcile(_)
                | ReadOperation::UidRecovery(_)
        ) {
            plaintext_limit.saturating_sub(plain_consumed)
        } else {
            plaintext_limit
        },
    };
    if source.endpoint.tls() == TlsMode::ImplicitTls
        && !matches!(
            verified.frame(GateMode::Greeting, None, &mut 1).await?,
            ReplyKind::GreetingOk
        )
    {
        return Err(MailProbeError::Protocol);
    }
    let caps = verified.capabilities("h3").await?;
    verified.authenticate(&source, &caps).await?;
    let caps = verified.capabilities("h5").await?;
    let capabilities = snapshot(&caps);
    let (receipt, logout_tag) = match operation {
        ReadOperation::Probe => (
            ReadReceipt::Probe(ImapProbeReceipt {
                settings_version: source.endpoint.settings_version(),
                capabilities,
            }),
            "h6",
        ),
        ReadOperation::Mailboxes => {
            let encoding = if has(&caps, "IMAP4REV2") && !has(&caps, "IMAP4REV1") {
                MailboxEncoding::Utf8
            } else {
                MailboxEncoding::ModifiedUtf7
            };
            let mailboxes = verified.mailboxes(encoding).await?;
            let snapshot = ImapMailboxSnapshot::new(
                source.account,
                source.endpoint.settings_version(),
                capabilities,
                mailboxes,
            )
            .map_err(|_| MailProbeError::Protocol)?;
            (ReadReceipt::Mailboxes(snapshot), "h7")
        }
        ReadOperation::UidObservation(request) => {
            let encoding = if has(&caps, "IMAP4REV2") && !has(&caps, "IMAP4REV1") {
                MailboxEncoding::Utf8
            } else {
                MailboxEncoding::ModifiedUtf7
            };
            let observation = verified.uid_observation(request, encoding).await;
            if observation.is_err() {
                return Ok(ReadReceipt::UidObservation(observation));
            }
            (ReadReceipt::UidObservation(observation), "h7")
        }
        ReadOperation::UidPage(request) => {
            let encoding = if has(&caps, "IMAP4REV2") && !has(&caps, "IMAP4REV1") {
                MailboxEncoding::Utf8
            } else {
                MailboxEncoding::ModifiedUtf7
            };
            let page = verified.uid_page(request, encoding).await;
            if page.is_err() {
                // Any failed exchange drops the connection; never retry or
                // return metadata collected before the failure.
                return Ok(ReadReceipt::UidPage(page));
            }
            (ReadReceipt::UidPage(page), "h9")
        }
        ReadOperation::UidReconcile(request) => {
            let encoding = if has(&caps, "IMAP4REV2") && !has(&caps, "IMAP4REV1") {
                MailboxEncoding::Utf8
            } else {
                MailboxEncoding::ModifiedUtf7
            };
            let page = verified.uid_reconcile_page(request, encoding).await;
            if page.is_err() {
                return Ok(ReadReceipt::UidReconcile(page));
            }
            (ReadReceipt::UidReconcile(page), "h9")
        }
        ReadOperation::UidRecovery(request) => {
            let encoding = if has(&caps, "IMAP4REV2") && !has(&caps, "IMAP4REV1") {
                MailboxEncoding::Utf8
            } else {
                MailboxEncoding::ModifiedUtf7
            };
            let page = verified.uid_recovery_page(request, encoding).await;
            if page.is_err() {
                return Ok(ReadReceipt::UidRecovery(page));
            }
            (ReadReceipt::UidRecovery(page), "h9")
        }
    };
    verified.command(logout_tag, CommandBody::Logout).await?;
    verified
        .finish(GateMode::Logout, logout_tag, &mut 32)
        .await?;
    Ok(receipt)
}
