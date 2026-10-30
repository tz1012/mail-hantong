use super::*;
use crate::smtp_probe::windows::{TlsSettings, verified_connection};
use hantong_domain::mail_connection::Authentication;
use lettre::address::Address;
use lettre::transport::smtp::{
    authentication::{Credentials, Mechanism},
    commands::{Data, Mail, Rcpt},
};
use std::{net::SocketAddr, str::FromStr, time::Duration};
use tokio::{
    net::{TcpStream, lookup_host},
    time::timeout,
};

pub(super) async fn send(
    store: &SecretStore,
    account: &str,
    endpoint: &hantong_domain::mail_connection::SmtpEndpointConfig,
    envelope: &SmtpSendEnvelope,
) -> Result<SmtpSendResult, SmtpSendError> {
    with_deadline(async {
        let tls = TlsSettings::platform(endpoint.host()).map_err(|_| SmtpSendError::Tls)?;
        let mut stream = None;
        for address in lookup_host((endpoint.host(), endpoint.port()))
            .await
            .map_err(|_| SmtpSendError::Connection)?
            .take(8)
        {
            if let Ok(Ok(connected)) =
                timeout(Duration::from_secs(8), TcpStream::connect(address)).await
            {
                stream = Some(connected);
                break;
            }
        }
        let stream = stream.ok_or(SmtpSendError::Connection)?;
        stream
            .set_nodelay(true)
            .map_err(|_| SmtpSendError::Connection)?;
        let peer = stream.peer_addr().map_err(|_| SmtpSendError::Connection)?;
        submit(store, account, endpoint, envelope, stream, peer, tls).await
    })
    .await
}

async fn with_deadline<T>(
    future: impl std::future::Future<Output = Result<T, SmtpSendError>>,
) -> Result<T, SmtpSendError> {
    timeout(Duration::from_secs(30), future)
        .await
        // A deadline can expire after DATA has reached the server. Treat it
        // as uncertain rather than allowing a caller to duplicate mail.
        .map_err(|_| SmtpSendError::Uncertain)?
}

async fn submit<S>(
    store: &SecretStore,
    account: &str,
    endpoint: &hantong_domain::mail_connection::SmtpEndpointConfig,
    envelope: &SmtpSendEnvelope,
    stream: S,
    peer: SocketAddr,
    tls: TlsSettings,
) -> Result<SmtpSendResult, SmtpSendError>
where
    S: tokio::io::AsyncRead
        + tokio::io::AsyncWrite
        + Send
        + Sync
        + Unpin
        + std::fmt::Debug
        + 'static,
{
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| SmtpSendError::Protocol)?;
    let message = encode_message(
        endpoint.login_name(),
        envelope,
        now.as_secs(),
        now.subsec_nanos(),
    )?;
    let crate::smtp_probe::windows::VerifiedSmtpConnection(mut connection) =
        verified_connection(endpoint, stream, peer, tls)
            .await
            .map_err(|e| match e {
                crate::smtp_probe::MailProbeError::Tls => SmtpSendError::Tls,
                crate::smtp_probe::MailProbeError::Protocol => SmtpSendError::Protocol,
                _ => SmtpSendError::Connection,
            })?;
    let mechanism = match endpoint.authentication() {
        Authentication::AppPassword => [Mechanism::Plain, Mechanism::Login].as_slice(),
        Authentication::Oauth2 => [Mechanism::Xoauth2].as_slice(),
    };
    let secret = store
        .unseal_smtp_credential(account, endpoint)
        .map_err(SmtpSendError::Credential)?;
    let credentials = Credentials::new(
        endpoint.login_name().to_owned(),
        secret.text().map_err(SmtpSendError::Credential)?.to_owned(),
    );
    let auth = connection
        .auth(mechanism, &credentials)
        .await
        .map_err(|_| SmtpSendError::Authentication)?;
    if !auth.has_code(235) {
        return Err(SmtpSendError::Authentication);
    }
    let sender =
        Address::from_str(endpoint.login_name()).map_err(|_| SmtpSendError::InvalidRecipient)?;
    let mail = match connection.command(Mail::new(Some(sender), vec![])).await {
        Ok(reply) => reply,
        Err(error) if error.status().is_some() => return Ok(SmtpSendResult::Rejected),
        Err(_) => return Err(SmtpSendError::Uncertain),
    };
    if !mail.has_code(250) {
        return Ok(SmtpSendResult::Rejected);
    }
    for recipient in &envelope.recipients {
        let address = Address::from_str(recipient).map_err(|_| SmtpSendError::InvalidRecipient)?;
        let reply = match connection.command(Rcpt::new(address, vec![])).await {
            Ok(reply) => reply,
            Err(error) if error.status().is_some() => return Ok(SmtpSendResult::Rejected),
            Err(_) => return Err(SmtpSendError::Uncertain),
        };
        if !reply.has_code(250) && !reply.has_code(251) {
            return Ok(SmtpSendResult::Rejected);
        }
    }
    let data = match connection.command(Data).await {
        Ok(reply) => reply,
        Err(error) if error.status().is_some() => return Ok(SmtpSendResult::Rejected),
        Err(_) => return Err(SmtpSendError::Uncertain),
    };
    if !data.has_code(354) {
        return Ok(SmtpSendResult::Rejected);
    }
    let accepted = match connection.message(message.as_slice()).await {
        Ok(reply) => reply,
        Err(error) if error.status().is_some() => return Ok(SmtpSendResult::Rejected),
        Err(_) => return Err(SmtpSendError::Uncertain),
    };
    if accepted.has_code(250) {
        Ok(SmtpSendResult::Accepted)
    } else {
        Ok(SmtpSendResult::Rejected)
    }
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
