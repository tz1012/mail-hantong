//! Bounded public values for one explicit observation of an existing UID page.
use crate::{
    ErrorCode,
    uid_page::{UidMailboxIdentity, UidPageRequest, UidPageState, UidPageStatusRequest},
};
use std::fmt;

#[derive(Clone, Eq, PartialEq)]
pub struct UidReconcileRequest(UidPageRequest);
impl UidReconcileRequest {
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
        let mut out = b"mail-hantong.imap-uid-reconcile-intent.v1\0".to_vec();
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
pub struct UidReconcileProgress {
    generation: u64,
    frozen_cursor_version: u64,
    source_cursor_version: u64,
    after_uid: u32,
    through_uid: u32,
    observed_count: u32,
    flags_changed_count: u32,
    newly_absent_count: u32,
    remaining_pages: u32,
    pass_complete: bool,
}
impl UidReconcileProgress {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        generation: u64,
        frozen_cursor_version: u64,
        source_cursor_version: u64,
        after_uid: u32,
        through_uid: u32,
        observed_count: u32,
        flags_changed_count: u32,
        newly_absent_count: u32,
        remaining_pages: u32,
        pass_complete: bool,
    ) -> Result<Self, ErrorCode> {
        if [generation, frozen_cursor_version, source_cursor_version]
            .iter()
            .any(|&v| v == 0 || v > i64::MAX as u64)
            || source_cursor_version > frozen_cursor_version
            || after_uid >= through_uid
            || through_uid == u32::MAX
            || through_uid - after_uid > 64
            || observed_count == 0
            || observed_count > through_uid - after_uid
            || u64::from(flags_changed_count) + u64::from(newly_absent_count)
                > u64::from(observed_count)
            || remaining_pages > 100000
            || pass_complete != (remaining_pages == 0)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            generation,
            frozen_cursor_version,
            source_cursor_version,
            after_uid,
            through_uid,
            observed_count,
            flags_changed_count,
            newly_absent_count,
            remaining_pages,
            pass_complete,
        })
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn frozen_cursor_version(&self) -> u64 {
        self.frozen_cursor_version
    }
    pub fn source_cursor_version(&self) -> u64 {
        self.source_cursor_version
    }
    pub fn after_uid(&self) -> u32 {
        self.after_uid
    }
    pub fn through_uid(&self) -> u32 {
        self.through_uid
    }
    pub fn observed_count(&self) -> u32 {
        self.observed_count
    }
    pub fn flags_changed_count(&self) -> u32 {
        self.flags_changed_count
    }
    pub fn newly_absent_count(&self) -> u32 {
        self.newly_absent_count
    }
    pub fn remaining_pages(&self) -> u32 {
        self.remaining_pages
    }
    pub fn pass_complete(&self) -> bool {
        self.pass_complete
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct UidReconcileReceipt {
    account_id: String,
    execution_id: [u8; 32],
    state: UidPageState,
    result: Option<UidReconcileProgress>,
    error: Option<ErrorCode>,
}
impl UidReconcileReceipt {
    pub fn new(
        account_id: &str,
        execution_id: [u8; 32],
        state: UidPageState,
        result: Option<UidReconcileProgress>,
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
    pub fn result(&self) -> Option<&UidReconcileProgress> {
        self.result.as_ref()
    }
    pub fn error(&self) -> Option<ErrorCode> {
        self.error
    }
}
macro_rules! redacted {($($name:ty),+)=>{$(impl fmt::Debug for $name {fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result{f.write_str(concat!(stringify!($name)," { <redacted> }"))}})+};}
redacted!(
    UidReconcileRequest,
    UidReconcileProgress,
    UidReconcileReceipt
);
#[cfg(test)]
#[path = "uid_reconcile_tests.rs"]
mod tests;
