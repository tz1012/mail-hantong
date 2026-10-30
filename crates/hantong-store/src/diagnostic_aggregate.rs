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
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Deferred, |tx| {
                storage::collect(tx, query)
            })
        }
        #[cfg(not(windows))]
        {
            let _ = query;
            Err(ErrorCode::CapabilityUnsupported)
        }
    }
}

#[cfg(windows)]
mod storage;

#[cfg(all(test, windows))]
mod tests;
