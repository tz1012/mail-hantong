//! One-shot SMTP diagnostics; no mail submission or public transport access.
use crate::{SecretError, SecretStore, validate_account};
use hantong_domain::mail_connection::SmtpEndpointConfig;
use std::fmt;

#[cfg(all(windows, test))]
mod tests;
#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailProbeError {
    InvalidAccount,
    UnsupportedPlatform,
    Connection,
    Tls,
    MechanismUnsupported,
    Authentication,
    Protocol,
    Timeout,
    Credential(SecretError),
}
impl fmt::Display for MailProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidAccount => "invalid mail account",
            Self::UnsupportedPlatform => "mail probe unsupported on this platform",
            Self::Connection => "mail connection failed",
            Self::Tls => "mail TLS verification failed",
            Self::MechanismUnsupported => "mail authentication mechanism unsupported",
            Self::Authentication => "mail authentication failed",
            Self::Protocol => "mail protocol failed",
            Self::Timeout => "mail probe timed out",
            Self::Credential(_) => "mail credential unavailable for these settings",
        })
    }
}
impl std::error::Error for MailProbeError {}

#[derive(Debug, Eq, PartialEq)]
pub struct SmtpProbeReceipt {
    settings_version: u64,
}
impl SmtpProbeReceipt {
    pub fn settings_version(&self) -> u64 {
        self.settings_version
    }
}

impl SecretStore {
    /// Checks TLS and authentication once. Never submits a message or grants authority.
    pub async fn probe_smtp(
        &self,
        account: &str,
        endpoint: &SmtpEndpointConfig,
    ) -> Result<SmtpProbeReceipt, MailProbeError> {
        validate_account(account).map_err(|_| MailProbeError::InvalidAccount)?;
        #[cfg(windows)]
        {
            windows::probe(self, account, endpoint).await
        }
        #[cfg(not(windows))]
        {
            let _ = endpoint;
            Err(MailProbeError::UnsupportedPlatform)
        }
    }
}
