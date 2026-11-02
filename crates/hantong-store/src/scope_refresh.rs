//! Default CLI scope history is stored data, never proof of user presence.
use crate::{DEFAULT_CLI_LABEL, RegistrationRecord, Store};
use hantong_contracts::ErrorCode;
use hantong_contracts::auth::{RequestedScope, default_cli_legacy_tools};
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
    hash_version: u8,
    decision_channel: String,
    base_registration: RegistrationRecord,
    target_accounts: Vec<String>,
    target_tools: Vec<String>,
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
    pub fn decision_channel(&self) -> &str {
        &self.decision_channel
    }
    pub fn target_accounts(&self) -> &[String] {
        &self.target_accounts
    }
    pub fn target_tools(&self) -> &[String] {
        &self.target_tools
    }
    pub fn target_scope(&self) -> RequestedScope {
        RequestedScope::new(self.target_accounts.clone(), self.target_tools.clone())
            .expect("validated scope")
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
        validate_accounts(&target_accounts)?;
        self.create_cli_scope_refresh_with_scope(
            refresh_id,
            RequestedScope::new(target_accounts, default_cli_legacy_tools())?,
            expected_server_public_key_hash,
        )
    }
    pub fn create_cli_scope_refresh_with_scope(
        &self,
        refresh_id: [u8; 32],
        target_scope: RequestedScope,
        expected_server_public_key_hash: [u8; 32],
    ) -> Result<CliScopeRefresh, ErrorCode> {
        if refresh_id == [0; 32]
            || expected_server_public_key_hash == [0; 32]
            || !hantong_contracts::auth::is_supported_default_cli_toolset(target_scope.tools())
        {
            return Err(ErrorCode::InvalidInput);
        }
        validate_accounts(target_scope.accounts())?;
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
                    target_scope,
                    expected_server_public_key_hash,
                    false,
                    false,
                )
            })
        }
    }
    /// Creates the first personal Google CLI scope change. The Google consent
    /// provenance is checked against the active registration; the normal CLI
    /// refresh path remains unchanged.
    pub fn create_google_oauth_cli_scope_refresh(
        &self,
        refresh_id: [u8; 32],
        target_scope: RequestedScope,
        server_pin: [u8; 32],
    ) -> Result<CliScopeRefresh, ErrorCode> {
        if refresh_id == [0; 32]
            || server_pin == [0; 32]
            || target_scope.accounts().len() != 1
            || !(target_scope.tools()
                == hantong_contracts::auth::default_cli_body_tools().as_slice()
                || target_scope.tools()
                    == hantong_contracts::auth::default_cli_send_tools().as_slice())
        {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            let _ = (refresh_id, target_scope, server_pin);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Immediate, |tx| {
                let channel: String = tx.query_row(
                    "SELECT approved_channel FROM client_registrations WHERE client_kind='cli' AND display_label=?1 AND state='active'",
                    [DEFAULT_CLI_LABEL], |r| r.get(0)).map_err(|_| ErrorCode::NotFound)?;
                if channel != "google_oauth" { return Err(ErrorCode::Conflict); }
                if target_scope.tools() == hantong_contracts::auth::default_cli_send_tools().as_slice()
                    && target_scope.accounts().len() != 1 { return Err(ErrorCode::InvalidInput); }
                storage::create(tx, refresh_id, target_scope, server_pin, true, false)
            })
        }
    }
    /// Creates the only supported personal compose upgrade: the exact active
    /// OAuth-origin B profile to the same account's C profile.
    pub fn create_google_oauth_cli_send_upgrade(
        &self,
        refresh_id: [u8; 32],
        target_scope: RequestedScope,
        server_pin: [u8; 32],
    ) -> Result<CliScopeRefresh, ErrorCode> {
        if refresh_id == [0; 32]
            || server_pin == [0; 32]
            || target_scope.accounts().len() != 1
            || target_scope.tools() != hantong_contracts::auth::default_cli_send_tools().as_slice()
        {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            let _ = (refresh_id, target_scope, server_pin);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(rusqlite::TransactionBehavior::Immediate, |tx| {
                let channel: String = tx
                    .query_row(
                        "SELECT approved_channel FROM client_registrations WHERE client_kind='cli' AND display_label=?1 AND state='active'",
                        [DEFAULT_CLI_LABEL],
                        |r| r.get(0),
                    )
                    .map_err(|_| ErrorCode::NotFound)?;
                if channel != "google_oauth" {
                    return Err(ErrorCode::Conflict);
                }
                storage::create(tx, refresh_id, target_scope, server_pin, true, true)
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
pub(crate) fn validate_history_v8(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    storage::validate_history_v8(connection)
}
#[cfg(windows)]
pub(crate) fn validate_history_v11(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    storage::validate_history_v11(connection)
}
#[cfg(windows)]
pub(crate) fn validate_history_v14(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    storage::validate_history_v14(connection)
}
#[cfg(windows)]
pub(crate) fn validate_history_v15(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    storage::validate_history_v15(connection)
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

#[cfg(windows)]
pub(crate) fn validate_history_v17(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    storage::validate_history_v17(c)
}

#[cfg(windows)]
pub(crate) fn validate_history_v20(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    storage::validate_history_v20(c)
}
#[cfg(windows)]
pub(crate) fn validate_history_v21(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    storage::validate_history_v21(c)
}

#[cfg(windows)]
pub(crate) fn validate_history_v22(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    storage::validate_history_v22(c)
}
