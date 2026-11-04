//! Unix Google OAuth coordinator.
//!
//! This is the Unix foreground setup boundary.  OAuth state and the pending
//! credential remain in Core memory/Secrets; the client receives only the
//! bounded browser URL and a redacted receipt.

use hantong_contracts::{
    ErrorCode,
    google_setup::{GoogleLocalProfile, GoogleSetupInput, GoogleSetupReceipt},
    platform::OpaqueSigner,
    unix_auth::UnixPeerContextV2,
};
use hantong_ipc_client::profile::ParsedClientProfile;
use hantong_store::{ClientKind, DEFAULT_CLI_LABEL, Store};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    path::Path,
    rc::Rc,
};

const MAX_SESSIONS: usize = 64;
const MAX_QUEUE: usize = 16;
const SESSION_MS: i64 = 300_000;

struct Session {
    input: GoogleSetupInput,
    client_id: String,
    expires_at_utc_ms: i64,
    peer: UnixPeerContextV2,
    auth: Option<hantong_secrets::google::GoogleAuthorization>,
    side_effect_started: bool,
    finalization: Option<Finalization>,
    receipt: GoogleSetupReceipt,
}

struct Finalization {
    account_id: String,
    input: GoogleSetupInput,
    attempts: u8,
    queued: bool,
}

pub(crate) struct State {
    sessions: Rc<RefCell<BTreeMap<[u8; 32], Session>>>,
    queue: Rc<RefCell<VecDeque<[u8; 32]>>>,
    notify: Rc<tokio::sync::Notify>,
}

pub(crate) struct Worker {
    sessions: Rc<RefCell<BTreeMap<[u8; 32], Session>>>,
    queue: Rc<RefCell<VecDeque<[u8; 32]>>>,
    notify: Rc<tokio::sync::Notify>,
}

impl State {
    pub(crate) fn new() -> (Self, Worker) {
        let sessions = Rc::new(RefCell::new(BTreeMap::new()));
        let queue = Rc::new(RefCell::new(VecDeque::new()));
        let notify = Rc::new(tokio::sync::Notify::new());
        (
            Self {
                sessions: sessions.clone(),
                queue: queue.clone(),
                notify: notify.clone(),
            },
            Worker {
                sessions,
                queue,
                notify,
            },
        )
    }

    pub(crate) async fn request(
        &self,
        store: &Store,
        root: &Path,
        peer: UnixPeerContextV2,
        input: GoogleSetupInput,
    ) -> Result<GoogleSetupReceipt, ErrorCode> {
        input.validate()?;
        if !hantong_platform::unix_peer::peer_matches_current_image(&peer)? {
            return Err(ErrorCode::ScopeDenied);
        }
        if input.local_profile == GoogleLocalProfile::None {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        if self.sessions.borrow().len() >= MAX_SESSIONS || self.queue.borrow().len() >= MAX_QUEUE {
            self.sessions
                .borrow_mut()
                .retain(|_, session| session.expires_at_utc_ms > now().unwrap_or(i64::MIN));
        }
        if self.sessions.borrow().len() >= MAX_SESSIONS || self.queue.borrow().len() >= MAX_QUEUE {
            return Err(ErrorCode::QuotaExceeded);
        }
        let client_id = if let Some(account) = input.reauthorize_account.as_deref() {
            let binding = store.google_account_binding(account)?;
            binding.client_id
        } else {
            hantong_secrets::google::load_installed_publisher_configuration()?
                .ok_or(ErrorCode::HumanStepRequired)?
                .client_id
        };
        let client_secret = if input.reauthorize_account.is_some() {
            let binding = store.google_account_binding(
                input
                    .reauthorize_account
                    .as_deref()
                    .ok_or(ErrorCode::InvalidInput)?,
            )?;
            let secrets =
                hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
            secrets
                .google_client_credential(&binding, &client_id)?
                .or_else(|| {
                    hantong_secrets::google::load_installed_publisher_configuration()
                        .ok()
                        .flatten()
                        .map(|p| p.client_secret)
                })
                .ok_or(ErrorCode::AuthRequired)?
        } else {
            hantong_secrets::google::load_installed_publisher_configuration()?
                .ok_or(ErrorCode::HumanStepRequired)?
                .client_secret
        };
        let auth = hantong_secrets::google::begin_google_authorization_for_mode(
            &client_id,
            input.force_consent,
            input.access_mode,
        )
        .await?
        .with_client_secret(client_secret)?;
        let request_id = hantong_platform::random_nonce()?;
        let expires_at_utc_ms = now()?.checked_add(SESSION_MS).ok_or(ErrorCode::Conflict)?;
        let receipt = GoogleSetupReceipt {
            request_id,
            expires_at_utc_ms,
            state: "awaiting_browser".into(),
            account_id: input.reauthorize_account.clone(),
            login_url: Some(auth.authorization_url().to_owned()),
            error_code: None,
            client_ticket: None,
            local_profile: input.local_profile,
        };
        receipt.validate()?;
        self.sessions.borrow_mut().insert(
            request_id,
            Session {
                input,
                client_id,
                expires_at_utc_ms,
                peer,
                auth: Some(auth),
                side_effect_started: false,
                finalization: None,
                receipt: receipt.clone(),
            },
        );
        self.queue.borrow_mut().push_back(request_id);
        self.notify.notify_one();
        Ok(receipt)
    }

    pub(crate) fn status(
        &self,
        id: [u8; 32],
        peer: &UnixPeerContextV2,
    ) -> Result<GoogleSetupReceipt, ErrorCode> {
        if !hantong_platform::unix_peer::peer_matches_current_image(peer)? {
            return Err(ErrorCode::ScopeDenied);
        }
        let sessions = self.sessions.borrow();
        let session = sessions.get(&id).ok_or(ErrorCode::NotFound)?;
        // Polling is performed by short-lived verified relay children, so the
        // original PID is deliberately not required. Keep the session bound
        // to the same Unix user and login session, however.
        if !same_login_session(&session.peer, peer) {
            return Err(ErrorCode::ScopeDenied);
        }
        drop(sessions);
        self.enqueue_finalization(id);
        let sessions = self.sessions.borrow();
        let session = sessions.get(&id).ok_or(ErrorCode::NotFound)?;
        Ok(status_receipt(&session.receipt))
    }

    fn enqueue_finalization(&self, id: [u8; 32]) {
        let mut sessions = self.sessions.borrow_mut();
        let Some(session) = sessions.get_mut(&id) else {
            return;
        };
        if let Some(finalization) = session.finalization.as_mut()
            && !finalization.queued
            && finalization.attempts < 3
            && self.queue.borrow().len() < MAX_QUEUE
        {
            finalization.queued = true;
            finalization.attempts += 1;
            self.queue.borrow_mut().push_back(id);
            self.notify.notify_one();
        }
    }
}

impl Worker {
    pub(crate) async fn run(
        &self,
        store: &Store,
        root: &Path,
        server: &mut impl OpaqueSigner,
    ) -> Result<(), ErrorCode> {
        loop {
            let next = { self.queue.borrow_mut().pop_front() };
            let id = match next {
                Some(id) => id,
                None => {
                    self.notify.notified().await;
                    continue;
                }
            };
            if let Some(session) = self.sessions.borrow_mut().get_mut(&id)
                && let Some(finalization) = session.finalization.as_mut()
            {
                finalization.queued = false;
            }
            let result = self.complete(store, root, server, id).await;
            if let Err(error) = result
                && let Some(session) = self.sessions.borrow_mut().get_mut(&id)
            {
                let exhausted = session
                    .finalization
                    .as_ref()
                    .is_some_and(|finalization| finalization.attempts >= 3);
                if let Some(finalization) = session.finalization.as_ref() {
                    session.receipt.account_id = Some(finalization.account_id.clone());
                }
                session.receipt.state = if error == ErrorCode::ApprovalExpired {
                    "expired"
                } else if exhausted {
                    // Stop automatic polling retries after the bounded
                    // finalization budget. The connected account remains
                    // visible so an explicit recovery can be chosen.
                    "interrupted"
                } else if session.finalization.is_some() {
                    "queued"
                } else if session.side_effect_started {
                    // Credentials/account/profile work may already be
                    // durable but there is no resumable finalization yet.
                    "interrupted"
                } else {
                    "failed"
                }
                .into();
                session.receipt.error_code = (session.receipt.state != "queued").then_some(error);
                session.receipt.login_url = None;
            }
        }
    }

    async fn complete(
        &self,
        store: &Store,
        root: &Path,
        server: &mut impl OpaqueSigner,
        id: [u8; 32],
    ) -> Result<(), ErrorCode> {
        if self
            .sessions
            .borrow()
            .get(&id)
            .and_then(|session| session.finalization.as_ref())
            .is_some()
        {
            return self.finalize(store, root, server, id);
        }
        let (auth, input, client_id, expires_at_utc_ms) = {
            let mut sessions = self.sessions.borrow_mut();
            let session = sessions.get_mut(&id).ok_or(ErrorCode::NotFound)?;
            if now()? >= session.expires_at_utc_ms {
                return Err(ErrorCode::ApprovalExpired);
            }
            (
                session.auth.take().ok_or(ErrorCode::Conflict)?,
                session.input.clone(),
                session.client_id.clone(),
                session.expires_at_utc_ms,
            )
        };
        let pending = auth.complete().await?;
        if now()? >= expires_at_utc_ms {
            return Err(ErrorCode::ApprovalExpired);
        }
        let identity = pending.identity().clone();
        identity.validate()?;
        if input
            .reauthorize_account
            .as_deref()
            .is_some_and(|a| a != identity.account_id)
        {
            return Err(ErrorCode::ScopeDenied);
        }
        let expected = store
            .google_account_for_subject(&identity.subject)?
            .map_or(0, |b| b.credential_version);
        let binding = pending.binding(expected.checked_add(1).ok_or(ErrorCode::Conflict)?)?;
        if binding.client_id != client_id || identity.access_mode != input.access_mode {
            return Err(ErrorCode::Conflict);
        }
        let secrets =
            hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
        if let Some(session) = self.sessions.borrow_mut().get_mut(&id) {
            session.side_effect_started = true;
        }
        pending.store(&secrets, &binding)?;
        let actual = store.activate_google_account(
            &identity,
            &client_id,
            expected,
            input.display_name.as_deref(),
        )?;
        if actual != binding {
            return Err(ErrorCode::Conflict);
        }
        if let Some(session) = self.sessions.borrow_mut().get_mut(&id) {
            session.finalization = Some(Finalization {
                account_id: identity.account_id,
                input,
                attempts: 0,
                queued: false,
            });
        }
        self.finalize(store, root, server, id)
    }

    fn finalize(
        &self,
        store: &Store,
        root: &Path,
        server: &mut impl OpaqueSigner,
        id: [u8; 32],
    ) -> Result<(), ErrorCode> {
        let (account_id, input) = {
            let sessions = self.sessions.borrow();
            let session = sessions.get(&id).ok_or(ErrorCode::NotFound)?;
            if now()? >= session.expires_at_utc_ms {
                return Err(ErrorCode::ApprovalExpired);
            }
            let finalization = session.finalization.as_ref().ok_or(ErrorCode::Conflict)?;
            (finalization.account_id.clone(), finalization.input.clone())
        };
        // Recover any registration/scope row left staged by a prior attempt
        // before creating a new row. This path never repeats OAuth/token work.
        crate::unix_registered_profiles::recover_startup(store, root, server)?;
        crate::unix_registration::recover_startup(store, root, server)?;
        if store.load_default_cli_registration()?.is_none() {
            publish_initial_cli(store, server, &account_id, root)?;
        }
        let record = store
            .load_default_cli_registration()?
            .ok_or(ErrorCode::InternalError)?;
        // Connecting another account is deliberately only a provider binding.
        // The existing multi-account union must be selected through the
        // foreground RequestManage workflow.
        if record.scope.accounts() != [account_id.as_str()] {
            if let Some(session) = self.sessions.borrow_mut().get_mut(&id) {
                session.finalization = None;
                session.receipt.state = "ready".into();
                session.receipt.error_code = None;
                session.receipt.account_id = Some(account_id);
                session.receipt.login_url = None;
            }
            return Ok(());
        }
        if record.approved_channel != "google_oauth" {
            if let Some(session) = self.sessions.borrow_mut().get_mut(&id) {
                session.finalization = None;
                session.receipt.state = "ready".into();
                session.receipt.error_code = None;
                session.receipt.account_id = Some(account_id);
                session.receipt.login_url = None;
            }
            return Ok(());
        }
        let target_tools = match input.local_profile {
            GoogleLocalProfile::PersonalRead => hantong_contracts::auth::default_cli_body_tools(),
            GoogleLocalProfile::PersonalReadSend => {
                hantong_contracts::auth::default_cli_send_tools()
            }
            GoogleLocalProfile::PersonalManage => {
                hantong_contracts::auth::default_cli_manage_tools()
            }
            GoogleLocalProfile::None => return Err(ErrorCode::InvalidInput),
        };
        let target =
            hantong_contracts::auth::RequestedScope::new(vec![account_id.clone()], target_tools)?;
        if record.scope == target {
            if let Some(session) = self.sessions.borrow_mut().get_mut(&id) {
                session.finalization = None;
                session.receipt.state = "ready".into();
                session.receipt.error_code = None;
                session.receipt.account_id = Some(account_id);
                session.receipt.login_url = None;
            }
            return Ok(());
        }
        let pin =
            hantong_platform::sha256(&server.public_key().map_err(|_| ErrorCode::InternalError)?)?;
        let refresh_id = hantong_platform::random_nonce()?;
        let refresh = match input.local_profile {
            GoogleLocalProfile::PersonalManage => {
                store.create_google_oauth_cli_manage_scope_refresh(refresh_id, target, pin)?
            }
            GoogleLocalProfile::PersonalReadSend if record.profile_epoch >= 2 => {
                store.create_google_oauth_cli_send_upgrade(refresh_id, target, pin)?
            }
            _ => store.create_google_oauth_cli_scope_refresh(refresh_id, target, pin)?,
        };
        crate::unix_scope_refresh::complete_google(store, root, server, &refresh)?;
        if let Some(session) = self.sessions.borrow_mut().get_mut(&id) {
            session.finalization = None;
            session.receipt.state = "ready".into();
            session.receipt.error_code = None;
            session.receipt.account_id = Some(account_id);
            session.receipt.login_url = None;
        }
        Ok(())
    }
}

fn publish_initial_cli(
    store: &Store,
    server: &mut impl OpaqueSigner,
    account: &str,
    root: &Path,
) -> Result<(), ErrorCode> {
    let scope = hantong_contracts::auth::RequestedScope::new(
        vec![account.to_owned()],
        hantong_contracts::auth::default_cli_legacy_tools(),
    )?;
    let request = match store.load_pending_default_cli_request()? {
        Some(request) => {
            if request.client_kind != ClientKind::Cli
                || request.display_label != DEFAULT_CLI_LABEL
                || request.scope != scope
            {
                return Err(ErrorCode::Conflict);
            }
            request
        }
        None => store.create_registration_request(
            hantong_platform::random_nonce()?,
            ClientKind::Cli,
            DEFAULT_CLI_LABEL,
            &scope,
        )?,
    };
    let registration_id = hantong_platform::registration_id_for_request(request.request_id)?;
    let client =
        hantong_secrets::unix_signing::load_client_signing_key(root, registration_id, None)
            .map_err(|_| ErrorCode::InternalError)?;
    let client_public_key = client.public_key().map_err(|_| ErrorCode::InternalError)?;
    let server_hash =
        hantong_platform::sha256(&server.public_key().map_err(|_| ErrorCode::InternalError)?)?;
    let staged = store.stage_google_oauth_cli_registration(
        &hantong_store::CliRegistrationStage::new_google_oauth(
            request,
            registration_id,
            client_public_key,
            server_hash,
        )?,
    )?;
    let profile = ParsedClientProfile::new_with_scope(
        registration_id,
        hantong_platform::client_key_id_for_registration(registration_id)?,
        client_public_key,
        server.public_key().map_err(|_| ErrorCode::InternalError)?,
        staged.profile_epoch,
        scope,
    )?;
    let bytes = profile.to_json()?;
    let mut publication =
        hantong_platform::unix_profile_publication::publish_default_profile(&bytes)?;
    store.activate_cli_registration(staged.registration_id, staged.version, server_hash)?;
    let _ = hantong_platform::unix_profile_publication::cleanup_profile_refresh_backup(
        &mut publication,
    );
    Ok(())
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

fn same_login_session(expected: &UnixPeerContextV2, current: &UnixPeerContextV2) -> bool {
    expected.effective_uid == current.effective_uid && expected.session_id == current.session_id
}

fn status_receipt(receipt: &GoogleSetupReceipt) -> GoogleSetupReceipt {
    let mut receipt = receipt.clone();
    // The browser URL is returned only by the initial Request response;
    // Status must not turn the request ID into a URL disclosure channel.
    receipt.login_url = None;
    receipt
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::google_write::GoogleAccessMode;

    fn fixture_session(peer: UnixPeerContextV2) -> Session {
        Session {
            input: GoogleSetupInput {
                access_mode: GoogleAccessMode::ReadOnly,
                local_profile: GoogleLocalProfile::PersonalRead,
                client_id: None,
                display_name: None,
                reauthorize_account: None,
                force_consent: false,
            },
            client_id: "client-id.apps.googleusercontent.com".into(),
            expires_at_utc_ms: i64::MAX,
            peer,
            auth: None,
            side_effect_started: true,
            finalization: Some(Finalization {
                account_id: format!("google_{}", "a".repeat(64)),
                input: GoogleSetupInput {
                    access_mode: GoogleAccessMode::ReadOnly,
                    local_profile: GoogleLocalProfile::PersonalRead,
                    client_id: None,
                    display_name: None,
                    reauthorize_account: None,
                    force_consent: false,
                },
                attempts: 0,
                queued: false,
            }),
            receipt: GoogleSetupReceipt {
                request_id: [1; 32],
                expires_at_utc_ms: i64::MAX,
                state: "awaiting_browser".into(),
                account_id: Some(format!("google_{}", "a".repeat(64))),
                login_url: Some("https://accounts.google.com/o/oauth2/v2/auth?secret".into()),
                error_code: None,
                client_ticket: None,
                local_profile: GoogleLocalProfile::PersonalRead,
            },
        }
    }

    #[test]
    fn sec03_google_status_is_session_bound_and_redacts_login_url() {
        let peer = hantong_platform::unix_peer::current_peer_context().unwrap();
        let (state, _) = State::new();
        state
            .sessions
            .borrow_mut()
            .insert([1; 32], fixture_session(peer.clone()));

        let receipt = status_receipt(&state.sessions.borrow()[&[1; 32]].receipt);
        assert!(receipt.login_url.is_none());
        receipt.validate().unwrap();
        assert!(same_login_session(
            &state.sessions.borrow()[&[1; 32]].peer,
            &peer
        ));
        state.enqueue_finalization([1; 32]);
        assert_eq!(state.queue.borrow().len(), 1);
        assert!(same_login_session(
            &state.sessions.borrow()[&[1; 32]].peer,
            &peer
        ));
        state.enqueue_finalization([1; 32]);
        assert_eq!(state.queue.borrow().len(), 1);

        let mut other_session = peer;
        other_session.session_id = other_session.session_id.wrapping_add(1).max(1);
        assert!(!same_login_session(
            &state.sessions.borrow()[&[1; 32]].peer,
            &other_session
        ));
    }

    #[test]
    fn sec03_google_status_stops_finalization_after_three_attempts() {
        let peer = hantong_platform::unix_peer::current_peer_context().unwrap();
        let (state, _) = State::new();
        let mut session = fixture_session(peer.clone());
        session
            .finalization
            .as_mut()
            .expect("finalization")
            .attempts = 3;
        state.sessions.borrow_mut().insert([1; 32], session);

        state.enqueue_finalization([1; 32]);
        assert!(state.queue.borrow().is_empty());
    }
}
