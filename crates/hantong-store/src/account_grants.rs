//! Account-qualified model-2 mail scope and one-use decision ledger.
//!
//! This module deliberately keeps the legacy registration scope untouched. A
//! caller must supply the already verified registration snapshot and provider
//! permit digest; this store layer never treats a UI boolean as authority.

use crate::Store;
use hantong_contracts::ErrorCode;
use hantong_contracts::account_scope::{AccountGrantV2, AccountScopeV2 as ContractScopeV2};
use rusqlite::{OptionalExtension, params};
#[cfg(test)]
use serde_json::{Map, Value};

const FULL_MAIL: [&str; 10] = [
    "mail.drafts.create",
    "mail.folders.list",
    "mail.folders.refresh",
    "mail.messages.get",
    "mail.messages.search",
    "mail.send.request",
    "mail.send.execute",
    "mail.sync.page",
    "mail.messages.trash.request",
    "mail.messages.trash.execute",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailGrantPurpose {
    Add,
    Upgrade,
}

impl MailGrantPurpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Upgrade => "upgrade",
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct StoredAccountScopeV2 {
    pub registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub baseline_registration_version: u64,
    pub baseline_profile_epoch: u64,
    pub effective_registration_version: u64,
    pub effective_profile_epoch: u64,
    pub state: String,
    pub baseline_binding_hash: [u8; 32],
    pub scope: ContractScopeV2,
    pub scope_json: String,
    pub scope_hash: [u8; 32],
}

#[derive(Clone, Eq, PartialEq)]
pub struct MailGrantDecision {
    pub decision_id: [u8; 32],
    pub registration_id: [u8; 16],
    pub target_account_id: String,
    pub purpose: MailGrantPurpose,
    pub state: String,
    pub expires_at_utc_ms: i64,
}

/// Metadata required to validate cleanup of a committed profile publication.
/// This is never an authority token and contains no secret material.
#[derive(Clone, Eq, PartialEq)]
pub struct FullMailCleanup {
    pub registration_id: [u8; 16],
    pub decision_id: [u8; 32],
    pub expected_registration_version: u64,
    pub expected_profile_epoch: u64,
    pub baseline_scope_hash: [u8; 32],
    pub legacy_read_only: bool,
    pub client_public_key: Vec<u8>,
    pub server_public_key_hash: [u8; 32],
    pub target_account_id: String,
}

impl std::fmt::Debug for FullMailCleanup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FullMailCleanup { <redacted> }")
    }
}

impl std::fmt::Debug for StoredAccountScopeV2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StoredAccountScopeV2 { <redacted> }")
    }
}
impl std::fmt::Debug for MailGrantDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MailGrantDecision { <redacted> }")
    }
}

fn valid_id(id: &str, max: usize) -> bool {
    !id.is_empty()
        && id.len() <= max
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn full_mail_scope(account: &str, baseline: Option<&ContractScopeV2>) -> Result<String, ErrorCode> {
    if !valid_id(account, 128) {
        return Err(ErrorCode::InvalidInput);
    }
    let principal = baseline.map_or_else(
        || vec!["mail.accounts.list".to_owned()],
        |s| s.principal_tools().to_vec(),
    );
    let mut grants = baseline.map_or_else(Vec::new, |s| s.grants().to_vec());
    let mut tools = baseline.map_or_else(Vec::new, |scope| {
        scope
            .grants()
            .iter()
            .find(|g| g.account_id() == account)
            .map_or_else(Vec::new, |grant| {
                grant
                    .tools()
                    .iter()
                    .filter(|tool| scope.permits_account(account, tool))
                    .cloned()
                    .collect()
            })
    });
    tools.extend(FULL_MAIL.iter().map(|v| (*v).to_owned()));
    let full = AccountGrantV2::new(account.to_owned(), tools, false)?;
    if let Some(existing) = grants.iter_mut().find(|g| g.account_id() == account) {
        *existing = full;
    } else {
        grants.push(full);
    }
    let scope = ContractScopeV2::new(principal, grants)?;
    String::from_utf8(scope.to_json()?).map_err(|_| ErrorCode::InternalError)
}

fn validate_scope(scope: &str, account: &str) -> Result<(), ErrorCode> {
    let parsed = ContractScopeV2::from_json(scope.as_bytes())?;
    let grant = parsed
        .grants()
        .iter()
        .find(|g| g.account_id() == account)
        .ok_or(ErrorCode::InvalidInput)?;
    if grant.read_only()
        || FULL_MAIL
            .iter()
            .any(|tool| !parsed.permits_account(account, tool))
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

fn scope_hash(scope: &str) -> Result<[u8; 32], ErrorCode> {
    hantong_platform::sha256(scope.as_bytes()).map_err(|_| ErrorCode::InternalError)
}

fn db<T>(result: rusqlite::Result<T>) -> Result<T, ErrorCode> {
    result.map_err(|_| ErrorCode::InternalError)
}

fn read_overlay(
    c: &rusqlite::Connection,
    id: [u8; 16],
) -> Result<Option<StoredAccountScopeV2>, ErrorCode> {
    let row = db(c.query_row(
        "SELECT profile_epoch,baseline_registration_version,baseline_profile_epoch,effective_registration_version,effective_profile_epoch,state,baseline_binding_hash,scope_json,scope_hash FROM account_scope_v2 WHERE registration_id=?1",
        [id.as_slice()], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,String>(5)?,r.get::<_,Vec<u8>>(6)?,r.get::<_,String>(7)?,r.get::<_,Vec<u8>>(8)?))
    ).optional())?;
    row.map(
        |(epoch, bv, be, version, effective_epoch, state, binding, json, hash)| {
            if epoch <= 0
                || bv <= 0
                || be <= 0
                || version <= bv
                || effective_epoch <= be
                || epoch != effective_epoch
                || !matches!(state.as_str(), "active" | "suspended" | "revoked")
                || binding.len() != 32
                || hash.len() != 32
                || scope_hash(&json)?.as_slice() != hash.as_slice()
            {
                return Err(ErrorCode::InternalError);
            }
            let scope = ContractScopeV2::from_json(json.as_bytes())
                .map_err(|_| ErrorCode::InternalError)?;
            Ok(StoredAccountScopeV2 {
                registration_id: id,
                profile_epoch: epoch as u64,
                baseline_registration_version: bv as u64,
                baseline_profile_epoch: be as u64,
                effective_registration_version: version as u64,
                effective_profile_epoch: effective_epoch as u64,
                state,
                baseline_binding_hash: binding.try_into().map_err(|_| ErrorCode::InternalError)?,
                scope,
                scope_json: json,
                scope_hash: hash.try_into().map_err(|_| ErrorCode::InternalError)?,
            })
        },
    )
    .transpose()
}

fn server_pin(c: &rusqlite::Connection) -> Result<[u8; 32], ErrorCode> {
    let public: Vec<u8> = db(c.query_row(
        "SELECT public_key FROM server_identity WHERE singleton=1 AND state='active'",
        [],
        |r| r.get(0),
    ))?;
    let public: [u8; 72] = public.try_into().map_err(|_| ErrorCode::InternalError)?;
    hantong_platform::validate_public_key(&public)?;
    hantong_platform::sha256(&public)
}
fn decision_server_pin(
    c: &rusqlite::Connection,
    id: [u8; 16],
    decision: [u8; 32],
) -> Result<[u8; 32], ErrorCode> {
    let expected:Vec<u8>=db(c.query_row("SELECT server_public_key_hash FROM account_scope_decisions_v2 WHERE registration_id=?1 AND decision_id=?2",params![id.as_slice(),decision.as_slice()],|r|r.get(0)))?;
    let current = server_pin(c)?;
    if expected.as_slice() != current {
        return Err(ErrorCode::Conflict);
    }
    Ok(current)
}

#[derive(Clone)]
pub struct FullMailStage {
    pub decision_id: [u8; 32],
    pub base: crate::RegistrationRecord,
    pub target: ContractScopeV2,
    pub target_version: u64,
    pub target_epoch: u64,
    pub legacy_binding_hash: [u8; 32],
    pub server_public_key_hash: [u8; 32],
}
impl std::fmt::Debug for FullMailStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FullMailStage { <redacted> }")
    }
}

struct Baseline {
    legacy: crate::RegistrationRecord,
    scope: ContractScopeV2,
    version: u64,
    epoch: u64,
    scope_hash: [u8; 32],
    binding_hash: [u8; 32],
}

/// Frozen metadata, key, source and old scope all remain part of the binding.
fn legacy_binding(record: &crate::RegistrationRecord) -> Result<[u8; 32], ErrorCode> {
    let value = serde_json::json!({"domain":"mail-hantong.account-grant-legacy.v2", "id":record.registration_id,
        "request":record.request_id, "kind":record.client_kind.as_str(), "label":record.display_label,
        "key":record.public_key.as_slice(), "epoch":record.profile_epoch,"version":record.version,
        "read_only":record.read_only,"rate":record.requests_per_minute,"source":record.approved_channel,
        "accounts":record.scope.accounts(),"tools":record.scope.tools()});
    hantong_platform::sha256(&serde_json::to_vec(&value).map_err(|_| ErrorCode::InternalError)?)
}

fn baseline(c: &rusqlite::Connection, id: [u8; 16]) -> Result<Baseline, ErrorCode> {
    let matching: i64 = db(c.query_row(
        "SELECT COUNT(*) FROM client_registrations WHERE client_kind='cli' AND display_label=?1",
        [crate::DEFAULT_CLI_LABEL],
        |r| r.get(0),
    ))?;
    if matching != 1 {
        return Err(ErrorCode::Conflict);
    }
    let canonical: Vec<u8> = db(c.query_row(
        "SELECT registration_id FROM client_registrations WHERE client_kind='cli' AND display_label=?1",
        [crate::DEFAULT_CLI_LABEL],
        |r| r.get(0),
    ))?;
    if canonical.as_slice() != id {
        return Err(ErrorCode::Conflict);
    }
    let legacy =
        db(crate::registrations::read_registration_legacy(c, &id))?.ok_or(ErrorCode::NotFound)?;
    if legacy.state != crate::RegistrationState::Active {
        return Err(ErrorCode::ClientSuspended);
    }
    if legacy.client_kind != crate::ClientKind::Cli
        || legacy.display_label != crate::DEFAULT_CLI_LABEL
    {
        return Err(ErrorCode::ScopeDenied);
    }
    let binding_hash = legacy_binding(&legacy)?;
    let (scope, version, epoch) = match read_overlay(c, id)? {
        None => (
            ContractScopeV2::from_legacy(&legacy.scope, legacy.read_only)?,
            legacy.version,
            legacy.profile_epoch,
        ),
        Some(overlay) => {
            if overlay.state != "active"
                || overlay.baseline_binding_hash != binding_hash
                || overlay.baseline_registration_version != legacy.version
                || overlay.baseline_profile_epoch != legacy.profile_epoch
            {
                return Err(ErrorCode::Conflict);
            }
            validate_committed_history(c, &legacy, &overlay)?;
            (
                overlay.scope,
                overlay.effective_registration_version,
                overlay.effective_profile_epoch,
            )
        }
    };
    let scope_hash = hantong_platform::sha256(&scope.to_json()?)?;
    Ok(Baseline {
        legacy,
        scope,
        version,
        epoch,
        scope_hash,
        binding_hash,
    })
}

// Every effective step must be derived from the previous account matrix. Old
// flat registration rows remain the immutable first link, never a tool union.
fn validate_committed_history(
    c: &rusqlite::Connection,
    legacy: &crate::RegistrationRecord,
    overlay: &StoredAccountScopeV2,
) -> Result<(), ErrorCode> {
    let mut scope = ContractScopeV2::from_legacy(&legacy.scope, legacy.read_only)?;
    let mut version = legacy.version;
    let mut epoch = legacy.profile_epoch;
    let binding = legacy_binding(legacy)?;
    let pin = server_pin(c)?;
    let mut q = db(c.prepare("SELECT expected_registration_version,expected_profile_epoch,baseline_scope_hash,baseline_binding_hash,target_account_id,target_scope_json,target_scope_hash,purpose,authority_binding_hash,published_profile_hash,server_public_key_hash FROM account_scope_decisions_v2 WHERE registration_id=?1 AND state='active' ORDER BY expected_registration_version LIMIT 129"))?;
    let mut rows = db(q.query([legacy.registration_id.as_slice()]))?;
    let mut count = 0;
    while let Some(row) = db(rows.next())? {
        count += 1;
        let v: i64 = db(row.get(0))?;
        let e: i64 = db(row.get(1))?;
        let old_hash: Vec<u8> = db(row.get(2))?;
        let old_binding: Vec<u8> = db(row.get(3))?;
        let account: String = db(row.get(4))?;
        let target: String = db(row.get(5))?;
        let hash: Vec<u8> = db(row.get(6))?;
        let purpose: String = db(row.get(7))?;
        let authority: Vec<u8> = db(row.get(8))?;
        let publication: Vec<u8> = db(row.get(9))?;
        let server: Vec<u8> = db(row.get(10))?;
        let has_account = scope.grants().iter().any(|g| g.account_id() == account);
        if count > 128
            || v as u64 != version
            || e as u64 != epoch
            || old_hash.as_slice() != hantong_platform::sha256(&scope.to_json()?)?
            || old_binding.as_slice() != binding
            || server.as_slice() != pin
            || authority.len() != 32
            || authority == [0; 32]
            || publication.len() != 32
            || publication == [0; 32]
            || !matches!(
                (purpose.as_str(), has_account),
                ("add", false) | ("upgrade", true)
            )
            || full_mail_scope(&account, Some(&scope))? != target
            || hash.as_slice() != scope_hash(&target)?
        {
            return Err(ErrorCode::InternalError);
        }
        scope = ContractScopeV2::from_json(target.as_bytes())?;
        version = version.checked_add(1).ok_or(ErrorCode::InternalError)?;
        epoch = epoch.checked_add(1).ok_or(ErrorCode::InternalError)?;
    }
    if count == 0
        || version != overlay.effective_registration_version
        || epoch != overlay.effective_profile_epoch
        || scope != overlay.scope
    {
        return Err(ErrorCode::InternalError);
    }
    Ok(())
}

fn no_legacy_refresh(c: &rusqlite::Connection, id: [u8; 16]) -> Result<(), ErrorCode> {
    let open: bool = db(c.query_row("SELECT EXISTS(SELECT 1 FROM client_scope_refreshes WHERE registration_id=?1 AND phase IN ('pending','staged'))",[id.as_slice()],|r|r.get(0)))?;
    if open {
        Err(ErrorCode::Conflict)
    } else {
        Ok(())
    }
}

pub(crate) fn project_registration(
    c: &rusqlite::Connection,
    mut record: crate::RegistrationRecord,
) -> Result<crate::RegistrationRecord, ErrorCode> {
    // Frozen schema validators also use the registration reader before v28.
    let exists: bool = db(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='account_scope_v2')",
        [],
        |r| r.get(0),
    ))?;
    if !exists || record.state != crate::RegistrationState::Active {
        return Ok(record);
    }
    let overlay = read_overlay(c, record.registration_id)?;
    let staged: bool = db(c.query_row("SELECT EXISTS(SELECT 1 FROM account_scope_decisions_v2 WHERE registration_id=?1 AND state='staged')",[record.registration_id.as_slice()],|r|r.get(0)))?;
    if overlay.is_none() && !staged {
        return Ok(record);
    }
    let effective = baseline(c, record.registration_id)?;
    if overlay.is_some() {
        record.scope =
            hantong_contracts::auth::RequestedScope::from_account_scope(effective.scope)?;
        record.read_only = false;
        record.version = effective.version;
        record.profile_epoch = effective.epoch;
    }
    // Publishing a new profile and activating its exact matrix is a recoverable
    // provisioning interval, not permission to authenticate the staged target.
    if staged {
        record.state = crate::RegistrationState::Provisioning;
    }
    Ok(record)
}

struct PublishedBinding {
    registration_id: [u8; 16],
    epoch: u64,
    client_key_hash: [u8; 32],
    server_key_hash: [u8; 32],
    scope_hash: [u8; 32],
    profile_hash: [u8; 32],
}

impl Store {
    fn grant_transaction<T>(
        &self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> Result<T, ErrorCode>,
    ) -> Result<T, ErrorCode> {
        self.with_connection(|c| {
            let tx =
                rusqlite::Transaction::new_unchecked(c, rusqlite::TransactionBehavior::Immediate)?;
            let result = f(&tx);
            // Expiration is a terminal metadata transition, never a permission change.
            if result.is_ok() || matches!(result, Err(ErrorCode::ApprovalExpired)) {
                tx.commit()?;
            }
            Ok(result)
        })?
    }

    pub fn load_account_scope_v2(
        &self,
        id: [u8; 16],
    ) -> Result<Option<StoredAccountScopeV2>, ErrorCode> {
        self.with_connection(|c| {
            Ok((|| {
                let overlay = read_overlay(c, id)?;
                if overlay.is_some() {
                    baseline(c, id)?;
                }
                Ok(overlay)
            })())
        })?
    }

    pub fn full_mail_legacy_binding_hash(&self, id: [u8; 16]) -> Result<[u8; 32], ErrorCode> {
        self.with_connection(|c| Ok(baseline(c, id).map(|b| b.binding_hash)))?
    }

    /// Snapshot for the Core-owned app intent. This value is data, not a permit.
    pub fn full_mail_baseline_hash(&self, id: [u8; 16]) -> Result<[u8; 32], ErrorCode> {
        self.with_connection(|c| Ok(baseline(c, id).map(|b| b.scope_hash)))?
    }

    /// Lists only fully committed decisions whose complete active history and
    /// current server identity still validate. The result is cleanup metadata,
    /// never a grant or a permission to publish.
    pub fn list_committed_full_mail_cleanup(&self) -> Result<Vec<FullMailCleanup>, ErrorCode> {
        self.with_connection(|c| Ok((|| {
            let mut query = db(c.prepare(
                "SELECT registration_id,decision_id,expected_registration_version,expected_profile_epoch,baseline_scope_hash,target_account_id FROM account_scope_decisions_v2 WHERE state='active' ORDER BY updated_at_utc_ms LIMIT 129",
            ))?;
            let mut rows = db(query.query([]))?;
            let mut result = Vec::new();
            while let Some(row) = db(rows.next())? {
                if result.len() >= 128 {
                    return Err(ErrorCode::QuotaExceeded);
                }
                let registration_id: [u8; 16] = {
                    let bytes: Vec<u8> = db(row.get(0))?;
                    bytes.try_into().map_err(|_| ErrorCode::InternalError)?
                };
                let decision_id: [u8; 32] = {
                    let bytes: Vec<u8> = db(row.get(1))?;
                    bytes.try_into().map_err(|_| ErrorCode::InternalError)?
                };
                let expected_registration_version: u64 = db(row.get::<_, i64>(2))?
                    .try_into()
                    .map_err(|_| ErrorCode::InternalError)?;
                let expected_profile_epoch: u64 = db(row.get::<_, i64>(3))?
                    .try_into()
                    .map_err(|_| ErrorCode::InternalError)?;
                let baseline_scope_hash: [u8; 32] = {
                    let bytes: Vec<u8> = db(row.get(4))?;
                    bytes.try_into().map_err(|_| ErrorCode::InternalError)?
                };
                let target_account_id: String = db(row.get(5))?;

                let current = baseline(c, registration_id)?;
                let server_key = decision_server_pin(c, registration_id, decision_id)?;
                let client_public_key = current.legacy.public_key.to_vec();
                let client_key: [u8; 72] = client_public_key
                    .as_slice()
                    .try_into()
                    .map_err(|_| ErrorCode::InternalError)?;
                hantong_platform::validate_public_key(&client_key)?;
                result.push(FullMailCleanup {
                    registration_id,
                    decision_id,
                    expected_registration_version,
                    expected_profile_epoch,
                    baseline_scope_hash,
                    legacy_read_only: current.legacy.read_only,
                    client_public_key,
                    server_public_key_hash: server_key,
                    target_account_id,
                });
            }
            Ok(result)
        })()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_full_mail_decision(
        &self,
        decision_id: [u8; 32],
        id: [u8; 16],
        version: u64,
        epoch: u64,
        scope_hash: [u8; 32],
        account: &str,
        purpose: MailGrantPurpose,
        operation: &str,
        idempotency: &str,
        peer: [u8; 32],
        expires: i64,
    ) -> Result<MailGrantDecision, ErrorCode> {
        let now = crate::utc_millis().map_err(|_| ErrorCode::InternalError)?;
        if decision_id == [0; 32]
            || id == [0; 16]
            || peer == [0; 32]
            || scope_hash == [0; 32]
            || version == 0
            || epoch == 0
            || version >= i64::MAX as u64
            || epoch >= i64::MAX as u64
            || !valid_id(account, 128)
            || !valid_id(operation, 128)
            || !valid_id(idempotency, 128)
        {
            return Err(ErrorCode::InvalidInput);
        }
        self.grant_transaction(|tx| {
            db(tx.execute("UPDATE account_scope_decisions_v2 SET state='expired',consumed_at_utc_ms=?1,updated_at_utc_ms=?1 WHERE registration_id=?2 AND state='pending' AND expires_at_utc_ms<=?1",params![now,id.as_slice()]))?;
            let existing=db(tx.query_row("SELECT decision_id,target_account_id,peer_digest,state,expires_at_utc_ms,baseline_scope_hash,purpose,operation_id,idempotency_key,expected_registration_version,expected_profile_epoch FROM account_scope_decisions_v2 WHERE registration_id=?1 AND (operation_id=?2 OR idempotency_key=?3)",
                params![id.as_slice(),operation,idempotency],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,String>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,r.get::<_,Vec<u8>>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,i64>(9)?,r.get::<_,i64>(10)?))).optional())?;
            if let Some((old_id,old_account,old_peer,state,old_expires,old_scope,old_purpose,old_operation,old_idempotency,old_version,old_epoch))=existing {
                if old_id.as_slice()!=decision_id || old_account!=account || old_peer.as_slice()!=peer
                    || old_scope.as_slice()!=scope_hash || old_purpose!=purpose.as_str() || old_operation!=operation
                    || old_idempotency!=idempotency || old_version as u64!=version || old_epoch as u64!=epoch || old_expires!=expires {
                    return Err(ErrorCode::Conflict);
                }
                if state=="expired" {return Err(ErrorCode::ApprovalExpired);}
                return Ok(MailGrantDecision{decision_id,registration_id:id,target_account_id:account.into(),purpose,state,expires_at_utc_ms:old_expires});
            }
            if expires<=now || expires.checked_sub(now).is_none_or(|ttl|ttl>300_000) {return Err(ErrorCode::InvalidInput);}
            let current=baseline(tx,id)?;
            if current.version!=version || current.epoch!=epoch || current.scope_hash!=scope_hash {return Err(ErrorCode::Conflict);}
            let has_account=current.scope.grants().iter().any(|g|g.account_id()==account);
            if (purpose==MailGrantPurpose::Add && has_account) || (purpose==MailGrantPurpose::Upgrade && !has_account) {return Err(ErrorCode::Conflict);}
            let target=full_mail_scope(account,Some(&current.scope))?;
            let target_hash=scope_hash_bytes(&target)?;
            let retained: i64=db(tx.query_row("SELECT COUNT(*) FROM account_scope_decisions_v2 WHERE registration_id=?1",[id.as_slice()],|r|r.get(0)))?;
            if retained>=128 {return Err(ErrorCode::QuotaExceeded);}
            let conflict:bool=db(tx.query_row("SELECT EXISTS(SELECT 1 FROM client_scope_refreshes WHERE registration_id=?1 AND phase IN ('pending','staged')) OR EXISTS(SELECT 1 FROM account_scope_decisions_v2 WHERE registration_id=?1 AND state IN ('pending','staged'))",[id.as_slice()],|r|r.get(0)))?;
            if conflict {return Err(ErrorCode::Conflict);}
            let pin=server_pin(tx)?;
            db(tx.execute("INSERT INTO account_scope_decisions_v2(decision_id,registration_id,expected_registration_version,expected_profile_epoch,baseline_scope_hash,baseline_binding_hash,target_account_id,target_scope_json,target_scope_hash,purpose,operation_id,idempotency_key,peer_digest,server_public_key_hash,state,created_at_utc_ms,expires_at_utc_ms,consumed_at_utc_ms,updated_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?16,'pending',?14,?15,NULL,?14)",
                params![decision_id.as_slice(),id.as_slice(),version as i64,epoch as i64,scope_hash.as_slice(),current.binding_hash.as_slice(),account,target,target_hash.as_slice(),purpose.as_str(),operation,idempotency,peer.as_slice(),now,expires,pin.as_slice()]))?;
            Ok(MailGrantDecision{decision_id,registration_id:id,target_account_id:account.into(),purpose,state:"pending".into(),expires_at_utc_ms:expires})
        })
    }

    pub fn cancel_full_mail_decision(
        &self,
        id: [u8; 16],
        decision: [u8; 32],
        peer: [u8; 32],
    ) -> Result<(), ErrorCode> {
        self.grant_transaction(|tx| {
            let now=crate::utc_millis().map_err(|_|ErrorCode::InternalError)?;
            let expiry=db(tx.query_row("SELECT expires_at_utc_ms FROM account_scope_decisions_v2 WHERE registration_id=?1 AND decision_id=?2 AND peer_digest=?3 AND state='pending'",params![id.as_slice(),decision.as_slice(),peer.as_slice()],|r|r.get::<_,i64>(0)).optional())?.ok_or(ErrorCode::Conflict)?;
            let expired=expiry<=now;
            db(tx.execute("UPDATE account_scope_decisions_v2 SET state=?1,consumed_at_utc_ms=?2,updated_at_utc_ms=?2 WHERE registration_id=?3 AND decision_id=?4 AND state='pending'",
                params![if expired{"expired"}else{"cancelled"},now,id.as_slice(),decision.as_slice()]))?;
            if expired {Err(ErrorCode::ApprovalExpired)} else {Ok(())}
        })
    }

    /// Called only after Core consumes its private verified provider/setup permit.
    #[allow(clippy::too_many_arguments)]
    pub fn consume_and_stage_full_mail_decision(
        &self,
        id: [u8; 16],
        decision: [u8; 32],
        version: u64,
        epoch: u64,
        peer: [u8; 32],
        authority: [u8; 32],
    ) -> Result<(), ErrorCode> {
        if peer == [0; 32] || authority == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        self.grant_transaction(|tx| {
            let row=db(tx.query_row("SELECT baseline_scope_hash,baseline_binding_hash,target_account_id,target_scope_json,target_scope_hash,expires_at_utc_ms FROM account_scope_decisions_v2 WHERE registration_id=?1 AND decision_id=?2 AND expected_registration_version=?3 AND expected_profile_epoch=?4 AND peer_digest=?5 AND state='pending'",
                params![id.as_slice(),decision.as_slice(),version as i64,epoch as i64,peer.as_slice()],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,Vec<u8>>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,Vec<u8>>(4)?,r.get::<_,i64>(5)?))).optional())?.ok_or(ErrorCode::Conflict)?;
            let now=crate::utc_millis().map_err(|_|ErrorCode::InternalError)?;
            if row.5<=now {
                db(tx.execute("UPDATE account_scope_decisions_v2 SET state='expired',consumed_at_utc_ms=?1,updated_at_utc_ms=?1 WHERE registration_id=?2 AND decision_id=?3 AND state='pending'",params![now,id.as_slice(),decision.as_slice()]))?;
                return Err(ErrorCode::ApprovalExpired);
            }
            no_legacy_refresh(tx,id)?;
            let current=baseline(tx,id)?;
            if current.version!=version || current.epoch!=epoch || row.0.as_slice()!=current.scope_hash || row.1.as_slice()!=current.binding_hash
                || full_mail_scope(&row.2,Some(&current.scope))?!=row.3 || scope_hash_bytes(&row.3)?.as_slice()!=row.4 {
                return Err(ErrorCode::Conflict);
            }
            validate_scope(&row.3,&row.2)?;
            decision_server_pin(tx,id,decision)?;
            let changed=db(tx.execute("UPDATE account_scope_decisions_v2 SET state='staged',authority_binding_hash=?1,consumed_at_utc_ms=?2,updated_at_utc_ms=?2 WHERE registration_id=?3 AND decision_id=?4 AND state='pending'",params![authority.as_slice(),now,id.as_slice(),decision.as_slice()]))?;
            if changed!=1 {return Err(ErrorCode::Conflict);}
            Ok(())
        })
    }

    pub fn staged_full_mail_decision(
        &self,
        id: [u8; 16],
        decision: [u8; 32],
    ) -> Result<FullMailStage, ErrorCode> {
        self.with_connection(|c| Ok((|| {
            no_legacy_refresh(c,id)?;
            let current=baseline(c,id)?;
            let row=db(c.query_row("SELECT expected_registration_version,expected_profile_epoch,baseline_scope_hash,baseline_binding_hash,target_account_id,target_scope_json,target_scope_hash FROM account_scope_decisions_v2 WHERE registration_id=?1 AND decision_id=?2 AND state='staged'",params![id.as_slice(),decision.as_slice()],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,Vec<u8>>(6)?))).optional())?.ok_or(ErrorCode::Conflict)?;
            if row.0 as u64!=current.version || row.1 as u64!=current.epoch || row.2.as_slice()!=current.scope_hash || row.3.as_slice()!=current.binding_hash || full_mail_scope(&row.4,Some(&current.scope))?!=row.5 || scope_hash_bytes(&row.5)?.as_slice()!=row.6 {return Err(ErrorCode::Conflict);}
            validate_scope(&row.5,&row.4)?;
            let pin=decision_server_pin(c,id,decision)?;
            let mut base=current.legacy;
            if read_overlay(c,id)?.is_some() {
                base.scope=hantong_contracts::auth::RequestedScope::from_account_scope(current.scope)?;
                base.read_only=false;
                base.version=current.version;
                base.profile_epoch=current.epoch;
            }
            Ok(FullMailStage{decision_id:decision,base,target:ContractScopeV2::from_json(row.5.as_bytes())?,target_version:current.version+1,target_epoch:current.epoch+1,legacy_binding_hash:current.binding_hash,server_public_key_hash:pin})
        })()))?
    }

    #[allow(clippy::type_complexity)] // Exact registration/decision identifiers, no authority.
    pub fn list_staged_full_mail_decisions(&self) -> Result<Vec<([u8; 16], [u8; 32])>, ErrorCode> {
        self.with_connection(|c| {
            let mut q=c.prepare("SELECT registration_id,decision_id FROM account_scope_decisions_v2 WHERE state='staged' ORDER BY created_at_utc_ms LIMIT 129")?;
            let values=q.query_map([],|r|Ok((crate::registrations::blob_column::<16>(r,0)?,crate::registrations::blob_column::<32>(r,1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(if values.len()>128 {Err(ErrorCode::QuotaExceeded)} else {Ok(values)})
        })?
    }

    /// Protected publication must complete before this exact target CAS.
    #[allow(clippy::too_many_arguments)]
    pub fn activate_full_mail_decision(
        &self,
        id: [u8; 16],
        decision: [u8; 32],
        version: u64,
        epoch: u64,
        binding: [u8; 32],
        publication: &hantong_platform::account_publication::VerifiedAccountProfilePublication,
    ) -> Result<(), ErrorCode> {
        self.activate_bound_publication(
            id,
            decision,
            version,
            epoch,
            binding,
            PublishedBinding {
                registration_id: *publication.registration_id(),
                epoch: publication.profile_epoch(),
                client_key_hash: *publication.client_key_hash(),
                server_key_hash: *publication.server_key_hash(),
                scope_hash: *publication.scope_hash(),
                profile_hash: *publication.profile_hash(),
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn activate_bound_publication(
        &self,
        id: [u8; 16],
        decision: [u8; 32],
        version: u64,
        epoch: u64,
        binding: [u8; 32],
        published: PublishedBinding,
    ) -> Result<(), ErrorCode> {
        let published_profile_hash = published.profile_hash;
        if published.registration_id != id || published.epoch != epoch {
            return Err(ErrorCode::Conflict);
        }
        if published_profile_hash == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        self.grant_transaction(|tx| {
            no_legacy_refresh(tx,id)?;
            let current=baseline(tx,id)?;
            let row=db(tx.query_row("SELECT expected_registration_version,expected_profile_epoch,baseline_scope_hash,baseline_binding_hash,target_account_id,target_scope_json,target_scope_hash,authority_binding_hash FROM account_scope_decisions_v2 WHERE registration_id=?1 AND decision_id=?2 AND state='staged'",params![id.as_slice(),decision.as_slice()],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,Vec<u8>>(2)?,r.get::<_,Vec<u8>>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,Vec<u8>>(6)?,r.get::<_,Vec<u8>>(7)?))).optional())?.ok_or(ErrorCode::Conflict)?;
            if row.0 as u64!=current.version || row.1 as u64!=current.epoch || version!=current.version+1 || epoch!=current.epoch+1
                || row.2.as_slice()!=current.scope_hash || row.3.as_slice()!=binding || binding!=current.binding_hash
                || row.7.len()!=32 || row.7==[0;32] || full_mail_scope(&row.4,Some(&current.scope))?!=row.5
                || scope_hash_bytes(&row.5)?.as_slice()!=row.6 {return Err(ErrorCode::Conflict);}
            validate_scope(&row.5,&row.4)?;
            let pin=decision_server_pin(tx,id,decision)?;
            if published.server_key_hash!=pin || published.client_key_hash!=hantong_platform::sha256(&current.legacy.public_key)?
                || row.6.as_slice()!=published.scope_hash {return Err(ErrorCode::Conflict);}
            let now=crate::utc_millis().map_err(|_|ErrorCode::InternalError)?;
            db(tx.execute("INSERT INTO account_scope_v2(registration_id,profile_epoch,baseline_registration_version,baseline_profile_epoch,effective_registration_version,effective_profile_epoch,state,baseline_binding_hash,scope_json,scope_hash,updated_at_utc_ms) VALUES(?1,?2,?3,?4,?5,?2,'active',?6,?7,?8,?9) ON CONFLICT(registration_id) DO UPDATE SET profile_epoch=excluded.profile_epoch,effective_registration_version=excluded.effective_registration_version,effective_profile_epoch=excluded.effective_profile_epoch,scope_json=excluded.scope_json,scope_hash=excluded.scope_hash,updated_at_utc_ms=excluded.updated_at_utc_ms",
                params![id.as_slice(),epoch as i64,current.legacy.version as i64,current.legacy.profile_epoch as i64,version as i64,binding.as_slice(),row.5,row.6,now]))?;
            let changed=db(tx.execute("UPDATE account_scope_decisions_v2 SET state='active',published_profile_hash=?1,updated_at_utc_ms=?2 WHERE registration_id=?3 AND decision_id=?4 AND state='staged'",params![published_profile_hash.as_slice(),now,id.as_slice(),decision.as_slice()]))?;
            if changed!=1 {return Err(ErrorCode::Conflict);}
            Ok(())
        })
    }
}
fn scope_hash_bytes(value: &str) -> Result<[u8; 32], ErrorCode> {
    scope_hash(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acc02_full_mail_scope_is_account_qualified_and_canonical() {
        let json = full_mail_scope("acct_a", None).unwrap();
        validate_scope(&json, "acct_a").unwrap();
        assert!(validate_scope(&json, "acct_b").is_err());
        let scope = ContractScopeV2::from_json(json.as_bytes()).unwrap();
        assert!(scope.permits_account("acct_a", "mail.messages.get"));
        assert!(scope.permits_account("acct_a", "mail.send.execute"));
        assert!(scope.permits_account("acct_a", "mail.messages.trash.execute"));
        assert!(!scope.permits_account("acct_b", "mail.messages.get"));
    }

    #[test]
    fn acc02_full_mail_scope_preserves_unselected_account_grants() {
        let baseline = ContractScopeV2::new(
            vec!["mail.accounts.list".into()],
            vec![
                AccountGrantV2::new("acct_a".into(), vec!["mail.messages.get".into()], true)
                    .unwrap(),
                AccountGrantV2::new("acct_b".into(), vec!["mail.messages.get".into()], false)
                    .unwrap(),
            ],
        )
        .unwrap();
        let upgraded = ContractScopeV2::from_json(
            full_mail_scope("acct_a", Some(&baseline))
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        assert!(upgraded.permits_account("acct_a", "mail.send.execute"));
        assert!(upgraded.permits_account("acct_b", "mail.messages.get"));
        assert!(!upgraded.permits_account("acct_b", "mail.send.execute"));
    }

    #[test]
    fn sec03_scope_rejects_union_or_read_only_fallback() {
        let mut value: Map<String, Value> =
            serde_json::from_str(&full_mail_scope("acct_a", None).unwrap()).unwrap();
        value.insert("scope_model_version".into(), Value::from(1));
        assert!(validate_scope(&serde_json::to_string(&value).unwrap(), "acct_a").is_err());
    }
}

#[cfg(test)]
#[path = "account_grants_tests.rs"]
mod ledger_tests;
