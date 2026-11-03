//! Explicit Google setup: opaque pending credentials never leave Secrets.
use hantong_contracts::{
    ErrorCode,
    google_setup::{GoogleSetupInput, GoogleSetupReceipt},
};
use hantong_store::Store;
use std::{cell::RefCell, collections::BTreeMap, path::Path, rc::Rc};
pub(crate) type Sessions = RefCell<BTreeMap<[u8; 32], Session>>;
fn new_terminal_captures() -> Rc<crate::terminal_google::Captures> {
    Rc::new(RefCell::new(BTreeMap::new()))
}
#[derive(Clone)]
pub(crate) struct Session {
    input: GoogleSetupInput,
    client_id: String,
    receipt: GoogleSetupReceipt,
    /// Observed by the authenticated pipe, never supplied by setup JSON.
    peer: Option<hantong_contracts::auth::PeerContext>,
    pub(crate) terminal_captures: Rc<crate::terminal_google::Captures>,
    pub(crate) advanced_pending: Rc<RefCell<Option<AdvancedPending>>>,
}
pub(crate) struct AdvancedPending {
    pub pending: hantong_secrets::google::PendingGoogleCredential,
    pub identity: hantong_contracts::google::GoogleIdentity,
    pub binding: hantong_contracts::google::GoogleAccountBinding,
    pub expected_version: u64,
    pub display_name: Option<String>,
}

/// Move-only proof produced only by a successful live Google OAuth exchange.
/// It is deliberately private to Core and can be consumed exactly once.
struct GoogleBootstrapAuthority {
    request_id: [u8; 32],
    peer: hantong_contracts::auth::PeerContext,
    account_id: String,
    subject: String,
    client_id: String,
    credential_version: u64,
    access_mode: hantong_contracts::google_write::GoogleAccessMode,
    local_profile: hantong_contracts::google_setup::GoogleLocalProfile,
    expires_at_utc_ms: i64,
    consumed: bool,
}
pub(crate) struct GoogleBootstrapPermit {
    request_id: [u8; 32],
    account_id: String,
    subject: String,
    client_id: String,
    credential_version: u64,
    access_mode: hantong_contracts::google_write::GoogleAccessMode,
    local_profile: hantong_contracts::google_setup::GoogleLocalProfile,
    peer: hantong_contracts::auth::PeerContext,
    expires_at_utc_ms: i64,
}
impl GoogleBootstrapPermit {
    pub(crate) fn into_registration_parts(
        self,
        store: &Store,
    ) -> Result<(String, hantong_contracts::google_setup::GoogleLocalProfile), ErrorCode> {
        if self.request_id == [0; 32] || now()? >= self.expires_at_utc_ms {
            return Err(ErrorCode::ApprovalExpired);
        }
        let binding = store.google_account_binding(&self.account_id)?;
        if binding.account_id != self.account_id
            || binding.subject != self.subject
            || binding.client_id != self.client_id
            || binding.credential_version != self.credential_version
            || binding.access_mode != self.access_mode
        {
            return Err(ErrorCode::Conflict);
        }
        if hantong_platform::current_peer_context()? != self.peer {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok((self.account_id, self.local_profile))
    }
}
impl GoogleBootstrapAuthority {
    fn new(
        session: &Session,
        identity: &hantong_contracts::google::GoogleIdentity,
        credential_version: u64,
    ) -> Result<Self, ErrorCode> {
        session.input.validate()?;
        identity.validate()?;
        if session
            .input
            .reauthorize_account
            .as_ref()
            .is_some_and(|account| account != &identity.account_id)
        {
            return Err(ErrorCode::ScopeDenied);
        }
        if session.receipt.request_id == [0; 32] || now()? >= session.receipt.expires_at_utc_ms {
            return Err(ErrorCode::ApprovalExpired);
        }
        let peer = session.peer.clone().ok_or(ErrorCode::ScopeDenied)?;
        if session.input.local_profile == hantong_contracts::google_setup::GoogleLocalProfile::None
            || session.input.local_profile.access_mode() != Some(identity.access_mode)
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            request_id: session.receipt.request_id,
            peer,
            account_id: identity.account_id.clone(),
            subject: identity.subject.clone(),
            client_id: session.client_id.clone(),
            credential_version,
            access_mode: identity.access_mode,
            local_profile: session.input.local_profile,
            expires_at_utc_ms: session.receipt.expires_at_utc_ms,
            consumed: false,
        })
    }
    fn consume(
        &mut self,
        request_id: [u8; 32],
        peer: &hantong_contracts::auth::PeerContext,
        identity: &hantong_contracts::google::GoogleIdentity,
        binding: &hantong_contracts::google::GoogleAccountBinding,
        client_id: &str,
        profile: hantong_contracts::google_setup::GoogleLocalProfile,
    ) -> Result<GoogleBootstrapPermit, ErrorCode> {
        if self.consumed
            || request_id != self.request_id
            || &self.peer != peer
            || self.account_id != identity.account_id
            || self.subject != identity.subject
            || self.client_id != client_id
            || binding.account_id != self.account_id
            || binding.subject != self.subject
            || binding.client_id != self.client_id
            || binding.credential_version != self.credential_version
            || binding.access_mode != self.access_mode
            || identity.access_mode != self.access_mode
            || profile != self.local_profile
            || now()? >= self.expires_at_utc_ms
        {
            return Err(ErrorCode::Conflict);
        }
        self.consumed = true;
        Ok(GoogleBootstrapPermit {
            request_id,
            account_id: self.account_id.clone(),
            subject: self.subject.clone(),
            client_id: self.client_id.clone(),
            credential_version: self.credential_version,
            access_mode: self.access_mode,
            local_profile: self.local_profile,
            peer: self.peer.clone(),
            expires_at_utc_ms: self.expires_at_utc_ms,
        })
    }
}
fn select_new_connection_client_id(
    explicit: Option<&str>,
    publisher: Option<&str>,
    legacy: Option<&str>,
) -> Option<String> {
    explicit.or(publisher).or(legacy).map(str::to_owned)
}
fn select_reauthorization_client_id(
    requested: Option<&str>,
    bound: &str,
) -> Result<String, ErrorCode> {
    if requested.is_some_and(|id| id != bound) {
        return Err(ErrorCode::Conflict);
    }
    Ok(bound.to_owned())
}
fn now() -> Result<i64, ErrorCode> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ErrorCode::InternalError)?
            .as_millis(),
    )
    .map_err(|_| ErrorCode::InternalError)
}
pub(crate) fn request(
    store: &Store,
    sessions: &Sessions,
    input: GoogleSetupInput,
) -> Result<GoogleSetupReceipt, ErrorCode> {
    request_with_peer(store, sessions, input, None)
}

pub(crate) fn request_with_peer(
    store: &Store,
    sessions: &Sessions,
    input: GoogleSetupInput,
    peer: Option<hantong_contracts::auth::PeerContext>,
) -> Result<GoogleSetupReceipt, ErrorCode> {
    input.validate()?;
    let client_id = if let Some(account) = &input.reauthorize_account {
        let binding = store.google_account_binding(account)?;
        select_reauthorization_client_id(input.client_id.as_deref(), &binding.client_id)?
    } else {
        let publisher = if input.client_id.is_none() {
            hantong_secrets::google::load_installed_publisher_configuration()?
                .map(|configuration| configuration.client_id)
        } else {
            None
        };
        select_new_connection_client_id(
            input.client_id.as_deref(),
            publisher.as_deref(),
            store.google_client_id()?.as_deref(),
        )
        .ok_or(ErrorCode::HumanStepRequired)?
    };
    hantong_contracts::google::validate_google_client_id(&client_id)?;
    let time = now()?;
    let mut sessions = sessions.borrow_mut();
    sessions.retain(|_, s| s.receipt.expires_at_utc_ms > time);
    if sessions.len() >= 64 {
        return Err(ErrorCode::QuotaExceeded);
    }
    let id = hantong_platform::random_nonce()?;
    let receipt = GoogleSetupReceipt {
        request_id: id,
        expires_at_utc_ms: time.checked_add(300000).ok_or(ErrorCode::InternalError)?,
        state: "queued".into(),
        account_id: None,
        login_url: None,
        error_code: None,
        client_ticket: None,
        local_profile: input.local_profile,
    };
    sessions.insert(
        id,
        Session {
            input,
            client_id,
            receipt: receipt.clone(),
            peer,
            terminal_captures: new_terminal_captures(),
            advanced_pending: Rc::new(RefCell::new(None)),
        },
    );
    Ok(receipt)
}

pub(crate) fn peer_matches(
    sessions: &Sessions,
    id: [u8; 32],
    peer: &hantong_contracts::auth::PeerContext,
) -> Result<(), ErrorCode> {
    let sessions_ref = sessions.borrow();
    let session = sessions_ref.get(&id).ok_or(ErrorCode::NotFound)?;
    if session.peer.as_ref().is_some_and(|bound| bound != peer) {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}
pub(crate) fn matches(sessions: &Sessions, id: [u8; 32], input: &GoogleSetupInput) -> bool {
    sessions
        .borrow()
        .get(&id)
        .is_some_and(|s| s.input == *input)
}
pub(crate) fn status(sessions: &Sessions, id: [u8; 32]) -> Result<GoogleSetupReceipt, ErrorCode> {
    let mut sessions = sessions.borrow_mut();
    let s = sessions.get_mut(&id).ok_or(ErrorCode::NotFound)?;
    if now()? >= s.receipt.expires_at_utc_ms
        && matches!(
            s.receipt.state.as_str(),
            "queued" | "awaiting_client" | "awaiting_browser" | "awaiting_review"
        )
    {
        s.receipt.state = "expired".into();
        s.receipt.login_url = None;
        s.receipt.error_code = Some(ErrorCode::ApprovalExpired)
    }
    Ok(s.receipt.clone())
}

/// Issues the opaque Secrets ticket used by the foreground terminal helper.
/// The ticket is bound to the authenticated pipe peer and Core server key.
pub(crate) fn terminal_client_ticket(
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
    server: &hantong_platform::ServerSigningKey,
    peer: &hantong_contracts::auth::PeerContext,
) -> Result<[u8; 320], ErrorCode> {
    let session = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    if session.peer.as_ref() != Some(peer) {
        return Err(ErrorCode::ScopeDenied);
    }
    if session.receipt.state != "awaiting_client"
        || session.receipt.error_code.is_some()
        || now()? >= session.receipt.expires_at_utc_ms
    {
        return Err(ErrorCode::ApprovalExpired);
    }
    crate::terminal_google::issue(
        &session.terminal_captures,
        root,
        id,
        &session.client_id,
        peer,
        server,
        session.receipt.expires_at_utc_ms,
    )
}

/// Stages the masked foreground input. The value remains in Secrets.
pub(crate) fn stage_terminal_client(
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
    opaque: &[u8],
    staged_handle: [u8; 32],
    peer: &hantong_contracts::auth::PeerContext,
) -> Result<[u8; 32], ErrorCode> {
    let session = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    if session.peer.as_ref() != Some(peer) {
        return Err(ErrorCode::ScopeDenied);
    }
    if session.receipt.state != "awaiting_client"
        || session.receipt.error_code.is_some()
        || now()? >= session.receipt.expires_at_utc_ms
    {
        return Err(ErrorCode::ApprovalExpired);
    }
    crate::terminal_google::stage(&session.terminal_captures, root, id, opaque, staged_handle)
}

pub(crate) fn advanced_terminal_preview(
    sessions: &Sessions,
    id: [u8; 32],
    peer: &hantong_contracts::auth::PeerContext,
) -> Result<
    (
        hantong_contracts::google::GoogleIdentity,
        hantong_contracts::google::GoogleAccountBinding,
        i64,
    ),
    ErrorCode,
> {
    let session = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    if session.peer.as_ref() != Some(peer) {
        return Err(ErrorCode::ScopeDenied);
    }
    if session.receipt.state != "awaiting_review"
        || session.receipt.error_code.is_some()
        || now()? >= session.receipt.expires_at_utc_ms
    {
        return Err(ErrorCode::ApprovalExpired);
    }
    let pending = session.advanced_pending.borrow();
    let pending = pending.as_ref().ok_or(ErrorCode::ApprovalRequired)?;
    Ok((
        pending.identity.clone(),
        pending.binding.clone(),
        session.receipt.expires_at_utc_ms,
    ))
}

pub(crate) fn complete_advanced_terminal(
    store: &Store,
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
    peer: &hantong_contracts::auth::PeerContext,
) -> Result<(), ErrorCode> {
    let session = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    if session.peer.as_ref() != Some(peer) {
        return Err(ErrorCode::ScopeDenied);
    }
    if session.receipt.state != "awaiting_review"
        || session.receipt.error_code.is_some()
        || now()? >= session.receipt.expires_at_utc_ms
    {
        return Err(ErrorCode::ApprovalExpired);
    }
    let pending_ref = session.advanced_pending.borrow();
    let pending = pending_ref.as_ref().ok_or(ErrorCode::NotFound)?;
    let current = store.google_account_for_subject(&pending.identity.subject)?;
    let matches_base = match current {
        None => pending.expected_version == 0,
        Some(current) => {
            current.account_id == pending.binding.account_id
                && current.client_id == pending.binding.client_id
                && current.credential_version == pending.expected_version
        }
    };
    if pending.binding.credential_version
        != pending
            .expected_version
            .checked_add(1)
            .ok_or(ErrorCode::Conflict)?
        || !matches_base
    {
        return Err(ErrorCode::Conflict);
    }
    drop(pending_ref);
    let pending = session
        .advanced_pending
        .borrow_mut()
        .take()
        .ok_or(ErrorCode::NotFound)?;
    let secrets = hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
    pending.pending.store(&secrets, &pending.binding)?;
    let actual = store.activate_google_account(
        &pending.identity,
        &pending.binding.client_id,
        pending.expected_version,
        pending.display_name.as_deref(),
    )?;
    if actual != pending.binding {
        return Err(ErrorCode::Conflict);
    }
    // Confirmation was accepted before expiry above. Synchronous persistence
    // may cross the deadline; do not report an expired decision after commit.
    let mut sessions = sessions.borrow_mut();
    let session = sessions.get_mut(&id).ok_or(ErrorCode::InternalError)?;
    session.receipt.state = "ready".into();
    session.receipt.login_url = None;
    session.receipt.account_id = Some(actual.account_id);
    session.receipt.error_code = None;
    session.receipt.validate()
}
fn update(
    sessions: &Sessions,
    id: [u8; 32],
    state: &str,
    url: Option<String>,
    account: Option<String>,
    error: Option<ErrorCode>,
) -> Result<(), ErrorCode> {
    let mut sessions = sessions.borrow_mut();
    let s = sessions.get_mut(&id).ok_or(ErrorCode::NotFound)?;
    if now()? >= s.receipt.expires_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    if state != s.receipt.state
        && matches!(
            state,
            "awaiting_client" | "awaiting_browser" | "awaiting_review"
        )
    {
        s.receipt.expires_at_utc_ms = now()?.checked_add(300000).ok_or(ErrorCode::InternalError)?;
    }
    s.receipt.state = state.into();
    s.receipt.login_url = url;
    s.receipt.account_id = account;
    s.receipt.error_code = error;
    s.receipt.validate()
}
pub(crate) async fn run(
    store: &Store,
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
    server: &hantong_platform::ServerSigningKey,
) -> Result<(), ErrorCode> {
    let s = sessions
        .borrow()
        .get(&id)
        .cloned()
        .ok_or(ErrorCode::NotFound)?;
    let result = connect(store, root, sessions, id, &s, server).await;
    if let Err(e) = result {
        let state = match e {
            ErrorCode::ApprovalExpired => "expired",
            ErrorCode::ApprovalDenied => "denied",
            ErrorCode::ApprovalRequired => "awaiting_review",
            ErrorCode::HumanStepRequired => "awaiting_client",
            _ => "failed",
        };
        if let Some(s) = sessions.borrow_mut().get_mut(&id) {
            s.receipt.state = state.into();
            s.receipt.login_url = None;
            s.receipt.error_code = (!matches!(
                e,
                ErrorCode::ApprovalRequired | ErrorCode::HumanStepRequired
            ))
            .then_some(e)
        }
    }
    result
}
fn credential_source(
    store: &Store,
    client_id: &str,
    reauthorize: Option<&str>,
) -> Result<Option<hantong_contracts::google::GoogleAccountBinding>, ErrorCode> {
    if let Some(account) = reauthorize {
        let binding = store.google_account_binding(account)?;
        if binding.client_id != client_id {
            return Err(ErrorCode::Conflict);
        }
        return Ok(Some(binding));
    }
    for account in store.list_registrable_account_ids()? {
        if account.starts_with("google_") {
            let binding = store.google_account_binding(&account)?;
            if binding.client_id == client_id {
                return Ok(Some(binding));
            }
        }
    }
    Ok(None)
}

async fn capture_client(
    sessions: &Sessions,
    id: [u8; 32],
    s: &Session,
    root: &Path,
    server: &hantong_platform::ServerSigningKey,
) -> Result<hantong_secrets::google::GoogleClientSecret, ErrorCode> {
    update(sessions, id, "awaiting_client", None, None, None)?;
    let peer = s.peer.as_ref().ok_or(ErrorCode::ScopeDenied)?;
    crate::terminal_google::consume(
        &s.terminal_captures,
        root,
        id,
        server,
        peer,
        s.receipt.expires_at_utc_ms,
    )
}

async fn connect(
    store: &Store,
    root: &Path,
    sessions: &Sessions,
    id: [u8; 32],
    s: &Session,
    server: &hantong_platform::ServerSigningKey,
) -> Result<(), ErrorCode> {
    let secrets = hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
    // An explicit public client ID means deliberate app setup/repair.
    let reused = if s.input.client_id.is_none() {
        credential_source(store, &s.client_id, s.input.reauthorize_account.as_deref())?
            .map(|binding| secrets.google_client_credential(&binding, &s.client_id))
            .transpose()?
            .flatten()
    } else {
        None
    };
    let publisher = if reused.is_none() && s.input.client_id.is_none() {
        hantong_secrets::google::load_installed_publisher_configuration()?
            .filter(|configuration| configuration.client_id == s.client_id)
    } else {
        None
    };
    let client_secret = match reused {
        Some(secret) => secret,
        None => match publisher {
            Some(configuration) => configuration.client_secret,
            None if s.input.client_id.is_some() => {
                capture_client(sessions, id, s, root, server).await?
            }
            // Normal end users never configure a Google Cloud app. Only an
            // explicit advanced client-id request may open credential capture.
            None => return Err(ErrorCode::HumanStepRequired),
        },
    };
    let auth = hantong_secrets::google::begin_google_authorization_for_mode(
        &s.client_id,
        s.input.force_consent,
        s.input.access_mode,
    )
    .await?
    .with_client_secret(client_secret)?;
    let url = auth.authorization_url().to_owned();
    update(
        sessions,
        id,
        "awaiting_browser",
        Some(url.clone()),
        None,
        None,
    )?;
    hantong_platform::google_browser::open_google_authorization_url(&url)?;
    let pending = auth.complete().await?;
    let identity = pending.identity().clone();
    identity.validate()?;
    if s.input
        .reauthorize_account
        .as_ref()
        .is_some_and(|a| a != &identity.account_id)
    {
        return Err(ErrorCode::ScopeDenied);
    }
    let expected = store
        .google_account_for_subject(&identity.subject)?
        .map_or(0, |b| b.credential_version);
    let binding = pending.binding(expected.checked_add(1).ok_or(ErrorCode::Conflict)?)?;
    let mut authority =
        if s.input.local_profile != hantong_contracts::google_setup::GoogleLocalProfile::None {
            Some(GoogleBootstrapAuthority::new(
                s,
                &identity,
                binding.credential_version,
            )?)
        } else {
            None
        };
    let personal =
        s.input.local_profile != hantong_contracts::google_setup::GoogleLocalProfile::None;
    update(
        sessions,
        id,
        if personal {
            "queued"
        } else {
            "awaiting_review"
        },
        None,
        Some(binding.account_id.clone()),
        None,
    )?;
    let pending = if personal {
        // Google consent is the account connection approval for the ordinary
        // personal CLI. Native HanTong/Hello review remains for advanced
        // setup, while this branch accepts only the live PKCE result above.
        if s.input.local_profile.access_mode() != Some(identity.access_mode) {
            return Err(ErrorCode::InvalidInput);
        }
        pending
    } else {
        *s.advanced_pending.borrow_mut() = Some(AdvancedPending {
            pending,
            identity: identity.clone(),
            binding: binding.clone(),
            expected_version: expected,
            display_name: s.input.display_name.clone(),
        });
        update(
            sessions,
            id,
            "awaiting_review",
            None,
            Some(binding.account_id),
            None,
        )?;
        return Err(ErrorCode::ApprovalRequired);
    };
    if store
        .google_account_for_subject(&identity.subject)?
        .map_or(0, |b| b.credential_version)
        != expected
    {
        return Err(ErrorCode::Conflict);
    }
    let secrets = hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
    pending.store(&secrets, &binding)?;
    let actual = store.activate_google_account(
        &identity,
        &s.client_id,
        expected,
        s.input.display_name.as_deref(),
    )?;
    if actual != binding {
        return Err(ErrorCode::InternalError);
    }
    if s.input.local_profile != hantong_contracts::google_setup::GoogleLocalProfile::None {
        let peer = s.peer.as_ref().ok_or(ErrorCode::ScopeDenied)?;
        let permit = authority.as_mut().ok_or(ErrorCode::Conflict)?.consume(
            id,
            peer,
            &identity,
            &binding,
            &s.client_id,
            s.input.local_profile,
        )?;
        crate::registration::complete_google_oauth_registration(store, server, permit)?;
    }
    update(sessions, id, "ready", None, Some(binding.account_id), None)
}
#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::google_setup::GoogleLocalProfile;

    #[test]
    fn acc02_terminal_google_expiry_and_peer_are_checked_before_pending_credentials() {
        let (session, _, _, peer) = authority_fixture(1);
        let sessions = RefCell::new(BTreeMap::from([([2; 32], session)]));
        assert_eq!(
            advanced_terminal_preview(&sessions, [2; 32], &peer),
            Err(ErrorCode::ApprovalExpired)
        );
        let other =
            hantong_contracts::auth::PeerContext::new("S-1-5-21-100-200-300-401", "S-1-5-5-0-1")
                .unwrap();
        assert_eq!(
            advanced_terminal_preview(&sessions, [2; 32], &other),
            Err(ErrorCode::ScopeDenied)
        );
    }

    fn authority_fixture(
        expiry: i64,
    ) -> (
        Session,
        hantong_contracts::google::GoogleIdentity,
        hantong_contracts::google::GoogleAccountBinding,
        hantong_contracts::auth::PeerContext,
    ) {
        let peer =
            hantong_contracts::auth::PeerContext::new("S-1-5-21-100-200-300-400", "S-1-5-5-0-1")
                .unwrap();
        let account_id = format!("google_{}", "a".repeat(64));
        let identity = hantong_contracts::google::GoogleIdentity {
            access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
            account_id: account_id.clone(),
            subject: "subject-fixture".into(),
            email: "fixture@example.test".into(),
            granted_scopes: hantong_contracts::google::GOOGLE_SCOPES
                .iter()
                .map(|s| (*s).into())
                .collect(),
        };
        let input = GoogleSetupInput {
            access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
            local_profile: hantong_contracts::google_setup::GoogleLocalProfile::PersonalRead,
            client_id: Some("fixture.apps.googleusercontent.com".into()),
            display_name: None,
            reauthorize_account: None,
            force_consent: false,
        };
        let session = Session {
            input,
            client_id: "fixture.apps.googleusercontent.com".into(),
            receipt: GoogleSetupReceipt {
                request_id: [2; 32],
                expires_at_utc_ms: expiry,
                state: "awaiting_review".into(),
                account_id: Some(account_id.clone()),
                login_url: None,
                error_code: None,
                client_ticket: None,
                local_profile: GoogleLocalProfile::PersonalRead,
            },
            peer: Some(peer.clone()),
            terminal_captures: new_terminal_captures(),
            advanced_pending: Rc::new(RefCell::new(None)),
        };
        let binding = hantong_contracts::google::GoogleAccountBinding {
            access_mode: identity.access_mode,
            account_id,
            subject: identity.subject.clone(),
            client_id: session.client_id.clone(),
            credential_version: 1,
        };
        (session, identity, binding, peer)
    }

    #[test]
    fn google_personal_authority_rejects_missing_peer_before_oauth_publication() {
        let (mut session, identity, _, _) = authority_fixture(now().unwrap() + 100_000);
        session.peer = None;
        assert!(matches!(
            GoogleBootstrapAuthority::new(&session, &identity, 1),
            Err(ErrorCode::ScopeDenied)
        ));
    }

    #[test]
    fn acc02_personal_send_reauthorization_keeps_exact_google_identity_binding() {
        use hantong_contracts::{google_setup::GoogleLocalProfile, google_write::GoogleAccessMode};
        let (mut session, mut identity, _, _) = authority_fixture(now().unwrap() + 100_000);
        session.input.access_mode = GoogleAccessMode::ReadSend;
        session.input.local_profile = GoogleLocalProfile::PersonalReadSend;
        session.input.reauthorize_account = Some(identity.account_id.clone());
        identity.access_mode = GoogleAccessMode::ReadSend;
        identity.granted_scopes = GoogleAccessMode::ReadSend
            .required_scopes()
            .iter()
            .map(|s| (*s).into())
            .collect();
        assert!(GoogleBootstrapAuthority::new(&session, &identity, 1).is_ok());
        session.input.reauthorize_account = Some(format!("google_{}", "b".repeat(64)));
        assert!(matches!(
            GoogleBootstrapAuthority::new(&session, &identity, 1),
            Err(ErrorCode::ScopeDenied)
        ));
    }

    #[test]
    fn google_personal_authority_rejects_expiry_peer_and_identity_mismatch() {
        let (session, identity, _binding, _peer) = authority_fixture(1);
        assert!(matches!(
            GoogleBootstrapAuthority::new(&session, &identity, 1),
            Err(ErrorCode::ApprovalExpired)
        ));

        let (session, identity, binding, _peer) = authority_fixture(now().unwrap() + 100_000);
        let mut authority = GoogleBootstrapAuthority::new(&session, &identity, 1).unwrap();
        let other_peer =
            hantong_contracts::auth::PeerContext::new("S-1-5-21-100-200-300-401", "S-1-5-5-0-1")
                .unwrap();
        assert!(matches!(
            authority.consume(
                session.receipt.request_id,
                &other_peer,
                &identity,
                &binding,
                &session.client_id,
                session.input.local_profile,
            ),
            Err(ErrorCode::Conflict)
        ));
    }

    #[test]
    fn google_personal_authority_is_single_use_and_binds_exact_identity() {
        let (session, identity, binding, peer) = authority_fixture(now().unwrap() + 100_000);
        let mut authority = GoogleBootstrapAuthority::new(&session, &identity, 1).unwrap();
        authority
            .consume(
                session.receipt.request_id,
                &peer,
                &identity,
                &binding,
                &session.client_id,
                session.input.local_profile,
            )
            .unwrap();
        assert!(matches!(
            authority.consume(
                session.receipt.request_id,
                &peer,
                &identity,
                &binding,
                &session.client_id,
                session.input.local_profile,
            ),
            Err(ErrorCode::Conflict)
        ));
        let mut altered = identity.clone();
        altered.subject = "different-subject".into();
        assert!(GoogleBootstrapAuthority::new(&session, &altered, 1).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn acc02_second_google_account_preserves_grant_until_terminal_scope_selection() {
        additional_google_account_case(
            hantong_contracts::google_setup::GoogleLocalProfile::PersonalRead,
        );
    }

    #[cfg(windows)]
    #[test]
    fn acc02_second_send_account_preserves_sending_until_terminal_scope_selection() {
        additional_google_account_case(
            hantong_contracts::google_setup::GoogleLocalProfile::PersonalReadSend,
        );
    }

    #[cfg(windows)]
    fn additional_google_account_case(
        profile: hantong_contracts::google_setup::GoogleLocalProfile,
    ) {
        use hantong_contracts::{
            auth::*, google_setup::GoogleLocalProfile, registration::GoogleReadAccount,
        };
        use hantong_store::{CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL};
        let mode = profile.access_mode().unwrap();
        let tools = match profile {
            GoogleLocalProfile::PersonalRead => default_cli_body_tools(),
            GoogleLocalProfile::PersonalReadSend => default_cli_send_tools(),
            GoogleLocalProfile::None => unreachable!(),
        };
        struct OwnedKey(Option<hantong_platform::ServerSigningKey>);
        impl Drop for OwnedKey {
            fn drop(&mut self) {
                self.0.take().unwrap().delete().unwrap();
            }
        }
        let root = std::env::temp_dir().join(format!(
            "hantong-google-add-{}",
            hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
        ));
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let selector = hantong_platform::random_nonce().unwrap();
        let key = OwnedKey(Some(
            hantong_platform::ServerSigningKey::create(selector).unwrap(),
        ));
        let server = key.0.as_ref().unwrap();
        let public = server.public_key().unwrap();
        let pin = hantong_platform::sha256(&public).unwrap();
        store.stage_server_identity(selector).unwrap();
        store.activate_server_identity(selector, public, 1).unwrap();
        let mut bindings = Vec::new();
        for subject in ["additional-fixture-first", "additional-fixture-second"] {
            let identity = hantong_contracts::google::GoogleIdentity {
                account_id: format!(
                    "google_{}",
                    hantong_contracts::wire_hex::encode(
                        &hantong_platform::sha256(subject.as_bytes()).unwrap()
                    )
                ),
                subject: subject.into(),
                email: "synthetic@example.test".into(),
                access_mode: mode,
                granted_scopes: mode.required_scopes().iter().map(|s| (*s).into()).collect(),
            };
            bindings.push(
                store
                    .activate_google_account(
                        &identity,
                        "fixture.apps.googleusercontent.com",
                        0,
                        None,
                    )
                    .unwrap(),
            );
        }
        let first = &bindings[0];
        let second = &bindings[1];
        let request = store
            .create_registration_request(
                [31; 32],
                ClientKind::Cli,
                DEFAULT_CLI_LABEL,
                &RequestedScope::new(vec![first.account_id.clone()], default_cli_legacy_tools())
                    .unwrap(),
            )
            .unwrap();
        let id = hantong_platform::registration_id_for_request([31; 32]).unwrap();
        store
            .stage_google_oauth_cli_registration(
                &CliRegistrationStage::new_google_oauth(request, id, public, pin).unwrap(),
            )
            .unwrap();
        store.activate_cli_registration(id, 1, pin).unwrap();
        let initial = store
            .create_google_oauth_cli_scope_refresh(
                [32; 32],
                RequestedScope::new(vec![first.account_id.clone()], tools.clone()).unwrap(),
                pin,
            )
            .unwrap();
        let staged = store.stage_cli_scope_refresh(&initial).unwrap();
        store.activate_cli_scope_refresh(&staged).unwrap();
        let before = store.load_default_cli_registration().unwrap().unwrap();
        let permit = || GoogleBootstrapPermit {
            request_id: [33; 32],
            account_id: second.account_id.clone(),
            subject: second.subject.clone(),
            client_id: second.client_id.clone(),
            credential_version: second.credential_version,
            access_mode: second.access_mode,
            local_profile: profile,
            peer: hantong_platform::current_peer_context().unwrap(),
            expires_at_utc_ms: now().unwrap() + 100_000,
        };
        crate::registration::complete_google_oauth_registration(&store, server, permit()).unwrap();
        assert_eq!(
            store.load_default_cli_registration().unwrap().unwrap(),
            before
        );
        assert!(store.load_pending_cli_scope_refresh().unwrap().is_none());
        assert_eq!(
            store.google_account_binding(&first.account_id).unwrap(),
            *first
        );
        assert_eq!(
            store.google_account_binding(&second.account_id).unwrap(),
            *second
        );
        let selection = match profile {
            GoogleLocalProfile::PersonalRead => crate::scope_refresh::prepare_google_readonly_add(
                &store,
                server,
                GoogleReadAccount::parse(&second.account_id).unwrap(),
            ),
            GoogleLocalProfile::PersonalReadSend => crate::scope_refresh::prepare_google_send(
                &store,
                server,
                hantong_contracts::scope_refresh::GoogleSendAccounts::parse(&[
                    first.account_id.clone(),
                    second.account_id.clone(),
                ])
                .unwrap(),
            ),
            GoogleLocalProfile::None => unreachable!(),
        }
        .unwrap();
        assert_eq!(selection.decision_channel(), "terminal_cli");
        assert_eq!(selection.target_accounts().len(), 2);
        assert_eq!(selection.target_tools(), tools);
        assert_eq!(
            store.load_default_cli_registration().unwrap().unwrap(),
            before
        );
        // Store transition fixture represents the existing terminal selection;
        // the OAuth completion above did not stage or activate this grant.
        let staged = store.stage_cli_scope_refresh(&selection).unwrap();
        let after = store.activate_cli_scope_refresh(&staged).unwrap();
        assert_eq!(after.registration_id, before.registration_id);
        assert_eq!(after.public_key, before.public_key);
        assert_eq!(after.scope.accounts(), selection.target_accounts());
        crate::registration::complete_google_oauth_registration(&store, server, permit()).unwrap();
        assert_eq!(
            store.load_default_cli_registration().unwrap().unwrap(),
            after
        );
        assert!(store.load_pending_cli_scope_refresh().unwrap().is_none());
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn google_personal_permit_revalidates_current_store_binding_and_peer() {
        let root = std::env::temp_dir().join(format!(
            "hantong-google-permit-{}",
            hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
        ));
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let account_id = format!(
            "google_{}",
            hantong_contracts::wire_hex::encode(
                &hantong_platform::sha256(b"permit-subject").unwrap()
            )
        );
        let identity = hantong_contracts::google::GoogleIdentity {
            access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
            account_id: account_id.clone(),
            subject: "permit-subject".into(),
            email: "permit@example.test".into(),
            granted_scopes: hantong_contracts::google::GOOGLE_SCOPES
                .iter()
                .map(|s| (*s).into())
                .collect(),
        };
        let binding = store
            .activate_google_account(&identity, "fixture.apps.googleusercontent.com", 0, None)
            .unwrap();
        let peer = hantong_platform::current_peer_context().unwrap();
        let permit = GoogleBootstrapPermit {
            request_id: [9; 32],
            account_id,
            subject: binding.subject.clone(),
            client_id: binding.client_id.clone(),
            credential_version: binding.credential_version,
            access_mode: binding.access_mode,
            local_profile: hantong_contracts::google_setup::GoogleLocalProfile::PersonalRead,
            peer,
            expires_at_utc_ms: now().unwrap() + 100_000,
        };
        assert_eq!(
            permit.into_registration_parts(&store).unwrap().1,
            hantong_contracts::google_setup::GoogleLocalProfile::PersonalRead
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn acc02_new_connection_prefers_publisher_client_over_legacy_store_client() {
        assert_eq!(
            select_new_connection_client_id(
                None,
                Some("publisher.apps.googleusercontent.com"),
                Some("legacy.apps.googleusercontent.com"),
            ),
            Some("publisher.apps.googleusercontent.com".into())
        );
    }

    #[test]
    fn acc02_new_connection_falls_back_to_legacy_without_publisher_client() {
        assert_eq!(
            select_new_connection_client_id(None, None, Some("legacy.apps.googleusercontent.com"),),
            Some("legacy.apps.googleusercontent.com".into())
        );
        assert_eq!(select_new_connection_client_id(None, None, None), None);
    }

    #[test]
    fn acc02_new_connection_explicit_client_wins_over_publisher_and_legacy() {
        assert_eq!(
            select_new_connection_client_id(
                Some("explicit.apps.googleusercontent.com"),
                Some("publisher.apps.googleusercontent.com"),
                Some("legacy.apps.googleusercontent.com"),
            ),
            Some("explicit.apps.googleusercontent.com".into())
        );
    }

    #[test]
    fn acc02_reauthorization_remains_bound_to_existing_client() {
        assert_eq!(
            select_reauthorization_client_id(None, "bound.apps.googleusercontent.com",).unwrap(),
            "bound.apps.googleusercontent.com"
        );
        assert_eq!(
            select_reauthorization_client_id(
                Some("different.apps.googleusercontent.com"),
                "bound.apps.googleusercontent.com",
            ),
            Err(ErrorCode::Conflict)
        );
    }

    #[test]
    fn acc02_google_setup_status_is_bounded_terminal_and_does_not_relaunch_browser() {
        let input = GoogleSetupInput {
            access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
            local_profile: hantong_contracts::google_setup::GoogleLocalProfile::None,
            client_id: Some("fixture.apps.googleusercontent.com".into()),
            display_name: None,
            reauthorize_account: None,
            force_consent: false,
        };
        let receipt = GoogleSetupReceipt {
            request_id: [1; 32],
            expires_at_utc_ms: 1,
            state: "awaiting_browser".into(),
            account_id: None,
            login_url: Some("https://accounts.google.com/o/oauth2/v2/auth?state=synthetic".into()),
            error_code: None,
            client_ticket: None,
            local_profile: input.local_profile,
        };
        let sessions = RefCell::new(BTreeMap::from([(
            [1; 32],
            Session {
                input: input.clone(),
                client_id: input.client_id.clone().unwrap(),
                receipt,
                peer: None,
                terminal_captures: new_terminal_captures(),
                advanced_pending: Rc::new(RefCell::new(None)),
            },
        )]));
        let expired = status(&sessions, [1; 32]).unwrap();
        assert_eq!(expired.state, "expired");
        assert_eq!(expired.error_code, Some(ErrorCode::ApprovalExpired));
        assert!(expired.login_url.is_none());
        expired.validate().unwrap();
        assert_eq!(status(&sessions, [1; 32]).unwrap(), expired);
        assert_eq!(status(&sessions, [2; 32]), Err(ErrorCode::NotFound));
        assert!(matches(&sessions, [1; 32], &input));
    }
}

#[cfg(test)]
mod deadline_tests {
    use super::*;
    use hantong_contracts::google_setup::GoogleLocalProfile;
    #[test]
    fn acc02_app_credential_source_never_crosses_client_or_reauthorization_account() {
        let root = std::env::temp_dir().join(format!(
            "hantong-onboarding-source-{}",
            hantong_contracts::wire_hex::encode(&hantong_platform::random_nonce().unwrap())
        ));
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let mut bindings = vec![];
        for (subject, client) in [
            ("synthetic-first", "first.apps.googleusercontent.com"),
            ("synthetic-second", "second.apps.googleusercontent.com"),
        ] {
            let identity = hantong_contracts::google::GoogleIdentity {
                access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
                account_id: format!(
                    "google_{}",
                    hantong_contracts::wire_hex::encode(
                        &hantong_platform::sha256(subject.as_bytes()).unwrap()
                    )
                ),
                subject: subject.into(),
                email: "synthetic@example.test".into(),
                granted_scopes: hantong_contracts::google::GOOGLE_SCOPES
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            };
            bindings.push(
                store
                    .activate_google_account(&identity, client, 0, None)
                    .unwrap(),
            );
        }
        for binding in &bindings {
            assert_eq!(
                credential_source(&store, &binding.client_id, None).unwrap(),
                Some(binding.clone())
            );
            assert_eq!(
                credential_source(&store, &binding.client_id, Some(&binding.account_id)).unwrap(),
                Some(binding.clone())
            );
        }
        assert!(
            credential_source(&store, "absent.apps.googleusercontent.com", None)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            credential_source(
                &store,
                &bindings[1].client_id,
                Some(&bindings[0].account_id)
            ),
            Err(ErrorCode::Conflict)
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn acc02_stage_deadlines_refresh_only_on_transition_never_poll_or_expiry() {
        let input = GoogleSetupInput {
            access_mode: hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
            local_profile: hantong_contracts::google_setup::GoogleLocalProfile::None,
            client_id: None,
            display_name: None,
            reauthorize_account: None,
            force_consent: false,
        };
        let sessions = RefCell::new(BTreeMap::from([(
            [1; 32],
            Session {
                input,
                client_id: "fixture.apps.googleusercontent.com".into(),
                receipt: GoogleSetupReceipt {
                    request_id: [1; 32],
                    expires_at_utc_ms: now().unwrap() + 1000,
                    state: "awaiting_client".into(),
                    account_id: None,
                    login_url: None,
                    error_code: None,
                    client_ticket: None,
                    local_profile: GoogleLocalProfile::None,
                },
                peer: None,
                terminal_captures: new_terminal_captures(),
                advanced_pending: Rc::new(RefCell::new(None)),
            },
        )]));
        let first = status(&sessions, [1; 32]).unwrap();
        assert_eq!(
            status(&sessions, [1; 32]).unwrap().expires_at_utc_ms,
            first.expires_at_utc_ms
        );
        update(&sessions, [1; 32], "awaiting_browser", None, None, None).unwrap();
        let browser = status(&sessions, [1; 32]).unwrap();
        assert!(browser.expires_at_utc_ms > first.expires_at_utc_ms + 290000);
        update(&sessions, [1; 32], "awaiting_browser", None, None, None).unwrap();
        assert_eq!(
            status(&sessions, [1; 32]).unwrap().expires_at_utc_ms,
            browser.expires_at_utc_ms
        );
        sessions
            .borrow_mut()
            .get_mut(&[1; 32])
            .unwrap()
            .receipt
            .expires_at_utc_ms = 1;
        assert_eq!(
            update(&sessions, [1; 32], "awaiting_review", None, None, None),
            Err(ErrorCode::ApprovalExpired)
        );
        assert_eq!(status(&sessions, [1; 32]).unwrap().state, "expired");
    }
}
