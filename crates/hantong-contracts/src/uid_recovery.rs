//! Bounded public values for one explicit first-page observation after UIDVALIDITY changes.
use crate::{
    ErrorCode,
    uid_page::{UidMailboxIdentity, UidPageRequest, UidPageState, UidPageStatusRequest},
};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct UidRecoveryRequest(UidPageRequest);
impl UidRecoveryRequest {
    pub fn new(
        account_id: &str,
        mailbox: UidMailboxIdentity,
        operation_id: &str,
        idempotency_key: &str,
        expected_catalog_generation: u64,
        expected_cursor_version: u64,
        intent_hash: &str,
    ) -> Result<Self, ErrorCode> {
        if expected_cursor_version == 0 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self(UidPageRequest::new(
            account_id,
            mailbox,
            operation_id,
            idempotency_key,
            expected_catalog_generation,
            expected_cursor_version,
            intent_hash,
        )?))
    }
    pub fn account_id(&self) -> &str {
        self.0.account_id()
    }
    pub fn mailbox(&self) -> &UidMailboxIdentity {
        self.0.mailbox()
    }
    pub fn operation_id(&self) -> &str {
        self.0.operation_id()
    }
    pub fn idempotency_key(&self) -> &str {
        self.0.idempotency_key()
    }
    pub fn expected_catalog_generation(&self) -> u64 {
        self.0.expected_catalog_generation()
    }
    pub fn expected_cursor_version(&self) -> u64 {
        self.0.expected_cursor_version()
    }
    pub fn intent_hash(&self) -> &str {
        self.0.intent_hash()
    }
    pub fn account_ids(&self) -> &[String] {
        self.0.account_ids()
    }
    pub fn intent_bytes(&self) -> Vec<u8> {
        let mut out = b"mail-hantong.imap-uid-recovery-intent.v1\0".to_vec();
        for value in [
            self.account_id(),
            self.operation_id(),
            self.idempotency_key(),
            self.mailbox().encoding().as_str(),
            self.mailbox().wire_hex(),
        ] {
            out.extend_from_slice(&(value.len() as u32).to_be_bytes());
            out.extend_from_slice(value.as_bytes());
        }
        out.extend_from_slice(&self.expected_catalog_generation().to_be_bytes());
        out.extend_from_slice(&self.expected_cursor_version().to_be_bytes());
        out
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct UidRecoveryProgress {
    epoch_generation: u64,
    cursor_version: u64,
    covered_through_uid: u32,
    inserted_count: u32,
}
impl UidRecoveryProgress {
    pub fn new(
        epoch_generation: u64,
        cursor_version: u64,
        covered_through_uid: u32,
        inserted_count: u32,
    ) -> Result<Self, ErrorCode> {
        if epoch_generation < 2
            || epoch_generation > cursor_version
            || cursor_version > i64::MAX as u64
            || covered_through_uid > 64
            || inserted_count > covered_through_uid
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            epoch_generation,
            cursor_version,
            covered_through_uid,
            inserted_count,
        })
    }
    pub fn epoch_generation(&self) -> u64 {
        self.epoch_generation
    }
    pub fn cursor_version(&self) -> u64 {
        self.cursor_version
    }
    pub fn covered_through_uid(&self) -> u32 {
        self.covered_through_uid
    }
    pub fn inserted_count(&self) -> u32 {
        self.inserted_count
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct UidRecoveryReceipt {
    account_id: String,
    execution_id: [u8; 32],
    state: UidPageState,
    result: Option<UidRecoveryProgress>,
    error: Option<ErrorCode>,
}
impl UidRecoveryReceipt {
    pub fn new(
        account_id: &str,
        execution_id: [u8; 32],
        state: UidPageState,
        result: Option<UidRecoveryProgress>,
        error: Option<ErrorCode>,
    ) -> Result<Self, ErrorCode> {
        UidPageStatusRequest::new(account_id, execution_id)?;
        if !matches!(
            (state, result.as_ref(), error),
            (UidPageState::Unresolved, None, None)
                | (UidPageState::Succeeded, Some(_), None)
                | (UidPageState::Failed, None, Some(_))
        ) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            account_id: account_id.into(),
            execution_id,
            state,
            result,
            error,
        })
    }
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn execution_id(&self) -> &[u8; 32] {
        &self.execution_id
    }
    pub fn state(&self) -> UidPageState {
        self.state
    }
    pub fn result(&self) -> Option<&UidRecoveryProgress> {
        self.result.as_ref()
    }
    pub fn error(&self) -> Option<ErrorCode> {
        self.error
    }
}
macro_rules! redacted {($($name:ty),+)=>{$(impl fmt::Debug for $name {fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str(concat!(stringify!($name)," { <redacted> }"))}})+};}
redacted!(UidRecoveryRequest, UidRecoveryProgress, UidRecoveryReceipt);
#[cfg(test)]
#[path = "uid_recovery_tests.rs"]
mod tests;
