//! Durable one-shot UID observations bound to their original audit and grants.
use crate::Store;
use hantong_contracts::{
    ErrorCode,
    uid_page::{UidPageReceipt, UidPageRequest, UidPageStatusRequest},
};
use hantong_domain::{
    imap_sync::{ImapUidPage, ImapUidPageRequest},
    mail_connection::ImapEndpointConfig,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UidQueueAdmission {
    Reserved,
    Full,
    Closed,
}
/// An Ok admission transfers audit ownership, including a Rejected result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UidPageAdmission {
    Admitted(UidPageReceipt),
    Existing(UidPageReceipt),
    Failed(UidPageReceipt),
    Rejected(ErrorCode),
}
#[derive(Debug, Eq, PartialEq)]
pub enum UidPageStart {
    Ready(Box<UidPageJob>),
    Finished(UidPageReceipt),
}
#[derive(Eq, PartialEq)]
pub struct UidPageJob {
    execution_id: [u8; 32],
    request: UidPageRequest,
    fetch_request: ImapUidPageRequest,
    endpoint: ImapEndpointConfig,
}
impl std::fmt::Debug for UidPageJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UidPageJob { <redacted> }")
    }
}
impl UidPageJob {
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn request(&self) -> &UidPageRequest {
        &self.request
    }
    pub fn fetch_request(&self) -> &ImapUidPageRequest {
        &self.fetch_request
    }
    pub fn endpoint(&self) -> &ImapEndpointConfig {
        &self.endpoint
    }
}
impl Store {
    pub fn admit_uid_page(
        &self,
        id: [u8; 32],
        request: &UidPageRequest,
        queue: UidQueueAdmission,
    ) -> Result<UidPageAdmission, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::admit(self, id, request, queue)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (id, request, queue);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn start_uid_page(&self, id: [u8; 32]) -> Result<UidPageStart, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::start(self, id)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = id;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn check_uid_page(&self, id: [u8; 32]) -> Result<(), ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::check(self, id)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = id;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn complete_uid_page_success(
        &self,
        id: [u8; 32],
        page: &ImapUidPage,
    ) -> Result<UidPageReceipt, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::succeed(self, id, page)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (id, page);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn complete_uid_page_failure(
        &self,
        id: [u8; 32],
        error: ErrorCode,
    ) -> Result<UidPageReceipt, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::fail(self, id, error)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (id, error);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn load_uid_page_receipt(
        &self,
        owner: [u8; 16],
        request: &UidPageStatusRequest,
    ) -> Result<UidPageReceipt, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::receipt(self, owner, request)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (owner, request);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
pub(crate) mod storage;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
pub(super) use storage::{execution_reserved, operation_reserved, validate_history};
#[cfg(all(test, windows))]
mod tests;
