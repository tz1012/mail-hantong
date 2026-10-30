//! Fixed recent UID ranges and explicit, compare-and-swap progress only.
//!
//! This model performs no I/O and does not confer permission to fetch, publish,
//! or resume mail. UID span measures candidate identifiers, never message count.
use std::fmt;

const BLOCK_SIZE: u64 = 64;
const MAX_VERSION: u64 = i64::MAX as u64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialSyncPlanError {
    InvalidInput,
    Conflict,
}
impl fmt::Display for InitialSyncPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidInput => "invalid initial sync plan input",
            Self::Conflict => "initial sync plan conflict",
        })
    }
}
impl std::error::Error for InitialSyncPlanError {}

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

#[derive(Clone, Eq, PartialEq)]
pub struct InitialSyncStep {
    plan_version: u64,
    ordinal: u8,
    uid_validity: u32,
    after_uid: u32,
    covered_through_uid: u32,
}
impl fmt::Debug for InitialSyncStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InitialSyncStep(<redacted>)")
    }
}
impl InitialSyncStep {
    pub fn plan_version(&self) -> u64 {
        self.plan_version
    }
    pub fn ordinal(&self) -> u8 {
        self.ordinal
    }
    pub fn uid_validity(&self) -> u32 {
        self.uid_validity
    }
    pub fn after_uid(&self) -> u32 {
        self.after_uid
    }
    pub fn covered_through_uid(&self) -> u32 {
        self.covered_through_uid
    }
}

#[derive(Clone, Eq, PartialEq)]
struct Range {
    after: u32,
    covered: u32,
}

#[derive(Clone, Eq, PartialEq)]
pub struct InitialSyncPlan {
    uid_validity: u32,
    target_uid_ceiling: u32,
    ranges: Vec<Range>,
    completed: u8,
    version: u64,
    state: InitialSyncState,
}
impl fmt::Debug for InitialSyncPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InitialSyncPlan(<redacted>)")
    }
}
impl InitialSyncPlan {
    pub fn new(
        uid_validity: u32,
        uid_next: u32,
        recent_pages: u8,
    ) -> Result<Self, InitialSyncPlanError> {
        if uid_validity == 0 || uid_next == 0 || !(1..=8).contains(&recent_pages) {
            return Err(InitialSyncPlanError::InvalidInput);
        }
        let ceiling = u64::from(uid_next) - 1;
        let mut ranges = Vec::new();
        if ceiling > 0 {
            let highest = (ceiling - 1) / BLOCK_SIZE;
            let selected = u64::from(recent_pages).min(highest + 1);
            for offset in 0..selected {
                let block = highest - offset;
                let after = block * BLOCK_SIZE;
                let covered = ceiling.min(after + BLOCK_SIZE);
                ranges.push(Range {
                    after: u32::try_from(after).map_err(|_| InitialSyncPlanError::InvalidInput)?,
                    covered: u32::try_from(covered)
                        .map_err(|_| InitialSyncPlanError::InvalidInput)?,
                });
            }
        }
        Ok(Self {
            uid_validity,
            target_uid_ceiling: u32::try_from(ceiling)
                .map_err(|_| InitialSyncPlanError::InvalidInput)?,
            state: if ranges.is_empty() {
                InitialSyncState::Complete
            } else {
                InitialSyncState::Ready
            },
            ranges,
            completed: 0,
            version: 1,
        })
    }

    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn state(&self) -> InitialSyncState {
        self.state
    }
    pub fn uid_validity(&self) -> u32 {
        self.uid_validity
    }
    pub fn target_uid_ceiling(&self) -> u32 {
        self.target_uid_ceiling
    }
    pub fn selected_lower_uid(&self) -> Option<u32> {
        self.ranges.last().map(|range| range.after + 1)
    }
    pub fn selected_page_count(&self) -> u8 {
        // Construction admits at most eight ranges; callers cannot alter them.
        self.ranges.len() as u8
    }
    pub fn completed_page_count(&self) -> u8 {
        self.completed
    }
    pub fn remaining_page_count(&self) -> u8 {
        self.selected_page_count() - self.completed
    }
    pub fn remaining_uid_span(&self) -> u32 {
        self.ranges
            .iter()
            .skip(usize::from(self.completed))
            .map(|range| range.covered - range.after)
            .sum()
    }
    pub fn not_selected_before_uid(&self) -> Option<u32> {
        self.selected_lower_uid()
            .filter(|lower| *lower > 1)
            .map(|lower| lower - 1)
    }

    fn check_version(&self, expected_version: u64) -> Result<(), InitialSyncPlanError> {
        if expected_version != self.version {
            Err(InitialSyncPlanError::Conflict)
        } else {
            Ok(())
        }
    }
    fn next_version(&self, expected_version: u64) -> Result<u64, InitialSyncPlanError> {
        self.check_version(expected_version)?;
        self.version
            .checked_add(1)
            .filter(|next| *next <= MAX_VERSION)
            .ok_or(InitialSyncPlanError::Conflict)
    }
    fn change_state(
        &mut self,
        expected_version: u64,
        state: InitialSyncState,
    ) -> Result<(), InitialSyncPlanError> {
        let version = self.next_version(expected_version)?;
        self.version = version;
        self.state = state;
        Ok(())
    }

    pub fn begin(
        &mut self,
        expected_version: u64,
    ) -> Result<InitialSyncStep, InitialSyncPlanError> {
        if !matches!(
            self.state,
            InitialSyncState::Ready
                | InitialSyncState::PausedUser
                | InitialSyncState::PausedBudget
                | InitialSyncState::Interrupted
                | InitialSyncState::Failed
        ) {
            return Err(InitialSyncPlanError::Conflict);
        }
        let version = self.next_version(expected_version)?;
        let range = self
            .ranges
            .get(usize::from(self.completed))
            .ok_or(InitialSyncPlanError::Conflict)?;
        let step = InitialSyncStep {
            plan_version: version,
            ordinal: self.completed,
            uid_validity: self.uid_validity,
            after_uid: range.after,
            covered_through_uid: range.covered,
        };
        self.version = version;
        self.state = InitialSyncState::Running;
        Ok(step)
    }

    pub fn complete_step(
        &mut self,
        step: &InitialSyncStep,
        observed_uid_validity: u32,
    ) -> Result<(), InitialSyncPlanError> {
        if self.state != InitialSyncState::Running
            || step.plan_version != self.version
            || step.ordinal != self.completed
            || step.uid_validity != self.uid_validity
            || observed_uid_validity != self.uid_validity
        {
            return Err(InitialSyncPlanError::Conflict);
        }
        let range = self
            .ranges
            .get(usize::from(self.completed))
            .ok_or(InitialSyncPlanError::Conflict)?;
        if step.after_uid != range.after || step.covered_through_uid != range.covered {
            return Err(InitialSyncPlanError::Conflict);
        }
        let version = self.next_version(step.plan_version)?;
        let completed = self
            .completed
            .checked_add(1)
            .filter(|count| *count <= self.selected_page_count())
            .ok_or(InitialSyncPlanError::Conflict)?;
        self.version = version;
        self.completed = completed;
        self.state = if self.remaining_page_count() == 0 {
            InitialSyncState::Complete
        } else {
            InitialSyncState::PausedBudget
        };
        Ok(())
    }

    pub fn pause(&mut self, expected_version: u64) -> Result<(), InitialSyncPlanError> {
        self.check_version(expected_version)?;
        let target = match self.state {
            InitialSyncState::Running => InitialSyncState::Pausing,
            InitialSyncState::Ready
            | InitialSyncState::PausedBudget
            | InitialSyncState::Interrupted
            | InitialSyncState::Failed => InitialSyncState::PausedUser,
            InitialSyncState::Pausing | InitialSyncState::PausedUser => return Ok(()),
            _ => return Err(InitialSyncPlanError::Conflict),
        };
        self.change_state(expected_version, target)
    }

    pub fn acknowledge_pause(&mut self, expected_version: u64) -> Result<(), InitialSyncPlanError> {
        if self.state != InitialSyncState::Pausing {
            return Err(InitialSyncPlanError::Conflict);
        }
        self.change_state(expected_version, InitialSyncState::PausedUser)
    }

    pub fn interrupt(&mut self, expected_version: u64) -> Result<(), InitialSyncPlanError> {
        let target = match self.state {
            InitialSyncState::Running => InitialSyncState::Interrupted,
            InitialSyncState::Pausing => InitialSyncState::PausedUser,
            _ => return Err(InitialSyncPlanError::Conflict),
        };
        self.change_state(expected_version, target)
    }

    pub fn fail(&mut self, expected_version: u64) -> Result<(), InitialSyncPlanError> {
        if self.state != InitialSyncState::Running {
            return Err(InitialSyncPlanError::Conflict);
        }
        self.change_state(expected_version, InitialSyncState::Failed)
    }

    pub fn invalidate(
        &mut self,
        expected_version: u64,
        needs_recovery: bool,
    ) -> Result<(), InitialSyncPlanError> {
        if matches!(
            self.state,
            InitialSyncState::Complete
                | InitialSyncState::StaleBinding
                | InitialSyncState::NeedsRecovery
        ) {
            return Err(InitialSyncPlanError::Conflict);
        }
        self.change_state(
            expected_version,
            if needs_recovery {
                InitialSyncState::NeedsRecovery
            } else {
                InitialSyncState::StaleBinding
            },
        )
    }
}

#[cfg(test)]
mod tests;
