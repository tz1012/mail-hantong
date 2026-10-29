//! One-shot IMAP diagnostics. No mailbox access or public session escape.
use crate::{MailProbeError, SecretStore, validate_account};
use hantong_domain::mail_connection::ImapEndpointConfig;

#[cfg(windows)]
mod gate;
#[cfg(all(windows, test))]
mod tests;
#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImapCapabilities {
    idle: bool,
    move_messages: bool,
    uidplus: bool,
    condstore: bool,
    qresync: bool,
    special_use: bool,
}
impl ImapCapabilities {
    pub fn idle(&self) -> bool {
        self.idle
    }
    pub fn move_messages(&self) -> bool {
        self.move_messages
    }
    pub fn uidplus(&self) -> bool {
        self.uidplus
    }
    pub fn condstore(&self) -> bool {
        self.condstore
    }
    pub fn qresync(&self) -> bool {
        self.qresync
    }
    pub fn special_use(&self) -> bool {
        self.special_use
    }
}

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
