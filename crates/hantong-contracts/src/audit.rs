//! Bounded, non-serializable audit value types.
//!
//! These values describe an execution record. They are not authentication or
//! approval evidence and contain no free-form request or message content.

use crate::{ErrorCode, auth::RequestedScope, client::AuditActor};
use std::fmt;

const MAX_EPOCH: u64 = i64::MAX as u64;

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum AuditTool {
    AccountsList,
    SendExecute,
}
impl AuditTool {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AccountsList => "mail.accounts.list",
            Self::SendExecute => "mail.send.execute",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ErrorCode> {
        match value {
            "mail.accounts.list" => Ok(Self::AccountsList),
            "mail.send.execute" => Ok(Self::SendExecute),
            _ => Err(ErrorCode::InvalidInput),
        }
    }
}
impl fmt::Debug for AuditTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuditTool(<redacted>)")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AuditBegin {
    execution_id: [u8; 32],
    registration_id: [u8; 16],
    profile_epoch: u64,
    tool: AuditTool,
    account_ids: Vec<String>,
}
impl AuditBegin {
    pub fn new(
        execution_id: [u8; 32],
        registration_id: [u8; 16],
        profile_epoch: u64,
        tool: AuditTool,
        account_ids: Vec<String>,
    ) -> Result<Self, ErrorCode> {
        if profile_epoch == 0 || profile_epoch > MAX_EPOCH {
            return Err(ErrorCode::InvalidInput);
        }
        let scope = RequestedScope::new(account_ids, Vec::new())?;
        if scope.accounts().is_empty() {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            execution_id,
            registration_id,
            profile_epoch,
            tool,
            account_ids: scope.accounts().to_vec(),
        })
    }
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn registration_id(&self) -> &[u8; 16] {
        &self.registration_id
    }
    pub fn profile_epoch(&self) -> u64 {
        self.profile_epoch
    }
    pub fn tool(&self) -> AuditTool {
        self.tool
    }
    pub fn account_ids(&self) -> &[String] {
        &self.account_ids
    }
}
impl fmt::Debug for AuditBegin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuditBegin { <redacted> }")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AuditTerminal {
    state: TerminalState,
}
#[derive(Clone, Eq, PartialEq)]
enum TerminalState {
    Succeeded { result_count: Option<u64> },
    Failed { error: ErrorCode },
}
impl AuditTerminal {
    pub fn succeeded(result_count: Option<u64>) -> Result<Self, ErrorCode> {
        if result_count.is_some_and(|value| value > MAX_EPOCH) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            state: TerminalState::Succeeded { result_count },
        })
    }
    pub fn failed(error: ErrorCode) -> Self {
        Self {
            state: TerminalState::Failed { error },
        }
    }
    pub fn is_success(&self) -> bool {
        matches!(self.state, TerminalState::Succeeded { .. })
    }
    pub fn result_count(&self) -> Option<u64> {
        match self.state {
            TerminalState::Succeeded { result_count } => result_count,
            TerminalState::Failed { .. } => None,
        }
    }
    pub fn error(&self) -> Option<ErrorCode> {
        match self.state {
            TerminalState::Succeeded { .. } => None,
            TerminalState::Failed { error } => Some(error),
        }
    }
}
impl fmt::Debug for AuditTerminal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self {
                state: TerminalState::Succeeded { .. },
            } => f.write_str("AuditTerminal::Succeeded(<redacted>)"),
            Self {
                state: TerminalState::Failed { error },
            } => f
                .debug_struct("AuditTerminal::Failed")
                .field("error", error)
                .finish(),
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AuditCompletion {
    completed_at_utc_ms: i64,
    terminal: AuditTerminal,
}
impl AuditCompletion {
    pub fn new(completed_at_utc_ms: i64, terminal: AuditTerminal) -> Result<Self, ErrorCode> {
        if completed_at_utc_ms <= 0 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            completed_at_utc_ms,
            terminal,
        })
    }
    pub fn completed_at_utc_ms(&self) -> i64 {
        self.completed_at_utc_ms
    }
    pub fn terminal(&self) -> &AuditTerminal {
        &self.terminal
    }
}
impl fmt::Debug for AuditCompletion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuditCompletion { <redacted> }")
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct AuditRecord {
    sequence: u64,
    execution_id: [u8; 32],
    registration_id: [u8; 16],
    profile_epoch: u64,
    tool: AuditTool,
    actor: AuditActor,
    account_ids: Vec<String>,
    started_at_utc_ms: i64,
    completion: Option<AuditCompletion>,
}
impl AuditRecord {
    pub fn begin(
        sequence: u64,
        audit: &AuditBegin,
        actor: AuditActor,
        started_at_utc_ms: i64,
    ) -> Result<Self, ErrorCode> {
        if sequence == 0 || sequence > MAX_EPOCH || started_at_utc_ms <= 0 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            sequence,
            execution_id: *audit.execution_id(),
            registration_id: *audit.registration_id(),
            profile_epoch: audit.profile_epoch(),
            tool: audit.tool(),
            actor,
            account_ids: audit.account_ids().to_vec(),
            started_at_utc_ms,
            completion: None,
        })
    }
    pub fn finish(
        mut self,
        completed_at_utc_ms: i64,
        terminal: AuditTerminal,
    ) -> Result<Self, ErrorCode> {
        if self.completion.is_some() || completed_at_utc_ms < self.started_at_utc_ms {
            return Err(ErrorCode::InvalidInput);
        }
        self.completion = Some(AuditCompletion::new(completed_at_utc_ms, terminal)?);
        Ok(self)
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn registration_id(&self) -> &[u8; 16] {
        &self.registration_id
    }
    pub fn profile_epoch(&self) -> u64 {
        self.profile_epoch
    }
    pub fn tool(&self) -> AuditTool {
        self.tool
    }
    pub fn actor(&self) -> &AuditActor {
        &self.actor
    }
    pub fn account_ids(&self) -> &[String] {
        &self.account_ids
    }
    pub fn started_at_utc_ms(&self) -> i64 {
        self.started_at_utc_ms
    }
    pub fn completion(&self) -> Option<&AuditCompletion> {
        self.completion.as_ref()
    }
}
impl fmt::Debug for AuditRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuditRecord { <redacted> }")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai10_begin_sorts_and_bounds_accounts() {
        let value = AuditBegin::new(
            [1; 32],
            [2; 16],
            1,
            AuditTool::AccountsList,
            vec!["z".into(), "a".into()],
        )
        .unwrap();
        assert_eq!(value.account_ids(), &["a", "z"]);
        assert_eq!(
            AuditTool::parse("mail.send.execute"),
            Ok(AuditTool::SendExecute)
        );
        assert!(AuditTool::parse("mail.other").is_err());
        assert!(
            AuditBegin::new(
                [1; 32],
                [2; 16],
                MAX_EPOCH,
                AuditTool::AccountsList,
                vec!["a".repeat(128)]
            )
            .is_ok()
        );
        assert!(
            AuditBegin::new(
                [1; 32],
                [2; 16],
                MAX_EPOCH + 1,
                AuditTool::AccountsList,
                vec!["a".into()]
            )
            .is_err()
        );
        let ids = (0..128).map(|i| format!("a{i}")).collect::<Vec<_>>();
        assert!(AuditBegin::new([1; 32], [2; 16], 1, AuditTool::AccountsList, ids).is_ok());
        let ids = (0..129).map(|i| format!("a{i}")).collect::<Vec<_>>();
        assert!(AuditBegin::new([1; 32], [2; 16], 1, AuditTool::AccountsList, ids).is_err());
    }
    #[test]
    fn ai10_rejects_empty_duplicate_invalid_and_epoch_overflow() {
        for accounts in [
            vec![],
            vec!["a".into(), "a".into()],
            vec!["a b".into()],
            vec!["한글".into()],
        ] {
            assert!(
                AuditBegin::new([0; 32], [0; 16], 1, AuditTool::AccountsList, accounts).is_err()
            );
        }
        assert!(
            AuditBegin::new(
                [0; 32],
                [0; 16],
                0,
                AuditTool::AccountsList,
                vec!["a".into()]
            )
            .is_err()
        );
        assert!(
            AuditBegin::new(
                [0; 32],
                [0; 16],
                u64::MAX,
                AuditTool::AccountsList,
                vec!["a".into()]
            )
            .is_err()
        );
    }
    #[test]
    fn ai10_terminal_and_record_boundaries_are_explicit() {
        assert!(AuditTerminal::succeeded(Some(MAX_EPOCH)).is_ok());
        assert!(AuditTerminal::succeeded(Some(u64::MAX)).is_err());
        let begin = AuditBegin::new(
            [3; 32],
            [4; 16],
            1,
            AuditTool::SendExecute,
            vec!["a".into()],
        )
        .unwrap();
        let actor = AuditActor::new(crate::client::ClientKind::Cli, "fixture".into()).unwrap();
        let record = AuditRecord::begin(1, &begin, actor.clone(), 10).unwrap();
        assert!(AuditRecord::begin(0, &begin, actor.clone(), 10).is_err());
        assert!(AuditRecord::begin(MAX_EPOCH, &begin, actor.clone(), 10).is_ok());
        assert!(AuditRecord::begin(MAX_EPOCH + 1, &begin, actor.clone(), 10).is_err());
        assert!(AuditRecord::begin(1, &begin, actor, 0).is_err());
        assert!(AuditCompletion::new(0, AuditTerminal::failed(ErrorCode::InternalError)).is_err());
        assert!(AuditCompletion::new(-1, AuditTerminal::failed(ErrorCode::InternalError)).is_err());
        assert!(
            record
                .clone()
                .finish(9, AuditTerminal::failed(ErrorCode::SendUnknown))
                .is_err()
        );
        let finished = record
            .finish(10, AuditTerminal::failed(ErrorCode::SendUnknown))
            .unwrap();
        assert_eq!(
            finished.completion().unwrap().terminal().error(),
            Some(ErrorCode::SendUnknown)
        );
        assert!(
            finished
                .finish(11, AuditTerminal::failed(ErrorCode::InternalError))
                .is_err()
        );
    }
    #[test]
    fn ai10_debug_redacts_identifiers_and_has_no_free_text() {
        let begin = AuditBegin::new(
            [7; 32],
            [8; 16],
            1,
            AuditTool::AccountsList,
            vec!["account-secret".into()],
        )
        .unwrap();
        let text = format!(
            "{begin:?}{:?}",
            AuditTerminal::failed(ErrorCode::InternalError)
        );
        assert!(!text.contains("execution-id"));
        assert!(!text.contains("account-secret"));
    }
}
