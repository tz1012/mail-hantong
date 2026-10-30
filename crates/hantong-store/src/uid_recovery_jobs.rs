//! Explicit namespace recovery, preserving immutable prior UID history.
use crate::Store;
use hantong_contracts::{
    ErrorCode,
    uid_page::UidPageStatusRequest,
    uid_recovery::{UidRecoveryReceipt, UidRecoveryRequest},
};
use hantong_domain::{
    imap_recovery::{ImapUidRecoveryPage, ImapUidRecoveryRequest},
    mail_connection::ImapEndpointConfig,
};

use crate::uid_page_jobs::UidQueueAdmission;
/// An Ok admission transfers audit ownership, including a Rejected result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UidRecoveryAdmission {
    Admitted(UidRecoveryReceipt),
    Existing(UidRecoveryReceipt),
    Failed(UidRecoveryReceipt),
    Rejected(ErrorCode),
}
#[derive(Debug, Eq, PartialEq)]
pub enum UidRecoveryStart {
    Ready(Box<UidRecoveryJob>),
    Finished(UidRecoveryReceipt),
}
#[derive(Eq, PartialEq)]
pub struct UidRecoveryJob {
    execution_id: [u8; 32],
    request: UidRecoveryRequest,
    fetch_request: ImapUidRecoveryRequest,
    endpoint: ImapEndpointConfig,
}
impl std::fmt::Debug for UidRecoveryJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UidRecoveryJob { <redacted> }")
    }
}
impl UidRecoveryJob {
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn request(&self) -> &UidRecoveryRequest {
        &self.request
    }
    pub fn fetch_request(&self) -> &ImapUidRecoveryRequest {
        &self.fetch_request
    }
    pub fn endpoint(&self) -> &ImapEndpointConfig {
        &self.endpoint
    }
}
impl Store {
    pub fn admit_uid_recovery(
        &self,
        id: [u8; 32],
        request: &UidRecoveryRequest,
        queue: UidQueueAdmission,
    ) -> Result<UidRecoveryAdmission, ErrorCode> {
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
    pub fn start_uid_recovery(&self, id: [u8; 32]) -> Result<UidRecoveryStart, ErrorCode> {
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
    pub fn check_uid_recovery(&self, id: [u8; 32]) -> Result<(), ErrorCode> {
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
    pub fn complete_uid_recovery_success(
        &self,
        id: [u8; 32],
        page: &ImapUidRecoveryPage,
    ) -> Result<UidRecoveryReceipt, ErrorCode> {
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
    pub fn complete_uid_recovery_failure(
        &self,
        id: [u8; 32],
        error: ErrorCode,
    ) -> Result<UidRecoveryReceipt, ErrorCode> {
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
    pub fn load_uid_recovery_receipt(
        &self,
        owner: [u8; 16],
        request: &UidPageStatusRequest,
    ) -> Result<UidRecoveryReceipt, ErrorCode> {
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
}
#[cfg(windows)]
pub(crate) mod storage;
#[cfg(windows)]
pub(super) use storage::{
    execution_reserved, operation_reserved, validate_epoch_link, validate_history,
};
#[cfg(all(test, windows))]
mod tests;
