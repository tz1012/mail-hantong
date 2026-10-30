#![forbid(unsafe_code)]

//! In-memory account-read policy. Inputs are trusted Core data, not proof.
use hantong_contracts::{ErrorCode, audit::AuditTool};
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(60);
const MAX_BUCKETS: usize = 1024;

#[derive(Default)]
pub struct ReadPolicy {
    buckets: BTreeMap<[u8; 16], VecDeque<Instant>>,
    last_observed: Option<Instant>,
}
impl ReadPolicy {
    pub fn new() -> Self {
        Self::default()
    }
    /// Checks current Store metadata. Success is local to this invocation and
    /// does not establish authentication, scope, or approval for other paths.
    pub fn check(
        &mut self,
        registration_id: [u8; 16],
        read_only: bool,
        requests_per_minute: u32,
        tool: AuditTool,
        now: Instant,
    ) -> Result<(), ErrorCode> {
        self.charge(registration_id, requests_per_minute, now)?;
        // A writable registration may still read; it gains no write path here.
        let _ = read_only;
        match tool {
            AuditTool::DisplaySettingsGet
            | AuditTool::DiagnosticsSnapshot
            | AuditTool::AccountsList
            | AuditTool::FoldersList
            | AuditTool::FoldersRefresh
            | AuditTool::MessagesGet
            | AuditTool::MessagesSearch
            | AuditTool::SyncPage => Ok(()),
            AuditTool::DraftCreate | AuditTool::SendRequest if !read_only => Ok(()),
            AuditTool::DraftCreate | AuditTool::SendRequest | AuditTool::SendExecute => {
                Err(ErrorCode::PolicyBlocked)
            }
        }
    }
    /// Constraints only. The Core native coordinator separately consumes the
    /// opaque human verification; this method cannot issue approval.
    pub fn check_native_send(
        &mut self,
        registration_id: [u8; 16],
        read_only: bool,
        requests_per_minute: u32,
        now: Instant,
    ) -> Result<(), ErrorCode> {
        self.charge(registration_id, requests_per_minute, now)?;
        if read_only {
            Err(ErrorCode::PolicyBlocked)
        } else {
            Ok(())
        }
    }
    fn charge(
        &mut self,
        registration_id: [u8; 16],
        requests_per_minute: u32,
        now: Instant,
    ) -> Result<(), ErrorCode> {
        if registration_id == [0; 16] || !(1..=600).contains(&requests_per_minute) {
            return Err(ErrorCode::InternalError);
        }
        if self.last_observed.is_some_and(|last| now < last) {
            return Err(ErrorCode::InternalError);
        }
        self.last_observed = Some(now);
        self.buckets.retain(|_, timestamps| {
            while timestamps
                .front()
                .is_some_and(|time| now.duration_since(*time) >= WINDOW)
            {
                timestamps.pop_front();
            }
            !timestamps.is_empty()
        });
        if !self.buckets.contains_key(&registration_id) && self.buckets.len() >= MAX_BUCKETS {
            return Err(ErrorCode::QuotaExceeded);
        }
        let timestamps = self.buckets.entry(registration_id).or_default();
        if timestamps.len() >= requests_per_minute as usize {
            return Err(ErrorCode::RateLimited);
        }
        timestamps.push_back(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
