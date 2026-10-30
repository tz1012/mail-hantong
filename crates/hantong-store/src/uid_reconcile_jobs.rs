//! One explicit bounded observation of already stored immutable UID metadata.
use crate::{Store, uid_page_jobs::UidQueueAdmission};
use hantong_contracts::{
    ErrorCode,
    uid_page::UidPageStatusRequest,
    uid_reconcile::{UidReconcileReceipt, UidReconcileRequest},
};
use hantong_domain::{
    imap_reconcile::{ImapUidReconcilePage, ImapUidReconcileRequest},
    mail_connection::ImapEndpointConfig,
    mailboxes::ImapMailboxName,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UidReconcileAdmission {
    Admitted(UidReconcileReceipt),
    Existing(UidReconcileReceipt),
    Failed(UidReconcileReceipt),
    Rejected(ErrorCode),
}
#[derive(Debug, Eq, PartialEq)]
pub enum UidReconcileStart {
    Ready(Box<UidReconcileJob>),
    Finished(UidReconcileReceipt),
}
#[derive(Eq, PartialEq)]
pub struct UidReconcileJob {
    execution_id: [u8; 32],
    request: UidReconcileRequest,
    fetch_request: ImapUidReconcileRequest,
    endpoint: ImapEndpointConfig,
}
impl std::fmt::Debug for UidReconcileJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UidReconcileJob { <redacted> }")
    }
}
impl UidReconcileJob {
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn request(&self) -> &UidReconcileRequest {
        &self.request
    }
    pub fn fetch_request(&self) -> &ImapUidReconcileRequest {
        &self.fetch_request
    }
    pub fn endpoint(&self) -> &ImapEndpointConfig {
        &self.endpoint
    }
}
/// An observation, not a claim about the server's state at read time.
#[derive(Clone, Eq, PartialEq)]
pub struct StoredImapUidObservation {
    flags: Option<Vec<String>>,
    generation: u64,
    observed_at_utc_ms: i64,
    execution_id: [u8; 32],
}
impl std::fmt::Debug for StoredImapUidObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StoredImapUidObservation { <redacted> }")
    }
}
impl StoredImapUidObservation {
    pub fn flags(&self) -> Option<&[String]> {
        self.flags.as_deref()
    }
    pub fn remote_absent(&self) -> bool {
        self.flags.is_none()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn observed_at_utc_ms(&self) -> i64 {
        self.observed_at_utc_ms
    }
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
}
impl Store {
    pub fn admit_uid_reconcile(
        &self,
        id: [u8; 32],
        request: &UidReconcileRequest,
        queue: UidQueueAdmission,
    ) -> Result<UidReconcileAdmission, ErrorCode> {
        #[cfg(windows)]
        {
            storage::admit(self, id, request, queue)
        }
        #[cfg(not(windows))]
        {
            let _ = (id, request, queue);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn start_uid_reconcile(&self, id: [u8; 32]) -> Result<UidReconcileStart, ErrorCode> {
        #[cfg(windows)]
        {
            storage::start(self, id)
        }
        #[cfg(not(windows))]
        {
            let _ = id;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn check_uid_reconcile(&self, id: [u8; 32]) -> Result<(), ErrorCode> {
        #[cfg(windows)]
        {
            storage::check(self, id)
        }
        #[cfg(not(windows))]
        {
            let _ = id;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn complete_uid_reconcile_success(
        &self,
        id: [u8; 32],
        page: &ImapUidReconcilePage,
    ) -> Result<UidReconcileReceipt, ErrorCode> {
        #[cfg(windows)]
        {
            storage::succeed(self, id, page)
        }
        #[cfg(not(windows))]
        {
            let _ = (id, page);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn complete_uid_reconcile_failure(
        &self,
        id: [u8; 32],
        error: ErrorCode,
    ) -> Result<UidReconcileReceipt, ErrorCode> {
        #[cfg(windows)]
        {
            storage::fail(self, id, error)
        }
        #[cfg(not(windows))]
        {
            let _ = (id, error);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn load_uid_reconcile_receipt(
        &self,
        owner: [u8; 16],
        request: &UidPageStatusRequest,
    ) -> Result<UidReconcileReceipt, ErrorCode> {
        #[cfg(windows)]
        {
            storage::receipt(self, owner, request)
        }
        #[cfg(not(windows))]
        {
            let _ = (owner, request);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    /// None means no recorded observation (or no immutable base UID), never present.
    pub fn load_imap_uid_observation(
        &self,
        account: &str,
        mailbox: &ImapMailboxName,
        validity: u32,
        uid: u32,
    ) -> Result<Option<StoredImapUidObservation>, ErrorCode> {
        #[cfg(windows)]
        {
            storage::observation(self, account, mailbox, validity, uid)
        }
        #[cfg(not(windows))]
        {
            let _ = (account, mailbox, validity, uid);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
#[cfg(windows)]
mod storage;
#[cfg(windows)]
pub(super) use storage::{execution_reserved, operation_reserved, validate_history};
#[cfg(all(test, windows))]
mod tests;
