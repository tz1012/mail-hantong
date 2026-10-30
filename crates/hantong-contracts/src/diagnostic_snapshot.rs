//! Public diagnostic selection. Trusted ownership is injected by Core.
use crate::{
    ErrorCode,
    diagnostic_aggregate::{DiagnosticSection, MAX_EXECUTIONS, MAX_WINDOW_MS},
};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct DiagnosticSnapshotRequest {
    from_utc_ms: i64,
    to_utc_ms: i64,
    max_executions: u32,
    sections: Vec<DiagnosticSection>,
}
impl DiagnosticSnapshotRequest {
    pub fn new(
        from_utc_ms: i64,
        to_utc_ms: i64,
        max_executions: u32,
        sections: &[DiagnosticSection],
    ) -> Result<Self, ErrorCode> {
        if from_utc_ms < 0
            || to_utc_ms <= from_utc_ms
            || to_utc_ms - from_utc_ms > MAX_WINDOW_MS
            || !(1..=MAX_EXECUTIONS).contains(&max_executions)
            || sections.is_empty()
            || sections.len() > 3
        {
            return Err(ErrorCode::InvalidInput);
        }
        let mut canonical = sections.to_vec();
        canonical.sort();
        canonical.dedup();
        if canonical.len() != sections.len() {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            from_utc_ms,
            to_utc_ms,
            max_executions,
            sections: canonical,
        })
    }
    pub fn from_utc_ms(&self) -> i64 {
        self.from_utc_ms
    }
    pub fn to_utc_ms(&self) -> i64 {
        self.to_utc_ms
    }
    pub fn max_executions(&self) -> u32 {
        self.max_executions
    }
    pub fn sections(&self) -> &[DiagnosticSection] {
        &self.sections
    }
}
impl fmt::Debug for DiagnosticSnapshotRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DiagnosticSnapshotRequest(<redacted>)")
    }
}
