//! Durable explicit preview operations; the base UID cursor is never advanced here.
use crate::Store;
pub use crate::uid_page_jobs::UidQueueAdmission;
use hantong_contracts::{ErrorCode, initial_sync::*, uid_page::UidPageStatusRequest};
use hantong_domain::{
    imap_observation::{ImapUidObservation, ImapUidObservationRequest},
    imap_reconcile::{ImapUidReconcilePage, ImapUidReconcileRequest},
    mail_connection::ImapEndpointConfig,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitialSyncAdmission {
    Admitted(InitialSyncReceipt),
    Existing(InitialSyncReceipt),
    Failed(InitialSyncReceipt),
    Rejected(ErrorCode),
}
#[derive(Debug, Eq, PartialEq)]
pub enum InitialSyncStart {
    Ready(Box<InitialSyncJob>),
    Finished(InitialSyncReceipt),
}
#[derive(Clone, Eq, PartialEq)]
pub enum InitialSyncFetch {
    Observe(ImapUidObservationRequest),
    Page(ImapUidReconcileRequest),
}
impl std::fmt::Debug for InitialSyncFetch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InitialSyncFetch(<redacted>)")
    }
}
#[derive(Eq, PartialEq)]
pub struct InitialSyncJob {
    execution_id: [u8; 32],
    request: InitialSyncRequest,
    fetch: InitialSyncFetch,
    endpoint: ImapEndpointConfig,
}
impl std::fmt::Debug for InitialSyncJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InitialSyncJob(<redacted>)")
    }
}
impl InitialSyncJob {
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn request(&self) -> &InitialSyncRequest {
        &self.request
    }
    pub fn fetch_request(&self) -> &InitialSyncFetch {
        &self.fetch
    }
    pub fn endpoint(&self) -> &ImapEndpointConfig {
        &self.endpoint
    }
}

impl Store {
    pub fn admit_initial_sync(
        &self,
        id: [u8; 32],
        request: &InitialSyncRequest,
        queue: UidQueueAdmission,
        instance: [u8; 32],
    ) -> Result<InitialSyncAdmission, ErrorCode> {
        #[cfg(windows)]
        {
            storage::admit(self, id, request, queue, instance)
        }
        #[cfg(not(windows))]
        {
            let _ = (id, request, queue, instance);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn pause_initial_sync(
        &self,
        id: [u8; 32],
        request: &InitialSyncRequest,
        instance: [u8; 32],
    ) -> Result<InitialSyncAdmission, ErrorCode> {
        if request.action() != InitialSyncAction::Pause {
            return Err(ErrorCode::InvalidInput);
        }
        self.admit_initial_sync(id, request, UidQueueAdmission::Reserved, instance)
    }
    pub fn start_initial_sync(&self, id: [u8; 32]) -> Result<InitialSyncStart, ErrorCode> {
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
    pub fn check_initial_sync(&self, id: [u8; 32]) -> Result<(), ErrorCode> {
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
    pub fn complete_initial_sync_observation(
        &self,
        id: [u8; 32],
        observation: &ImapUidObservation,
    ) -> Result<InitialSyncReceipt, ErrorCode> {
        #[cfg(windows)]
        {
            storage::observe(self, id, observation)
        }
        #[cfg(not(windows))]
        {
            let _ = (id, observation);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn complete_initial_sync_page(
        &self,
        id: [u8; 32],
        page: &ImapUidReconcilePage,
    ) -> Result<InitialSyncReceipt, ErrorCode> {
        #[cfg(windows)]
        {
            storage::page(self, id, page)
        }
        #[cfg(not(windows))]
        {
            let _ = (id, page);
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
    pub fn complete_initial_sync_failure(
        &self,
        id: [u8; 32],
        error: ErrorCode,
    ) -> Result<InitialSyncReceipt, ErrorCode> {
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
    pub fn load_initial_sync_receipt(
        &self,
        owner: [u8; 16],
        request: &UidPageStatusRequest,
    ) -> Result<InitialSyncReceipt, ErrorCode> {
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
    pub fn load_initial_sync_plan(
        &self,
        owner: [u8; 16],
        request: &InitialSyncPlanRequest,
    ) -> Result<InitialSyncPlanSnapshot, ErrorCode> {
        #[cfg(windows)]
        {
            storage::plan_snapshot(self, owner, request)
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
pub(crate) use storage::{execution_reserved, operation_reserved, validate_history};
#[cfg(all(test, windows))]
mod tests;
