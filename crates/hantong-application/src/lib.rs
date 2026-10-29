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
pub use tool_request::{AccountsListRequest, FolderRefreshAction, SendExecuteRequest, ToolRequest};

mod execution;
pub use execution::{AuditHandoff, ExecutionContext, ReadResult, execute_tool};
