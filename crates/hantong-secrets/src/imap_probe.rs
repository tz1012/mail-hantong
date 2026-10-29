//! Bounded IMAP diagnostics and folder discovery without a public session escape.
use crate::{MailProbeError, SecretStore, validate_account};
pub use hantong_domain::capabilities::ImapCapabilities;
use hantong_domain::mail_connection::ImapEndpointConfig;

#[cfg(windows)]
mod gate;
#[cfg(windows)]
mod list_gate;
#[cfg(all(windows, test))]
mod tests;
#[cfg(windows)]
mod windows;

#[derive(Debug, Eq, PartialEq)]
pub struct ImapProbeReceipt {
    settings_version: u64,
    capabilities: ImapCapabilities,
}
impl ImapProbeReceipt {
    pub fn settings_version(&self) -> u64 {
        self.settings_version
    }
    pub fn capabilities(&self) -> &ImapCapabilities {
        &self.capabilities
    }
}
impl SecretStore {
    /// Returns a complete bounded folder snapshot, without exposing a session.
    pub async fn discover_imap_mailboxes(
        &self,
        account: &str,
        endpoint: &ImapEndpointConfig,
    ) -> Result<hantong_domain::mailboxes::ImapMailboxSnapshot, MailProbeError> {
        validate_account(account).map_err(|_| MailProbeError::InvalidAccount)?;
        #[cfg(windows)]
        {
            windows::discover(self, account, endpoint).await
        }
        #[cfg(not(windows))]
        {
            let _ = endpoint;
            Err(MailProbeError::UnsupportedPlatform)
        }
    }
    /// Verifies TLS, authentication and bounded capabilities once. Grants no authority.
    pub async fn probe_imap(
        &self,
        account: &str,
        endpoint: &ImapEndpointConfig,
    ) -> Result<ImapProbeReceipt, MailProbeError> {
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
