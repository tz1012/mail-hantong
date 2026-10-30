use super::*;
use crate::mail_transport::{BoundedAsyncStream, LettreTokioStream, StarttlsReadGate};
use hantong_domain::{
    mail_connection::{Authentication, TlsMode},
    smtp_capabilities::{SmtpAuthMechanism, SmtpCapabilities, SmtpFeature},
};
use lettre::transport::smtp::{
    authentication::{Credentials, Mechanism},
    client::{AsyncSmtpConnection, TlsParameters},
    commands::Quit,
    extension::{ClientId, Extension, ServerInfo},
};
use rustls::{ClientConfig, pki_types::ServerName};
use rustls_platform_verifier::BuilderVerifierExt;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpStream, lookup_host},
    time::timeout,
};
use tokio_rustls::TlsConnector;

pub(crate) struct TlsSettings {
    pub(crate) implicit: Arc<ClientConfig>,
    pub(crate) starttls: TlsParameters,
}
impl TlsSettings {
    pub(crate) fn platform(host: &str) -> Result<Self, MailProbeError> {
        let implicit = ClientConfig::builder()
            .with_platform_verifier()
            .map_err(|_| MailProbeError::Tls)?
            .with_no_client_auth();
        let starttls = TlsParameters::builder(host.to_owned())
            .build_rustls()
            .map_err(|_| MailProbeError::Tls)?;
        Ok(Self {
            implicit: Arc::new(implicit),
            starttls,
        })
    }
}

pub(super) struct Source<'a> {
    pub(super) store: &'a SecretStore,
    pub(super) account: &'a str,
    pub(super) endpoint: &'a SmtpEndpointConfig,
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
            .unseal_smtp_credential(self.account, self.endpoint)
            .map_err(MailProbeError::Credential)
    }
}

pub(super) async fn probe(
    store: &SecretStore,
    account: &str,
    endpoint: &SmtpEndpointConfig,
) -> Result<SmtpProbeReceipt, MailProbeError> {
    with_deadline(async {
        let tls = TlsSettings::platform(endpoint.host())?;
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
        let peer = stream.peer_addr().map_err(|_| MailProbeError::Connection)?;
        run_connected(
            Source {
                store,
                account,
                endpoint,
                #[cfg(test)]
                accesses: None,
            },
            stream,
            peer,
            tls,
        )
        .await
    })
    .await
}

pub(super) async fn with_deadline<T>(
    future: impl std::future::Future<Output = Result<T, MailProbeError>>,
) -> Result<T, MailProbeError> {
    timeout(Duration::from_secs(30), future)
        .await
        .map_err(|_| MailProbeError::Timeout)?
}

// Constructible only by verified_connection after the selected TLS flow succeeds.
// Lettre's injected implicit TLS stream is tagged plain internally; the completed
// Rustls handshake here, rather than Lettre's state query, is the TLS evidence.
pub(crate) struct VerifiedSmtpConnection(pub(crate) AsyncSmtpConnection);
pub(crate) async fn verified_connection<S>(
    endpoint: &SmtpEndpointConfig,
    stream: S,
    peer: SocketAddr,
    tls: TlsSettings,
) -> Result<VerifiedSmtpConnection, MailProbeError>
where
    S: AsyncRead + AsyncWrite + Send + Sync + Unpin + fmt::Debug + 'static,
{
    let hello = ClientId::Domain("mail-hantong.invalid".to_owned());
    let connection = match endpoint.tls() {
        TlsMode::ImplicitTls => {
            let server_name = ServerName::try_from(endpoint.host().to_owned())
                .map_err(|_| MailProbeError::Tls)?;
            let secure = TlsConnector::from(tls.implicit)
                .connect(server_name, BoundedAsyncStream::new(stream))
                .await
                .map_err(|_| MailProbeError::Tls)?;
            // Both ciphertext (including handshake) and plaintext are bounded.
            AsyncSmtpConnection::connect_with_transport(
                Box::new(LettreTokioStream::new(secure, peer)),
                &hello,
            )
            .await
            .map_err(|_| MailProbeError::Protocol)?
        }
        TlsMode::StarttlsRequired => {
            let (stream, upgraded) = StarttlsReadGate::new(stream);
            let mut connection = AsyncSmtpConnection::connect_with_transport(
                Box::new(LettreTokioStream::new(stream, peer)),
                &hello,
            )
            .await
            .map_err(|_| MailProbeError::Protocol)?;
            if !connection.can_starttls() {
                return Err(MailProbeError::Tls);
            }
            connection
                .starttls(tls.starttls, &hello)
                .await
                .map_err(|_| MailProbeError::Tls)?;
            if !connection.is_encrypted() {
                return Err(MailProbeError::Tls);
            }
            upgraded.store(true, std::sync::atomic::Ordering::Release);
            connection
        }
    };
    Ok(VerifiedSmtpConnection(connection))
}

pub(super) async fn run_connected<S>(
    source: Source<'_>,
    stream: S,
    peer: SocketAddr,
    tls: TlsSettings,
) -> Result<SmtpProbeReceipt, MailProbeError>
where
    S: AsyncRead + AsyncWrite + Send + Sync + Unpin + fmt::Debug + 'static,
{
    validate_account(source.account).map_err(|_| MailProbeError::InvalidAccount)?;
    let VerifiedSmtpConnection(mut connection) =
        verified_connection(source.endpoint, stream, peer, tls).await?;
    let capabilities = map_capabilities(connection.server_info());
    let candidates: &[Mechanism] = match source.endpoint.authentication() {
        Authentication::AppPassword => &[Mechanism::Plain, Mechanism::Login],
        Authentication::Oauth2 => &[Mechanism::Xoauth2],
    };
    let mechanism = candidates
        .iter()
        .copied()
        .find(|m| connection.server_info().supports_auth_mechanism(*m))
        .ok_or(MailProbeError::MechanismUnsupported)?;
    let authenticated_mechanism = match mechanism {
        Mechanism::Plain => SmtpAuthMechanism::Plain,
        Mechanism::Login => SmtpAuthMechanism::Login,
        Mechanism::Xoauth2 => SmtpAuthMechanism::Xoauth2,
    };
    let auth = {
        let secret = source.credential()?;
        let credentials = Credentials::new(
            source.endpoint.login_name().to_owned(),
            secret
                .text()
                .map_err(MailProbeError::Credential)?
                .to_owned(),
        );
        connection
            .auth(&[mechanism], &credentials)
            .await
            .map_err(|_| MailProbeError::Authentication)?
    };
    if !auth.has_code(235) {
        return Err(MailProbeError::Authentication);
    }
    // Typed QUIT avoids lettre::quit's error recovery issuing a second QUIT.
    let reply = connection
        .command(Quit)
        .await
        .map_err(|_| MailProbeError::Protocol)?;
    if !reply.has_code(221) {
        return Err(MailProbeError::Protocol);
    }
    Ok(SmtpProbeReceipt {
        settings_version: source.endpoint.settings_version(),
        capabilities,
        authenticated_mechanism,
    })
}

fn map_capabilities(info: &ServerInfo) -> SmtpCapabilities {
    let mut features = Vec::new();
    if info.supports_feature(Extension::EightBitMime) {
        features.push(SmtpFeature::EightBitMime);
    }
    if info.supports_auth_mechanism(Mechanism::Login) {
        features.push(SmtpFeature::AuthLogin);
    }
    if info.supports_auth_mechanism(Mechanism::Plain) {
        features.push(SmtpFeature::AuthPlain);
    }
    if info.supports_auth_mechanism(Mechanism::Xoauth2) {
        features.push(SmtpFeature::AuthXoauth2);
    }
    if info.supports_feature(Extension::SmtpUtfEight) {
        features.push(SmtpFeature::SmtpUtf8);
    }
    SmtpCapabilities::from_features(features)
}
