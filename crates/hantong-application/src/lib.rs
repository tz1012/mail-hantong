#![forbid(unsafe_code)]

//! Application services and their bounded diagnostic records.

mod accounts_response;
pub mod lifecycle;
pub use accounts_response::render_account_list;
mod folders_response;
pub use folders_response::{render_folder_page, render_folder_refresh_receipt};

mod diagnostics;
pub use diagnostics::{DiagnosticEvent, EventKind, LogError, Severity, redact_sensitive};

mod diagnostic_snapshot;
pub use diagnostic_snapshot::{
    DiagnosticSelection, DiagnosticSnapshot, SnapshotError, SnapshotSection,
};

mod tool_request;
pub use tool_request::{
    AccountsListRequest, FolderRefreshAction, SendExecuteRequest, ToolRequest, UidPageAction,
};
mod uid_page_response;
pub use uid_page_response::{
    render_uid_cursor, render_uid_inspection, render_uid_receipt, render_uid_reconcile_receipt,
    render_uid_recovery_receipt, validated_uid_mailbox,
};

mod execution;
pub use execution::{AuditHandoff, ExecutionContext, ReadResult, execute_tool};
#[cfg(test)]
mod uid_page_tests;
