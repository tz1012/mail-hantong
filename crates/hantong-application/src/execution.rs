//! Fail-closed coordinator for authenticated tool execution.

use hantong_contracts::{ErrorCode, audit::AuditTerminal};

use crate::{AccountsListRequest, ToolRequest};

/// A bounded service result. The service owns the value and optional count;
/// callers cannot manufacture an invalid count through this type.
pub struct ReadResult<T> {
    value: T,
    count: Option<u64>,
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
