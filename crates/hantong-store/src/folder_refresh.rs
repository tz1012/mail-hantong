//! Durable, one-shot folder observations bound to their original audit and grants.
use crate::Store;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
use hantong_contracts::folders::FolderRefreshState;
use hantong_contracts::{
    ErrorCode,
    folders::{FolderRefreshReceipt, FolderRefreshRequest, FolderRefreshStatusRequest},
};
use hantong_domain::{mail_connection::ImapEndpointConfig, mailboxes::ImapMailboxSnapshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FolderQueueAdmission {
    Reserved,
    Full,
    Closed,
}

/// An Ok admission transfers audit ownership, even when the request is rejected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FolderRefreshAdmission {
    Admitted(FolderRefreshReceipt),
    Existing(FolderRefreshReceipt),
    Failed(FolderRefreshReceipt),
    Rejected(ErrorCode),
}
#[derive(Debug, Eq, PartialEq)]
pub enum FolderRefreshStart {
    Ready(FolderRefreshJob),
    Finished(FolderRefreshReceipt),
}
#[derive(Eq, PartialEq)]
pub struct FolderRefreshJob {
    execution_id: [u8; 32],
    request: FolderRefreshRequest,
    endpoint: ImapEndpointConfig,
}
impl std::fmt::Debug for FolderRefreshJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FolderRefreshJob { <redacted> }")
    }
}
impl FolderRefreshJob {
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn request(&self) -> &FolderRefreshRequest {
        &self.request
    }
    pub fn endpoint(&self) -> &ImapEndpointConfig {
        &self.endpoint
    }
}

impl Store {
    pub fn admit_folder_refresh(
        &self,
        audit_id: [u8; 32],
        request: &FolderRefreshRequest,
        queue: FolderQueueAdmission,
    ) -> Result<FolderRefreshAdmission, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::admit(self, audit_id, request, queue)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (audit_id, request, queue);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn start_folder_refresh(&self, id: [u8; 32]) -> Result<FolderRefreshStart, ErrorCode> {
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
    pub fn check_folder_refresh(&self, id: [u8; 32]) -> Result<(), ErrorCode> {
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
    pub fn complete_folder_refresh_success(
        &self,
        id: [u8; 32],
        snapshot: &ImapMailboxSnapshot,
    ) -> Result<FolderRefreshReceipt, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            storage::succeed(self, id, snapshot)
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = (id, snapshot);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn complete_folder_refresh_failure(
        &self,
        id: [u8; 32],
        error: ErrorCode,
    ) -> Result<FolderRefreshReceipt, ErrorCode> {
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
    pub fn load_folder_refresh_receipt(
        &self,
        owner: [u8; 16],
        request: &FolderRefreshStatusRequest,
    ) -> Result<FolderRefreshReceipt, ErrorCode> {
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
mod storage;
#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
pub(super) use storage::{operation_reserved, validate_history};
#[cfg(all(test, windows))]
mod tests;
