//! Fixed, identifier-free diagnostic aggregates. Ownership inputs are not authentication proof.
use crate::ErrorCode;
use std::fmt;

pub const MAX_WINDOW_MS: i64 = 31 * 24 * 60 * 60 * 1000;
pub const MAX_EXECUTIONS: u32 = 1000;
pub const MAX_SNAPSHOT_BYTES: usize = 16_384;

macro_rules! vocabulary {
    ($name:ident, $len:expr, {$($variant:ident => $text:literal),+ $(,)?}) => {
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: [Self; $len] = [$(Self::$variant),+];
            pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $text),+ } }
            pub fn parse(value: &str) -> Result<Self, ErrorCode> { match value { $($text => Ok(Self::$variant)),+, _ => Err(ErrorCode::InvalidInput) } }
            pub const fn index(self) -> usize { self as usize }
        }
    };
}
vocabulary!(DiagnosticSection, 3, { Sync => "sync", RecentWork => "recent_work", Errors => "errors" });
vocabulary!(DiagnosticAuditState, 3, { Unresolved => "unresolved", ReturnedOk => "returned_ok", ReturnedError => "returned_error" });
vocabulary!(DiagnosticJobState, 4, { Admitted => "admitted", Running => "running", Succeeded => "succeeded", Failed => "failed" });
vocabulary!(DiagnosticJobKind, 4, { FolderRefresh => "folder_refresh", UidAppend => "uid_append", UidReconcile => "uid_reconcile", UidRecovery => "uid_recovery" });
vocabulary!(DiagnosticTool, 8, {
    AccountsList => "mail.accounts.list", FoldersList => "mail.folders.list",
    FoldersRefresh => "mail.folders.refresh", SyncPage => "mail.sync.page",
    MessagesSearch => "mail.messages.search", SendExecute => "mail.send.execute",
    DisplaySettingsGet => "app.settings.display.get", DiagnosticsSnapshot => "app.diagnostics.snapshot"
});

pub const DIAGNOSTIC_ERRORS: [ErrorCode; 22] = [
    ErrorCode::InvalidInput,
    ErrorCode::AuthRequired,
    ErrorCode::ScopeDenied,
    ErrorCode::PolicyBlocked,
    ErrorCode::Conflict,
    ErrorCode::RateLimited,
    ErrorCode::Offline,
    ErrorCode::NotFound,
    ErrorCode::SendUnknown,
    ErrorCode::ApprovalRequired,
    ErrorCode::ApprovalExpired,
    ErrorCode::ApprovalDenied,
    ErrorCode::ApprovalRateLimited,
    ErrorCode::ClientNotRegistered,
    ErrorCode::ClientSuspended,
    ErrorCode::HumanStepRequired,
    ErrorCode::CoreUnavailable,
    ErrorCode::VersionMismatch,
    ErrorCode::CursorExpired,
    ErrorCode::CapabilityUnsupported,
    ErrorCode::QuotaExceeded,
    ErrorCode::InternalError,
];

#[derive(Clone, Eq, PartialEq)]
pub struct DiagnosticAggregateOwner {
    registration_id: [u8; 16],
    expected_profile_epoch: u64,
    excluded_execution_id: Option<[u8; 32]>,
}
impl DiagnosticAggregateOwner {
    pub fn new(
        registration_id: [u8; 16],
        expected_profile_epoch: u64,
        excluded_execution_id: Option<[u8; 32]>,
    ) -> Result<Self, ErrorCode> {
        if expected_profile_epoch == 0 || expected_profile_epoch > i64::MAX as u64 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            registration_id,
            expected_profile_epoch,
            excluded_execution_id,
        })
    }
    pub fn registration_id(&self) -> &[u8; 16] {
        &self.registration_id
    }
    pub fn expected_profile_epoch(&self) -> u64 {
        self.expected_profile_epoch
    }
    pub fn excluded_execution_id(&self) -> Option<&[u8; 32]> {
        self.excluded_execution_id.as_ref()
    }
}
impl fmt::Debug for DiagnosticAggregateOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DiagnosticAggregateOwner(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct DiagnosticAggregateQuery {
    owner: DiagnosticAggregateOwner,
    from_utc_ms: i64,
    to_utc_ms: i64,
    max_executions: u32,
    sections: Vec<DiagnosticSection>,
}
impl DiagnosticAggregateQuery {
    pub fn new(
        owner: DiagnosticAggregateOwner,
        from_utc_ms: i64,
        to_utc_ms: i64,
        max_executions: u32,
        sections: &[DiagnosticSection],
    ) -> Result<Self, ErrorCode> {
        window_bounds(from_utc_ms, to_utc_ms, max_executions)?;
        Ok(Self {
            owner,
            from_utc_ms,
            to_utc_ms,
            max_executions,
            sections: canonical_sections(sections)?,
        })
    }
    pub fn owner(&self) -> &DiagnosticAggregateOwner {
        &self.owner
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
impl fmt::Debug for DiagnosticAggregateQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DiagnosticAggregateQuery(<redacted>)")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticWindow {
    from: i64,
    to: i64,
    max: u32,
    included: u32,
    truncated: bool,
}
impl DiagnosticWindow {
    pub fn new(
        from: i64,
        to: i64,
        max: u32,
        included: u32,
        truncated: bool,
    ) -> Result<Self, ErrorCode> {
        window_bounds(from, to, max)?;
        if included > max || (truncated && included != max) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            from,
            to,
            max,
            included,
            truncated,
        })
    }
    pub fn from_utc_ms(&self) -> i64 {
        self.from
    }
    pub fn to_utc_ms(&self) -> i64 {
        self.to
    }
    pub fn max_executions(&self) -> u32 {
        self.max
    }
    pub fn included_executions(&self) -> u32 {
        self.included
    }
    pub fn truncated(&self) -> bool {
        self.truncated
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticRecentWork {
    states: [u32; 3],
    tools: [u32; 8],
    count: u32,
}
impl DiagnosticRecentWork {
    pub fn new(states: [u32; 3], tools: [u32; 8]) -> Result<Self, ErrorCode> {
        let count = bounded_sum(&states)?;
        if count != bounded_sum(&tools)? {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            states,
            tools,
            count,
        })
    }
    pub fn count(&self) -> u32 {
        self.count
    }
    pub fn state_count(&self, state: DiagnosticAuditState) -> u32 {
        self.states[state.index()]
    }
    pub fn tool_count(&self, tool: DiagnosticTool) -> u32 {
        self.tools[tool.index()]
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticSync {
    states: [u32; 4],
    kinds: [u32; 4],
    count: u32,
}
impl DiagnosticSync {
    pub fn new(states: [u32; 4], kinds: [u32; 4]) -> Result<Self, ErrorCode> {
        let count = bounded_sum(&states)?;
        if count != bounded_sum(&kinds)? {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            states,
            kinds,
            count,
        })
    }
    pub fn count(&self) -> u32 {
        self.count
    }
    pub fn state_count(&self, state: DiagnosticJobState) -> u32 {
        self.states[state.index()]
    }
    pub fn kind_count(&self, kind: DiagnosticJobKind) -> u32 {
        self.kinds[kind.index()]
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticErrors {
    counts: [u32; 22],
    last: Option<ErrorCode>,
    count: u32,
}
impl DiagnosticErrors {
    pub fn new(counts: [u32; 22], last: Option<ErrorCode>) -> Result<Self, ErrorCode> {
        let count = bounded_sum(&counts)?;
        match last {
            None if count == 0 => {}
            Some(code)
                if DIAGNOSTIC_ERRORS
                    .iter()
                    .position(|c| *c == code)
                    .is_some_and(|i| counts[i] > 0) => {}
            _ => return Err(ErrorCode::InvalidInput),
        }
        Ok(Self {
            counts,
            last,
            count,
        })
    }
    pub fn count(&self) -> u32 {
        self.count
    }
    pub fn last_error_code(&self) -> Option<ErrorCode> {
        self.last
    }
    pub fn count_for(&self, code: ErrorCode) -> u32 {
        DIAGNOSTIC_ERRORS
            .iter()
            .position(|c| *c == code)
            .map_or(0, |i| self.counts[i])
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticAggregate {
    generated: i64,
    sections: Vec<DiagnosticSection>,
    window: DiagnosticWindow,
    sync: Option<DiagnosticSync>,
    recent: Option<DiagnosticRecentWork>,
    errors: Option<DiagnosticErrors>,
}
impl DiagnosticAggregate {
    pub fn new(
        generated: i64,
        sections: &[DiagnosticSection],
        window: DiagnosticWindow,
        sync: Option<DiagnosticSync>,
        recent: Option<DiagnosticRecentWork>,
        errors: Option<DiagnosticErrors>,
    ) -> Result<Self, ErrorCode> {
        let sections = canonical_sections(sections)?;
        if generated < window.to
            || sections.contains(&DiagnosticSection::Sync) != sync.is_some()
            || sections.contains(&DiagnosticSection::RecentWork) != recent.is_some()
            || sections.contains(&DiagnosticSection::Errors) != errors.is_some()
            || sync.as_ref().is_some_and(|v| v.count() > window.included)
            || recent
                .as_ref()
                .is_some_and(|v| v.count() != window.included)
            || errors.as_ref().is_some_and(|v| v.count() > window.included)
        {
            return Err(ErrorCode::InvalidInput);
        }
        if let (Some(recent), Some(errors)) = (&recent, &errors)
            && recent.state_count(DiagnosticAuditState::ReturnedError) != errors.count()
        {
            return Err(ErrorCode::InvalidInput);
        }
        if let (Some(sync), Some(errors)) = (&sync, &errors)
            && sync.state_count(DiagnosticJobState::Failed) > errors.count()
        {
            return Err(ErrorCode::InvalidInput);
        }
        if let (Some(sync), Some(recent)) = (&sync, &recent)
            && (sync.state_count(DiagnosticJobState::Failed)
                > recent.state_count(DiagnosticAuditState::ReturnedError)
                || sync.state_count(DiagnosticJobState::Succeeded)
                    > recent.state_count(DiagnosticAuditState::ReturnedOk)
                || sync.state_count(DiagnosticJobState::Admitted)
                    + sync.state_count(DiagnosticJobState::Running)
                    > recent.state_count(DiagnosticAuditState::Unresolved))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            generated,
            sections,
            window,
            sync,
            recent,
            errors,
        })
    }
    pub fn generated_at_utc_ms(&self) -> i64 {
        self.generated
    }
    pub fn sections(&self) -> &[DiagnosticSection] {
        &self.sections
    }
    pub fn window(&self) -> &DiagnosticWindow {
        &self.window
    }
    pub fn sync(&self) -> Option<&DiagnosticSync> {
        self.sync.as_ref()
    }
    pub fn recent_work(&self) -> Option<&DiagnosticRecentWork> {
        self.recent.as_ref()
    }
    pub fn errors(&self) -> Option<&DiagnosticErrors> {
        self.errors.as_ref()
    }
}

fn window_bounds(from: i64, to: i64, max: u32) -> Result<(), ErrorCode> {
    if from < 0 || to <= from || to - from > MAX_WINDOW_MS || !(1..=MAX_EXECUTIONS).contains(&max) {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn canonical_sections(values: &[DiagnosticSection]) -> Result<Vec<DiagnosticSection>, ErrorCode> {
    if values.is_empty() || values.len() > 3 {
        return Err(ErrorCode::InvalidInput);
    }
    let mut result = values.to_vec();
    result.sort_unstable();
    result.dedup();
    if result.len() != values.len() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(result)
}
fn bounded_sum(values: &[u32]) -> Result<u32, ErrorCode> {
    values.iter().try_fold(0u32, |sum, value| {
        sum.checked_add(*value)
            .filter(|v| *v <= MAX_EXECUTIONS)
            .ok_or(ErrorCode::InvalidInput)
    })
}

#[cfg(test)]
mod tests;
