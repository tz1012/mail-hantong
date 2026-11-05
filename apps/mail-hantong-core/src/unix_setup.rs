//! Unix setup routing for the existing Store-backed IMAP/SMTP lifecycle.
//!
//! This module admits setup intent and reports durable progress. Credential
//! capture and terminal review remain separate typed operations.

use hantong_contracts::platform::OpaqueSigner;
use hantong_contracts::{
    ErrorCode,
    imap_setup::ImapSetupCommand,
    scope_refresh::{ScopeRefreshCommand, ScopeRefreshProgress, ScopeRefreshReceipt},
    smtp_setup::SmtpSetupCommand,
    terminal_review::{TerminalReviewCommand, TerminalReviewKind, TerminalReviewReceipt},
};
use hantong_ipc_server::setup_dispatch::{SetupCommand, SetupReceipt};
use hantong_secrets::TerminalSecretTicket;
use hantong_store::Store;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    path::Path,
    rc::Rc,
};

struct ActionReview {
    kind: TerminalReviewKind,
    request_id: [u8; 32],
    owner: [u8; 16],
    epoch: u64,
    payload_hash: [u8; 32],
    expires_at_utc_ms: i64,
    peer: hantong_contracts::unix_auth::UnixPeerContextV2,
}

struct SecretJob {
    kind: TerminalReviewKind,
    request_id: [u8; 32],
    ticket: TerminalSecretTicket,
    handle: hantong_secrets::TerminalSecretHandle,
    binding: hantong_secrets::TerminalSecretBinding,
}

pub(crate) struct SecretReview {
    pub(crate) kind: TerminalReviewKind,
    pub(crate) request_id: [u8; 32],
    pub(crate) account_id: String,
    pub(crate) payload_hash: [u8; 32],
    pub(crate) ticket: TerminalSecretTicket,
    pub(crate) peer: hantong_contracts::unix_auth::UnixPeerContextV2,
    pub(crate) owner: [u8; 16],
}

pub(crate) struct State {
    pub(crate) google: crate::unix_google::State,
    pub(crate) smtp: crate::setup_smtp::Sessions,
    pub(crate) registration: crate::unix_registration::State,
    pub(crate) secret_reviews: RefCell<BTreeMap<[u8; 32], SecretReview>>,
    action_reviews: RefCell<BTreeMap<[u8; 32], ActionReview>>,
    action_jobs: RefCell<VecDeque<ActionReview>>,
    secret_jobs: RefCell<VecDeque<SecretJob>>,
    active_secret_job: Cell<Option<[u8; 32]>>,
    action_notify: Rc<tokio::sync::Notify>,
    pub(crate) policy: Rc<RefCell<hantong_policy::ReadPolicy>>,
}

impl State {
    pub(crate) fn new(google: crate::unix_google::State) -> Self {
        Self {
            google,
            smtp: RefCell::new(BTreeMap::new()),
            registration: crate::unix_registration::State::new(),
            secret_reviews: RefCell::new(BTreeMap::new()),
            action_reviews: RefCell::new(BTreeMap::new()),
            action_jobs: RefCell::new(VecDeque::new()),
            secret_jobs: RefCell::new(VecDeque::new()),
            active_secret_job: Cell::new(None),
            action_notify: Rc::new(tokio::sync::Notify::new()),
            policy: Rc::new(RefCell::new(hantong_policy::ReadPolicy::new())),
        }
    }

    /// Removes a validated Unix setup review capture for the async provider
    /// completion path. The secret ticket remains opaque to this module's
    /// callers and is consumed exactly once.
    pub(crate) fn take_secret_review(&self, id: [u8; 32]) -> Option<SecretReview> {
        self.secret_reviews.borrow_mut().remove(&id)
    }

    fn has_secret_job(&self, id: [u8; 32]) -> bool {
        self.active_secret_job.get() == Some(id)
            || self
                .secret_jobs
                .borrow()
                .iter()
                .any(|job| job.request_id == id)
    }

    fn has_secret_review(&self, id: [u8; 32]) -> bool {
        self.secret_reviews
            .borrow()
            .values()
            .any(|review| review.request_id == id)
    }

    fn has_any_secret_work(&self) -> bool {
        self.active_secret_job.get().is_some()
            || !self.secret_jobs.borrow().is_empty()
            || !self.secret_reviews.borrow().is_empty()
    }

    fn check_secret_review_slot(&self, id: [u8; 32]) -> Result<(), ErrorCode> {
        if self.secret_reviews.borrow().len() >= 64
            || self.has_secret_review(id)
            || self.has_secret_job(id)
        {
            Err(ErrorCode::RateLimited)
        } else {
            Ok(())
        }
    }

    /// Moves one confirmed send/trash operation into the Core-owned worker
    /// queue. The client connection is not part of the job lifetime.
    pub(crate) async fn run_next_action_job(
        &self,
        store: &Store,
        secrets_root: &Path,
    ) -> Option<Result<(), ErrorCode>> {
        let review = self.action_jobs.borrow_mut().pop_front()?;
        let result = match review.kind {
            TerminalReviewKind::ImapTrash => {
                crate::imap_trash::run_job(store, secrets_root, &self.policy, review.request_id)
                    .await
            }
            TerminalReviewKind::GoogleMutation => {
                crate::google_mutation::run_job(
                    store,
                    secrets_root,
                    &self.policy,
                    review.request_id,
                    true,
                )
                .await
            }
            TerminalReviewKind::PersonalSend => {
                crate::send_mail::run_job(
                    store,
                    secrets_root,
                    &self.policy,
                    review.request_id,
                    true,
                    true,
                )
                .await
            }
            TerminalReviewKind::ProtectedSend => {
                crate::send_mail::run_job(
                    store,
                    secrets_root,
                    &self.policy,
                    review.request_id,
                    true,
                    false,
                )
                .await
            }
            _ => Err(ErrorCode::CapabilityUnsupported),
        };
        Some(result)
    }

    async fn run_next_secret_job(
        &self,
        store: &Store,
        secrets_root: &Path,
    ) -> Option<Result<(), ErrorCode>> {
        let job = self.secret_jobs.borrow_mut().pop_front()?;
        self.active_secret_job.set(Some(job.request_id));
        let result = match job.kind {
            TerminalReviewKind::ImapSetup => {
                crate::setup_imap::terminal_submit(
                    store,
                    secrets_root,
                    job.request_id,
                    job.ticket,
                    job.handle,
                    job.binding,
                )
                .await
            }
            TerminalReviewKind::SmtpSetup => {
                crate::setup_smtp::terminal_submit(
                    store,
                    secrets_root,
                    &self.smtp,
                    job.request_id,
                    job.ticket,
                    job.handle,
                    job.binding,
                )
                .await
            }
            _ => Err(ErrorCode::CapabilityUnsupported),
        };
        self.active_secret_job.set(None);
        if let Err(error) = result {
            report_secret_submit("secret_job_execute", "job_failed", error);
        }
        Some(result)
    }

    /// Runs the Core-owned send/trash queue for the lifetime of the primary
    /// runtime. A client disconnect cannot cancel a provider operation after
    /// its Store request has been claimed.
    pub(crate) async fn run_action_worker(
        &self,
        store: &Store,
        secrets_root: &Path,
    ) -> Result<(), ErrorCode> {
        loop {
            if self
                .run_next_action_job(store, secrets_root)
                .await
                .is_none()
                && self
                    .run_next_secret_job(store, secrets_root)
                    .await
                    .is_none()
            {
                self.action_notify.notified().await;
            }
        }
    }

    fn prune_expired_reviews(&self, now: i64) {
        self.action_reviews
            .borrow_mut()
            .retain(|_, review| review.expires_at_utc_ms > now);
        self.secret_reviews
            .borrow_mut()
            .retain(|_, review| review.ticket.binding().expires_at_utc_ms() > now);
    }
}

/// Resolves the persisted owner before dispatching a foreground review. The
/// relay profile is not an authority claim; pending review data remains the
/// source of truth for send/trash/setup operations.
pub(crate) fn review_owner(
    state: &State,
    store: &Store,
    command: &SetupCommand,
    default_owner: [u8; 16],
) -> Result<[u8; 16], ErrorCode> {
    let SetupCommand::TerminalReview(command) = command else {
        return Ok(default_owner);
    };
    match command {
        TerminalReviewCommand::Get(get) => match get.kind {
            TerminalReviewKind::Registration => {
                // A fresh registration has no owner registration yet.  Bind
                // the review to its deterministic request-derived identity;
                // Confirm then uses the in-memory ticket's exact owner.
                hantong_platform::registration_id_for_request(get.request_id)
            }
            TerminalReviewKind::ImapTrash => Ok(store
                .pending_imap_trash(get.request_id)?
                .owner_registration_id),
            TerminalReviewKind::PersonalSend | TerminalReviewKind::ProtectedSend => Ok(store
                .pending_mail_send(get.request_id)?
                .owner_registration_id),
            TerminalReviewKind::GoogleMutation => Ok(store
                .pending_google_mutation(get.request_id)?
                .owner_registration_id),
            TerminalReviewKind::ImapSetup | TerminalReviewKind::SmtpSetup => {
                hantong_platform::registration_id_for_request(get.request_id)
            }
            _ => Ok(default_owner),
        },
        TerminalReviewCommand::Confirm(confirm) => {
            if let Some(owner) = state.registration.ticket_owner(confirm.ticket) {
                return Ok(owner);
            }
            if let Some(review) = state.action_reviews.borrow().get(&confirm.ticket) {
                return Ok(review.owner);
            }
            if let Some(review) = state.secret_reviews.borrow().get(&confirm.ticket) {
                return Ok(review.owner);
            }
            Ok(default_owner)
        }
        TerminalReviewCommand::Cancel(cancel) => {
            if let Some(owner) = state.registration.ticket_owner(cancel.ticket) {
                return Ok(owner);
            }
            if let Some(review) = state.action_reviews.borrow().get(&cancel.ticket) {
                return Ok(review.owner);
            }
            if let Some(review) = state.secret_reviews.borrow().get(&cancel.ticket) {
                return Ok(review.owner);
            }
            Ok(default_owner)
        }
        TerminalReviewCommand::SecretSubmit(submit) => state
            .secret_reviews
            .borrow()
            .get(&submit.ticket)
            .map(|review| review.owner)
            .ok_or(ErrorCode::NotFound),
    }
}

fn check_mcp_action_owner(
    store: &Store,
    owner: [u8; 16],
    epoch: u64,
    account: &str,
) -> Result<(), ErrorCode> {
    let registration = store
        .load_registration(owner)?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    if registration.state != hantong_store::RegistrationState::Active
        || registration.profile_epoch != epoch
    {
        return Err(ErrorCode::PolicyBlocked);
    }
    if registration.client_kind == hantong_store::ClientKind::Mcp {
        store.check_mcp_ai_consent(owner, epoch, &[account.to_owned()])?;
    }
    Ok(())
}

pub(crate) fn dispatch(
    state: &State,
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    owner: [u8; 16],
    peer: hantong_contracts::unix_auth::UnixPeerContextV2,
    command: SetupCommand,
) -> Result<SetupReceipt, ErrorCode> {
    match command {
        SetupCommand::Imap(command) => match command {
            ImapSetupCommand::Request(input) => {
                let (request, fresh) = crate::setup_imap::prepare_request(store, &input)?;
                imap_status(state, store, *request.intent().request_id(), fresh)
                    .map(SetupReceipt::Imap)
            }
            ImapSetupCommand::Resume(id) => {
                let parent = store.resolve_imap_setup_leaf(id)?;
                let id = *parent.intent().request_id();
                if state.has_secret_job(id)
                    || state.has_secret_review(id)
                    || parent.events().last().is_some_and(|event| {
                        matches!(
                            event.event(),
                            hantong_store::imap_setup::ImapSetupEvent::ProbeFinished(
                                hantong_store::account_provisioning::InitialImapProbeOutcome::Ready(
                                    _
                                )
                            )
                        )
                    })
                {
                    return imap_status(state, store, id, false).map(SetupReceipt::Imap);
                }
                if state.has_any_secret_work() {
                    return Err(ErrorCode::RateLimited);
                }
                if parent.phase() == hantong_store::imap_setup::ImapSetupPhase::ReviewPending
                    && parent.intent().resume_binding().is_some()
                    && now_ms()? < parent.expires_at_utc_ms()
                {
                    return imap_status(state, store, id, true).map(SetupReceipt::Imap);
                }
                let child = crate::setup_imap::prepare_resume(store, &parent)?;
                imap_status(state, store, *child.intent().request_id(), true)
                    .map(SetupReceipt::Imap)
            }
            ImapSetupCommand::Status(id) => {
                imap_status(state, store, id, false).map(SetupReceipt::Imap)
            }
        },
        SetupCommand::Smtp(command) => match command {
            SmtpSetupCommand::Request(input) => {
                crate::setup_smtp::request(store, secrets_root, &state.smtp, input)
                    .map(SetupReceipt::Smtp)
            }
            SmtpSetupCommand::Status(id) => {
                crate::setup_smtp::status(&state.smtp, id, true).map(SetupReceipt::Smtp)
            }
        },
        SetupCommand::Registration(command) => {
            crate::unix_registration::registration(store, command)
        }
        SetupCommand::TerminalReview(command) => {
            terminal_review_sync(state, store, secrets_root, server, owner, peer, command)
        }
        SetupCommand::Google(_) => Err(ErrorCode::CapabilityUnsupported),
        SetupCommand::ScopeRefresh(command) => scope_refresh_request(store, server, command),
    }
}

fn imap_status(
    state: &State,
    store: &Store,
    id: [u8; 32],
    fresh: bool,
) -> Result<hantong_contracts::imap_setup::ImapSetupReceipt, ErrorCode> {
    use hantong_contracts::imap_setup::{ImapSetupNextAction, ImapSetupReceipt, ImapSetupState};
    let request = crate::setup_imap::load(store, id)?;
    if state.has_secret_job(id)
        && request.phase() == hantong_store::imap_setup::ImapSetupPhase::ReviewPending
    {
        return ImapSetupReceipt::new(
            id,
            request.expires_at_utc_ms(),
            ImapSetupState::Connecting,
            ImapSetupNextAction::Wait,
        )?
        .with_account_id(request.intent().draft().account_id().to_owned());
    }
    let reviewing = state.has_secret_review(id);
    crate::setup_imap::status(store, id, fresh || reviewing || state.has_secret_job(id))
}

fn scope_refresh_request(
    store: &Store,
    server: &mut impl OpaqueSigner,
    command: ScopeRefreshCommand,
) -> Result<SetupReceipt, ErrorCode> {
    match command {
        ScopeRefreshCommand::Status(id) => {
            let refresh = store
                .load_cli_scope_refresh(id)?
                .ok_or(ErrorCode::NotFound)?;
            let state = match refresh.phase() {
                hantong_store::scope_refresh::ScopeRefreshPhase::Pending => {
                    ScopeRefreshProgress::Pending
                }
                hantong_store::scope_refresh::ScopeRefreshPhase::Staged => {
                    ScopeRefreshProgress::Provisioning
                }
                hantong_store::scope_refresh::ScopeRefreshPhase::Active => {
                    ScopeRefreshProgress::Ready
                }
                hantong_store::scope_refresh::ScopeRefreshPhase::Denied => {
                    ScopeRefreshProgress::Denied
                }
                hantong_store::scope_refresh::ScopeRefreshPhase::Expired => {
                    ScopeRefreshProgress::Expired
                }
            };
            Ok(SetupReceipt::ScopeRefresh(ScopeRefreshReceipt::new(
                *refresh.refresh_id(),
                refresh.expires_at_utc_ms(),
                state,
            )?))
        }
        command @ (ScopeRefreshCommand::Request
        | ScopeRefreshCommand::RequestManage(_)
        | ScopeRefreshCommand::RequestGoogleSend(_)
        | ScopeRefreshCommand::RequestGoogleChanges(_)
        | ScopeRefreshCommand::RequestGoogleReadOnly(_)
        | ScopeRefreshCommand::AddGoogleReadOnly(_)) => {
            let refresh = crate::unix_scope_refresh::prepare(store, server, command)?;
            Ok(SetupReceipt::ScopeRefresh(ScopeRefreshReceipt::new(
                *refresh.refresh_id(),
                refresh.expires_at_utc_ms(),
                ScopeRefreshProgress::Pending,
            )?))
        }
    }
}

fn terminal_review_sync(
    state: &State,
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    owner: [u8; 16],
    peer: hantong_contracts::unix_auth::UnixPeerContextV2,
    command: TerminalReviewCommand,
) -> Result<SetupReceipt, ErrorCode> {
    crate::unix_registration::terminal_review(
        &state.registration,
        store,
        secrets_root,
        server,
        owner,
        peer.clone(),
        command.clone(),
    )
    .or_else(|error| {
        if error != ErrorCode::CapabilityUnsupported {
            return Err(error);
        }
        terminal_secret_get(
            state,
            store,
            secrets_root,
            server,
            owner,
            peer.clone(),
            command,
        )
    })
}

async fn action_review(
    state: &State,
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    owner: [u8; 16],
    peer: hantong_contracts::unix_auth::UnixPeerContextV2,
    command: TerminalReviewCommand,
) -> Result<SetupReceipt, ErrorCode> {
    use hantong_contracts::terminal_review::{TerminalReviewCancel, TerminalReviewConfirm};
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ErrorCode::InternalError)?
        .as_millis() as i64;
    match command {
        TerminalReviewCommand::Get(get) => {
            if state.action_reviews.borrow().len() >= 64 {
                return Err(ErrorCode::RateLimited);
            }
            let (_account_id, payload_hash, expires, binding_owner, binding_epoch, fields) =
                match get.kind {
                    TerminalReviewKind::ImapTrash => {
                        let pending = store.pending_imap_trash(get.request_id)?;
                        check_mcp_action_owner(
                            store,
                            pending.owner_registration_id,
                            pending.profile_epoch,
                            pending.request.account_id(),
                        )?;
                        if get
                            .account_id
                            .as_deref()
                            .is_some_and(|v| v != pending.request.account_id())
                            || (get.payload_hash != [0; 32]
                                && get.payload_hash != *pending.request.payload_hash())
                            || !store.can_confirm_imap_trash(&pending)?
                        {
                            return Err(ErrorCode::PolicyBlocked);
                        }
                        (
                            pending.request.account_id().to_owned(),
                            *pending.request.payload_hash(),
                            pending.expires_at_utc_ms,
                            pending.owner_registration_id,
                            pending.profile_epoch,
                            vec![
                                crate::terminal_review::field(
                                    "account",
                                    pending.request.account_id().into(),
                                )?,
                                crate::terminal_review::field("action", "move_to_trash".into())?,
                                crate::terminal_review::field(
                                    "uid",
                                    pending.request.uid().to_string(),
                                )?,
                            ],
                        )
                    }
                    TerminalReviewKind::GoogleMutation => {
                        let pending = store.pending_google_mutation(get.request_id)?;
                        check_mcp_action_owner(
                            store,
                            pending.owner_registration_id,
                            pending.profile_epoch,
                            pending.request.account_id(),
                        )?;
                        if get
                            .account_id
                            .as_deref()
                            .is_some_and(|v| v != pending.request.account_id())
                            || (get.payload_hash != [0; 32]
                                && get.payload_hash != *pending.request.payload_hash())
                        {
                            return Err(ErrorCode::PolicyBlocked);
                        }
                        (
                            pending.request.account_id().to_owned(),
                            pending.request.payload_hash().to_owned(),
                            pending.expires_at_utc_ms,
                            pending.owner_registration_id,
                            pending.profile_epoch,
                            vec![
                                crate::terminal_review::field(
                                    "account",
                                    pending.request.account_id().to_owned(),
                                )?,
                                crate::terminal_review::field("action", "gmail_mutation".into())?,
                            ],
                        )
                    }
                    TerminalReviewKind::PersonalSend | TerminalReviewKind::ProtectedSend => {
                        let pending = store.pending_mail_send(get.request_id)?;
                        check_mcp_action_owner(
                            store,
                            pending.owner_registration_id,
                            pending.profile_epoch,
                            pending.request.account_id(),
                        )?;
                        let personal = get.kind == TerminalReviewKind::PersonalSend;
                        if get
                            .account_id
                            .as_deref()
                            .is_some_and(|v| v != pending.request.account_id())
                            || (get.payload_hash != [0; 32]
                                && get.payload_hash != *pending.request.payload_hash())
                            || (personal != store.can_confirm_personal_mail_send(&pending)?)
                        {
                            return Err(ErrorCode::PolicyBlocked);
                        }
                        let preview = crate::send_mail::preview(&pending)?;
                        let fields = ["from", "to", "subject", "body", "attachments"]
                            .into_iter()
                            .filter_map(|key| preview.get(key).map(|value| (key, value)))
                            .map(|(key, value)| {
                                let text = if matches!(key, "from" | "subject" | "body") {
                                    value.as_str().unwrap_or_default().to_owned()
                                } else {
                                    serde_json::to_string(value).unwrap_or_default()
                                };
                                crate::terminal_review::field(key, text)
                            })
                            .collect::<Result<Vec<_>, ErrorCode>>()?;
                        (
                            pending.request.account_id().to_owned(),
                            *pending.request.payload_hash(),
                            pending.expires_at_utc_ms,
                            pending.owner_registration_id,
                            pending.profile_epoch,
                            fields,
                        )
                    }
                    TerminalReviewKind::ScopeRefresh => {
                        let refresh = store
                            .load_cli_scope_refresh(get.request_id)?
                            .ok_or(ErrorCode::NotFound)?;
                        if refresh.phase()
                            != hantong_store::scope_refresh::ScopeRefreshPhase::Pending
                            || refresh.decision_channel() != "terminal_cli"
                            || (get.payload_hash != [0; 32]
                                && get.payload_hash != *refresh.payload_hash())
                            || refresh.base_registration().registration_id != owner
                        {
                            return Err(ErrorCode::PolicyBlocked);
                        }
                        (
                            refresh.target_accounts().join(","),
                            *refresh.payload_hash(),
                            refresh.expires_at_utc_ms(),
                            refresh.base_registration().registration_id,
                            refresh.base_registration().profile_epoch,
                            vec![
                                crate::terminal_review::field(
                                    "accounts",
                                    refresh.target_accounts().join(", "),
                                )?,
                                crate::terminal_review::field(
                                    "tools",
                                    refresh.target_tools().join(", "),
                                )?,
                            ],
                        )
                    }
                    _ => return Err(ErrorCode::CapabilityUnsupported),
                };
            if expires <= now {
                return Err(ErrorCode::ApprovalExpired);
            }
            if binding_owner != owner {
                return Err(ErrorCode::PolicyBlocked);
            }
            let ticket = hantong_platform::random_nonce()?;
            state.action_reviews.borrow_mut().insert(
                ticket,
                ActionReview {
                    kind: get.kind,
                    request_id: get.request_id,
                    owner: binding_owner,
                    epoch: binding_epoch,
                    payload_hash,
                    expires_at_utc_ms: expires,
                    peer: peer.clone(),
                },
            );
            let receipt = TerminalReviewReceipt {
                kind: get.kind,
                request_id: get.request_id,
                ticket: Some(ticket),
                snapshot_hash: payload_hash,
                owner_registration_id: binding_owner,
                profile_epoch: binding_epoch,
                expires_at_utc_ms: expires,
                state: "pending".into(),
                secret_ticket_opaque: None,
                fields,
            };
            receipt.validate().map_err(|_| ErrorCode::InternalError)?;
            Ok(SetupReceipt::TerminalReview(receipt))
        }
        TerminalReviewCommand::Confirm(TerminalReviewConfirm {
            ticket,
            snapshot_hash,
        }) => {
            let valid = state
                .action_reviews
                .borrow()
                .get(&ticket)
                .is_some_and(|review| {
                    review.owner == owner
                        && review.peer == peer
                        && review.payload_hash == snapshot_hash
                        && review.expires_at_utc_ms > now
                });
            if !valid {
                return Err(ErrorCode::PolicyBlocked);
            }
            let queued_action = state
                .action_reviews
                .borrow()
                .get(&ticket)
                .is_some_and(|review| {
                    matches!(
                        review.kind,
                        TerminalReviewKind::PersonalSend
                            | TerminalReviewKind::ProtectedSend
                            | TerminalReviewKind::ImapTrash
                            | TerminalReviewKind::GoogleMutation
                    )
                });
            if queued_action && state.action_jobs.borrow().len() >= 16 {
                return Err(ErrorCode::RateLimited);
            }
            {
                let reviews = state.action_reviews.borrow();
                let review = reviews.get(&ticket).ok_or(ErrorCode::Conflict)?;
                match review.kind {
                    TerminalReviewKind::ImapTrash => {
                        let pending = store.pending_imap_trash(review.request_id)?;
                        check_mcp_action_owner(
                            store,
                            pending.owner_registration_id,
                            pending.profile_epoch,
                            pending.request.account_id(),
                        )?;
                    }
                    TerminalReviewKind::GoogleMutation => {
                        let pending = store.pending_google_mutation(review.request_id)?;
                        check_mcp_action_owner(
                            store,
                            pending.owner_registration_id,
                            pending.profile_epoch,
                            pending.request.account_id(),
                        )?;
                    }
                    TerminalReviewKind::PersonalSend | TerminalReviewKind::ProtectedSend => {
                        let pending = store.pending_mail_send(review.request_id)?;
                        check_mcp_action_owner(
                            store,
                            pending.owner_registration_id,
                            pending.profile_epoch,
                            pending.request.account_id(),
                        )?;
                    }
                    _ => {}
                }
            }
            let review = state
                .action_reviews
                .borrow_mut()
                .remove(&ticket)
                .ok_or(ErrorCode::Conflict)?;
            if matches!(
                review.kind,
                TerminalReviewKind::PersonalSend
                    | TerminalReviewKind::ProtectedSend
                    | TerminalReviewKind::ImapTrash
                    | TerminalReviewKind::GoogleMutation
            ) {
                let receipt = TerminalReviewReceipt {
                    kind: review.kind,
                    request_id: review.request_id,
                    ticket: None,
                    snapshot_hash: review.payload_hash,
                    owner_registration_id: review.owner,
                    profile_epoch: review.epoch,
                    expires_at_utc_ms: review.expires_at_utc_ms,
                    state: "queued".into(),
                    secret_ticket_opaque: None,
                    fields: Vec::new(),
                };
                receipt.validate().map_err(|_| ErrorCode::InternalError)?;
                state.action_jobs.borrow_mut().push_back(review);
                state.action_notify.notify_one();
                return Ok(SetupReceipt::TerminalReview(receipt));
            }
            let result = match review.kind {
                TerminalReviewKind::ScopeRefresh => {
                    crate::unix_registration::activate_scope_refresh(
                        store,
                        secrets_root,
                        server,
                        review.request_id,
                    )
                    .map(|_| ())
                }
                _ => Err(ErrorCode::CapabilityUnsupported),
            };
            let state_name = if result.is_ok() { "ready" } else { "unknown" };
            let receipt = TerminalReviewReceipt {
                kind: review.kind,
                request_id: review.request_id,
                ticket: None,
                snapshot_hash: review.payload_hash,
                owner_registration_id: review.owner,
                profile_epoch: review.epoch,
                expires_at_utc_ms: review.expires_at_utc_ms,
                state: state_name.into(),
                secret_ticket_opaque: None,
                fields: Vec::new(),
            };
            result.map(|_| SetupReceipt::TerminalReview(receipt))
        }
        TerminalReviewCommand::Cancel(TerminalReviewCancel { ticket, .. }) => {
            let review = state
                .action_reviews
                .borrow_mut()
                .remove(&ticket)
                .ok_or(ErrorCode::NotFound)?;
            if review.owner != owner || review.peer != peer {
                return Err(ErrorCode::PolicyBlocked);
            }
            match review.kind {
                TerminalReviewKind::ImapTrash => {
                    store.deny_imap_trash(review.request_id, ErrorCode::ApprovalDenied)?
                }
                TerminalReviewKind::PersonalSend | TerminalReviewKind::ProtectedSend => {
                    store.deny_mail_send(review.request_id, ErrorCode::ApprovalDenied)?
                }
                TerminalReviewKind::GoogleMutation => {
                    store.deny_google_mutation(review.request_id, ErrorCode::ApprovalDenied)?
                }
                _ => return Err(ErrorCode::CapabilityUnsupported),
            }
            Ok(SetupReceipt::TerminalReview(TerminalReviewReceipt {
                kind: review.kind,
                request_id: review.request_id,
                ticket: None,
                snapshot_hash: review.payload_hash,
                owner_registration_id: review.owner,
                profile_epoch: review.epoch,
                expires_at_utc_ms: review.expires_at_utc_ms,
                state: "denied".into(),
                secret_ticket_opaque: None,
                fields: Vec::new(),
            }))
        }
        TerminalReviewCommand::SecretSubmit(_) => Err(ErrorCode::CapabilityUnsupported),
    }
}

/// Async Unix setup entry point used by the Core runtime. Ordinary bootstrap
/// commands retain the synchronous validation path; secret submission is
/// completed here so the existing provider probe/Store transitions run on the
/// runtime without blocking it.
pub(crate) async fn dispatch_async(
    state: &State,
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    owner: [u8; 16],
    peer: hantong_contracts::unix_auth::UnixPeerContextV2,
    command: SetupCommand,
) -> Result<SetupReceipt, ErrorCode> {
    state.prune_expired_reviews(now_ms()?);
    match command {
        SetupCommand::Google(hantong_contracts::google_setup::GoogleSetupCommand::Request(
            input,
        )) => state
            .google
            .request(store, secrets_root, peer, input)
            .await
            .map(SetupReceipt::Google),
        SetupCommand::Google(hantong_contracts::google_setup::GoogleSetupCommand::Status(id)) => {
            state.google.status(id, &peer).map(SetupReceipt::Google)
        }

        SetupCommand::TerminalReview(TerminalReviewCommand::Get(get))
            if matches!(
                get.kind,
                TerminalReviewKind::PersonalSend
                    | TerminalReviewKind::ProtectedSend
                    | TerminalReviewKind::ImapTrash
                    | TerminalReviewKind::GoogleMutation
                    | TerminalReviewKind::ScopeRefresh
            ) =>
        {
            action_review(
                state,
                store,
                secrets_root,
                server,
                owner,
                peer.clone(),
                TerminalReviewCommand::Get(get),
            )
            .await
        }
        SetupCommand::TerminalReview(TerminalReviewCommand::Confirm(confirm))
            if state.action_reviews.borrow().contains_key(&confirm.ticket) =>
        {
            action_review(
                state,
                store,
                secrets_root,
                server,
                owner,
                peer.clone(),
                TerminalReviewCommand::Confirm(confirm),
            )
            .await
        }
        SetupCommand::TerminalReview(TerminalReviewCommand::Cancel(cancel))
            if state.action_reviews.borrow().contains_key(&cancel.ticket) =>
        {
            action_review(
                state,
                store,
                secrets_root,
                server,
                owner,
                peer.clone(),
                TerminalReviewCommand::Cancel(cancel),
            )
            .await
        }
        SetupCommand::TerminalReview(TerminalReviewCommand::Cancel(cancel))
            if state.secret_reviews.borrow().contains_key(&cancel.ticket) =>
        {
            cancel_secret_review(state, store, owner, peer.clone(), cancel.ticket)
        }
        SetupCommand::TerminalReview(
            hantong_contracts::terminal_review::TerminalReviewCommand::SecretSubmit(request),
        ) => submit_secret(state, secrets_root, owner, peer.clone(), request).await,
        command => dispatch(state, store, secrets_root, server, owner, peer, command),
    }
}

fn cancel_secret_review(
    state: &State,
    store: &Store,
    owner: [u8; 16],
    peer: hantong_contracts::unix_auth::UnixPeerContextV2,
    ticket: [u8; 32],
) -> Result<SetupReceipt, ErrorCode> {
    let valid = state
        .secret_reviews
        .borrow()
        .get(&ticket)
        .map(|review| review.owner == owner && review.peer == peer)
        .ok_or(ErrorCode::NotFound)?;
    if !valid {
        return Err(ErrorCode::PolicyBlocked);
    }
    let review = state
        .take_secret_review(ticket)
        .ok_or(ErrorCode::Conflict)?;
    match review.kind {
        TerminalReviewKind::ImapSetup => {
            let setup = crate::setup_imap::load(store, review.request_id)?;
            store.close_imap_setup_review(&setup, ErrorCode::ApprovalDenied)?;
        }
        TerminalReviewKind::SmtpSetup => {
            crate::setup_smtp::cancel(&state.smtp, review.request_id)?;
        }
        _ => return Err(ErrorCode::CapabilityUnsupported),
    }
    Ok(SetupReceipt::TerminalReview(TerminalReviewReceipt {
        kind: review.kind,
        request_id: review.request_id,
        ticket: None,
        snapshot_hash: review.payload_hash,
        owner_registration_id: review.owner,
        profile_epoch: 1,
        expires_at_utc_ms: review.ticket.binding().expires_at_utc_ms(),
        state: "denied".into(),
        secret_ticket_opaque: None,
        fields: Vec::new(),
    }))
}

async fn submit_secret(
    state: &State,
    secrets_root: &Path,
    owner: [u8; 16],
    peer: hantong_contracts::unix_auth::UnixPeerContextV2,
    request: hantong_contracts::terminal_review::TerminalReviewSecretSubmit,
) -> Result<SetupReceipt, ErrorCode> {
    submit_secret_with(state, owner, peer, request, |ticket, handle| {
        let secrets = hantong_secrets::SecretStore::open(secrets_root)
            .map_err(|error| secret_submit_failure("store_open", error))?;
        secrets
            .terminal_secret_handle_from_opaque(ticket, handle)
            .map_err(|error| secret_submit_failure("stage_handle_open", error))
    })
}

fn secret_submit_failure(stage: &'static str, error: hantong_secrets::SecretError) -> ErrorCode {
    use hantong_secrets::SecretError;
    let (kind, code) = match error {
        SecretError::InputExpired => ("expired", ErrorCode::ApprovalExpired),
        SecretError::InvalidRoot => ("invalid_root", ErrorCode::InternalError),
        SecretError::UnsupportedPlatform => {
            ("platform_unavailable", ErrorCode::CapabilityUnsupported)
        }
        SecretError::Io => ("io", ErrorCode::InternalError),
        SecretError::Protection => ("protection", ErrorCode::InternalError),
        SecretError::NotFound => ("not_found", ErrorCode::NotFound),
        SecretError::CredentialMismatch | SecretError::InvalidCiphertext => {
            ("binding_mismatch", ErrorCode::ScopeDenied)
        }
        SecretError::InvalidInputWindow => ("tty_unavailable", ErrorCode::HumanStepRequired),
        SecretError::InvalidCredential
        | SecretError::InvalidAccountId
        | SecretError::EmptySecret
        | SecretError::SecretTooLarge => ("invalid_input", ErrorCode::InvalidInput),
    };
    report_secret_submit(stage, kind, code);
    code
}

fn report_secret_submit(stage: &'static str, kind: &'static str, code: ErrorCode) {
    use std::io::Write;
    // Only call-site constants and closed error labels; never backend text or identifiers.
    let _ = writeln!(
        std::io::stderr().lock(),
        "hantong_terminal_secret stage={stage} kind={kind} code={}",
        code.as_str()
    );
}

fn submit_secret_with(
    state: &State,
    owner: [u8; 16],
    peer: hantong_contracts::unix_auth::UnixPeerContextV2,
    request: hantong_contracts::terminal_review::TerminalReviewSecretSubmit,
    open_handle: impl FnOnce(
        &TerminalSecretTicket,
        [u8; 32],
    ) -> Result<hantong_secrets::TerminalSecretHandle, ErrorCode>,
) -> Result<SetupReceipt, ErrorCode> {
    if state.secret_jobs.borrow().len() >= 16 {
        report_secret_submit("secret_job_submit", "queue_full", ErrorCode::RateLimited);
        return Err(ErrorCode::RateLimited);
    }
    let (kind, request_id, account_id, payload_hash, expected_binding, expected_opaque_id) = {
        let reviews = state.secret_reviews.borrow();
        let review = reviews.get(&request.ticket).ok_or(ErrorCode::NotFound)?;
        if review.owner != owner
            || review.peer != peer
            || review.payload_hash != request.snapshot_hash
            || review.ticket.to_opaque().as_slice() != request.secret_ticket_opaque.as_slice()
        {
            return Err(ErrorCode::Conflict);
        }
        (
            review.kind,
            review.request_id,
            review.account_id.clone(),
            review.payload_hash,
            review.ticket.binding(),
            review.ticket.opaque_id(),
        )
    };
    if state.has_secret_job(request_id) {
        report_secret_submit("secret_job_submit", "duplicate_job", ErrorCode::Conflict);
        return Err(ErrorCode::Conflict);
    }
    if expected_binding.is_expired() {
        report_secret_submit("secret_job_submit", "expired", ErrorCode::ApprovalExpired);
        return Err(ErrorCode::ApprovalExpired);
    }
    let opaque = TerminalSecretTicket::from_opaque(&request.secret_ticket_opaque)
        .map_err(|_| ErrorCode::InvalidInput)?;
    if opaque.binding() != expected_binding || opaque.opaque_id() != expected_opaque_id {
        return Err(ErrorCode::Conflict);
    }
    // A failed open must leave the review intact and never enqueue a provider job.
    let handle = {
        let reviews = state.secret_reviews.borrow();
        let review = reviews.get(&request.ticket).ok_or(ErrorCode::Conflict)?;
        if review.ticket.is_expired() {
            report_secret_submit("secret_job_submit", "expired", ErrorCode::ApprovalExpired);
            return Err(ErrorCode::ApprovalExpired);
        }
        open_handle(&review.ticket, request.staged_handle)?
    };
    if expected_binding.is_expired() {
        report_secret_submit("secret_job_submit", "expired", ErrorCode::ApprovalExpired);
        return Err(ErrorCode::ApprovalExpired);
    }
    let receipt = TerminalReviewReceipt {
        kind,
        request_id,
        ticket: None,
        snapshot_hash: payload_hash,
        owner_registration_id: owner,
        profile_epoch: 1,
        expires_at_utc_ms: expected_binding.expires_at_utc_ms(),
        state: "provisioning".into(),
        secret_ticket_opaque: None,
        fields: vec![
            hantong_contracts::terminal_review::TerminalReviewField::new(
                "account".into(),
                account_id,
            )?,
        ],
    };
    receipt.validate().map_err(|_| ErrorCode::InternalError)?;
    let review = state
        .take_secret_review(request.ticket)
        .ok_or(ErrorCode::Conflict)?;
    state.secret_jobs.borrow_mut().push_back(SecretJob {
        kind,
        request_id,
        ticket: review.ticket,
        handle,
        binding: expected_binding,
    });
    state.action_notify.notify_one();
    Ok(SetupReceipt::TerminalReview(receipt))
}

fn terminal_secret_get(
    state: &State,
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    owner: [u8; 16],
    peer: hantong_contracts::unix_auth::UnixPeerContextV2,
    command: TerminalReviewCommand,
) -> Result<SetupReceipt, ErrorCode> {
    let TerminalReviewCommand::Get(get) = command else {
        return Err(ErrorCode::CapabilityUnsupported);
    };
    if !matches!(
        get.kind,
        TerminalReviewKind::ImapSetup | TerminalReviewKind::SmtpSetup
    ) {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    state.check_secret_review_slot(get.request_id)?;
    let (account, hash, version, expires, endpoint) = if get.kind == TerminalReviewKind::ImapSetup {
        if state.has_secret_job(get.request_id) {
            return Err(ErrorCode::RateLimited);
        }
        let setup = crate::setup_imap::load(store, get.request_id)?;
        let i = setup.intent();
        if setup.phase() != hantong_store::imap_setup::ImapSetupPhase::ReviewPending {
            return Err(ErrorCode::Conflict);
        }
        (
            i.draft().account_id().to_owned(),
            *i.review_payload_hash(),
            i.endpoint().settings_version(),
            setup.expires_at_utc_ms(),
            serde_json::to_value(i.endpoint()).map_err(|_| ErrorCode::InternalError)?,
        )
    } else {
        let context =
            crate::setup_smtp::validate_terminal_context(store, &state.smtp, get.request_id)?;
        (
            context.account_id,
            context.payload_hash,
            context.account_version,
            context.expires_at_utc_ms,
            serde_json::to_value(&context.endpoint).map_err(|_| ErrorCode::InternalError)?,
        )
    };
    if get.account_id.as_deref().is_some_and(|v| v != account)
        || (get.payload_hash != [0; 32] && get.payload_hash != hash)
    {
        return Err(ErrorCode::Conflict);
    }
    let kind = if get.kind == TerminalReviewKind::ImapSetup {
        *b"terminal-imap-v1"
    } else {
        *b"terminal-smtp-v1"
    };
    let endpoint_bytes = serde_json::to_vec(&endpoint).map_err(|_| ErrorCode::InternalError)?;
    let mut canonical = Vec::new();
    for value in [
        b"terminal-endpoint-v1".as_slice(),
        &kind,
        account.as_bytes(),
        &endpoint_bytes,
    ] {
        canonical.extend_from_slice(&(value.len() as u32).to_le_bytes());
        canonical.extend_from_slice(value);
    }
    let uid = peer.effective_uid.to_string();
    let session = peer.session_id.to_string();
    let binding = hantong_secrets::TerminalSecretBinding::new(
        kind,
        get.request_id,
        hash,
        hantong_platform::sha256(account.as_bytes())?,
        hantong_platform::sha256(&canonical)?,
        version,
        hantong_platform::sha256(uid.as_bytes())?,
        hantong_platform::sha256(session.as_bytes())?,
        hantong_platform::sha256(&server.public_key().map_err(|_| ErrorCode::InternalError)?)?,
        expires,
    )
    .map_err(|_| ErrorCode::InvalidInput)?;
    let ticket = hantong_secrets::SecretStore::open(secrets_root)
        .map_err(|_| ErrorCode::InternalError)?
        .issue_terminal_secret_ticket(binding)
        .map_err(|_| ErrorCode::InternalError)?;
    let secret_ticket_opaque = ticket.to_opaque().to_vec();
    let review_id = hantong_platform::random_nonce()?;
    state.secret_reviews.borrow_mut().insert(
        review_id,
        SecretReview {
            kind: get.kind,
            request_id: get.request_id,
            account_id: account.clone(),
            payload_hash: hash,
            ticket,
            peer,
            owner,
        },
    );
    let receipt = TerminalReviewReceipt {
        kind: get.kind,
        request_id: get.request_id,
        ticket: Some(review_id),
        snapshot_hash: hash,
        owner_registration_id: owner,
        profile_epoch: 1,
        expires_at_utc_ms: expires,
        state: "pending".into(),
        secret_ticket_opaque: Some(secret_ticket_opaque),
        fields: vec![
            hantong_contracts::terminal_review::TerminalReviewField::new(
                "account".into(),
                account,
            )?,
            hantong_contracts::terminal_review::TerminalReviewField::new(
                "endpoint".into(),
                endpoint.to_string(),
            )?,
        ],
    };
    receipt.validate().map_err(|_| ErrorCode::InternalError)?;
    Ok(SetupReceipt::TerminalReview(receipt))
}

fn now_ms() -> Result<i64, ErrorCode> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ErrorCode::InternalError)?
        .as_millis() as i64)
}

#[cfg(test)]
mod secret_submit_tests {
    use super::*;
    use hantong_contracts::terminal_review::TerminalReviewSecretSubmit;

    fn fixture() -> (
        State,
        hantong_contracts::unix_auth::UnixPeerContextV2,
        TerminalReviewSecretSubmit,
    ) {
        // Public opaque format only: synthetic metadata, no credential or host keyring access.
        use std::hash::{Hash, Hasher};
        let mut bytes = vec![1; 312];
        bytes[144..152].copy_from_slice(&1_u64.to_be_bytes());
        bytes[248..256].copy_from_slice(&i64::MAX.to_be_bytes());
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.as_slice().hash(&mut hasher);
        bytes.extend_from_slice(&hasher.finish().to_be_bytes());
        let ticket = TerminalSecretTicket::from_opaque(&bytes).unwrap();
        let peer = hantong_platform::unix_peer::current_peer_context().unwrap();
        let (google, _) = crate::unix_google::State::new();
        let state = State::new(google);
        let request =
            TerminalReviewSecretSubmit::new([2; 32], [1; 32], bytes, ticket.opaque_id()).unwrap();
        state.secret_reviews.borrow_mut().insert(
            [2; 32],
            SecretReview {
                kind: TerminalReviewKind::ImapSetup,
                request_id: [1; 32],
                account_id: "synthetic-account".into(),
                payload_hash: [1; 32],
                ticket,
                peer: peer.clone(),
                owner: [1; 16],
            },
        );
        (state, peer, request)
    }

    #[test]
    fn sec08_failed_store_or_stage_open_preserves_review_and_never_queues() {
        for error in [
            ErrorCode::InternalError,
            ErrorCode::CapabilityUnsupported,
            ErrorCode::NotFound,
            ErrorCode::ScopeDenied,
        ] {
            let (state, peer, request) = fixture();
            let result = submit_secret_with(&state, [1; 16], peer, request, |_, _| Err(error));
            assert!(matches!(result, Err(actual) if actual == error));
            assert_eq!(state.secret_reviews.borrow().len(), 1);
            assert!(state.secret_jobs.borrow().is_empty());
        }
    }

    #[test]
    fn sec08_changed_submit_binding_never_opens_stage_or_queues() {
        let (state, peer, mut request) = fixture();
        request.snapshot_hash = [3; 32];
        assert!(matches!(
            submit_secret_with(&state, [1; 16], peer, request, |_, _| panic!(
                "invalid binding must not open protected storage"
            )),
            Err(ErrorCode::Conflict)
        ));
        assert_eq!(state.secret_reviews.borrow().len(), 1);
        assert!(state.secret_jobs.borrow().is_empty());
    }

    #[test]
    fn sec08_changed_capture_material_never_opens_stage_or_queues() {
        use std::hash::{Hash, Hasher};
        let (state, peer, mut request) = fixture();
        request.secret_ticket_opaque[288] ^= 1;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        request.secret_ticket_opaque[..312].hash(&mut hasher);
        request.secret_ticket_opaque[312..].copy_from_slice(&hasher.finish().to_be_bytes());
        // Valid encoding and unchanged public binding/ID must not permit different capture material.
        TerminalSecretTicket::from_opaque(&request.secret_ticket_opaque).unwrap();
        assert!(matches!(
            submit_secret_with(&state, [1; 16], peer, request, |_, _| {
                panic!("changed capture material must not open protected storage")
            }),
            Err(ErrorCode::Conflict)
        ));
        assert_eq!(state.secret_reviews.borrow().len(), 1);
        assert!(state.secret_jobs.borrow().is_empty());
    }

    #[test]
    fn sec08_running_setup_rejects_second_review_before_stage_open() {
        let (state, peer, request) = fixture();
        state.active_secret_job.set(Some([1; 32]));
        assert!(matches!(
            submit_secret_with(&state, [1; 16], peer, request, |_, _| panic!(
                "a running setup must not open another captured stage"
            )),
            Err(ErrorCode::Conflict)
        ));
        assert!(state.secret_jobs.borrow().is_empty());
        assert_eq!(state.secret_reviews.borrow().len(), 1);
    }

    #[test]
    fn sec08_same_pending_review_or_running_job_cannot_issue_another_capture_ticket() {
        let (state, _, _) = fixture();
        assert_eq!(
            state.check_secret_review_slot([1; 32]),
            Err(ErrorCode::RateLimited)
        );
        assert_eq!(state.check_secret_review_slot([9; 32]), Ok(()));
        state.active_secret_job.set(Some([9; 32]));
        assert_eq!(
            state.check_secret_review_slot([9; 32]),
            Err(ErrorCode::RateLimited)
        );
        assert_eq!(state.secret_reviews.borrow().len(), 1);
        assert!(state.secret_jobs.borrow().is_empty());
    }

    #[test]
    fn sec08_submit_failure_retains_stable_error_categories() {
        use hantong_secrets::SecretError;
        for (error, code) in [
            (SecretError::InvalidRoot, ErrorCode::InternalError),
            (
                SecretError::UnsupportedPlatform,
                ErrorCode::CapabilityUnsupported,
            ),
            (SecretError::NotFound, ErrorCode::NotFound),
            (SecretError::CredentialMismatch, ErrorCode::ScopeDenied),
            (SecretError::InputExpired, ErrorCode::ApprovalExpired),
        ] {
            assert_eq!(secret_submit_failure("stage_handle_open", error), code);
        }
    }
}
