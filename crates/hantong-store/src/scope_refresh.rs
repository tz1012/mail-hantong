//! Default CLI scope history is stored data, never proof of user presence.
use crate::{RegistrationRecord, Store};
use hantong_contracts::ErrorCode;
use std::fmt;
#[cfg(windows)]
mod storage;
#[cfg(windows)]
use storage::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeRefreshPhase {
    Pending,
    Staged,
    Active,
    Denied,
    Expired,
}

#[derive(Clone, Eq, PartialEq)]
pub struct CliScopeRefresh {
    refresh_id: [u8; 32],
    base_registration: RegistrationRecord,
    target_accounts: Vec<String>,
    client_public_key_hash: [u8; 32],
    server_public_key_hash: [u8; 32],
    payload_hash: [u8; 32],
    phase: ScopeRefreshPhase,
    version: u64,
    created_at_utc_ms: i64,
    expires_at_utc_ms: i64,
    updated_at_utc_ms: i64,
    decided_at_utc_ms: Option<i64>,
    activated_at_utc_ms: Option<i64>,
}
impl fmt::Debug for CliScopeRefresh {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CliScopeRefresh { <redacted> }")
    }
}
impl CliScopeRefresh {
    pub fn refresh_id(&self) -> &[u8; 32] {
        &self.refresh_id
    }
    pub fn base_registration(&self) -> &RegistrationRecord {
        &self.base_registration
    }
    pub fn target_accounts(&self) -> &[String] {
        &self.target_accounts
    }
    pub fn target_profile_epoch(&self) -> u64 {
        self.base_registration.profile_epoch + 1
    }
    pub fn client_public_key_hash(&self) -> &[u8; 32] {
        &self.client_public_key_hash
    }
    pub fn server_public_key_hash(&self) -> &[u8; 32] {
        &self.server_public_key_hash
    }
    pub fn payload_hash(&self) -> &[u8; 32] {
        &self.payload_hash
    }
    pub fn phase(&self) -> ScopeRefreshPhase {
        self.phase
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
    pub fn updated_at_utc_ms(&self) -> i64 {
        self.updated_at_utc_ms
    }
    pub fn decided_at_utc_ms(&self) -> Option<i64> {
        self.decided_at_utc_ms
    }
    pub fn activated_at_utc_ms(&self) -> Option<i64> {
        self.activated_at_utc_ms
    }
}
impl Store {
    pub fn create_cli_scope_refresh(
        &self,
        refresh_id: [u8; 32],
        target_accounts: Vec<String>,
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<CliScopeRefresh, ErrorCode> {
        if refresh_id == [0; 32] || expected_server_public_key_hash == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        validate_accounts(&target_accounts)?;
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Immediate, |tx| {
                create(
                    tx,
                    refresh_id,
                    target_accounts,
                    expected_server_public_key_hash,
                )
            })
        }
    }
    pub fn load_cli_scope_refresh(
        &self,
        id: [u8; 32],
    ) -> Result<Option<CliScopeRefresh>, ErrorCode> {
        if id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Deferred, |tx| load(tx, id))
        }
    }
    pub fn load_pending_cli_scope_refresh(&self) -> Result<Option<CliScopeRefresh>, ErrorCode> {
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Deferred, |tx| {
                Ok(list_phase(tx, ScopeRefreshPhase::Pending)?.pop())
            })
        }
    }
    pub fn list_staged_cli_scope_refreshes(&self) -> Result<Vec<CliScopeRefresh>, ErrorCode> {
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Deferred, |tx| {
                list_phase(tx, ScopeRefreshPhase::Staged)
            })
        }
    }
    pub fn deny_cli_scope_refresh(
        &self,
        expected: &CliScopeRefresh,
    ) -> Result<CliScopeRefresh, ErrorCode> {
        self.change_scope_refresh(expected, ScopeRefreshPhase::Denied)
    }
    pub fn expire_cli_scope_refresh(&self, expected: &CliScopeRefresh) -> Result<bool, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Immediate, |tx| {
                expire(tx, expected)
            })
        }
    }
    pub fn stage_cli_scope_refresh(
        &self,
        expected: &CliScopeRefresh,
    ) -> Result<CliScopeRefresh, ErrorCode> {
        self.change_scope_refresh(expected, ScopeRefreshPhase::Staged)
    }
    pub fn activate_cli_scope_refresh(
        &self,
        expected: &CliScopeRefresh,
    ) -> Result<RegistrationRecord, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = expected;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Immediate, |tx| {
                activate(tx, expected)
            })
        }
    }
    fn change_scope_refresh(
        &self,
        expected: &CliScopeRefresh,
        phase: ScopeRefreshPhase,
    ) -> Result<CliScopeRefresh, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = (expected, phase);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Immediate, |tx| {
                change(tx, expected, phase)
            })
        }
    }
}
#[cfg(windows)]
pub(crate) fn validate_history(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    context(connection)
        .map(|_| ())
        .map_err(|_| rusqlite::Error::InvalidQuery)
}
#[cfg(windows)]
pub(super) fn validate_registration(
    connection: &rusqlite::Connection,
    record: &RegistrationRecord,
) -> rusqlite::Result<()> {
    if record.client_kind == crate::ClientKind::Cli
        && record.display_label == crate::DEFAULT_CLI_LABEL
    {
        let current = context(connection)
            .map_err(|_| rusqlite::Error::InvalidQuery)?
            .ok_or(rusqlite::Error::InvalidQuery)?;
        if current.registration != *record {
            return Err(rusqlite::Error::InvalidQuery);
        }
    }
    Ok(())
}
fn validate_accounts(accounts: &[String]) -> Result<(), ErrorCode> {
    let scope = hantong_contracts::auth::RequestedScope::new(
        accounts.to_vec(),
        vec!["mail.accounts.list".into()],
    )?;
    if scope.accounts() != accounts {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests;

#[cfg(all(test, not(windows)))]
#[test]
fn cli15_refresh_requires_the_windows_store() {
    let store = Store {};
    assert_eq!(
        store.load_cli_scope_refresh([1; 32]),
        Err(ErrorCode::CapabilityUnsupported)
    );
    assert_eq!(
        store.load_pending_cli_scope_refresh(),
        Err(ErrorCode::CapabilityUnsupported)
    );
    assert_eq!(
        store.list_staged_cli_scope_refreshes(),
        Err(ErrorCode::CapabilityUnsupported)
    );
    assert_eq!(
        store.create_cli_scope_refresh([1; 32], vec!["a".into()], [2; 32]),
        Err(ErrorCode::CapabilityUnsupported)
    );
}
