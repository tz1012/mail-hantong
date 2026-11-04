//! Secrets-owned single-UID IMAP MOVE transport.

use crate::{MailProbeError, SecretStore, validate_account};
use hantong_contracts::{imap_trash::ImapTrashRequest, uid_page::UidMailboxIdentity};
use hantong_domain::mail_connection::ImapEndpointConfig;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapTrashOutcome {
    Accepted,
    Rejected,
    Unknown,
}

impl SecretStore {
    /// Moves exactly one UID with an authenticated, Secrets-owned IMAP session.
    /// The session never escapes this method and never performs COPY, STORE,
    /// EXPUNGE, CLOSE, or mailbox creation.
    pub async fn move_imap_to_trash(
        &self,
        account: &str,
        endpoint: &ImapEndpointConfig,
        request: &ImapTrashRequest,
        destination: &UidMailboxIdentity,
        cancel: &AtomicBool,
    ) -> Result<ImapTrashOutcome, MailProbeError> {
        validate_account(account).map_err(|_| MailProbeError::InvalidAccount)?;
        if request.account_id() != account || cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(ImapTrashOutcome::Rejected);
        }
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            crate::imap_probe::windows::move_to_trash(self, request, endpoint, destination, cancel)
                .await
                .map(|outcome| match outcome {
                    crate::imap_probe::windows::ImapTrashOutcome::Accepted => {
                        ImapTrashOutcome::Accepted
                    }
                    crate::imap_probe::windows::ImapTrashOutcome::Rejected => {
                        ImapTrashOutcome::Rejected
                    }
                    crate::imap_probe::windows::ImapTrashOutcome::Unknown => {
                        ImapTrashOutcome::Unknown
                    }
                })
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (endpoint, request, destination);
            Err(MailProbeError::UnsupportedPlatform)
        }
    }
}
