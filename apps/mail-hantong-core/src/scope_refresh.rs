//! Core-only scope refresh. Stored snapshots and CLI requests are not approvals.
use hantong_approval::ScopeRefreshReview;
use hantong_contracts::scope_refresh::{ScopeRefreshProgress, ScopeRefreshReceipt};
use hantong_contracts::{ErrorCode, auth::RequestedScope};
use hantong_ipc_client::profile::ParsedClientProfile;
use hantong_platform::{ClientSigningKey, ServerSigningKey};
use hantong_store::{
    RegistrationRecord,
    scope_refresh::{CliScopeRefresh, ScopeRefreshPhase},
};
use hantong_store::{ServerIdentity, Store};

pub(super) fn prepare(
    store: &Store,
    server: &ServerSigningKey,
) -> Result<CliScopeRefresh, ErrorCode> {
    let effects = Native { store, server };
    let pin = hantong_platform::sha256(&effects.server_public()?)?;
    prepare_for_pin(store, pin)
}

fn prepare_for_pin(store: &Store, pin: [u8; 32]) -> Result<CliScopeRefresh, ErrorCode> {
    prepare_selected_for_pin(store, pin, None)
}

pub(super) fn matches_google_readonly(
    r: &CliScopeRefresh,
    account: hantong_contracts::registration::GoogleReadAccount,
) -> bool {
    r.target_accounts() == [account.account_id()]
        && r.target_tools() == hantong_contracts::auth::default_cli_body_tools()
}

pub(super) fn prepare_google_readonly(
    store: &Store,
    server: &ServerSigningKey,
    account: hantong_contracts::registration::GoogleReadAccount,
) -> Result<CliScopeRefresh, ErrorCode> {
    let effects = Native { store, server };
    let pin = hantong_platform::sha256(&effects.server_public()?)?;
    prepare_selected_for_pin(store, pin, Some(account))
}

fn readonly_add_scope(
    base: &RequestedScope,
    account: hantong_contracts::registration::GoogleReadAccount,
) -> Result<RequestedScope, ErrorCode> {
    let id = account.account_id();
    if base.tools() != hantong_contracts::auth::default_cli_body_tools()
        && !(base.tools() == hantong_contracts::auth::default_cli_legacy_tools()
            && base.accounts() == [id.clone()])
    {
        return Err(ErrorCode::Conflict);
    }
    let mut accounts = base.accounts().to_vec();
    if !accounts.contains(&id) {
        accounts.push(id);
    }
    accounts.sort();
    RequestedScope::new(accounts, hantong_contracts::auth::default_cli_body_tools())
}
pub(super) fn matches_google_readonly_add(
    r: &CliScopeRefresh,
    account: hantong_contracts::registration::GoogleReadAccount,
) -> bool {
    readonly_add_scope(&r.base_registration().scope, account).is_ok_and(|scope| {
        scope.accounts() == r.target_accounts() && scope.tools() == r.target_tools()
    })
}
pub(super) fn prepare_google_readonly_add(
    store: &Store,
    server: &ServerSigningKey,
    account: hantong_contracts::registration::GoogleReadAccount,
) -> Result<CliScopeRefresh, ErrorCode> {
    prepare_add_for_pin(
        store,
        hantong_platform::sha256(&server.public_key()?)?,
        account,
    )
}
fn prepare_add_for_pin(
    store: &Store,
    pin: [u8; 32],
    account: hantong_contracts::registration::GoogleReadAccount,
) -> Result<CliScopeRefresh, ErrorCode> {
    if !store
        .list_registrable_account_ids()?
        .contains(&account.account_id())
    {
        return Err(ErrorCode::NotFound);
    }
    let base = store
        .load_default_cli_registration()?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    let target = readonly_add_scope(&base.scope, account)?;
    if let Some(staged) = store.list_staged_cli_scope_refreshes()?.into_iter().next() {
        if !matches_google_readonly_add(&staged, account) || staged.server_public_key_hash() != &pin
        {
            return Err(ErrorCode::Conflict);
        }
        return Ok(staged);
    }
    if let Some(pending) = store.load_pending_cli_scope_refresh()? {
        if !matches_google_readonly_add(&pending, account)
            || pending.base_registration() != &base
            || pending.server_public_key_hash() != &pin
        {
            return Err(ErrorCode::Conflict);
        }
        if !store.expire_cli_scope_refresh(&pending)? {
            return Ok(pending);
        }
    }
    store.create_cli_scope_refresh_with_scope(hantong_platform::random_nonce()?, target, pin)
}

fn prepare_selected_for_pin(
    store: &Store,
    pin: [u8; 32],
    selected: Option<hantong_contracts::registration::GoogleReadAccount>,
) -> Result<CliScopeRefresh, ErrorCode> {
    if let Some(account) = selected {
        let id = account.account_id();
        if !store.list_registrable_account_ids()?.contains(&id) {
            return Err(ErrorCode::NotFound);
        }
        let base = store
            .load_default_cli_registration()?
            .ok_or(ErrorCode::ClientNotRegistered)?;
        if base.scope.accounts() != [id]
            || !(base.scope.tools() == hantong_contracts::auth::default_cli_legacy_tools()
                || base.scope.tools() == hantong_contracts::auth::default_cli_body_tools())
        {
            return Err(ErrorCode::Conflict);
        }
    }
    if let Some(staged) = store.list_staged_cli_scope_refreshes()?.into_iter().next() {
        if selected.is_some_and(|account| !matches_google_readonly(&staged, account)) {
            return Err(ErrorCode::Conflict);
        }
        return Ok(staged);
    }
    if let Some(pending) = store.load_pending_cli_scope_refresh()?
        && !store.expire_cli_scope_refresh(&pending)?
    {
        let current = store
            .load_default_cli_registration()?
            .ok_or(ErrorCode::NotFound)?;
        if &current != pending.base_registration()
            || match selected {
                Some(account) => !matches_google_readonly(&pending, account),
                None => store.list_registrable_account_ids()? != pending.target_accounts(),
            }
            || !hantong_contracts::auth::is_supported_default_cli_toolset(pending.target_tools())
            || &pin != pending.server_public_key_hash()
        {
            return Err(ErrorCode::Conflict);
        }
        return Ok(pending);
    }
    store.create_cli_scope_refresh_with_scope(
        hantong_platform::random_nonce()?,
        RequestedScope::new(
            match selected {
                Some(account) => vec![account.account_id()],
                None => store.list_registrable_account_ids()?,
            },
            if selected.is_some() {
                hantong_contracts::auth::default_cli_body_tools()
            } else {
                hantong_contracts::auth::default_cli_send_tools()
            },
        )?,
        pin,
    )
}
pub(super) fn status(
    store: &Store,
    id: [u8; 32],
    active: bool,
) -> Result<ScopeRefreshReceipt, ErrorCode> {
    let mut r = store
        .load_cli_scope_refresh(id)?
        .ok_or(ErrorCode::NotFound)?;
    if r.phase() == ScopeRefreshPhase::Pending && store.expire_cli_scope_refresh(&r)? {
        r = store
            .load_cli_scope_refresh(id)?
            .ok_or(ErrorCode::InternalError)?;
    }
    let state = match r.phase() {
        ScopeRefreshPhase::Pending if active => ScopeRefreshProgress::Pending,
        ScopeRefreshPhase::Staged if active => ScopeRefreshProgress::Provisioning,
        ScopeRefreshPhase::Pending | ScopeRefreshPhase::Staged => ScopeRefreshProgress::Interrupted,
        ScopeRefreshPhase::Active => ScopeRefreshProgress::Ready,
        ScopeRefreshPhase::Denied => ScopeRefreshProgress::Denied,
        ScopeRefreshPhase::Expired => ScopeRefreshProgress::Expired,
    };
    ScopeRefreshReceipt::new(*r.refresh_id(), r.expires_at_utc_ms(), state)
}
pub(super) async fn run_job(
    store: &Store,
    server: &ServerSigningKey,
    id: [u8; 32],
) -> Result<(), ErrorCode> {
    let effects = Native { store, server };
    let expected = effects.load(id)?;
    match expected.phase() {
        ScopeRefreshPhase::Staged => recover_staged(&effects, &expected),
        ScopeRefreshPhase::Active => Ok(()),
        ScopeRefreshPhase::Pending => {
            let review = review_snapshot(&expected)?;
            let config = store.load_config()?.ok_or(ErrorCode::InternalError)?;
            match hantong_approval_windows::review_scope_refresh(
                review.clone(),
                config.gui.language,
                config.gui.theme,
            )
            .await
            {
                Ok(verified) => {
                    let consumed = verified.consume(&review)?;
                    commit_consumed_review(&effects, &expected, &consumed)
                }
                Err(ErrorCode::ApprovalDenied) => store.deny_cli_scope_refresh(&expected).map(drop),
                Err(ErrorCode::ApprovalExpired) => {
                    store.expire_cli_scope_refresh(&expected)?;
                    Err(ErrorCode::ApprovalExpired)
                }
                Err(error) => Err(error),
            }
        }
        _ => Err(ErrorCode::Conflict),
    }
}

/// Completes the first personal CLI L->B/C transition after Google consent.
/// The Store source channel is checked before staging; no native review token
/// is accepted or manufactured on this path.
pub(super) fn run_google_oauth_job(
    store: &Store,
    server: &ServerSigningKey,
    id: [u8; 32],
) -> Result<(), ErrorCode> {
    let effects = Native { store, server };
    complete_google_oauth_scope(&effects, id)
}

fn complete_google_oauth_scope(effects: &impl Effects, id: [u8; 32]) -> Result<(), ErrorCode> {
    let expected = effects.load(id)?;
    if expected.phase() != ScopeRefreshPhase::Pending
        || expected.base_registration().approved_channel != "google_oauth"
        || expected.decision_channel() != "google_oauth"
    {
        return Err(ErrorCode::Conflict);
    }
    let staged = effects.stage(&expected)?;
    recover_staged(effects, &staged)
}
pub(super) fn recover_startup(store: &Store, server: &ServerSigningKey) -> Result<(), ErrorCode> {
    let effects = Native { store, server };
    // A Google-consent refresh is already durably authorized by its exact
    // source-bound Store row. Resume only that recorded channel after a crash;
    // never mint a new OAuth authority during recovery.
    if let Some(pending) = store.load_pending_cli_scope_refresh()?
        && pending.decision_channel() == "google_oauth"
    {
        let result = effects
            .stage(&pending)
            .and_then(|staged| recover_staged(&effects, &staged));
        if let Err(error) = result {
            effects.report(error);
        }
    }
    for staged in store.list_staged_cli_scope_refreshes()? {
        if let Err(error) = recover_staged(&effects, &staged) {
            effects.report(error);
        }
    }
    Ok(())
}

fn review_snapshot(r: &CliScopeRefresh) -> Result<ScopeRefreshReview, ErrorCode> {
    if r.phase() != ScopeRefreshPhase::Pending {
        return Err(ErrorCode::Conflict);
    }
    let b = r.base_registration();
    ScopeRefreshReview::new_with_tools(
        *r.refresh_id(),
        r.version(),
        b.registration_id,
        b.request_id,
        b.version,
        b.profile_epoch,
        *r.client_public_key_hash(),
        *r.server_public_key_hash(),
        b.scope.accounts().to_vec(),
        r.target_accounts().to_vec(),
        b.scope.tools().to_vec(),
        r.target_tools().to_vec(),
        r.created_at_utc_ms(),
        r.expires_at_utc_ms(),
    )
}
fn profiles(r: &CliScopeRefresh, server: [u8; 72]) -> Result<(Vec<u8>, Vec<u8>), ErrorCode> {
    let b = r.base_registration();
    if hantong_platform::sha256(&server)? != *r.server_public_key_hash() {
        return Err(ErrorCode::Conflict);
    }
    let key_id = hantong_platform::client_key_id_for_registration(b.registration_id)?;
    let old = ParsedClientProfile::new_with_scope(
        b.registration_id,
        key_id,
        b.public_key,
        server,
        b.profile_epoch,
        b.scope.clone(),
    )?
    .to_json()?;
    let new = ParsedClientProfile::new_with_scope(
        b.registration_id,
        key_id,
        b.public_key,
        server,
        r.target_profile_epoch(),
        r.target_scope(),
    )?
    .to_json()?;
    Ok((old, new))
}
trait Effects {
    type Key;
    type Publication;
    fn load(&self, id: [u8; 32]) -> Result<CliScopeRefresh, ErrorCode>;
    fn server_public(&self) -> Result<[u8; 72], ErrorCode>;
    fn open_key(&self, id: [u8; 16]) -> Result<Self::Key, ErrorCode>;
    fn key_public(&self, key: &Self::Key) -> Result<[u8; 72], ErrorCode>;
    fn stage(&self, expected: &CliScopeRefresh) -> Result<CliScopeRefresh, ErrorCode>;
    fn publish(&self, id: [u8; 32], old: &[u8], new: &[u8])
    -> Result<Self::Publication, ErrorCode>;
    fn activate(&self, expected: &CliScopeRefresh) -> Result<RegistrationRecord, ErrorCode>;
    fn cleanup(&self, publication: &mut Self::Publication) -> Result<(), ErrorCode>;
    fn report(&self, error: ErrorCode);
}
// Called only after the owned native token has been consumed. The private fake
// seam tests ordering without constructing an OS verification token.
fn commit_consumed_review<E: Effects>(
    effects: &E,
    expected: &CliScopeRefresh,
    review: &ScopeRefreshReview,
) -> Result<(), ErrorCode> {
    if effects.load(*expected.refresh_id())? != *expected
        || review_snapshot(expected)? != *review
        || hantong_platform::sha256(&effects.server_public()?)?
            != *expected.server_public_key_hash()
    {
        return Err(ErrorCode::Conflict);
    }
    let staged = effects.stage(expected)?;
    recover_staged(effects, &staged)
}
fn recover_staged<E: Effects>(effects: &E, expected: &CliScopeRefresh) -> Result<(), ErrorCode> {
    if expected.phase() != ScopeRefreshPhase::Staged
        || effects.load(*expected.refresh_id())? != *expected
    {
        return Err(ErrorCode::Conflict);
    }
    let server = effects.server_public()?;
    let (old, new) = profiles(expected, server)?;
    let base = expected.base_registration();
    let key = effects.open_key(base.registration_id)?;
    if effects.key_public(&key)? != base.public_key {
        return Err(ErrorCode::Conflict);
    }
    let mut publication = effects.publish(*expected.refresh_id(), &old, &new)?;
    effects.activate(expected)?;
    if let Err(error) = effects.cleanup(&mut publication) {
        effects.report(error);
    }
    Ok(())
}

struct Native<'a> {
    store: &'a Store,
    server: &'a ServerSigningKey,
}
impl Effects for Native<'_> {
    type Key = ClientSigningKey;
    type Publication = hantong_approval_windows::PublishedProfile;
    fn load(&self, id: [u8; 32]) -> Result<CliScopeRefresh, ErrorCode> {
        self.store
            .load_cli_scope_refresh(id)?
            .ok_or(ErrorCode::NotFound)
    }
    fn server_public(&self) -> Result<[u8; 72], ErrorCode> {
        let public = self.server.public_key()?;
        match self.store.load_server_identity()? {
            Some(ServerIdentity::Active {
                key_id, public_key, ..
            }) if key_id == self.server.id() && public_key == public => Ok(public),
            _ => Err(ErrorCode::Conflict),
        }
    }
    fn open_key(&self, id: [u8; 16]) -> Result<Self::Key, ErrorCode> {
        ClientSigningKey::open_for_registration(id)
    }
    fn key_public(&self, key: &Self::Key) -> Result<[u8; 72], ErrorCode> {
        key.public_key()
    }
    fn stage(&self, r: &CliScopeRefresh) -> Result<CliScopeRefresh, ErrorCode> {
        self.store.stage_cli_scope_refresh(r)
    }
    fn publish(
        &self,
        id: [u8; 32],
        old: &[u8],
        new: &[u8],
    ) -> Result<Self::Publication, ErrorCode> {
        hantong_approval_windows::refresh_default_profile(id, old, new)
    }
    fn activate(&self, r: &CliScopeRefresh) -> Result<RegistrationRecord, ErrorCode> {
        self.store.activate_cli_scope_refresh(r)
    }
    fn cleanup(&self, p: &mut Self::Publication) -> Result<(), ErrorCode> {
        hantong_approval_windows::cleanup_profile_refresh_backup(p)
    }
    fn report(&self, error: ErrorCode) {
        use std::io::Write;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_millis()).ok())
            .unwrap_or(0);
        if let Ok(line) = crate::registration::registration_failure_line(error, now) {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests;

pub(super) fn matches_google_send(
    r: &CliScopeRefresh,
    selected: hantong_contracts::scope_refresh::GoogleSendAccounts,
) -> bool {
    r.target_accounts() == selected.accounts()
        && r.target_tools() == hantong_contracts::auth::default_cli_send_tools()
}
fn google_changes_scope(
    base: &RequestedScope,
    selected: hantong_contracts::scope_refresh::GoogleSendAccounts,
) -> Result<RequestedScope, ErrorCode> {
    if base.tools() != hantong_contracts::auth::default_cli_send_tools()
        && !hantong_contracts::auth::is_google_change_toolset(base.tools())
    {
        return Err(ErrorCode::Conflict);
    }
    let mut tools = base.tools().to_vec();
    for account in selected.accounts() {
        if !base.accounts().contains(&account) {
            return Err(ErrorCode::Conflict);
        }
        let grant = hantong_contracts::google_mutation::account_grant(&account)?;
        if !tools.contains(&grant) {
            tools.push(grant);
        }
    }
    tools.sort();
    if !hantong_contracts::auth::is_google_change_toolset(&tools) {
        return Err(ErrorCode::QuotaExceeded);
    }
    RequestedScope::new(base.accounts().to_vec(), tools)
}
pub(super) fn matches_google_changes(
    r: &CliScopeRefresh,
    selected: hantong_contracts::scope_refresh::GoogleSendAccounts,
) -> bool {
    google_changes_scope(&r.base_registration().scope, selected).is_ok_and(|scope| {
        scope.accounts() == r.target_accounts() && scope.tools() == r.target_tools()
    })
}
pub(super) fn prepare_google_changes(
    store: &Store,
    server: &ServerSigningKey,
    selected: hantong_contracts::scope_refresh::GoogleSendAccounts,
) -> Result<CliScopeRefresh, ErrorCode> {
    prepare_google_changes_for_pin(
        store,
        hantong_platform::sha256(&server.public_key()?)?,
        selected,
    )
}
fn prepare_google_changes_for_pin(
    store: &Store,
    pin: [u8; 32],
    selected: hantong_contracts::scope_refresh::GoogleSendAccounts,
) -> Result<CliScopeRefresh, ErrorCode> {
    let available = store.list_registrable_account_ids()?;
    for account in selected.accounts() {
        if !available.contains(&account) {
            return Err(ErrorCode::NotFound);
        }
        store
            .google_account_binding(&account)?
            .access_mode
            .check_operation(hantong_contracts::google_write::GoogleOperation::ChangeLabels)?;
    }
    let base = store
        .load_default_cli_registration()?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    let target = google_changes_scope(&base.scope, selected)?;
    if let Some(staged) = store.list_staged_cli_scope_refreshes()?.into_iter().next() {
        if !matches_google_changes(&staged, selected) || staged.server_public_key_hash() != &pin {
            return Err(ErrorCode::Conflict);
        }
        return Ok(staged);
    }
    if let Some(pending) = store.load_pending_cli_scope_refresh()? {
        if !matches_google_changes(&pending, selected)
            || pending.base_registration() != &base
            || pending.server_public_key_hash() != &pin
        {
            return Err(ErrorCode::Conflict);
        }
        if !store.expire_cli_scope_refresh(&pending)? {
            return Ok(pending);
        }
    }
    store.create_cli_scope_refresh_with_scope(hantong_platform::random_nonce()?, target, pin)
}
pub(super) fn prepare_google_send(
    store: &Store,
    server: &ServerSigningKey,
    selected: hantong_contracts::scope_refresh::GoogleSendAccounts,
) -> Result<CliScopeRefresh, ErrorCode> {
    prepare_google_send_for_pin(
        store,
        hantong_platform::sha256(&server.public_key()?)?,
        selected,
    )
}
fn prepare_google_send_for_pin(
    store: &Store,
    pin: [u8; 32],
    selected: hantong_contracts::scope_refresh::GoogleSendAccounts,
) -> Result<CliScopeRefresh, ErrorCode> {
    let accounts = selected.accounts();
    let available = store.list_registrable_account_ids()?;
    for account in &accounts {
        if !available.contains(account) {
            return Err(ErrorCode::NotFound);
        }
        store
            .google_account_binding(account)?
            .access_mode
            .check_operation(hantong_contracts::google_write::GoogleOperation::Send)?;
    }
    let base = store
        .load_default_cli_registration()?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    // Do not silently remove existing account grants or expand other accounts.
    if base
        .scope
        .accounts()
        .iter()
        .any(|id| !accounts.contains(id))
        || !hantong_contracts::auth::is_supported_default_cli_toolset(base.scope.tools())
    {
        return Err(ErrorCode::Conflict);
    }
    let target = RequestedScope::new(accounts, hantong_contracts::auth::default_cli_send_tools())?;
    if let Some(staged) = store.list_staged_cli_scope_refreshes()?.into_iter().next() {
        if !matches_google_send(&staged, selected) || staged.server_public_key_hash() != &pin {
            return Err(ErrorCode::Conflict);
        }
        return Ok(staged);
    }
    if let Some(pending) = store.load_pending_cli_scope_refresh()? {
        if !matches_google_send(&pending, selected)
            || pending.base_registration() != &base
            || pending.server_public_key_hash() != &pin
        {
            return Err(ErrorCode::Conflict);
        }
        if !store.expire_cli_scope_refresh(&pending)? {
            return Ok(pending);
        }
    }
    store.create_cli_scope_refresh_with_scope(hantong_platform::random_nonce()?, target, pin)
}
