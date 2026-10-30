//! Fail-closed coordinator for authenticated tool execution.

use hantong_contracts::{ErrorCode, audit::AuditTerminal};

use crate::{AccountsListRequest, FolderRefreshAction, ToolRequest, UidPageAction};
use hantong_contracts::folders::{FolderListRequest, FolderRefreshStatusRequest};
use hantong_contracts::message_search::ImapMetadataSearchRequest;
use hantong_contracts::uid_page::{UidCursorRequest, UidPageRequest, UidPageStatusRequest};
use hantong_contracts::uid_reconcile::UidReconcileRequest;
use hantong_contracts::uid_recovery::UidRecoveryRequest;

/// A bounded service result. The service owns the value and optional count;
/// callers cannot manufacture an invalid count through this type.
pub struct ReadResult<T> {
    value: T,
    count: Option<u64>,
}

pub enum AuditHandoff<T> {
    Response(T),
    Error(ErrorCode),
}

impl<T> AuditHandoff<T> {
    /// The Store has durably taken ownership of the audit attempt.
    pub fn response(value: T) -> Self {
        Self::Response(value)
    }
    /// The Store owns the attempted durable admission outcome. It may have
    /// recorded a terminal result, or its commit outcome may be uncertain;
    /// the caller must not attempt another terminal write.
    pub fn error(error: ErrorCode) -> Self {
        Self::Error(error)
    }
}

impl<T> ReadResult<T> {
    pub fn new(value: T, count: Option<u64>) -> Result<Self, ErrorCode> {
        AuditTerminal::succeeded(count)?;
        Ok(Self { value, count })
    }

    pub fn value(self) -> T {
        self.value
    }

    pub fn count(&self) -> Option<u64> {
        self.count
    }
}

pub trait ExecutionContext {
    type Output;

    fn revalidate(&mut self) -> Result<(), ErrorCode>;
    fn begin_audit(&mut self, request: &ToolRequest) -> Result<[u8; 32], ErrorCode>;
    fn check_scope(&mut self, request: &ToolRequest) -> Result<(), ErrorCode>;
    fn check_policy(&mut self, request: &ToolRequest) -> Result<(), ErrorCode>;
    fn read_accounts(
        &mut self,
        request: &AccountsListRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode>;
    fn read_message_search(
        &mut self,
        _request: &ImapMetadataSearchRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn read_folders(
        &mut self,
        _request: &FolderListRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn read_folder_refresh_status(
        &mut self,
        _request: &FolderRefreshStatusRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn request_folder_refresh(
        &mut self,
        _audit_id: [u8; 32],
        _request: &hantong_contracts::folders::FolderRefreshRequest,
    ) -> Result<AuditHandoff<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn read_uid_cursor(
        &mut self,
        _request: &UidCursorRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn read_uid_inspection(
        &mut self,
        _request: &UidCursorRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn read_uid_page_status(
        &mut self,
        _request: &UidPageStatusRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn request_uid_page(
        &mut self,
        _audit_id: [u8; 32],
        _request: &UidPageRequest,
    ) -> Result<AuditHandoff<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn read_uid_reconcile_status(
        &mut self,
        _request: &UidPageStatusRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn request_uid_reconcile(
        &mut self,
        _audit_id: [u8; 32],
        _request: &UidReconcileRequest,
    ) -> Result<AuditHandoff<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn read_uid_recovery_status(
        &mut self,
        _request: &UidPageStatusRequest,
    ) -> Result<ReadResult<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn request_uid_recovery(
        &mut self,
        _audit_id: [u8; 32],
        _request: &UidRecoveryRequest,
    ) -> Result<AuditHandoff<Self::Output>, ErrorCode> {
        Err(ErrorCode::CapabilityUnsupported)
    }
    fn finish_audit(&mut self, id: [u8; 32], terminal: AuditTerminal) -> Result<(), ErrorCode>;
}

pub fn execute_tool<C: ExecutionContext>(
    context: &mut C,
    method: &str,
    envelope: &serde_json::Value,
) -> Result<C::Output, ErrorCode> {
    let request = ToolRequest::validate_method(method, envelope)?;
    context.revalidate()?;
    let audit_id = context.begin_audit(&request)?;

    if let Err(error) = context.check_scope(&request) {
        return finish_failure(context, audit_id, error);
    }
    if let Err(error) = context.check_policy(&request) {
        return finish_failure(context, audit_id, error);
    }

    match request {
        ToolRequest::AccountsList(request) => match context.read_accounts(&request) {
            Ok(result) => {
                let count = result.count();
                let value = result.value;
                finish_success(context, audit_id, count)?;
                Ok(value)
            }
            Err(error) => finish_failure(context, audit_id, error),
        },
        ToolRequest::MessagesSearch { request, .. } => {
            match context.read_message_search(&request) {
                Ok(result) => {
                    let count = result.count();
                    let value = result.value;
                    finish_success(context, audit_id, count)?;
                    Ok(value)
                }
                Err(error) => finish_failure(context, audit_id, error),
            }
        }
        ToolRequest::FoldersList { request, .. } => match context.read_folders(&request) {
            Ok(result) => {
                let count = result.count();
                let value = result.value;
                finish_success(context, audit_id, count)?;
                Ok(value)
            }
            Err(error) => finish_failure(context, audit_id, error),
        },
        ToolRequest::FoldersRefresh { action, .. } => match action {
            FolderRefreshAction::Status(request) => {
                match context.read_folder_refresh_status(&request) {
                    Ok(result) => {
                        let count = result.count();
                        let value = result.value;
                        finish_success(context, audit_id, count)?;
                        Ok(value)
                    }
                    Err(error) => finish_failure(context, audit_id, error),
                }
            }
            FolderRefreshAction::Request(request) => {
                match context.request_folder_refresh(audit_id, &request) {
                    Ok(AuditHandoff::Response(value)) => Ok(value),
                    Ok(AuditHandoff::Error(error)) => Err(error),
                    Err(error) => finish_failure(context, audit_id, error),
                }
            }
        },
        ToolRequest::SyncPage { action, .. } => match action {
            UidPageAction::Request(request) => match context.request_uid_page(audit_id, &request) {
                Ok(AuditHandoff::Response(value)) => Ok(value),
                Ok(AuditHandoff::Error(error)) => Err(error),
                Err(error) => finish_failure(context, audit_id, error),
            },
            UidPageAction::Reconcile(request) => {
                match context.request_uid_reconcile(audit_id, &request) {
                    Ok(AuditHandoff::Response(value)) => Ok(value),
                    Ok(AuditHandoff::Error(error)) => Err(error),
                    Err(error) => finish_failure(context, audit_id, error),
                }
            }
            UidPageAction::Recover(request) => {
                match context.request_uid_recovery(audit_id, &request) {
                    Ok(AuditHandoff::Response(value)) => Ok(value),
                    Ok(AuditHandoff::Error(error)) => Err(error),
                    Err(error) => finish_failure(context, audit_id, error),
                }
            }
            other => {
                let result = match other {
                    UidPageAction::Inspect(request) => context.read_uid_inspection(&request),
                    UidPageAction::Cursor(request) => context.read_uid_cursor(&request),
                    UidPageAction::Status(request) => context.read_uid_page_status(&request),
                    UidPageAction::ReconcileStatus(request) => {
                        context.read_uid_reconcile_status(&request)
                    }
                    UidPageAction::RecoverStatus(request) => {
                        context.read_uid_recovery_status(&request)
                    }
                    UidPageAction::Request(_)
                    | UidPageAction::Reconcile(_)
                    | UidPageAction::Recover(_) => {
                        unreachable!("handled above")
                    }
                };
                match result {
                    Ok(result) => {
                        let count = result.count();
                        let value = result.value;
                        finish_success(context, audit_id, count)?;
                        Ok(value)
                    }
                    Err(error) => finish_failure(context, audit_id, error),
                }
            }
        },
        // The send contract deliberately has no service callback yet. An
        // approval_id in the input is not approval evidence.
        ToolRequest::SendExecute(_) => finish_failure(context, audit_id, ErrorCode::PolicyBlocked),
    }
}

fn finish_success<C: ExecutionContext>(
    context: &mut C,
    audit_id: [u8; 32],
    count: Option<u64>,
) -> Result<(), ErrorCode> {
    context
        .finish_audit(
            audit_id,
            AuditTerminal::succeeded(count).map_err(|_| ErrorCode::InternalError)?,
        )
        .map_err(|_| ErrorCode::InternalError)
}

fn finish_failure<C: ExecutionContext>(
    context: &mut C,
    audit_id: [u8; 32],
    error: ErrorCode,
) -> Result<C::Output, ErrorCode> {
    context
        .finish_audit(audit_id, AuditTerminal::failed(error))
        .map_err(|_| ErrorCode::InternalError)
        .and(Err(error))
}

#[cfg(test)]
mod tests;
