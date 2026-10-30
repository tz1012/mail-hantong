//! Bounded IMAP diagnostics and folder discovery without a public session escape.
use crate::{MailProbeError, SecretStore, validate_account};
pub use hantong_domain::capabilities::ImapCapabilities;
use hantong_domain::imap_observation::{ImapUidObservation, ImapUidObservationRequest};
use hantong_domain::imap_reconcile::{ImapUidReconcilePage, ImapUidReconcileRequest};
use hantong_domain::imap_recovery::{ImapUidRecoveryPage, ImapUidRecoveryRequest};
use hantong_domain::imap_sync::{ImapUidPage, ImapUidPageRequest};
use hantong_domain::mail_connection::ImapEndpointConfig;

#[cfg(windows)]
mod gate;
#[cfg(windows)]
mod list_gate;
#[cfg(all(windows, test))]
mod tests;
#[cfg(windows)]
mod uid_page_gate;
#[cfg(windows)]
mod windows;

#[derive(Debug, Eq, PartialEq)]
pub struct ImapProbeReceipt {
    settings_version: u64,
    capabilities: ImapCapabilities,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapUidPageError {
    Probe(MailProbeError),
    UidValidityChanged,
    CursorInvalid,
}
impl std::fmt::Display for ImapUidPageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Probe(_) => "IMAP UID page probe failed",
            Self::UidValidityChanged => "IMAP UID validity changed",
            Self::CursorInvalid => "invalid IMAP UID page cursor",
        })
    }
}
impl std::error::Error for ImapUidPageError {}
impl From<MailProbeError> for ImapUidPageError {
    fn from(error: MailProbeError) -> Self {
        Self::Probe(error)
    }
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
    /// Observes UIDVALIDITY/UIDNEXT through one bounded, read-only EXAMINE.
    /// No messages are fetched and no provider session leaves Secrets.
    pub async fn fetch_imap_uid_observation(
        &self,
        request: &ImapUidObservationRequest,
        endpoint: &ImapEndpointConfig,
    ) -> Result<ImapUidObservation, ImapUidPageError> {
        if request.endpoint_version() != endpoint.settings_version() {
            return Err(ImapUidPageError::CursorInvalid);
        }
        validate_account(request.account_id())
            .map_err(|_| ImapUidPageError::Probe(MailProbeError::InvalidAccount))?;
        #[cfg(windows)]
        {
            windows::fetch_uid_observation(self, request, endpoint).await
        }
        #[cfg(not(windows))]
        {
            let _ = endpoint;
            Err(ImapUidPageError::Probe(MailProbeError::UnsupportedPlatform))
        }
    }

    /// Reads only the first metadata page after an explicitly expected epoch change.
    pub async fn fetch_imap_uid_recovery_page(
        &self,
        request: &ImapUidRecoveryRequest,
        endpoint: &ImapEndpointConfig,
    ) -> Result<ImapUidRecoveryPage, ImapUidPageError> {
        if request.endpoint_version() != endpoint.settings_version() {
            return Err(ImapUidPageError::CursorInvalid);
        }
        validate_account(request.account_id())
            .map_err(|_| ImapUidPageError::Probe(MailProbeError::InvalidAccount))?;
        #[cfg(windows)]
        {
            windows::fetch_uid_recovery_page(self, request, endpoint).await
        }
        #[cfg(not(windows))]
        {
            let _ = endpoint;
            Err(ImapUidPageError::Probe(MailProbeError::UnsupportedPlatform))
        }
    }

    /// Observes one already stored, fixed UID range through Secrets-owned TLS.
    pub async fn fetch_imap_uid_reconcile_page(
        &self,
        request: &ImapUidReconcileRequest,
        endpoint: &ImapEndpointConfig,
    ) -> Result<ImapUidReconcilePage, ImapUidPageError> {
        if request.endpoint_version() != endpoint.settings_version() {
            return Err(ImapUidPageError::CursorInvalid);
        }
        validate_account(request.account_id())
            .map_err(|_| ImapUidPageError::Probe(MailProbeError::InvalidAccount))?;
        #[cfg(windows)]
        {
            windows::fetch_uid_reconcile_page(self, request, endpoint).await
        }
        #[cfg(not(windows))]
        {
            let _ = endpoint;
            Err(ImapUidPageError::Probe(MailProbeError::UnsupportedPlatform))
        }
    }

    /// Fetches one bounded, read-only UID metadata page through the private
    /// Secrets transport. This method never exposes an IMAP session.
    pub async fn fetch_imap_uid_page(
        &self,
        request: &ImapUidPageRequest,
        endpoint: &ImapEndpointConfig,
    ) -> Result<ImapUidPage, ImapUidPageError> {
        if request.endpoint_version() != endpoint.settings_version() {
            return Err(ImapUidPageError::CursorInvalid);
        }
        validate_account(request.account_id())
            .map_err(|_| ImapUidPageError::Probe(MailProbeError::InvalidAccount))?;
        #[cfg(windows)]
        {
            windows::fetch_uid_page(self, request, endpoint).await
        }
        #[cfg(not(windows))]
        {
            let _ = endpoint;
            Err(ImapUidPageError::Probe(MailProbeError::UnsupportedPlatform))
        }
    }

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
