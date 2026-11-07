//! Core-owned, one-use app connection intent for account-qualified Google mail.
//!
//! This module only defines bounded intent and verified-delta metadata. It does
//! not persist, activate, or publish a grant.
#![allow(dead_code)]

use hantong_contracts::{
    ErrorCode,
    account_auth_peer::{CanonicalPeer, encode as encode_peer},
    account_scope::AccountGrantV2,
    auth::PeerContext,
    google::{GoogleIdentity, validate_google_client_id},
    google_write::GoogleAccessMode,
    unix_auth::UnixPeerContextV2,
};
use hantong_secrets::google::PendingGoogleCredential;
use hantong_store::account_grants::{MailGrantDecision, MailGrantPurpose};
use hantong_store::{ClientKind, DEFAULT_CLI_LABEL, RegistrationRecord, RegistrationState, Store};
use std::fmt;

const INTENT_TTL_MS: i64 = 5 * 60 * 1000;
const FULL_MAIL_TOOLS: [&str; 10] = [
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

#[derive(Clone, Eq, PartialEq)]
pub(crate) enum ObservedPeer {
    Windows(PeerContext),
    Unix(UnixPeerContextV2),
}
impl fmt::Debug for ObservedPeer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ObservedPeer(<redacted>)")
    }
}

/// A UI selector is not provider identity. New account identity is fixed only
/// from the verified pending credential.
#[derive(Clone, Eq, PartialEq)]
pub(crate) enum AccountSelection {
    NewFromProvider,
    ExistingExact { account_id: String, subject: String },
}
impl fmt::Debug for AccountSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountSelection(<redacted>)")
    }
}

/// Immutable metadata captured before provider consent. It is not an approval.
pub(crate) struct AccountConnectIntent {
    intent_id: [u8; 32],
    registration_id: [u8; 16],
    client_public_key: [u8; 72],
    client_id: String,
    profile_epoch: u64,
    registration_version: u64,
    server_key_pin: [u8; 32],
    baseline_scope_hash: [u8; 32],
    peer: ObservedPeer,
    selection: AccountSelection,
    target_tools: Vec<String>,
    issued_at_utc_ms: i64,
    expires_at_utc_ms: i64,
}
impl fmt::Debug for AccountConnectIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccountConnectIntent")
            .field("intent_id", &"<redacted>")
            .field("registration_id", &"<redacted>")
            .field("client_id", &"<redacted>")
            .field("selection", &"<redacted>")
            .field("issued_at_utc_ms", &self.issued_at_utc_ms)
            .field("expires_at_utc_ms", &self.expires_at_utc_ms)
            .finish_non_exhaustive()
    }
}

impl AccountConnectIntent {
    pub(crate) fn is_expired(&self, now: i64) -> bool {
        now < self.issued_at_utc_ms || now >= self.expires_at_utc_ms
    }
    pub(crate) fn client_id(&self) -> &str {
        &self.client_id
    }
    pub(crate) fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
    pub(crate) fn cancel(self) {}

    #[allow(clippy::too_many_arguments)] // Each value is an independently checked intent binding.
    pub(crate) fn new(
        registration: &RegistrationRecord,
        intent_id: [u8; 32],
        server_key_pin: [u8; 32],
        peer: ObservedPeer,
        client_id: String,
        selection: AccountSelection,
        target_tools: Vec<String>,
        now: i64,
    ) -> Result<Self, ErrorCode> {
        if registration.registration_id == [0; 16]
            || intent_id == [0; 32]
            || server_key_pin == [0; 32]
            || registration.client_kind != ClientKind::Cli
            || registration.display_label != DEFAULT_CLI_LABEL
            || registration.state != RegistrationState::Active
            || registration.profile_epoch == 0
            || registration.version == 0
            || now < 0
        {
            return Err(ErrorCode::InvalidInput);
        }
        hantong_platform::validate_public_key(&registration.public_key)?;
        validate_google_client_id(&client_id)?;
        let baseline_scope_hash = registration_scope_hash(registration)?;
        validate_full_mail_tools(&target_tools)?;
        validate_selection(&selection)?;
        let expires_at_utc_ms = now
            .checked_add(INTENT_TTL_MS)
            .ok_or(ErrorCode::InvalidInput)?;
        Ok(Self {
            intent_id,
            registration_id: registration.registration_id,
            client_public_key: registration.public_key,
            client_id,
            profile_epoch: registration.profile_epoch,
            registration_version: registration.version,
            server_key_pin,
            baseline_scope_hash,
            peer,
            selection,
            target_tools,
            issued_at_utc_ms: now,
            expires_at_utc_ms,
        })
    }

    /// Consumes the intent. Provider identity and the pending secret must come
    /// from the live Secrets exchange; metadata alone cannot mint a delta.
    #[allow(clippy::too_many_arguments)] // Live proof and current consumer bindings stay explicit.
    pub(crate) fn authorize(
        self,
        pending: &PendingGoogleCredential,
        registration: &RegistrationRecord,
        current_scope_hash: [u8; 32],
        current_server_key_pin: [u8; 32],
        observed_peer: &ObservedPeer,
        credential_version: u64,
        now: i64,
    ) -> Result<VerifiedGoogleDelta, ErrorCode> {
        if self.is_expired(now) {
            return Err(ErrorCode::ApprovalExpired);
        }
        let registration_hash = registration_scope_hash(registration)?;
        if current_scope_hash == [0; 32]
            || current_server_key_pin == [0; 32]
            || registration.registration_id != self.registration_id
            || registration.client_kind != ClientKind::Cli
            || registration.display_label != DEFAULT_CLI_LABEL
            || registration.public_key != self.client_public_key
            || registration.profile_epoch != self.profile_epoch
            || registration.version != self.registration_version
            || registration.state != RegistrationState::Active
            || current_scope_hash != self.baseline_scope_hash
            || current_scope_hash != registration_hash
            || current_server_key_pin != self.server_key_pin
            || observed_peer != &self.peer
            || credential_version == 0
        {
            return Err(ErrorCode::Conflict);
        }
        hantong_platform::validate_public_key(&registration.public_key)?;
        let identity = pending.identity();
        identity.validate()?;
        if identity.access_mode != GoogleAccessMode::Modify
            || !selection_matches(&self.selection, identity)
            || !selection_allowed_by_registration(&self.selection, identity, registration)
        {
            return Err(ErrorCode::ScopeDenied);
        }
        let binding = pending.binding(credential_version)?;
        if binding.client_id != self.client_id
            || binding.account_id != identity.account_id
            || binding.subject != identity.subject
            || binding.access_mode != GoogleAccessMode::Modify
        {
            return Err(ErrorCode::ScopeDenied);
        }
        let target = AccountGrantV2::new(identity.account_id.clone(), self.target_tools, false)?;
        let purpose = selection_purpose(&self.selection, registration);
        Ok(VerifiedGoogleDelta {
            intent_id: self.intent_id,
            registration_id: self.registration_id,
            account_id: identity.account_id.clone(),
            subject: identity.subject.clone(),
            client_id: binding.client_id,
            client_public_key: self.client_public_key,
            credential_version,
            profile_epoch: self.profile_epoch,
            registration_version: self.registration_version,
            server_key_pin: self.server_key_pin,
            baseline_scope_hash: self.baseline_scope_hash,
            peer: self.peer,
            target,
            purpose,
            issued_at_utc_ms: self.issued_at_utc_ms,
            expires_at_utc_ms: self.expires_at_utc_ms,
        })
    }
}

/// Move-only proof produced by a live provider/Secrets exchange.
pub(crate) struct VerifiedGoogleDelta {
    intent_id: [u8; 32],
    registration_id: [u8; 16],
    account_id: String,
    subject: String,
    client_id: String,
    client_public_key: [u8; 72],
    credential_version: u64,
    profile_epoch: u64,
    registration_version: u64,
    server_key_pin: [u8; 32],
    baseline_scope_hash: [u8; 32],
    peer: ObservedPeer,
    target: AccountGrantV2,
    purpose: MailGrantPurpose,
    issued_at_utc_ms: i64,
    expires_at_utc_ms: i64,
}
impl fmt::Debug for VerifiedGoogleDelta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedGoogleDelta(<redacted>)")
    }
}
impl VerifiedGoogleDelta {
    fn is_expired(&self, now: i64) -> bool {
        now < self.issued_at_utc_ms || now >= self.expires_at_utc_ms
    }

    pub(crate) fn intent_id(&self) -> &[u8; 32] {
        &self.intent_id
    }
    pub(crate) fn registration_id(&self) -> &[u8; 16] {
        &self.registration_id
    }
    pub(crate) fn account_id(&self) -> &str {
        &self.account_id
    }
    pub(crate) fn subject(&self) -> &str {
        &self.subject
    }
    pub(crate) fn client_id(&self) -> &str {
        &self.client_id
    }
    pub(crate) fn credential_version(&self) -> u64 {
        self.credential_version
    }
    pub(crate) fn server_key_pin(&self) -> &[u8; 32] {
        &self.server_key_pin
    }
    pub(crate) fn baseline_scope_hash(&self) -> &[u8; 32] {
        &self.baseline_scope_hash
    }
    pub(crate) fn peer(&self) -> &ObservedPeer {
        &self.peer
    }
    pub(crate) fn target(&self) -> &AccountGrantV2 {
        &self.target
    }
    pub(crate) fn issued_at_utc_ms(&self) -> i64 {
        self.issued_at_utc_ms
    }
    pub(crate) fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }

    pub(crate) fn begin_full_mail_decision(
        &self,
        store: &Store,
        operation: &str,
        idempotency: &str,
    ) -> Result<MailGrantDecision, ErrorCode> {
        store.begin_full_mail_decision(
            self.intent_id,
            self.registration_id,
            self.registration_version,
            self.profile_epoch,
            self.baseline_scope_hash,
            self.account_id(),
            self.purpose,
            operation,
            idempotency,
            peer_digest(&self.peer)?,
            self.expires_at_utc_ms,
        )
    }

    pub(crate) fn consume_and_stage(
        self,
        store: &Store,
        decision: [u8; 32],
        current_server_key_pin: [u8; 32],
        observed_peer: &ObservedPeer,
        now_utc_ms: i64,
    ) -> Result<(), ErrorCode> {
        let registration = store
            .load_registration(self.registration_id)?
            .ok_or(ErrorCode::NotFound)?;
        if self.is_expired(now_utc_ms)
            || decision != self.intent_id
            || current_server_key_pin != self.server_key_pin
            || observed_peer != &self.peer
            || registration.registration_id != self.registration_id
            || registration.public_key != self.client_public_key
            || registration.profile_epoch != self.profile_epoch
            || registration.version != self.registration_version
            || registration.client_kind != ClientKind::Cli
            || registration.display_label != DEFAULT_CLI_LABEL
            || registration.state != RegistrationState::Active
            || registration_scope_hash(&registration)? != self.baseline_scope_hash
        {
            return Err(ErrorCode::Conflict);
        }
        let binding = store.google_account_binding(self.account_id())?;
        if binding.client_id != self.client_id
            || binding.subject != self.subject
            || binding.credential_version != self.credential_version
            || binding.access_mode != GoogleAccessMode::Modify
        {
            return Err(ErrorCode::ScopeDenied);
        }
        store.consume_and_stage_full_mail_decision(
            self.registration_id,
            decision,
            self.registration_version,
            self.profile_epoch,
            peer_digest(&self.peer)?,
            self.authority_binding_digest()?,
        )
    }

    pub(crate) fn authority_binding_digest(&self) -> Result<[u8; 32], ErrorCode> {
        let peer = peer_digest(&self.peer)?;
        let mut bytes = b"mail-hantong/account-connect/full-mail/v2\0".to_vec();
        for value in [
            self.intent_id.as_slice(),
            self.registration_id.as_slice(),
            &self.profile_epoch.to_be_bytes(),
            &self.registration_version.to_be_bytes(),
            self.client_id.as_bytes(),
            self.account_id().as_bytes(),
            self.subject.as_bytes(),
            &self.credential_version.to_be_bytes(),
            self.target.tools().join("\0").as_bytes(),
            &self.server_key_pin,
            &self.baseline_scope_hash,
            &peer,
        ] {
            bytes.extend_from_slice(&(value.len() as u32).to_be_bytes());
            bytes.extend_from_slice(value);
        }
        hantong_platform::sha256(&bytes)
    }
}

/// Publishes and activates only the exact staged target. A profile already
/// equal to the staged target is recovered without replaying OAuth.
fn profile_matches_identity(
    profile: &hantong_ipc_client::profile::ParsedClientProfile,
    owner: [u8; 16],
    client_public_key: [u8; 72],
    server_public_key: [u8; 72],
    epoch: u64,
    scope: &hantong_contracts::account_scope::AccountScopeV2,
) -> bool {
    profile.profile() == hantong_ipc_client::profile::ProfileSelector::Default
        && profile.registration_id() == &owner
        && profile.client_public_key() == &client_public_key
        && profile.server_public_key() == &server_public_key
        && profile.profile_epoch() == epoch
        && profile.scope().account_scope() == Some(scope)
}

fn profile_matches_legacy_base(
    profile: &hantong_ipc_client::profile::ParsedClientProfile,
    base: &RegistrationRecord,
    server_public_key: [u8; 72],
) -> bool {
    profile.profile() == hantong_ipc_client::profile::ProfileSelector::Default
        && profile.registration_id() == &base.registration_id
        && profile.client_public_key() == &base.public_key
        && profile.server_public_key() == &server_public_key
        && profile.profile_epoch() == base.profile_epoch
        && profile.scope() == &base.scope
}

pub(crate) fn publish_staged(
    store: &Store,
    owner: [u8; 16],
    decision: [u8; 32],
    server_public_key: [u8; 72],
) -> Result<(), ErrorCode> {
    let staged = publication_step(
        "staged_binding",
        store.staged_full_mail_decision(owner, decision),
    )?;
    let server_hash = hantong_platform::sha256(&server_public_key)?;
    if staged.server_public_key_hash != server_hash
        || staged.target_version != staged.base.version + 1
        || staged.target_epoch != staged.base.profile_epoch + 1
    {
        return Err(ErrorCode::Conflict);
    }
    let current = publication_step(
        "profile_snapshot",
        hantong_platform::load_default_client_profile(),
    )?;
    let current_profile =
        hantong_ipc_client::profile::ParsedClientProfile::from_json(current.bytes())?;
    let old = if profile_matches_identity(
        &current_profile,
        owner,
        staged.base.public_key,
        server_public_key,
        staged.target_epoch,
        &staged.target,
    ) {
        load_publication_backup(decision)?
    } else {
        current.bytes().to_vec()
    };
    // The ordinary loader denies replacement on Windows. Release that old
    // image pin before the native publisher acquires its exclusive handles.
    drop(current);
    let old_profile = hantong_ipc_client::profile::ParsedClientProfile::from_json(&old)?;
    if !profile_matches_legacy_base(&old_profile, &staged.base, server_public_key) {
        return Err(ErrorCode::Conflict);
    }
    let key_id = hantong_platform::client_key_id_for_registration(owner)?;
    let profile = hantong_ipc_client::profile::ParsedClientProfile::new_for_account_scope(
        hantong_ipc_client::profile::ProfileSelector::Default,
        owner,
        key_id,
        staged.base.public_key,
        server_public_key,
        staged.target_epoch,
        staged.target.clone(),
    )?;
    if !profile_matches_identity(
        &profile,
        owner,
        staged.base.public_key,
        server_public_key,
        staged.target_epoch,
        &staged.target,
    ) {
        return Err(ErrorCode::Conflict);
    }
    let bytes = profile.to_json()?;
    #[cfg(windows)]
    let mut publication =
        hantong_approval_windows::refresh_default_profile(decision, &old, &bytes)?;
    #[cfg(all(not(windows), unix))]
    let mut publication = hantong_platform::unix_profile_publication::refresh_default_profile(
        decision, &old, &bytes,
    )?;
    #[cfg(not(any(windows, unix)))]
    return Err(ErrorCode::CapabilityUnsupported);
    #[cfg(windows)]
    let published = hantong_approval_windows::verify_published_default_profile(&publication)?;
    #[cfg(all(not(windows), unix))]
    let published = publication_step(
        "publication_evidence",
        hantong_platform::account_publication::VerifiedAccountProfilePublication::load_default(),
    )?;
    publication_step(
        "grant_activation",
        store.activate_full_mail_decision(
            owner,
            decision,
            staged.target_version,
            staged.target_epoch,
            staged.legacy_binding_hash,
            &published,
        ),
    )?;
    // Activation is committed. Cleanup failure must not report a staged grant;
    // the validated committed-history cleanup path retries after restart.
    #[cfg(windows)]
    let cleanup = hantong_approval_windows::cleanup_profile_refresh_backup(&mut publication);
    #[cfg(all(not(windows), unix))]
    let cleanup = hantong_platform::unix_profile_publication::cleanup_profile_refresh_backup(
        &mut publication,
    );
    if let Err(error) = cleanup {
        publication_cleanup_failed(error);
    }
    Ok(())
}

/// Replays only already-staged decisions after a restart. It never performs
/// OAuth, reads pending credentials, or creates a new decision.
pub(crate) fn recover_startup(store: &Store, server_public_key: [u8; 72]) -> Result<(), ErrorCode> {
    for (owner, decision) in store.list_staged_full_mail_decisions()? {
        publish_staged(store, owner, decision, server_public_key)?;
    }
    if let Err(error) = cleanup_committed_publications(store, server_public_key) {
        publication_cleanup_failed(error);
    }
    Ok(())
}

fn publication_cleanup_failed(error: ErrorCode) {
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr().lock(),
        "hantong_account_connect stage=profile_backup_cleanup outcome=deferred code={}",
        error.as_str()
    );
}

fn publication_step<T>(stage: &'static str, result: Result<T, ErrorCode>) -> Result<T, ErrorCode> {
    if let Err(error) = &result {
        use std::io::Write;
        let _ = writeln!(
            std::io::stderr().lock(),
            "hantong_account_connect stage={stage} outcome=failed code={}",
            error.as_str()
        );
    }
    result
}

fn load_publication_backup(decision: [u8; 32]) -> Result<Vec<u8>, ErrorCode> {
    #[cfg(windows)]
    return hantong_approval_windows::load_default_profile_refresh_backup(decision);
    #[cfg(all(not(windows), unix))]
    return hantong_platform::unix_profile_publication::load_default_profile_refresh_backup(
        decision,
    );
    #[cfg(not(any(windows, unix)))]
    Err(ErrorCode::CapabilityUnsupported)
}

fn cleanup_backup_matches(
    profile: &hantong_ipc_client::profile::ParsedClientProfile,
    cleanup: &hantong_store::account_grants::FullMailCleanup,
    server: [u8; 72],
) -> Result<bool, ErrorCode> {
    let scope = match profile.scope().account_scope() {
        Some(scope) => scope.clone(),
        None => hantong_contracts::account_scope::AccountScopeV2::from_legacy(
            profile.scope(),
            cleanup.legacy_read_only,
        )?,
    };
    Ok(
        profile.profile() == hantong_ipc_client::profile::ProfileSelector::Default
            && profile.registration_id() == &cleanup.registration_id
            && profile.client_public_key().as_slice() == cleanup.client_public_key
            && profile.server_public_key() == &server
            && hantong_platform::sha256(&server)? == cleanup.server_public_key_hash
            && profile.profile_epoch() == cleanup.expected_profile_epoch
            && hantong_platform::sha256(&scope.to_json()?)? == cleanup.baseline_scope_hash,
    )
}

fn cleanup_committed_publications(store: &Store, server: [u8; 72]) -> Result<(), ErrorCode> {
    for cleanup in store.list_committed_full_mail_cleanup()? {
        let old = match load_publication_backup(cleanup.decision_id) {
            Ok(old) => old,
            Err(ErrorCode::NotFound) => continue,
            Err(error) => return Err(error),
        };
        let profile = hantong_ipc_client::profile::ParsedClientProfile::from_json(&old)?;
        if !cleanup_backup_matches(&profile, &cleanup, server)? {
            return Err(ErrorCode::Conflict);
        }
        #[cfg(windows)]
        hantong_approval_windows::discard_default_profile_refresh_backup(
            cleanup.decision_id,
            &old,
        )?;
        #[cfg(all(not(windows), unix))]
        hantong_platform::unix_profile_publication::discard_default_profile_refresh_backup(
            cleanup.decision_id,
            &old,
        )?;
    }
    Ok(())
}

fn peer_digest(peer: &ObservedPeer) -> Result<[u8; 32], ErrorCode> {
    let canonical = match peer {
        ObservedPeer::Windows(peer) => encode_peer(&CanonicalPeer::Windows(peer.clone()))?,
        ObservedPeer::Unix(peer) => encode_peer(&CanonicalPeer::Unix(peer.clone()))?,
    };
    hantong_platform::sha256(&canonical)
}

fn validate_full_mail_tools(tools: &[String]) -> Result<(), ErrorCode> {
    if tools.len() != FULL_MAIL_TOOLS.len()
        || tools
            .iter()
            .any(|tool| !FULL_MAIL_TOOLS.contains(&tool.as_str()))
        || FULL_MAIL_TOOLS
            .iter()
            .any(|tool| !tools.iter().any(|v| v == tool))
    {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    Ok(())
}
fn validate_selection(selection: &AccountSelection) -> Result<(), ErrorCode> {
    match selection {
        AccountSelection::NewFromProvider => Ok(()),
        AccountSelection::ExistingExact {
            account_id,
            subject,
        } if !account_id.is_empty()
            && account_id.len() <= 128
            && account_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
            && !subject.is_empty()
            && subject.len() <= 255
            && subject.bytes().all(|byte| byte.is_ascii_graphic()) =>
        {
            Ok(())
        }
        AccountSelection::ExistingExact { .. } => Err(ErrorCode::InvalidInput),
    }
}
fn selection_matches(selection: &AccountSelection, identity: &GoogleIdentity) -> bool {
    match selection {
        AccountSelection::NewFromProvider => true,
        AccountSelection::ExistingExact {
            account_id,
            subject,
        } => account_id == &identity.account_id && subject == &identity.subject,
    }
}
fn selection_allowed_by_registration(
    selection: &AccountSelection,
    identity: &GoogleIdentity,
    registration: &RegistrationRecord,
) -> bool {
    let already_granted = registration
        .scope
        .accounts()
        .iter()
        .any(|account| account == &identity.account_id);
    match selection {
        AccountSelection::NewFromProvider => !already_granted,
        AccountSelection::ExistingExact { .. } => true,
    }
}
fn selection_purpose(
    selection: &AccountSelection,
    registration: &RegistrationRecord,
) -> MailGrantPurpose {
    match selection {
        AccountSelection::NewFromProvider => MailGrantPurpose::Add,
        AccountSelection::ExistingExact { account_id, .. }
            if registration
                .scope
                .accounts()
                .iter()
                .any(|account| account == account_id) =>
        {
            MailGrantPurpose::Upgrade
        }
        AccountSelection::ExistingExact { .. } => MailGrantPurpose::Add,
    }
}
fn registration_scope_hash(registration: &RegistrationRecord) -> Result<[u8; 32], ErrorCode> {
    let scope = match registration.scope.account_scope() {
        Some(scope) => scope.clone(),
        None => hantong_contracts::account_scope::AccountScopeV2::from_legacy(
            &registration.scope,
            registration.read_only,
        )?,
    };
    hantong_platform::sha256(&scope.to_json()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::{
        auth::RequestedScope,
        unix_auth::{UnixPeerContextV2, UnixPlatform},
    };
    fn public_key() -> [u8; 72] {
        let mut key = [0; 72];
        key[..8].copy_from_slice(b"ECS1\x20\0\0\0");
        key[8..40].copy_from_slice(&hex(
            "6b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c296",
        ));
        key[40..].copy_from_slice(&hex(
            "4fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5",
        ));
        key
    }
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }
    fn account() -> String {
        "google_".to_owned() + &"a".repeat(64)
    }
    fn registration() -> RegistrationRecord {
        RegistrationRecord {
            registration_id: [1; 16],
            request_id: [2; 32],
            client_kind: ClientKind::Cli,
            display_label: DEFAULT_CLI_LABEL.into(),
            public_key: public_key(),
            profile_epoch: 4,
            state: RegistrationState::Active,
            read_only: false,
            requests_per_minute: 60,
            approved_channel: "terminal_cli".into(),
            version: 5,
            scope: RequestedScope::new(vec![account()], vec!["mail.accounts.list".into()]).unwrap(),
        }
    }
    fn peer(id: u8) -> ObservedPeer {
        ObservedPeer::Unix(
            UnixPeerContextV2::new(UnixPlatform::Linux, 1000, 1000, 42, 7, [id; 32]).unwrap(),
        )
    }
    fn tools() -> Vec<String> {
        FULL_MAIL_TOOLS.iter().map(|v| (*v).to_owned()).collect()
    }
    fn target_scope() -> hantong_contracts::account_scope::AccountScopeV2 {
        hantong_contracts::account_scope::AccountScopeV2::new(
            vec!["mail.accounts.list".into()],
            vec![
                hantong_contracts::account_scope::AccountGrantV2::new(account(), tools(), false)
                    .unwrap(),
            ],
        )
        .unwrap()
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sec03_native_publication_recovers_old_and_already_published_target() {
        use hantong_store::account_grants::MailGrantPurpose;
        use std::os::unix::fs::PermissionsExt;
        const CASE: &str = "HANTONG_TEST_ACCOUNT_PUBLICATION_CASE";
        if let Ok(case) = std::env::var(CASE) {
            assert!(matches!(case.as_str(), "old" | "target"));
            let f = crate::account_connect_generic::tests::Fixture::new();
            let base = &f.base;
            let server = base.public_key;
            let decision = [44; 32];
            let old = hantong_ipc_client::profile::ParsedClientProfile::new_with_scope(
                base.registration_id,
                hantong_platform::client_key_id_for_registration(base.registration_id).unwrap(),
                base.public_key,
                server,
                base.profile_epoch,
                base.scope.clone(),
            )
            .unwrap()
            .to_json()
            .unwrap();
            drop(
                hantong_platform::unix_profile_publication::publish_default_profile(&old).unwrap(),
            );
            let expiry = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64
                + 60_000;
            f.store
                .begin_full_mail_decision(
                    decision,
                    base.registration_id,
                    base.version,
                    base.profile_epoch,
                    f.store
                        .full_mail_baseline_hash(base.registration_id)
                        .unwrap(),
                    "baseline-account",
                    MailGrantPurpose::Upgrade,
                    "native-publish",
                    "native-publish",
                    [5; 32],
                    expiry,
                )
                .unwrap();
            f.store
                .consume_and_stage_full_mail_decision(
                    base.registration_id,
                    decision,
                    base.version,
                    base.profile_epoch,
                    [5; 32],
                    [6; 32],
                )
                .unwrap();
            if case == "target" {
                let staged = f
                    .store
                    .staged_full_mail_decision(base.registration_id, decision)
                    .unwrap();
                let target =
                    hantong_ipc_client::profile::ParsedClientProfile::new_for_account_scope(
                        hantong_ipc_client::profile::ProfileSelector::Default,
                        base.registration_id,
                        hantong_platform::client_key_id_for_registration(base.registration_id)
                            .unwrap(),
                        base.public_key,
                        server,
                        staged.target_epoch,
                        staged.target,
                    )
                    .unwrap()
                    .to_json()
                    .unwrap();
                // Crash boundary: native publication completed, Store activation did not.
                drop(
                    hantong_platform::unix_profile_publication::refresh_default_profile(
                        decision, &old, &target,
                    )
                    .unwrap(),
                );
            }
            assert_eq!(
                f.store
                    .load_default_cli_registration()
                    .unwrap()
                    .unwrap()
                    .state,
                RegistrationState::Provisioning
            );
            publish_staged(&f.store, base.registration_id, decision, server).unwrap();
            let active = f.store.load_default_cli_registration().unwrap().unwrap();
            assert_eq!(
                (active.state, active.version, active.profile_epoch),
                (
                    RegistrationState::Active,
                    base.version + 1,
                    base.profile_epoch + 1
                )
            );
            assert!(
                active
                    .scope
                    .permits_account("baseline-account", "mail.send.execute")
            );
            assert_eq!(
                load_publication_backup(decision).err(),
                Some(ErrorCode::NotFound)
            );
            cleanup_committed_publications(&f.store, server).unwrap();
            return;
        }
        for case in ["old", "target"] {
            let root = std::fs::canonicalize(std::env::temp_dir())
                .unwrap()
                .join(format!(
                    "hantong-account-publish-{}-{case}",
                    std::process::id()
                ));
            std::fs::create_dir(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","account_connect::tests::sec03_native_publication_recovers_old_and_already_published_target","--test-threads=1"])
                .env(CASE,case).env("XDG_CONFIG_HOME",&root).env("XDG_DATA_HOME",&root)
                .spawn().unwrap();
            let started = std::time::Instant::now();
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if started.elapsed() > std::time::Duration::from_secs(120) {
                    child.kill().unwrap();
                    let _ = child.wait();
                    panic!("synthetic publication child timeout");
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            };
            std::fs::remove_dir_all(&root).unwrap();
            assert!(status.success(), "synthetic publication child failed");
        }
    }
    #[test]
    fn sec03_full_mail_target_is_exact_and_unix_peer_is_typed() {
        let record = registration();
        assert!(
            AccountConnectIntent::new(
                &record,
                [1; 32],
                [2; 32],
                peer(1),
                "client.apps.googleusercontent.com".into(),
                AccountSelection::NewFromProvider,
                tools(),
                10
            )
            .is_ok()
        );
        let mut bad = tools();
        bad.push("mail.unknown".into());
        assert_eq!(
            AccountConnectIntent::new(
                &record,
                [1; 32],
                [2; 32],
                peer(1),
                "client.apps.googleusercontent.com".into(),
                AccountSelection::NewFromProvider,
                bad,
                10
            )
            .unwrap_err(),
            ErrorCode::CapabilityUnsupported
        );
    }
    #[test]
    fn sec03_selection_mismatch_and_expiry_are_rejected_before_proof() {
        let record = registration();
        let intent = AccountConnectIntent::new(
            &record,
            [1; 32],
            [2; 32],
            peer(1),
            "client.apps.googleusercontent.com".into(),
            AccountSelection::ExistingExact {
                account_id: account(),
                subject: "subject".into(),
            },
            tools(),
            10,
        )
        .unwrap();
        assert!(!intent.is_expired(10));
        assert!(intent.is_expired(10 + INTENT_TTL_MS));
        assert_ne!(peer(1), peer(2));
    }

    #[test]
    fn sec03_account_selection_requires_exact_provider_identity_and_purpose() {
        let identity = GoogleIdentity {
            access_mode: GoogleAccessMode::Modify,
            account_id: account(),
            subject: "subject".into(),
            email: "fixture@example.test".into(),
            granted_scopes: Vec::new(),
        };
        assert!(selection_matches(
            &AccountSelection::NewFromProvider,
            &identity
        ));
        let record = registration();
        assert!(!selection_allowed_by_registration(
            &AccountSelection::NewFromProvider,
            &identity,
            &record,
        ));
        assert!(selection_matches(
            &AccountSelection::ExistingExact {
                account_id: account(),
                subject: "subject".into(),
            },
            &identity,
        ));
        assert!(selection_allowed_by_registration(
            &AccountSelection::ExistingExact {
                account_id: account(),
                subject: "subject".into(),
            },
            &identity,
            &record,
        ));
        assert!(!selection_matches(
            &AccountSelection::ExistingExact {
                account_id: account(),
                subject: "other-subject".into(),
            },
            &identity,
        ));
        assert!(!selection_matches(
            &AccountSelection::ExistingExact {
                account_id: "google_".to_owned() + &"b".repeat(64),
                subject: "subject".into(),
            },
            &identity,
        ));
        assert_eq!(
            selection_purpose(
                &AccountSelection::ExistingExact {
                    account_id: account(),
                    subject: "subject".into(),
                },
                &record,
            ),
            MailGrantPurpose::Upgrade
        );
        let other_account = "google_".to_owned() + &"b".repeat(64);
        assert!(selection_allowed_by_registration(
            &AccountSelection::ExistingExact {
                account_id: other_account.clone(),
                subject: "subject".into(),
            },
            &GoogleIdentity {
                account_id: other_account.clone(),
                ..identity.clone()
            },
            &record,
        ));
        assert_eq!(
            selection_purpose(&AccountSelection::NewFromProvider, &record),
            MailGrantPurpose::Add
        );
        assert_eq!(
            selection_purpose(
                &AccountSelection::ExistingExact {
                    account_id: other_account,
                    subject: "subject".into(),
                },
                &record,
            ),
            MailGrantPurpose::Add
        );
    }

    #[test]
    fn sec03_intent_rejects_invalid_client_pin_and_existing_selector() {
        let record = registration();
        let selection = AccountSelection::ExistingExact {
            account_id: account(),
            subject: "subject".into(),
        };
        assert_eq!(
            AccountConnectIntent::new(
                &record,
                [1; 32],
                [2; 32],
                peer(1),
                "not-a-google-client".into(),
                selection.clone(),
                tools(),
                10,
            )
            .unwrap_err(),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            AccountConnectIntent::new(
                &record,
                [1; 32],
                [0; 32],
                peer(1),
                "client.apps.googleusercontent.com".into(),
                selection.clone(),
                tools(),
                10,
            )
            .unwrap_err(),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            AccountConnectIntent::new(
                &record,
                [1; 32],
                [2; 32],
                peer(1),
                "client.apps.googleusercontent.com".into(),
                AccountSelection::ExistingExact {
                    account_id: "bad/id".into(),
                    subject: "subject".into(),
                },
                tools(),
                10,
            )
            .unwrap_err(),
            ErrorCode::InvalidInput
        );
    }
    #[test]
    fn sec03_cancel_consumes_intent_without_delta() {
        let record = registration();
        let intent = AccountConnectIntent::new(
            &record,
            [1; 32],
            [2; 32],
            peer(1),
            "client.apps.googleusercontent.com".into(),
            AccountSelection::NewFromProvider,
            tools(),
            10,
        )
        .unwrap();
        intent.cancel();
    }

    #[test]
    fn sec03_publish_profile_matching_distinguishes_old_and_target_recovery() {
        let base = registration();
        let server = public_key();
        let old = hantong_ipc_client::profile::ParsedClientProfile::new_with_scope(
            base.registration_id,
            [9; 32],
            base.public_key,
            server,
            base.profile_epoch,
            base.scope.clone(),
        )
        .unwrap();
        assert!(profile_matches_legacy_base(&old, &base, server));

        let target_scope = target_scope();
        let target = hantong_ipc_client::profile::ParsedClientProfile::new_for_account_scope(
            hantong_ipc_client::profile::ProfileSelector::Default,
            base.registration_id,
            [9; 32],
            base.public_key,
            server,
            base.profile_epoch + 1,
            target_scope.clone(),
        )
        .unwrap();
        assert!(profile_matches_identity(
            &target,
            base.registration_id,
            base.public_key,
            server,
            base.profile_epoch + 1,
            &target_scope,
        ));
        assert!(!profile_matches_identity(
            &target,
            base.registration_id,
            base.public_key,
            server,
            base.profile_epoch,
            &target_scope,
        ));
        assert!(!profile_matches_identity(
            &target,
            base.registration_id,
            base.public_key,
            [8; 72],
            base.profile_epoch + 1,
            &target_scope,
        ));
        assert!(!profile_matches_identity(
            &target,
            [7; 16],
            base.public_key,
            server,
            base.profile_epoch + 1,
            &target_scope,
        ));
        let other_scope = hantong_contracts::account_scope::AccountScopeV2::new(
            vec!["mail.accounts.list".into()],
            vec![
                hantong_contracts::account_scope::AccountGrantV2::new(
                    "google_".to_owned() + &"b".repeat(64),
                    tools(),
                    false,
                )
                .unwrap(),
            ],
        )
        .unwrap();
        assert!(!profile_matches_identity(
            &target,
            base.registration_id,
            base.public_key,
            server,
            base.profile_epoch + 1,
            &other_scope,
        ));
        assert!(!profile_matches_legacy_base(&target, &base, server));
    }

    #[test]
    fn sec03_committed_backup_cleanup_requires_exact_identity_epoch_and_scope() {
        let base = registration();
        let server = public_key();
        let old = hantong_ipc_client::profile::ParsedClientProfile::new_with_scope(
            base.registration_id,
            [9; 32],
            base.public_key,
            server,
            base.profile_epoch,
            base.scope.clone(),
        )
        .unwrap();
        let cleanup = hantong_store::account_grants::FullMailCleanup {
            registration_id: base.registration_id,
            decision_id: [7; 32],
            expected_registration_version: base.version,
            expected_profile_epoch: base.profile_epoch,
            baseline_scope_hash: registration_scope_hash(&base).unwrap(),
            legacy_read_only: base.read_only,
            client_public_key: base.public_key.to_vec(),
            server_public_key_hash: hantong_platform::sha256(&server).unwrap(),
            target_account_id: account(),
        };
        assert!(cleanup_backup_matches(&old, &cleanup, server).unwrap());
        for field in 0..5 {
            let mut changed = cleanup.clone();
            match field {
                0 => changed.registration_id[0] ^= 1,
                1 => changed.client_public_key[8] ^= 1,
                2 => changed.expected_profile_epoch += 1,
                3 => changed.baseline_scope_hash[0] ^= 1,
                _ => changed.server_public_key_hash[0] ^= 1,
            }
            assert!(!cleanup_backup_matches(&old, &changed, server).unwrap());
        }
    }
}
