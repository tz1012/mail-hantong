//! Local metadata coverage from the most recently stored append or recovery page.
use crate::{
    ErrorCode,
    uid_page::{UidCursorRequest, UidMailboxIdentity},
};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct UidObservationSummary {
    cursor_version: u64,
    epoch_generation: u64,
    covered_through_uid: u32,
    observed_uid_ceiling: u32,
    stored_at_utc_ms: i64,
}
impl UidObservationSummary {
    pub fn new(
        cursor_version: u64,
        epoch_generation: u64,
        covered_through_uid: u32,
        observed_uid_ceiling: u32,
        stored_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        if epoch_generation == 0
            || epoch_generation > cursor_version
            || cursor_version > i64::MAX as u64
            || covered_through_uid > observed_uid_ceiling
            || observed_uid_ceiling == u32::MAX
            || stored_at_utc_ms <= 0
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            cursor_version,
            epoch_generation,
            covered_through_uid,
            observed_uid_ceiling,
            stored_at_utc_ms,
        })
    }
    pub fn cursor_version(&self) -> u64 {
        self.cursor_version
    }
    pub fn epoch_generation(&self) -> u64 {
        self.epoch_generation
    }
    pub fn covered_through_uid(&self) -> u32 {
        self.covered_through_uid
    }
    pub fn observed_uid_ceiling(&self) -> u32 {
        self.observed_uid_ceiling
    }
    pub fn stored_at_utc_ms(&self) -> i64 {
        self.stored_at_utc_ms
    }
    pub fn coverage_at_observed_ceiling(&self) -> bool {
        self.covered_through_uid == self.observed_uid_ceiling
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct UidInspectionSnapshot {
    account_id: String,
    mailbox: UidMailboxIdentity,
    catalog_generation: u64,
    binding_current: bool,
    observation: Option<UidObservationSummary>,
}
impl UidInspectionSnapshot {
    pub fn new(
        account_id: &str,
        mailbox: UidMailboxIdentity,
        catalog_generation: u64,
        binding_current: bool,
        observation: Option<UidObservationSummary>,
    ) -> Result<Self, ErrorCode> {
        UidCursorRequest::new(account_id, mailbox.clone())?;
        if catalog_generation == 0 || catalog_generation > i64::MAX as u64 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.into(),
            mailbox,
            catalog_generation,
            binding_current,
            observation,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }
    pub fn binding_current(&self) -> bool {
        self.binding_current
    }
    pub fn observation(&self) -> Option<&UidObservationSummary> {
        self.observation.as_ref()
    }
}
impl fmt::Debug for UidObservationSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UidObservationSummary { <redacted> }")
    }
}
impl fmt::Debug for UidInspectionSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UidInspectionSnapshot { <redacted> }")
    }
}
#[cfg(test)]
#[path = "uid_inspection_tests.rs"]
mod tests;
