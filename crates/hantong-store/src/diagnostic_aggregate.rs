//! Read-only bounded aggregation of retained local execution state.
use crate::Store;
use hantong_contracts::{
    ErrorCode,
    diagnostic_aggregate::{DiagnosticAggregate, DiagnosticAggregateQuery},
};

impl Store {
    pub fn collect_diagnostic_aggregate(
        &self,
        query: &DiagnosticAggregateQuery,
    ) -> Result<DiagnosticAggregate, ErrorCode> {
        #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Deferred, |tx| {
                storage::collect(tx, query)
            })
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            let _ = query;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}

#[cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod storage;

#[cfg(all(test, windows))]
mod tests;
