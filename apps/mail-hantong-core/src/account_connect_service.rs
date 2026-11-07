//! Core-owned account connection service.
//!
//! This service is the only stateful coordinator for the authenticated
//! app-connect contract. It accepts a verified caller and peer from the Core
//! transport, keeps OAuth state in memory, and never treats a UI flag as
//! authority.

use crate::account_connect::{AccountConnectIntent, AccountSelection, ObservedPeer};
use crate::account_connect_existing::{ExistingConnectIntent, VerifiedExistingDelta};
use crate::account_connect_generic::GenericConnectIntent;
use hantong_contracts::{
    ErrorCode,
    account_connect::{
        AccountConnectCommand, AccountConnectProvider, AccountConnectReceipt,
        AccountConnectSelection,
    },
    google_write::GoogleAccessMode,
};
use hantong_secrets::google::{
    GoogleAuthorization, GoogleClientSecret, PendingGoogleCredential,
    load_installed_publisher_configuration,
};
use hantong_store::{ClientKind, DEFAULT_CLI_LABEL, RegistrationRecord, RegistrationState, Store};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    path::Path,
    rc::Rc,
};
use tokio::sync::Notify;

const MAX_SESSIONS: usize = 64;
const MAX_QUEUE: usize = 16;

struct Session {
    owner: [u8; 16],
    client_public_key: [u8; 72],
    profile_epoch: u64,
    registration_version: u64,
    baseline_hash: [u8; 32],
    peer: ObservedPeer,
    provider: AccountConnectProvider,
    intent: Option<AccountConnectIntent>,
    authorization: Option<GoogleAuthorization>,
    client_secret: Option<GoogleClientSecret>,
    pending: Option<PendingGoogleCredential>,
    state: String,
    staged_decision: Option<[u8; 32]>,
    account_id: Option<String>,
    login_url: Option<String>,
    error_code: Option<ErrorCode>,
    expires_at_utc_ms: i64,
    selection_is_new: bool,
    generic: Option<GenericConnectIntent>,
    generic_queued: bool,
    existing: Option<ExistingConnectIntent>,
    existing_delta: Option<VerifiedExistingDelta>,
    existing_review: Option<ExistingSecretReview>,
    existing_capture: Option<ExistingSecretCapture>,
}

struct Inner {
    sessions: BTreeMap<[u8; 32], Session>,
    queue: VecDeque<[u8; 32]>,
    setup_reviews: BTreeMap<[u8; 32], BoundSetupReview>,
}

impl Session {
    fn matches_setup(
        &self,
        kind: hantong_contracts::terminal_review::TerminalReviewKind,
        id: [u8; 32],
    ) -> bool {
        self.generic
            .as_ref()
            .is_some_and(|g| g.matches_request(kind, id))
            || self
                .existing
                .as_ref()
                .is_some_and(|e| e.matches_request(kind, id))
    }
    fn validate_setup_current(
        &self,
        store: &Store,
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<(), ErrorCode> {
        if let Some(g) = self.generic.as_ref() {
            g.validate_current(store, peer, now)
        } else if let Some(e) = self.existing.as_ref() {
            e.validate_current(store, peer, now)
        } else {
            Err(ErrorCode::Conflict)
        }
    }
}

struct BoundSetupReview {
    intent_id: [u8; 32],
    expires_at: i64,
    ticket: Option<hantong_secrets::TerminalSecretTicket>,
}

#[derive(Clone)]
pub(crate) struct State {
    inner: Rc<RefCell<Inner>>,
    notify: Rc<Notify>,
}

pub(crate) struct Worker {
    inner: Rc<RefCell<Inner>>,
    notify: Rc<Notify>,
}

impl State {
    fn mark_staged(
        &self,
        id: [u8; 32],
        decision: [u8; 32],
        account: &str,
    ) -> Result<(), ErrorCode> {
        if id != decision || account.is_empty() {
            return Err(ErrorCode::Conflict);
        }
        let mut inner = self.inner.borrow_mut();
        let session = inner.sessions.get_mut(&id).ok_or(ErrorCode::NotFound)?;
        session.staged_decision = Some(decision);
        session.account_id = Some(account.to_owned());
        session.state = "staged".into();
        session.error_code = None;
        Ok(())
    }
    pub(crate) fn new() -> (Self, Worker) {
        let inner = Rc::new(RefCell::new(Inner {
            sessions: BTreeMap::new(),
            queue: VecDeque::new(),
            setup_reviews: BTreeMap::new(),
        }));
        let notify = Rc::new(Notify::new());
        (
            Self {
                inner: inner.clone(),
                notify: notify.clone(),
            },
            Worker { inner, notify },
        )
    }

    #[allow(clippy::too_many_arguments)] // Kernel peer, server and consumer remain separate checked inputs.
    pub(crate) fn command(
        &self,
        store: &Store,
        root: &Path,
        server_public_key: [u8; 72],
        caller: &RegistrationRecord,
        peer: ObservedPeer,
        sessions: &crate::setup_smtp::Sessions,
        command: AccountConnectCommand,
    ) -> Result<AccountConnectReceipt, ErrorCode> {
        validate_owner(caller)?;
        if store.load_default_cli_registration()?.as_ref() != Some(caller) {
            return Err(ErrorCode::ScopeDenied);
        }
        expire_sessions(&mut self.inner.borrow_mut(), now()?);
        self.cleanup_inactive_captures(root)?;
        match command {
            AccountConnectCommand::Prepare {
                provider,
                selection,
            } => self.prepare(
                store,
                server_public_key,
                caller,
                peer,
                sessions,
                provider,
                selection,
            ),
            AccountConnectCommand::Status(intent_id) => {
                self.session_checked(intent_id, caller, &peer)?;
                self.status(intent_id)
            }
            AccountConnectCommand::Cancel(intent_id) => {
                self.session_checked(intent_id, caller, &peer)?;
                self.cleanup_existing_capture(root, intent_id)?;
                self.cancel(intent_id, caller, &peer)
            }
            AccountConnectCommand::Complete(intent_id) => {
                self.complete(store, root, server_public_key, intent_id, caller, &peer)
            }
            AccountConnectCommand::BindSmtp {
                intent_id,
                request_id,
            } => self.bind_smtp(store, sessions, intent_id, request_id, caller, &peer),
        }
    }

    #[allow(clippy::too_many_arguments)] // Exact app intent inputs; no caller-supplied authority.
    fn prepare(
        &self,
        store: &Store,
        server_public_key: [u8; 72],
        caller: &RegistrationRecord,
        peer: ObservedPeer,
        _sessions: &crate::setup_smtp::Sessions,
        provider: AccountConnectProvider,
        selection: AccountConnectSelection,
    ) -> Result<AccountConnectReceipt, ErrorCode> {
        self.prune_terminal_sessions(now()?);
        if provider == AccountConnectProvider::ImapSmtp {
            if let AccountConnectSelection::Existing(account) = &selection {
                return self.prepare_existing(
                    store,
                    server_public_key,
                    caller,
                    peer,
                    account.clone(),
                );
            }
            let AccountConnectSelection::ImapSetup(request_id) = selection else {
                return Ok(failed_receipt(
                    hantong_platform::random_nonce()?,
                    provider,
                    ErrorCode::InvalidInput,
                ));
            };
            let intent_id = hantong_platform::random_nonce()?;
            let generic = GenericConnectIntent::new(
                store,
                caller,
                intent_id,
                hantong_platform::sha256(&server_public_key)?,
                peer.clone(),
                request_id,
                now()?,
            )?;
            let expires = generic.expires_at_utc_ms();
            let account_id = generic.account_id().to_owned();
            let session = Session {
                owner: caller.registration_id,
                client_public_key: caller.public_key,
                profile_epoch: caller.profile_epoch,
                registration_version: caller.version,
                baseline_hash: store.full_mail_baseline_hash(caller.registration_id)?,
                peer,
                provider,
                intent: None,
                authorization: None,
                client_secret: None,
                pending: None,
                staged_decision: None,
                state: "awaiting_setup".into(),
                account_id: Some(account_id.clone()),
                login_url: None,
                error_code: None,
                expires_at_utc_ms: expires,
                selection_is_new: false,
                generic: Some(generic),
                generic_queued: false,
                existing: None,
                existing_delta: None,
                existing_review: None,
                existing_capture: None,
            };
            let mut inner = self.inner.borrow_mut();
            if inner.sessions.values().any(|session| {
                session
                    .generic
                    .as_ref()
                    .is_some_and(|intent| intent.imap_request_id() == request_id)
                    && !terminal_state(&session.state)
            }) {
                return Err(ErrorCode::Conflict);
            }
            if inner.sessions.len() >= MAX_SESSIONS {
                return Err(ErrorCode::QuotaExceeded);
            }
            inner.sessions.insert(intent_id, session);
            return Ok(AccountConnectReceipt {
                intent_id,
                expires_at_utc_ms: expires,
                state: "awaiting_setup".into(),
                provider,
                account_id: Some(account_id),
                login_url: None,
                error_code: None,
            });
        }
        if provider != AccountConnectProvider::Google {
            return Ok(failed_receipt(
                hantong_platform::random_nonce()?,
                provider,
                ErrorCode::CapabilityUnsupported,
            ));
        }
        let selection_is_new = matches!(selection, AccountConnectSelection::New);
        let (selection, wire_account, selected_client_id) = match selection {
            AccountConnectSelection::New => (AccountSelection::NewFromProvider, None, None),
            AccountConnectSelection::Existing(account_id) => {
                let binding = store.google_account_binding(&account_id)?;
                let subject = binding.subject.clone();
                (
                    AccountSelection::ExistingExact {
                        account_id: account_id.clone(),
                        subject,
                    },
                    Some(account_id),
                    Some(binding.client_id),
                )
            }
            AccountConnectSelection::ImapSetup(_) => {
                return Ok(failed_receipt(
                    hantong_platform::random_nonce()?,
                    provider,
                    ErrorCode::CapabilityUnsupported,
                ));
            }
        };
        let publisher =
            load_installed_publisher_configuration()?.ok_or(ErrorCode::HumanStepRequired)?;
        let client_id = selected_client_id
            .or(store.google_client_id()?)
            .unwrap_or_else(|| publisher.client_id.clone());
        if publisher.client_id != client_id {
            return Err(ErrorCode::ScopeDenied);
        }
        let intent_id = hantong_platform::random_nonce()?;
        let pin = hantong_platform::sha256(&server_public_key)?;
        let intent = AccountConnectIntent::new(
            caller,
            intent_id,
            pin,
            peer.clone(),
            client_id,
            selection,
            full_mail_tools(),
            now()?,
        )?;
        let expires = intent.expires_at_utc_ms();
        let baseline_hash = store.full_mail_baseline_hash(caller.registration_id)?;
        let session = Session {
            owner: caller.registration_id,
            client_public_key: caller.public_key,
            profile_epoch: caller.profile_epoch,
            registration_version: caller.version,
            baseline_hash,
            peer,
            provider,
            intent: Some(intent),
            authorization: None,
            client_secret: Some(publisher.client_secret),
            pending: None,
            staged_decision: None,
            state: "queued".into(),
            account_id: wire_account.clone(),
            login_url: None,
            error_code: None,
            expires_at_utc_ms: expires,
            selection_is_new,
            generic: None,
            generic_queued: false,
            existing: None,
            existing_delta: None,
            existing_review: None,
            existing_capture: None,
        };
        let mut inner = self.inner.borrow_mut();
        if inner.sessions.len() >= MAX_SESSIONS || inner.queue.len() >= MAX_QUEUE {
            return Err(ErrorCode::QuotaExceeded);
        }
        if (wire_account.as_deref().is_some() || selection_is_new)
            && inner.sessions.values().any(|existing| {
                existing.owner == caller.registration_id
                    && existing.provider == provider
                    && existing.selection_is_new == selection_is_new
                    && wire_account
                        .as_deref()
                        .is_none_or(|account_id| existing.account_id.as_deref() == Some(account_id))
                    && !matches!(
                        existing.state.as_str(),
                        "cancelled" | "expired" | "failed" | "ready"
                    )
            })
        {
            return Err(ErrorCode::Conflict);
        }
        inner.sessions.insert(intent_id, session);
        inner.queue.push_back(intent_id);
        drop(inner);
        self.notify.notify_one();
        Ok(AccountConnectReceipt {
            intent_id,
            expires_at_utc_ms: expires,
            state: "queued".into(),
            provider,
            account_id: None,
            login_url: None,
            error_code: None,
        })
    }

    fn session_checked(
        &self,
        intent_id: [u8; 32],
        caller: &RegistrationRecord,
        peer: &ObservedPeer,
    ) -> Result<(), ErrorCode> {
        let inner = self.inner.borrow();
        let session = inner.sessions.get(&intent_id).ok_or(ErrorCode::NotFound)?;
        let identity_matches = session.owner == caller.registration_id
            && session.client_public_key == caller.public_key
            && &session.peer == peer;
        let binding_matches = if session.state == "ready" {
            session.profile_epoch.checked_add(1) == Some(caller.profile_epoch)
                && session.registration_version.checked_add(1) == Some(caller.version)
                && session.account_id.as_deref().is_some_and(|account| {
                    full_mail_tools()
                        .iter()
                        .all(|tool| caller.scope.permits_account(account, tool))
                })
        } else {
            session.profile_epoch == caller.profile_epoch
                && session.registration_version == caller.version
        };
        if !identity_matches || !binding_matches {
            return Err(ErrorCode::ScopeDenied);
        }
        Ok(())
    }

    fn prune_terminal_sessions(&self, current: i64) {
        // Staged decisions retain their exact recovery path; only expired
        // terminal receipts can be discarded from this bounded memory cache.
        self.inner.borrow_mut().sessions.retain(|_, session| {
            !terminal_state(&session.state) || current < session.expires_at_utc_ms
        });
    }

    fn cancel(
        &self,
        intent_id: [u8; 32],
        caller: &RegistrationRecord,
        peer: &ObservedPeer,
    ) -> Result<AccountConnectReceipt, ErrorCode> {
        self.session_checked(intent_id, caller, peer)?;
        let inner = self.inner.borrow();
        let session = inner.sessions.get(&intent_id).ok_or(ErrorCode::NotFound)?;
        if session.state == "staged" || session.state == "ready" {
            return Err(ErrorCode::Conflict);
        }
        if now()? >= session.expires_at_utc_ms {
            drop(inner);
            let mut inner = self.inner.borrow_mut();
            let session = inner
                .sessions
                .get_mut(&intent_id)
                .ok_or(ErrorCode::NotFound)?;
            session.intent.take();
            session.authorization.take();
            session.pending.take();
            session.existing.take();
            session.existing_delta.take();
            session.state = "expired".into();
            session.error_code = Some(ErrorCode::ApprovalExpired);
            inner.queue.retain(|queued| queued != &intent_id);
            let session = inner.sessions.get(&intent_id).ok_or(ErrorCode::NotFound)?;
            return Ok(receipt(intent_id, session));
        }
        if session.state == "expired" || session.state == "cancelled" {
            return Ok(receipt(intent_id, session));
        }
        drop(inner);
        let mut inner = self.inner.borrow_mut();
        let session = inner
            .sessions
            .get_mut(&intent_id)
            .ok_or(ErrorCode::NotFound)?;
        session.intent.take();
        session.authorization.take();
        session.pending.take();
        session.existing.take();
        session.existing_delta.take();
        session.existing_review.take();
        session.state = "cancelled".into();
        session.error_code = Some(ErrorCode::ApprovalDenied);
        inner.queue.retain(|queued| queued != &intent_id);
        drop(inner);
        self.notify.notify_one();
        let inner = self.inner.borrow();
        let session = inner.sessions.get(&intent_id).ok_or(ErrorCode::NotFound)?;
        Ok(receipt_from_mut(intent_id, session))
    }

    /// Check that a setup secret submission belongs to a live generic
    /// connection session and the observed peer is the one that created it.
    /// This is called before the existing setup dispatcher accepts the
    /// protected input; it has no state-changing effect.
    pub(crate) fn check_setup_capture(
        &self,
        store: &Store,
        kind: hantong_contracts::terminal_review::TerminalReviewKind,
        request_id: [u8; 32],
        peer: &ObservedPeer,
    ) -> Result<(), ErrorCode> {
        let now = now()?;
        let inner = self.inner.borrow();
        let Some(session) = inner
            .sessions
            .values()
            .find(|session| session.matches_setup(kind, request_id))
        else {
            return Ok(());
        };
        if !matches!(session.state.as_str(), "awaiting_setup" | "provider_ready") {
            return Err(ErrorCode::Conflict);
        }
        session.validate_setup_current(store, peer, now)?;
        if let Some(intent) = session.generic.as_ref() {
            intent.check_capture(kind, request_id, peer, now)
        } else {
            session
                .existing
                .as_ref()
                .ok_or(ErrorCode::ScopeDenied)?
                .check_capture(kind, request_id, peer, now)
        }
    }

    /// Record a setup secret submission only after the typed setup dispatcher
    /// has accepted it.  Matching is repeated under the mutable session
    /// borrow, so an expired, cancelled, or already-consumed request cannot
    /// be marked successful by a stale callback.
    pub(crate) fn record_setup_capture(
        &self,
        store: &Store,
        kind: hantong_contracts::terminal_review::TerminalReviewKind,
        request_id: [u8; 32],
        peer: &ObservedPeer,
    ) -> Result<(), ErrorCode> {
        let now = now()?;
        let mut inner = self.inner.borrow_mut();
        let Some(session) = inner
            .sessions
            .values_mut()
            .find(|session| session.matches_setup(kind, request_id))
        else {
            return Ok(());
        };
        if !matches!(session.state.as_str(), "awaiting_setup" | "provider_ready") {
            return Err(ErrorCode::Conflict);
        }
        session.validate_setup_current(store, peer, now)?;
        if let Some(intent) = session.generic.as_mut() {
            intent.capture_submitted(kind, request_id, peer, now)
        } else {
            session
                .existing
                .as_mut()
                .ok_or(ErrorCode::ScopeDenied)?
                .capture_submitted(kind, request_id, peer, now)
        }
    }

    fn bind_smtp(
        &self,
        store: &Store,
        sessions: &crate::setup_smtp::Sessions,
        intent_id: [u8; 32],
        request_id: [u8; 32],
        caller: &RegistrationRecord,
        peer: &ObservedPeer,
    ) -> Result<AccountConnectReceipt, ErrorCode> {
        self.session_checked(intent_id, caller, peer)?;
        let mut inner = self.inner.borrow_mut();
        let session = inner
            .sessions
            .get_mut(&intent_id)
            .ok_or(ErrorCode::NotFound)?;
        if session.provider != AccountConnectProvider::ImapSmtp
            || session.generic_queued
            || !matches!(session.state.as_str(), "awaiting_setup" | "provider_ready")
        {
            return Err(ErrorCode::Conflict);
        }
        if let Some(intent) = session.existing.as_mut() {
            if session.existing_review.is_some()
                || session.existing_capture.is_some()
                || session.existing_delta.is_some()
            {
                return Err(ErrorCode::Conflict);
            }
            intent.bind_smtp(store, sessions, request_id, now()?)?;
            session.expires_at_utc_ms = intent.expires_at_utc_ms();
            // Existing accounts still need fresh protected IMAP authentication.
            session.state = if intent.smtp_capture_required() {
                "awaiting_setup"
            } else {
                "provider_ready"
            }
            .into();
            return Ok(receipt(intent_id, session));
        }
        let intent = session.generic.as_mut().ok_or(ErrorCode::Conflict)?;
        intent.bind_smtp(store, sessions, request_id, now()?)?;
        session.expires_at_utc_ms = intent.expires_at_utc_ms();
        session.state = if intent.smtp_capture_required() {
            "awaiting_setup"
        } else {
            "provider_ready"
        }
        .into();
        Ok(receipt(intent_id, session))
    }

    fn status(&self, intent_id: [u8; 32]) -> Result<AccountConnectReceipt, ErrorCode> {
        let should_expire = {
            let inner = self.inner.borrow();
            let session = inner.sessions.get(&intent_id).ok_or(ErrorCode::NotFound)?;
            !matches!(
                session.state.as_str(),
                "cancelled" | "expired" | "staged" | "ready"
            ) && now()? >= session.expires_at_utc_ms
        };
        if should_expire {
            let mut inner = self.inner.borrow_mut();
            let session = inner
                .sessions
                .get_mut(&intent_id)
                .ok_or(ErrorCode::NotFound)?;
            session.intent.take();
            session.authorization.take();
            session.pending.take();
            session.existing.take();
            session.existing_delta.take();
            session.state = "expired".into();
            session.error_code = Some(ErrorCode::ApprovalExpired);
            inner.queue.retain(|queued| queued != &intent_id);
        }
        let inner = self.inner.borrow();
        let session = inner.sessions.get(&intent_id).ok_or(ErrorCode::NotFound)?;
        Ok(receipt(intent_id, session))
    }

    fn complete(
        &self,
        store: &Store,
        root: &Path,
        server_public_key: [u8; 72],
        intent_id: [u8; 32],
        caller: &RegistrationRecord,
        peer: &ObservedPeer,
    ) -> Result<AccountConnectReceipt, ErrorCode> {
        self.session_checked(intent_id, caller, peer)?;
        let inner = self.inner.borrow();
        let session = inner.sessions.get(&intent_id).ok_or(ErrorCode::NotFound)?;
        if session.state != "provider_ready"
            && !(session.generic.is_some() && session.state == "awaiting_setup")
        {
            return Err(ErrorCode::Conflict);
        }
        drop(inner);
        let mut inner = self.inner.borrow_mut();
        let session = inner
            .sessions
            .get_mut(&intent_id)
            .ok_or(ErrorCode::NotFound)?;
        if session.generic.is_some() {
            if session.generic_queued {
                return Err(ErrorCode::Conflict);
            }
            if !matches!(session.state.as_str(), "awaiting_setup" | "provider_ready") {
                return Err(ErrorCode::Conflict);
            }
            session.generic_queued = true;
            session.state = "queued".into();
            inner.queue.push_back(intent_id);
            drop(inner);
            self.notify.notify_one();
            let inner = self.inner.borrow();
            let session = inner.sessions.get(&intent_id).ok_or(ErrorCode::NotFound)?;
            return Ok(receipt(intent_id, session));
        }
        let expected_baseline = session.baseline_hash;
        if let Some(delta) = session.existing_delta.take() {
            drop(inner);
            let staged = delta.stage(
                store,
                hantong_platform::sha256(&server_public_key)?,
                peer,
                now()?,
            );
            let (decision, account) = match staged {
                Ok(value) => value,
                Err(error) => {
                    if let Some(session) = self.inner.borrow_mut().sessions.get_mut(&intent_id) {
                        session.state = "failed".into();
                        session.error_code = Some(error);
                    }
                    return Err(error);
                }
            };
            self.mark_staged(intent_id, decision, &account)?;
            let published = crate::account_connect::publish_staged(
                store,
                caller.registration_id,
                decision,
                server_public_key,
            );
            let mut inner = self.inner.borrow_mut();
            let session = inner
                .sessions
                .get_mut(&intent_id)
                .ok_or(ErrorCode::NotFound)?;
            session.account_id = Some(account);
            session.state = if published.is_ok() { "ready" } else { "staged" }.into();
            if let Err(error) = published {
                report_publication_failure(error);
            }
            session.error_code = None;
            return Ok(receipt(intent_id, session));
        }
        let intent = session.intent.take().ok_or(ErrorCode::Conflict)?;
        let pending = session.pending.take().ok_or(ErrorCode::Conflict)?;
        drop(inner);
        let result = (|| {
            let identity = pending.identity().clone();
            let baseline = store.full_mail_baseline_hash(caller.registration_id)?;
            if baseline != expected_baseline {
                return Err(ErrorCode::Conflict);
            }
            let credential_version = store
                .google_account_for_subject(&identity.subject)?
                .map_or(0, |binding| binding.credential_version)
                .checked_add(1)
                .ok_or(ErrorCode::Conflict)?;
            let binding = pending.binding(credential_version)?;
            let proof = intent.authorize(
                &pending,
                caller,
                baseline,
                hantong_platform::sha256(&server_public_key)?,
                peer,
                credential_version,
                now()?,
            )?;
            let secrets =
                hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
            pending.store(&secrets, &binding)?;
            let activated = store.activate_google_account(
                &identity,
                proof.client_id(),
                credential_version.saturating_sub(1),
                None,
            )?;
            if activated != binding {
                return Err(ErrorCode::InternalError);
            }
            let operation_id = format!(
                "account-connect-{}",
                hantong_contracts::wire_hex::encode(&intent_id)
            );
            let decision = proof.begin_full_mail_decision(store, &operation_id, &operation_id)?;
            proof.consume_and_stage(
                store,
                decision.decision_id,
                hantong_platform::sha256(&server_public_key)?,
                peer,
                now()?,
            )?;
            Ok((decision.decision_id, identity.account_id))
        })();
        let (decision_id, account_id) = match result {
            Ok(value) => value,
            Err(error) => {
                let mut inner = self.inner.borrow_mut();
                if let Some(session) = inner.sessions.get_mut(&intent_id) {
                    session.state = "failed".into();
                    session.error_code = Some(error);
                }
                return Err(error);
            }
        };
        self.mark_staged(intent_id, decision_id, &account_id)?;
        if let Err(error) = crate::account_connect::publish_staged(
            store,
            caller.registration_id,
            decision_id,
            server_public_key,
        ) {
            let mut inner = self.inner.borrow_mut();
            if let Some(session) = inner.sessions.get_mut(&intent_id) {
                session.state = "staged".into();
                session.error_code = None;
            }
            report_publication_failure(error);
            return Ok(receipt(
                intent_id,
                inner.sessions.get(&intent_id).ok_or(ErrorCode::NotFound)?,
            ));
        }
        let mut inner = self.inner.borrow_mut();
        let session = inner
            .sessions
            .get_mut(&intent_id)
            .ok_or(ErrorCode::NotFound)?;
        session.account_id = Some(account_id);
        session.state = "ready".into();
        Ok(receipt_from_mut(intent_id, session))
    }
}

impl Worker {
    fn retry_staged(&self, store: &Store, server: [u8; 72]) {
        self.retry_staged_with(|owner, decision, account, version, epoch| {
            if store
                .load_default_cli_registration()?
                .is_some_and(|r| r.state == RegistrationState::Provisioning)
            {
                crate::account_connect::publish_staged(store, owner, decision, server)?;
            }
            let current = store
                .load_default_cli_registration()?
                .ok_or(ErrorCode::NotFound)?;
            if current.registration_id != owner
                || !store.list_committed_full_mail_cleanup()?.iter().any(|row| {
                    row.registration_id == owner
                        && row.decision_id == decision
                        && row.target_account_id == account
                        && row.expected_registration_version == version
                        && row.expected_profile_epoch == epoch
                })
            {
                return Err(ErrorCode::Conflict);
            }
            // The exact decision committed, but later legitimate scope changes
            // may supersede its target before a recovery sweep observes it.
            Ok(current.state == RegistrationState::Active
                && Some(current.version) == version.checked_add(1)
                && Some(current.profile_epoch) == epoch.checked_add(1)
                && full_mail_tools()
                    .iter()
                    .all(|tool| current.scope.permits_account(account, tool)))
        });
    }

    fn retry_staged_with(
        &self,
        mut publish: impl FnMut([u8; 16], [u8; 32], &str, u64, u64) -> Result<bool, ErrorCode>,
    ) {
        let staged: Vec<_> = self
            .inner
            .borrow()
            .sessions
            .iter()
            .filter_map(|(id, s)| {
                (s.state == "staged")
                    .then_some((id, s))
                    .and_then(|(id, s)| {
                        Some((
                            *id,
                            s.owner,
                            s.staged_decision?,
                            s.account_id.clone()?,
                            s.registration_version,
                            s.profile_epoch,
                        ))
                    })
            })
            .collect();
        for (id, owner, decision, account, version, epoch) in staged {
            // This synchronous sweep cannot race another session worker.
            if let Ok(current) = publish(owner, decision, &account, version, epoch)
                && let Some(session) = self.inner.borrow_mut().sessions.get_mut(&id)
                && session.state == "staged"
                && session.staged_decision == Some(decision)
            {
                session.state = if current { "ready" } else { "failed" }.into();
                session.error_code = if current {
                    None
                } else {
                    Some(ErrorCode::Conflict)
                };
            }
        }
    }
    pub(crate) async fn run(
        &self,
        store: &Store,
        root: &Path,
        server: [u8; 72],
        sessions: &crate::setup_smtp::Sessions,
    ) -> Result<(), ErrorCode> {
        let expiry = async {
            let mut reported_cleanup_error = None;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                expire_sessions(&mut self.inner.borrow_mut(), now()?);
                self.retry_staged(store, server);
                let cleanup = State {
                    inner: self.inner.clone(),
                    notify: self.notify.clone(),
                }
                .cleanup_inactive_captures(root);
                match cleanup {
                    Ok(()) => reported_cleanup_error = None,
                    Err(error) => {
                        // Keep exact deletion selectors for the next sweep.
                        if reported_cleanup_error != Some(error) {
                            use std::io::Write;
                            let _ = writeln!(
                                std::io::stderr().lock(),
                                "hantong_account_connect stage=expired_capture_cleanup outcome=deferred code={}",
                                error.as_str()
                            );
                            reported_cleanup_error = Some(error);
                        }
                    }
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), ErrorCode>(())
        };
        tokio::select! {
            result=self.process(store,root,server,sessions)=>result,
            result=expiry=>result,
        }
    }

    async fn process(
        &self,
        _store: &Store,
        _root: &Path,
        _server_public_key: [u8; 72],
        _sessions: &crate::setup_smtp::Sessions,
    ) -> Result<(), ErrorCode> {
        loop {
            let intent_id = loop {
                expire_sessions(&mut self.inner.borrow_mut(), now()?);
                if let Some(id) = self.inner.borrow_mut().queue.pop_front() {
                    break id;
                }
                tokio::select! {
                    _=self.notify.notified()=>{},
                    _=tokio::time::sleep(std::time::Duration::from_secs(1))=>{},
                }
            };
            if self
                .run_existing_capture(_store, _root, _sessions, intent_id)
                .await
            {
                continue;
            }
            let is_generic = {
                let inner = self.inner.borrow();
                inner
                    .sessions
                    .get(&intent_id)
                    .is_some_and(|session| session.generic.is_some() && session.generic_queued)
            };
            if is_generic {
                let generic = {
                    let mut inner = self.inner.borrow_mut();
                    let Some(session) = inner.sessions.get_mut(&intent_id) else {
                        continue;
                    };
                    session.generic.take()
                };
                let Some(generic) = generic else {
                    self.fail_session(intent_id, ErrorCode::Conflict);
                    continue;
                };
                let pin = generic.server_pin();
                let peer = generic.peer().clone();
                let owner = generic.registration_id();
                let account_id = generic.account_id().to_owned();
                let delta = match generic
                    .authorize_ready(_store, _root, _sessions, pin, &peer, now()?)
                    .await
                {
                    Ok(delta) => delta,
                    Err(error) => {
                        self.fail_session(intent_id, error);
                        continue;
                    }
                };
                {
                    let inner = self.inner.borrow();
                    let Some(session) = inner.sessions.get(&intent_id) else {
                        continue;
                    };
                    if session.state == "cancelled"
                        || session.state == "expired"
                        || now().map_or(true, |current| current >= session.expires_at_utc_ms)
                    {
                        continue;
                    }
                }
                let decision = match delta.stage(_store, pin, &peer, now()?) {
                    Ok(decision) => decision,
                    Err(error) => {
                        self.fail_session(intent_id, error);
                        continue;
                    }
                };
                State {
                    inner: self.inner.clone(),
                    notify: self.notify.clone(),
                }
                .mark_staged(intent_id, decision.0, &account_id)?;
                if let Err(error) = crate::account_connect::publish_staged(
                    _store,
                    owner,
                    decision.0,
                    _server_public_key,
                ) {
                    let mut inner = self.inner.borrow_mut();
                    if let Some(session) = inner.sessions.get_mut(&intent_id) {
                        session.state = "staged".into();
                        session.error_code = None;
                    }
                    report_publication_failure(error);
                    continue;
                }
                let mut inner = self.inner.borrow_mut();
                if let Some(session) = inner.sessions.get_mut(&intent_id) {
                    session.account_id = Some(account_id);
                    session.state = "ready".into();
                    session.error_code = None;
                }
                continue;
            }
            let client_id = {
                let inner = self.inner.borrow();
                inner
                    .sessions
                    .get(&intent_id)
                    .and_then(|session| session.intent.as_ref())
                    .map(AccountConnectIntent::client_id)
                    .map(str::to_owned)
            };
            let Some(client_id) = client_id else {
                self.fail_session(intent_id, ErrorCode::Conflict);
                continue;
            };
            let authorization = match hantong_secrets::google::begin_google_authorization_for_mode(
                &client_id,
                true,
                GoogleAccessMode::Modify,
            )
            .await
            {
                Ok(authorization) => authorization,
                Err(error) => {
                    self.fail_session(intent_id, error);
                    continue;
                }
            };
            let authorization = {
                let mut inner = self.inner.borrow_mut();
                let Some(session) = inner.sessions.get_mut(&intent_id) else {
                    continue;
                };
                if session.state != "queued"
                    || now().map_or(true, |current| current >= session.expires_at_utc_ms)
                {
                    continue;
                }
                let Some(secret) = session.client_secret.take() else {
                    session.state = "failed".into();
                    session.error_code = Some(ErrorCode::Conflict);
                    continue;
                };
                match authorization.with_client_secret(secret) {
                    Ok(value) => value,
                    Err(error) => {
                        session.state = "failed".into();
                        session.error_code = Some(error);
                        continue;
                    }
                }
            };
            let url = authorization.authorization_url().to_owned();
            if let Err(error) =
                hantong_platform::google_browser::open_google_authorization_url(&url)
            {
                let mut inner = self.inner.borrow_mut();
                if let Some(session) = inner.sessions.get_mut(&intent_id) {
                    session.state = "failed".into();
                    session.error_code = Some(error);
                }
                continue;
            }
            {
                let mut inner = self.inner.borrow_mut();
                let Some(session) = inner.sessions.get_mut(&intent_id) else {
                    continue;
                };
                session.login_url = Some(url);
                session.state = "awaiting_browser".into();
                session.authorization = Some(authorization);
            }
            let authorization = {
                let mut inner = self.inner.borrow_mut();
                let Some(authorization) = inner
                    .sessions
                    .get_mut(&intent_id)
                    .and_then(|session| session.authorization.take())
                else {
                    continue;
                };
                authorization
            };
            let pending = match authorization.complete().await {
                Ok(pending) => pending,
                Err(error) => {
                    self.fail_session(intent_id, error);
                    continue;
                }
            };
            let account_id = pending.identity().account_id.clone();
            let mut inner = self.inner.borrow_mut();
            let Some(session) = inner.sessions.get_mut(&intent_id) else {
                continue;
            };
            if session.state == "cancelled" {
                continue;
            }
            if session.state == "expired"
                || now().map_or(true, |current| current >= session.expires_at_utc_ms)
            {
                session.state = "expired".into();
                session.error_code = Some(ErrorCode::ApprovalExpired);
                continue;
            }
            session.account_id = Some(account_id);
            session.pending = Some(pending);
            session.state = "provider_ready".into();
        }
    }

    fn fail_session(&self, intent_id: [u8; 32], error: ErrorCode) {
        if let Some(session) = self.inner.borrow_mut().sessions.get_mut(&intent_id) {
            if terminal_state(&session.state) || session.state == "staged" {
                return;
            }
            if now().map_or(true, |current| current >= session.expires_at_utc_ms) {
                session.state = "expired".into();
                session.error_code = Some(ErrorCode::ApprovalExpired);
                return;
            }
            session.state = "failed".into();
            session.error_code = Some(error);
        }
    }
}

fn terminal_state(state: &str) -> bool {
    matches!(state, "cancelled" | "expired" | "failed" | "ready")
}

fn report_publication_failure(error: ErrorCode) {
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr().lock(),
        "hantong_account_connect stage=profile_publication outcome=failed code={}",
        error.as_str()
    );
}

fn expire_sessions(inner: &mut Inner, current: i64) {
    for s in inner.sessions.values_mut() {
        if !terminal_state(&s.state) && s.state != "staged" && current >= s.expires_at_utc_ms {
            s.intent.take();
            s.authorization.take();
            s.pending.take();
            s.existing.take();
            s.existing_delta.take();
            s.state = "expired".into();
            s.error_code = Some(ErrorCode::ApprovalExpired);
        }
    }
    inner
        .queue
        .retain(|id| inner.sessions.get(id).is_some_and(|s| s.state == "queued"));
}

fn validate_owner(record: &RegistrationRecord) -> Result<(), ErrorCode> {
    if record.state != RegistrationState::Active {
        return Err(ErrorCode::ClientSuspended);
    }
    if record.client_kind != ClientKind::Cli || record.display_label != DEFAULT_CLI_LABEL {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}

fn full_mail_tools() -> Vec<String> {
    [
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
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

fn receipt(id: [u8; 32], session: &Session) -> AccountConnectReceipt {
    AccountConnectReceipt {
        intent_id: id,
        expires_at_utc_ms: session.expires_at_utc_ms,
        state: session.state.clone(),
        provider: session.provider,
        account_id: session.account_id.clone(),
        login_url: session.login_url.clone(),
        error_code: session.error_code,
    }
}

fn receipt_from_mut(id: [u8; 32], session: &Session) -> AccountConnectReceipt {
    receipt(id, session)
}

fn failed_receipt(
    id: [u8; 32],
    provider: AccountConnectProvider,
    error_code: ErrorCode,
) -> AccountConnectReceipt {
    AccountConnectReceipt {
        intent_id: id,
        expires_at_utc_ms: 1,
        state: "failed".into(),
        provider,
        account_id: None,
        login_url: None,
        error_code: Some(error_code),
    }
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

include!("account_connect_service_review.rs");

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::{auth::RequestedScope, unix_auth::UnixPlatform};

    fn caller() -> RegistrationRecord {
        RegistrationRecord {
            registration_id: [1; 16],
            request_id: [2; 32],
            client_kind: ClientKind::Cli,
            display_label: DEFAULT_CLI_LABEL.into(),
            public_key: [3; 72],
            profile_epoch: 4,
            state: RegistrationState::Active,
            read_only: false,
            requests_per_minute: 60,
            approved_channel: "terminal_cli".into(),
            version: 5,
            scope: RequestedScope::new(vec!["acct".into()], vec!["mail.accounts.list".into()])
                .unwrap(),
        }
    }

    fn peer(tag: u8) -> ObservedPeer {
        ObservedPeer::Unix(
            hantong_contracts::unix_auth::UnixPeerContextV2::new(
                UnixPlatform::Linux,
                1000,
                1000,
                42,
                7,
                [tag; 32],
            )
            .unwrap(),
        )
    }

    #[test]
    fn sec03_staged_publication_retries_exact_decision_without_new_authority() {
        let (state, worker) = State::new();
        let caller = caller();
        let id = insert(&state, &caller, peer(1), "queued", i64::MAX);
        assert_eq!(
            state.mark_staged(id, [8; 32], "acct"),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(state.status(id).unwrap().state, "queued");
        state.mark_staged(id, id, "acct").unwrap();
        worker.retry_staged_with(|owner, decision, account, version, epoch| {
            assert_eq!(
                (owner, decision, account, version, epoch),
                (
                    caller.registration_id,
                    id,
                    "acct",
                    caller.version,
                    caller.profile_epoch
                )
            );
            Err(ErrorCode::CoreUnavailable)
        });
        assert_eq!(state.status(id).unwrap().state, "staged");
        worker.retry_staged_with(|_, _, _, _, _| Ok(true));
        assert_eq!(state.status(id).unwrap().state, "ready");
        worker.retry_staged_with(|_, _, _, _, _| panic!("active decision must not publish twice"));
        assert!(state.inner.borrow().queue.is_empty());
        assert!(state.inner.borrow().sessions[&id].intent.is_none());
        assert!(state.inner.borrow().sessions[&id].pending.is_none());
    }

    #[test]
    fn sec03_committed_but_superseded_decision_does_not_stay_staged_or_republish() {
        let (state, worker) = State::new();
        let caller = caller();
        let id = insert(&state, &caller, peer(1), "queued", i64::MAX);
        state.mark_staged(id, id, "acct").unwrap();
        worker.retry_staged_with(|_, _, _, _, _| Ok(false));
        let receipt = state.status(id).unwrap();
        assert_eq!(receipt.state, "failed");
        assert_eq!(receipt.error_code, Some(ErrorCode::Conflict));
        worker.retry_staged_with(|_, _, _, _, _| panic!("committed decision cannot publish again"));
        assert!(state.inner.borrow().queue.is_empty());
    }

    fn insert(
        state: &State,
        caller: &RegistrationRecord,
        peer: ObservedPeer,
        phase: &str,
        expiry: i64,
    ) -> [u8; 32] {
        let id = [9; 32];
        state.inner.borrow_mut().sessions.insert(
            id,
            Session {
                owner: caller.registration_id,
                client_public_key: caller.public_key,
                profile_epoch: caller.profile_epoch,
                registration_version: caller.version,
                baseline_hash: [4; 32],
                peer,
                provider: AccountConnectProvider::Google,
                intent: None,
                authorization: None,
                client_secret: None,
                pending: None,
                staged_decision: None,
                state: phase.into(),
                account_id: Some("acct".into()),
                login_url: None,
                error_code: None,
                expires_at_utc_ms: expiry,
                selection_is_new: phase == "queued",
                generic: None,
                generic_queued: false,
                existing: None,
                existing_delta: None,
                existing_review: None,
                existing_capture: None,
            },
        );
        id
    }

    #[test]
    fn sec03_cancel_is_bound_and_terminal_without_provider_effects() {
        let (state, _) = State::new();
        let record = caller();
        let id = insert(&state, &record, peer(1), "queued", i64::MAX);
        let cancelled = state.cancel(id, &record, &peer(1)).unwrap();
        assert_eq!(cancelled.state, "cancelled");
        assert_eq!(cancelled.error_code, Some(ErrorCode::ApprovalDenied));
        assert_eq!(
            state.cancel(id, &record, &peer(1)).unwrap().state,
            "cancelled"
        );
        assert_eq!(
            state.cancel(id, &record, &peer(2)),
            Err(ErrorCode::ScopeDenied)
        );
    }

    #[test]
    fn sec03_expired_status_is_terminal_and_staged_cannot_cancel() {
        let (state, _) = State::new();
        let record = caller();
        let expired = insert(&state, &record, peer(1), "awaiting_browser", 0);
        assert_eq!(state.status(expired).unwrap().state, "expired");
        let staged = insert(&state, &record, peer(1), "staged", i64::MAX);
        assert_eq!(
            state.cancel(staged, &record, &peer(1)),
            Err(ErrorCode::Conflict)
        );
    }

    #[test]
    fn sec03_wrong_registration_binding_is_rejected_before_state_change() {
        let (state, _) = State::new();
        let record = caller();
        let id = insert(&state, &record, peer(1), "awaiting_browser", i64::MAX);
        let mut wrong = caller();
        wrong.version += 1;
        assert_eq!(
            state.session_checked(id, &wrong, &peer(1)),
            Err(ErrorCode::ScopeDenied)
        );
        assert_eq!(
            state.cancel(id, &wrong, &peer(1)),
            Err(ErrorCode::ScopeDenied)
        );
        assert_eq!(state.status(id).unwrap().state, "awaiting_browser");
    }

    #[test]
    fn sec03_full_mail_tools_are_exactly_the_approved_set() {
        let tools = full_mail_tools();
        assert_eq!(tools.len(), 10);
        assert!(tools.contains(&"mail.messages.trash.request".into()));
        assert!(tools.contains(&"mail.messages.trash.execute".into()));
        assert!(!tools.contains(&"mail.accounts.list".into()));
    }

    #[test]
    fn sec03_late_worker_failure_cannot_resurrect_cancelled_or_staged_session() {
        let (state, worker) = State::new();
        let record = caller();
        for phase in ["cancelled", "expired", "ready", "staged"] {
            let id = insert(&state, &record, peer(1), phase, i64::MAX);
            worker.fail_session(id, ErrorCode::Offline);
            assert_eq!(state.status(id).unwrap().state, phase);
        }
        let id = insert(&state, &record, peer(1), "queued", 0);
        worker.fail_session(id, ErrorCode::Offline);
        let status = state.status(id).unwrap();
        assert_eq!(status.state, "expired");
        assert_eq!(status.error_code, Some(ErrorCode::ApprovalExpired));
    }

    #[test]
    fn sec03_cache_pruning_preserves_live_and_staged_recovery() {
        let (state, _) = State::new();
        let record = caller();
        for phase in ["queued", "awaiting_setup", "staged"] {
            let id = insert(&state, &record, peer(1), phase, 10);
            state.prune_terminal_sessions(20);
            assert!(state.inner.borrow().sessions.contains_key(&id));
            state.inner.borrow_mut().sessions.clear();
        }
        for phase in ["cancelled", "expired", "failed", "ready"] {
            let id = insert(&state, &record, peer(1), phase, 10);
            state.prune_terminal_sessions(20);
            assert!(!state.inner.borrow().sessions.contains_key(&id));
        }
    }

    #[test]
    fn sec03_cancelled_setup_ticket_stays_bound_after_session_pruning() {
        use hantong_contracts::terminal_review::*;
        use hantong_ipc_server::setup_dispatch::{SetupCommand, SetupReceipt};
        let f = crate::account_connect_generic::tests::Fixture::new();
        let (state, _) = State::new();
        let smtp = crate::setup_smtp::Sessions::default();
        let prepare = || AccountConnectCommand::Prepare {
            provider: AccountConnectProvider::ImapSmtp,
            selection: AccountConnectSelection::ImapSetup(*f.request.intent().request_id()),
        };
        let prepared = state
            .command(
                &f.store,
                &f.root,
                [3; 72],
                &f.base,
                f.peer.clone(),
                &smtp,
                prepare(),
            )
            .unwrap();
        assert_eq!(
            state.command(
                &f.store,
                &f.root,
                [3; 72],
                &f.base,
                f.peer.clone(),
                &smtp,
                prepare()
            ),
            Err(ErrorCode::Conflict)
        );
        let issued = TerminalReviewReceipt {
            kind: TerminalReviewKind::ImapSetup,
            request_id: *f.request.intent().request_id(),
            ticket: Some([7; 32]),
            snapshot_hash: [8; 32],
            owner_registration_id: f.base.registration_id,
            profile_epoch: f.base.profile_epoch,
            expires_at_utc_ms: i64::MAX,
            state: "pending".into(),
            secret_ticket_opaque: None,
            fields: vec![],
        };
        state
            .bind_setup_review_receipt(&SetupReceipt::TerminalReview(issued))
            .unwrap();
        state.cancel(prepared.intent_id, &f.base, &f.peer).unwrap();
        let submit = SetupCommand::TerminalReview(TerminalReviewCommand::SecretSubmit(
            TerminalReviewSecretSubmit::new([7; 32], [8; 32], vec![1; 320], [9; 32]).unwrap(),
        ));
        assert_eq!(
            state.check_setup_review_command(&f.store, &f.peer, &submit, None),
            Err(ErrorCode::Conflict)
        );
        state
            .inner
            .borrow_mut()
            .sessions
            .get_mut(&prepared.intent_id)
            .unwrap()
            .expires_at_utc_ms = 0;
        state.prune_terminal_sessions(now().unwrap());
        assert_eq!(
            state.check_setup_review_command(&f.store, &f.peer, &submit, None),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            f.store.load_registration(f.base.registration_id).unwrap(),
            Some(f.base.clone())
        );
        assert!(
            f.store
                .list_staged_full_mail_decisions()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn sec03_daemon_expiry_drops_queued_authority_without_status_rpc() {
        let (state, _) = State::new();
        let record = caller();
        let id = insert(&state, &record, peer(1), "queued", 10);
        state.inner.borrow_mut().queue.push_back(id);
        expire_sessions(&mut state.inner.borrow_mut(), 10);
        assert_eq!(state.inner.borrow().sessions[&id].state, "expired");
        assert!(state.inner.borrow().queue.is_empty());
        assert_eq!(record, caller());
    }

    #[test]
    fn sec03_expired_queued_existing_capture_is_discarded_without_grant() {
        use hantong_contracts::terminal_review::*;
        use hantong_ipc_server::setup_dispatch::{SetupCommand, SetupReceipt};
        use hantong_store::mail_connection::{MailConnectionState, MailEndpointKind};
        let f = crate::account_connect_generic::tests::Fixture::new();
        f.store
            .record_mail_observation(
                "baseline-account",
                MailEndpointKind::Imap,
                1,
                0,
                MailConnectionState::Ready,
            )
            .unwrap();
        let (state, _) = State::new();
        let smtp = crate::setup_smtp::Sessions::default();
        let prepared = state
            .command(
                &f.store,
                &f.root,
                [3; 72],
                &f.base,
                f.peer.clone(),
                &smtp,
                AccountConnectCommand::Prepare {
                    provider: AccountConnectProvider::ImapSmtp,
                    selection: AccountConnectSelection::Existing("baseline-account".into()),
                },
            )
            .unwrap();
        let get = SetupCommand::TerminalReview(TerminalReviewCommand::Get(
            TerminalReviewGet::new(
                TerminalReviewKind::AccountConnectSetup,
                prepared.intent_id,
                None,
                [0; 32],
            )
            .unwrap(),
        ));
        let SetupReceipt::TerminalReview(review) = state
            .existing_review(&f.store, &f.root, [3; 72], &f.peer, &get)
            .unwrap()
            .unwrap()
        else {
            panic!("wrong receipt");
        };
        let opaque = review.secret_ticket_opaque.as_ref().unwrap();
        let ticket = hantong_secrets::TerminalSecretTicket::from_opaque(opaque).unwrap();
        let secrets = hantong_secrets::SecretStore::open(&f.root).unwrap();
        let handle = secrets
            .stage_terminal_secret(&ticket, std::io::Cursor::new(b"synthetic-only"))
            .unwrap();
        let path = f.root.join(format!(
            "terminal-v1-{}.stage",
            hantong_contracts::wire_hex::encode(&handle.opaque_id())
        ));
        assert!(path.is_file());
        let submit = SetupCommand::TerminalReview(TerminalReviewCommand::SecretSubmit(
            TerminalReviewSecretSubmit::new(
                review.ticket.unwrap(),
                review.snapshot_hash,
                opaque.clone(),
                handle.opaque_id(),
            )
            .unwrap(),
        ));
        state
            .existing_review(&f.store, &f.root, [3; 72], &f.peer, &submit)
            .unwrap()
            .unwrap();
        expire_sessions(&mut state.inner.borrow_mut(), prepared.expires_at_utc_ms);
        state.cleanup_inactive_captures(&f.root).unwrap();
        assert!(!path.exists());
        assert!(state.inner.borrow().queue.is_empty());
        assert_eq!(
            f.store.load_registration(f.base.registration_id).unwrap(),
            Some(f.base.clone())
        );
        assert!(
            f.store
                .list_staged_full_mail_decisions()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn sec03_unbound_legacy_setup_capture_remains_unchanged() {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        crate::test_support::ensure_test_backend();
        let root = std::env::temp_dir().join(format!(
            "hantong-unbound-capture-{}-{}",
            std::process::id(),
            now().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let store = Store::open(root.join("data"), root.join("keys")).unwrap();
        let (state, _) = State::new();
        let kind = hantong_contracts::terminal_review::TerminalReviewKind::ImapSetup;
        assert_eq!(
            state.check_setup_capture(&store, kind, [7; 32], &peer(1)),
            Ok(())
        );
        assert_eq!(
            state.record_setup_capture(&store, kind, [7; 32], &peer(1)),
            Ok(())
        );
        assert!(state.inner.borrow().sessions.is_empty());
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn sec03_ready_status_requires_exact_next_epoch_and_selected_full_mail() {
        let (state, _) = State::new();
        let old = caller();
        let id = insert(&state, &old, peer(1), "ready", i64::MAX);
        assert_eq!(
            state.session_checked(id, &old, &peer(1)),
            Err(ErrorCode::ScopeDenied)
        );
        let mut current = old.clone();
        current.version += 1;
        current.profile_epoch += 1;
        current.scope = RequestedScope::new(vec!["acct".into()], full_mail_tools()).unwrap();
        assert_eq!(state.session_checked(id, &current, &peer(1)), Ok(()));
        current.profile_epoch += 1;
        assert_eq!(
            state.session_checked(id, &current, &peer(1)),
            Err(ErrorCode::ScopeDenied)
        );
    }
}
