//! Explicit, bounded initial-preview requests and receipts. IDs are not authority.
use crate::{ErrorCode, uid_page::UidMailboxIdentity, wire_hex};
use std::fmt;

const MAX_VERSION: u64 = i64::MAX as u64;
fn id(value: &str) -> Result<String, ErrorCode> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(value.into())
    }
}
fn positive(value: u64) -> Result<(), ErrorCode> {
    if value == 0 || value > MAX_VERSION {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn frame(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialSyncAction {
    Start,
    Resume,
    Pause,
}
impl InitialSyncAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Start => "initial-start",
            Self::Resume => "initial-resume",
            Self::Pause => "initial-pause",
        }
    }
    pub fn parse(s: &str) -> Result<Self, ErrorCode> {
        match s {
            "initial-start" => Ok(Self::Start),
            "initial-resume" => Ok(Self::Resume),
            "initial-pause" => Ok(Self::Pause),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialSyncJobState {
    Admitted,
    Running,
    Succeeded,
    Failed,
}
impl InitialSyncJobState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }
    pub fn parse(s: &str) -> Result<Self, ErrorCode> {
        match s {
            "admitted" => Ok(Self::Admitted),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialSyncState {
    Ready,
    Running,
    Pausing,
    PausedUser,
    PausedBudget,
    Interrupted,
    Complete,
    Failed,
    StaleBinding,
    NeedsRecovery,
}
impl InitialSyncState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Pausing => "pausing",
            Self::PausedUser => "paused_user",
            Self::PausedBudget => "paused_budget",
            Self::Interrupted => "interrupted",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::StaleBinding => "stale_binding",
            Self::NeedsRecovery => "needs_recovery",
        }
    }
    pub fn parse(s: &str) -> Result<Self, ErrorCode> {
        match s {
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "pausing" => Ok(Self::Pausing),
            "paused_user" => Ok(Self::PausedUser),
            "paused_budget" => Ok(Self::PausedBudget),
            "interrupted" => Ok(Self::Interrupted),
            "complete" => Ok(Self::Complete),
            "failed" => Ok(Self::Failed),
            "stale_binding" => Ok(Self::StaleBinding),
            "needs_recovery" => Ok(Self::NeedsRecovery),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct InitialSyncMutation {
    operation: String,
    key: String,
    hash: String,
}
impl fmt::Debug for InitialSyncMutation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InitialSyncMutation(<redacted>)")
    }
}
impl InitialSyncMutation {
    pub fn new(operation: &str, key: &str, hash: &str) -> Result<Self, ErrorCode> {
        wire_hex::decode_fixed::<32>(hash)?;
        if hash.len() != 64
            || hash
                .bytes()
                .any(|b| !b.is_ascii_digit() && !matches!(b, b'a'..=b'f'))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            operation: id(operation)?,
            key: id(key)?,
            hash: hash.into(),
        })
    }
    pub fn operation_id(&self) -> &str {
        &self.operation
    }
    pub fn idempotency_key(&self) -> &str {
        &self.key
    }
    pub fn intent_hash(&self) -> &str {
        &self.hash
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct InitialSyncRequest {
    account: String,
    mailbox: UidMailboxIdentity,
    mutation: InitialSyncMutation,
    action: InitialSyncAction,
    catalog: Option<u64>,
    generation: Option<u64>,
    recent: Option<u8>,
    plan: Option<[u8; 32]>,
    version: Option<u64>,
}
impl fmt::Debug for InitialSyncRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InitialSyncRequest(<redacted>)")
    }
}
impl InitialSyncRequest {
    pub fn new_start(
        account: &str,
        mailbox: UidMailboxIdentity,
        mutation: InitialSyncMutation,
        catalog: u64,
        generation: u64,
        recent: u8,
    ) -> Result<Self, ErrorCode> {
        positive(catalog)?;
        if generation >= MAX_VERSION || !(1..=8).contains(&recent) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account: id(account)?,
            mailbox,
            mutation,
            action: InitialSyncAction::Start,
            catalog: Some(catalog),
            generation: Some(generation),
            recent: Some(recent),
            plan: None,
            version: None,
        })
    }
    pub fn new_resume(
        account: &str,
        mailbox: UidMailboxIdentity,
        mutation: InitialSyncMutation,
        plan: [u8; 32],
        version: u64,
        catalog: u64,
    ) -> Result<Self, ErrorCode> {
        positive(version)?;
        positive(catalog)?;
        if plan == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account: id(account)?,
            mailbox,
            mutation,
            action: InitialSyncAction::Resume,
            catalog: Some(catalog),
            generation: None,
            recent: None,
            plan: Some(plan),
            version: Some(version),
        })
    }
    pub fn new_pause(
        account: &str,
        mailbox: UidMailboxIdentity,
        mutation: InitialSyncMutation,
        plan: [u8; 32],
        version: u64,
    ) -> Result<Self, ErrorCode> {
        positive(version)?;
        if plan == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account: id(account)?,
            mailbox,
            mutation,
            action: InitialSyncAction::Pause,
            catalog: None,
            generation: None,
            recent: None,
            plan: Some(plan),
            version: Some(version),
        })
    }
    pub fn action(&self) -> InitialSyncAction {
        self.action
    }
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account)
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub fn operation_id(&self) -> &str {
        self.mutation.operation_id()
    }
    pub fn idempotency_key(&self) -> &str {
        self.mutation.idempotency_key()
    }
    pub fn intent_hash(&self) -> &str {
        self.mutation.intent_hash()
    }
    pub fn expected_catalog_generation(&self) -> Option<u64> {
        self.catalog
    }
    pub fn expected_preview_generation(&self) -> Option<u64> {
        self.generation
    }
    pub fn recent_pages(&self) -> Option<u8> {
        self.recent
    }
    pub fn plan_id(&self) -> Option<&[u8; 32]> {
        self.plan.as_ref()
    }
    pub fn expected_plan_version(&self) -> Option<u64> {
        self.version
    }
    pub fn intent_bytes(&self) -> Vec<u8> {
        let mut out = b"mail-hantong.imap-initial-preview-intent.v1\0".to_vec();
        for value in [
            self.action.as_str(),
            &self.account,
            self.mailbox.wire_hex(),
            self.mailbox.encoding().as_str(),
            self.operation_id(),
            self.idempotency_key(),
        ] {
            frame(&mut out, value.as_bytes());
        }
        match self.action {
            InitialSyncAction::Start => {
                out.extend_from_slice(&self.catalog.unwrap().to_be_bytes());
                out.extend_from_slice(&self.generation.unwrap().to_be_bytes());
                out.push(self.recent.unwrap());
            }
            InitialSyncAction::Resume => {
                out.extend_from_slice(self.plan.as_ref().unwrap());
                out.extend_from_slice(&self.version.unwrap().to_be_bytes());
                out.extend_from_slice(&self.catalog.unwrap().to_be_bytes());
            }
            InitialSyncAction::Pause => {
                out.extend_from_slice(self.plan.as_ref().unwrap());
                out.extend_from_slice(&self.version.unwrap().to_be_bytes());
            }
        }
        out
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct InitialSyncPlanRequest {
    account: String,
    mailbox: UidMailboxIdentity,
    plan: [u8; 32],
}
impl fmt::Debug for InitialSyncPlanRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InitialSyncPlanRequest(<redacted>)")
    }
}
impl InitialSyncPlanRequest {
    pub fn new(
        account: &str,
        mailbox: UidMailboxIdentity,
        plan: [u8; 32],
    ) -> Result<Self, ErrorCode> {
        if plan == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account: id(account)?,
            mailbox,
            plan,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn account_ids(&self) -> &[String] {
        std::slice::from_ref(&self.account)
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub fn plan_id(&self) -> &[u8; 32] {
        &self.plan
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialSyncProgress {
    validity: u32,
    next: u32,
    recent: u8,
    completed: u8,
    state: InitialSyncState,
    version: u64,
}
impl InitialSyncProgress {
    pub fn new(
        validity: u32,
        next: u32,
        recent: u8,
        completed: u8,
        state: InitialSyncState,
        version: u64,
    ) -> Result<Self, ErrorCode> {
        positive(version)?;
        if validity == 0 || next == 0 || !(1..=8).contains(&recent) {
            return Err(ErrorCode::InvalidInput);
        }
        let v = Self {
            validity,
            next,
            recent,
            completed,
            state,
            version,
        };
        let selected = v.selected_page_count();
        if completed > selected
            || (state == InitialSyncState::Complete) != (completed == selected)
            || (state == InitialSyncState::Ready && (completed != 0 || version != 1))
            || matches!(state, InitialSyncState::Running | InitialSyncState::Pausing)
                && completed == selected
            || state == InitialSyncState::PausedBudget && completed == 0
        {
            return Err(ErrorCode::InvalidInput);
        }
        let base = 1 + 2 * u64::from(completed);
        let minimum = match state {
            InitialSyncState::Ready => 1,
            InitialSyncState::Running => base + 1,
            InitialSyncState::Pausing
            | InitialSyncState::Interrupted
            | InitialSyncState::Failed => base + 2,
            InitialSyncState::PausedUser
            | InitialSyncState::StaleBinding
            | InitialSyncState::NeedsRecovery => base + 1,
            InitialSyncState::PausedBudget | InitialSyncState::Complete => base,
        };
        if version < minimum
            || (selected == 0 && version != 1)
            || (completed == 0 && state == InitialSyncState::PausedUser && version == 3)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(v)
    }
    pub fn state(&self) -> InitialSyncState {
        self.state
    }
    pub fn model_version(&self) -> u64 {
        self.version
    }
    pub fn uid_validity(&self) -> u32 {
        self.validity
    }
    pub fn uid_next(&self) -> u32 {
        self.next
    }
    pub fn recent_pages(&self) -> u8 {
        self.recent
    }
    pub fn target_uid_ceiling(&self) -> u32 {
        self.next - 1
    }
    pub fn selected_page_count(&self) -> u8 {
        if self.next == 1 {
            0
        } else {
            u64::from(self.recent).min((u64::from(self.next) - 2) / 64 + 1) as u8
        }
    }
    pub fn selected_lower_uid(&self) -> Option<u32> {
        let n = self.selected_page_count();
        (n > 0).then(|| ((((u64::from(self.next) - 2) / 64 + 1) - u64::from(n)) * 64 + 1) as u32)
    }
    pub fn completed_page_count(&self) -> u8 {
        self.completed
    }
    pub fn remaining_page_count(&self) -> u8 {
        self.selected_page_count() - self.completed
    }
    pub fn remaining_uid_span(&self) -> u32 {
        if self.remaining_page_count() == 0 {
            0
        } else {
            let ceiling = if self.completed == 0 {
                self.next - 1
            } else {
                (((u64::from(self.next) - 2) / 64 + 1 - u64::from(self.completed)) * 64) as u32
            };
            ceiling - self.selected_lower_uid().unwrap() + 1
        }
    }
    pub fn not_selected_before_uid(&self) -> Option<u32> {
        self.selected_lower_uid().filter(|n| *n > 1).map(|n| n - 1)
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct InitialSyncPlanIdentity {
    plan: [u8; 32],
    account: String,
    mailbox: UidMailboxIdentity,
    generation: u64,
}
impl fmt::Debug for InitialSyncPlanIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InitialSyncPlanIdentity(<redacted>)")
    }
}
impl InitialSyncPlanIdentity {
    pub fn new(
        plan: [u8; 32],
        account: &str,
        mailbox: UidMailboxIdentity,
        generation: u64,
    ) -> Result<Self, ErrorCode> {
        positive(generation)?;
        if plan == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            plan,
            account: id(account)?,
            mailbox,
            generation,
        })
    }
    pub fn plan_id(&self) -> &[u8; 32] {
        &self.plan
    }
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        &self.mailbox
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialSyncPlanSnapshot {
    identity: InitialSyncPlanIdentity,
    version: u64,
    progress: InitialSyncProgress,
    current: bool,
    binding: bool,
    at: i64,
}
impl InitialSyncPlanSnapshot {
    pub fn new(
        identity: InitialSyncPlanIdentity,
        version: u64,
        progress: InitialSyncProgress,
        current: bool,
        binding: bool,
        at: i64,
    ) -> Result<Self, ErrorCode> {
        positive(version)?;
        if at <= 0 || binding && !current {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            identity,
            version,
            progress,
            current,
            binding,
            at,
        })
    }
    pub fn identity(&self) -> &InitialSyncPlanIdentity {
        &self.identity
    }
    pub fn plan_id(&self) -> &[u8; 32] {
        self.identity.plan_id()
    }
    pub fn account_id(&self) -> &str {
        self.identity.account_id()
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        self.identity.mailbox()
    }
    pub fn generation(&self) -> u64 {
        self.identity.generation()
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn progress(&self) -> &InitialSyncProgress {
        &self.progress
    }
    pub fn is_current(&self) -> bool {
        self.current
    }
    pub fn binding_current(&self) -> bool {
        self.binding
    }
    pub fn stored_at_utc_ms(&self) -> i64 {
        self.at
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct InitialSyncReceipt {
    account: String,
    execution: [u8; 32],
    action: InitialSyncAction,
    state: InitialSyncJobState,
    plan: Option<InitialSyncPlanSnapshot>,
    error: Option<ErrorCode>,
}
impl fmt::Debug for InitialSyncReceipt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InitialSyncReceipt(<redacted>)")
    }
}
impl InitialSyncReceipt {
    pub fn new(
        account: &str,
        execution: [u8; 32],
        action: InitialSyncAction,
        state: InitialSyncJobState,
        plan: Option<InitialSyncPlanSnapshot>,
        error: Option<ErrorCode>,
    ) -> Result<Self, ErrorCode> {
        if execution == [0; 32]
            || (state == InitialSyncJobState::Failed) != error.is_some()
            || state == InitialSyncJobState::Succeeded && plan.is_none()
            || !state.is_terminal() && plan.is_some()
            || plan.as_ref().is_some_and(|p| p.account_id() != account)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account: id(account)?,
            execution,
            action,
            state,
            plan,
            error,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account
    }
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution
    }
    pub fn action(&self) -> InitialSyncAction {
        self.action
    }
    pub fn state(&self) -> InitialSyncJobState {
        self.state
    }
    pub fn plan(&self) -> Option<&InitialSyncPlanSnapshot> {
        self.plan.as_ref()
    }
    pub fn error(&self) -> Option<ErrorCode> {
        self.error
    }
}

#[cfg(test)]
mod tests;
