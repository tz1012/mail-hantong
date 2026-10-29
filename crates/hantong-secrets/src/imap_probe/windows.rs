use super::{
    gate::{GateMode, ReplyKind, StrictResponseGate},
    *,
};
use crate::{SensitiveBuffer, mail_transport::BoundedAsyncStream};
use hantong_domain::mail_connection::{Authentication, TlsMode};
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
        run_connected(
            Source {
                store,
                account,
                endpoint,
                #[cfg(test)]
                accesses: None,
            },
            stream,
            tls,
        )
        .await
    })
    .await
}

// No public/raw command/session API. All command constructors below are fixed.
struct Protocol<S> {
    io: S,
    written: usize,
}
impl<S: AsyncRead + AsyncWrite + Unpin> Protocol<S> {
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
            let mut byte = [0];
            if self
                .io
                .read(&mut byte)
                .await
                .map_err(|_| MailProbeError::Protocol)?
                == 0
            {
                return Err(MailProbeError::Protocol);
            }
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
fn has(caps: &[String], name: &str) -> bool {
    caps.iter().any(|s| s == name)
}
fn snapshot(caps: &[String]) -> ImapCapabilities {
    let rev2 = has(caps, "IMAP4REV2") && !has(caps, "IMAP4REV1");
    ImapCapabilities {
        idle: rev2 || has(caps, "IDLE"),
        move_messages: rev2 || has(caps, "MOVE"),
        uidplus: rev2 || has(caps, "UIDPLUS"),
        condstore: has(caps, "CONDSTORE"),
        qresync: has(caps, "QRESYNC"),
        special_use: has(caps, "SPECIAL-USE"),
    }
}
pub(super) async fn run_connected<S>(
    source: Source<'_>,
    stream: S,
    tls: Arc<ClientConfig>,
) -> Result<ImapProbeReceipt, MailProbeError>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    validate_account(source.account).map_err(|_| MailProbeError::InvalidAccount)?;
    // Prefetch remains BELOW TLS, including bytes following STARTTLS OK.
    let mut plain = Protocol {
        io: BufReader::new(BoundedAsyncStream::new(stream)),
        written: 0,
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
    let secure = TlsConnector::from(tls)
        .connect(
            ServerName::try_from(source.endpoint.host().to_owned())
                .map_err(|_| MailProbeError::Tls)?,
            plain.io,
        )
        .await
        .map_err(|_| MailProbeError::Tls)?;
    let mut verified = Protocol {
        io: BufReader::new(BoundedAsyncStream::new(secure)),
        written: plain.written,
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
    let capabilities = snapshot(&verified.capabilities("h5").await?);
    verified.command("h6", CommandBody::Logout).await?;
    verified.finish(GateMode::Logout, "h6", &mut 32).await?;
    Ok(ImapProbeReceipt {
        settings_version: source.endpoint.settings_version(),
        capabilities,
    })
}
