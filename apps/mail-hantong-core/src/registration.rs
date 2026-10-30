//! Trusted Core registration coordinator. IPC requests carry no approval data.
use hantong_approval::RegistrationReview;
use hantong_contracts::imap_setup::ImapSetupCommand;
use hantong_contracts::scope_refresh::ScopeRefreshCommand;
use hantong_contracts::{
    ErrorCode,
    auth::RequestedScope,
    registration::{RegistrationCommand, RegistrationProgress, RegistrationReceipt},
};
use hantong_ipc_client::profile::ParsedClientProfile;
use hantong_ipc_server::setup_dispatch::{SetupCommand, SetupReceipt};
use hantong_platform::{ClientSigningKey, ServerSigningKey};
use hantong_store::{
    CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, DEFAULT_GUI_LABEL, DEFAULT_MCP_LABEL,
    GuiRegistrationStage, McpMailRegistrationStage, McpRegistrationStage, RegistrationRecord,
    RegistrationRequest, RegistrationRequestState, RegistrationState, ServerIdentity, Store,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Job {
    Registration([u8; 32]),
    Imap([u8; 32]),
    Smtp([u8; 32]),
    ScopeRefresh([u8; 32]),
    SendReview([u8; 32]),
}

pub(crate) struct RegistrationHandle<'a> {
    state: Rc<Shared<'a>>,
    sender: mpsc::Sender<Job>,
}
pub(crate) struct RegistrationWorker<'a> {
    state: Rc<Shared<'a>>,
    receiver: mpsc::Receiver<Job>,
}
impl Drop for RegistrationWorker<'_> {
    fn drop(&mut self) {
        self.state.scheduled.set(None);
    }
}
struct Shared<'a> {
    store: &'a Store,
    server: &'a ServerSigningKey,
    scheduled: Cell<Option<Job>>,
    secrets_root: &'a std::path::Path,
    policy: Rc<RefCell<hantong_policy::ReadPolicy>>,
    smtp_sessions: crate::setup_smtp::Sessions,
}

#[cfg(test)]
pub(crate) fn coordinator<'a>(
    store: &'a Store,
    server: &'a ServerSigningKey,
    secrets_root: &'a std::path::Path,
) -> (RegistrationHandle<'a>, RegistrationWorker<'a>) {
    coordinator_with_policy(
        store,
        server,
        secrets_root,
        Rc::new(RefCell::new(hantong_policy::ReadPolicy::new())),
    )
}
pub(crate) fn coordinator_with_policy<'a>(
    store: &'a Store,
    server: &'a ServerSigningKey,
    secrets_root: &'a std::path::Path,
    policy: Rc<RefCell<hantong_policy::ReadPolicy>>,
) -> (RegistrationHandle<'a>, RegistrationWorker<'a>) {
    let state = Rc::new(Shared {
        store,
        server,
        secrets_root,
        policy,
        scheduled: Cell::new(None),
        smtp_sessions: RefCell::new(std::collections::BTreeMap::new()),
    });
    let (sender, receiver) = mpsc::channel(1);
    (
        RegistrationHandle {
            state: state.clone(),
            sender,
        },
        RegistrationWorker { state, receiver },
    )
}
impl RegistrationHandle<'_> {
    pub(crate) fn request_send(
        &self,
        owner: [u8; 16],
        epoch: u64,
        request: &hantong_contracts::mail_send::SendReviewRequest,
    ) -> Result<hantong_store::mail_send::SendReceipt, ErrorCode> {
        if self.sender.is_closed() {
            return Err(ErrorCode::CoreUnavailable);
        }
        if self
            .state
            .scheduled
            .get()
            .is_some_and(|job| !matches!(job, Job::SendReview(_)))
        {
            return Err(ErrorCode::RateLimited);
        }
        let receipt = self
            .state
            .store
            .request_mail_send_review(owner, epoch, request)?;
        if receipt.state == "pending" {
            match self.state.scheduled.get() {
                Some(Job::SendReview(id)) if id == receipt.review_id => {}
                Some(_) => return Err(ErrorCode::RateLimited),
                None => enqueue(
                    &self.state.scheduled,
                    &self.sender,
                    Job::SendReview(receipt.review_id),
                )?,
            }
        }
        Ok(receipt)
    }

    pub(crate) fn setup_command(&self, command: SetupCommand) -> Result<SetupReceipt, ErrorCode> {
        match command {
            SetupCommand::Smtp(command) => {
                use hantong_contracts::smtp_setup::{SmtpSetupCommand, SmtpSetupState};
                match command {
                    SmtpSetupCommand::Status(id) => crate::setup_smtp::status(
                        &self.state.smtp_sessions,
                        id,
                        self.state.scheduled.get() == Some(Job::Smtp(id)),
                    )
                    .map(SetupReceipt::Smtp),
                    SmtpSetupCommand::Request(input) => {
                        if self.sender.is_closed() {
                            return Err(ErrorCode::CoreUnavailable);
                        }
                        if self
                            .state
                            .scheduled
                            .get()
                            .is_some_and(|job| !matches!(job, Job::Smtp(_)))
                        {
                            return Err(ErrorCode::RateLimited);
                        }
                        if let Some(Job::Smtp(id)) = self.state.scheduled.get() {
                            if !crate::setup_smtp::matches(&self.state.smtp_sessions, id, &input) {
                                return Err(ErrorCode::RateLimited);
                            }
                            return crate::setup_smtp::status(&self.state.smtp_sessions, id, true)
                                .map(SetupReceipt::Smtp);
                        }
                        let r = crate::setup_smtp::request(
                            self.state.store,
                            self.state.secrets_root,
                            &self.state.smtp_sessions,
                            input,
                        )?;
                        if r.state == SmtpSetupState::Pending {
                            match self.state.scheduled.get() {
                                Some(Job::Smtp(id)) if id == r.request_id => {}
                                Some(_) => return Err(ErrorCode::RateLimited),
                                None => enqueue(
                                    &self.state.scheduled,
                                    &self.sender,
                                    Job::Smtp(r.request_id),
                                )?,
                            }
                        }
                        Ok(SetupReceipt::Smtp(r))
                    }
                }
            }
            SetupCommand::ScopeRefresh(ScopeRefreshCommand::Status(id)) => {
                crate::scope_refresh::status(
                    self.state.store,
                    id,
                    self.state.scheduled.get() == Some(Job::ScopeRefresh(id)),
                )
                .map(SetupReceipt::ScopeRefresh)
            }
            SetupCommand::ScopeRefresh(ScopeRefreshCommand::Request) => {
                if let Some(job) = self.state.scheduled.get() {
                    let Job::ScopeRefresh(id) = job else {
                        return Err(ErrorCode::RateLimited);
                    };
                    return crate::scope_refresh::status(self.state.store, id, true)
                        .map(SetupReceipt::ScopeRefresh);
                }
                if self.sender.is_closed() {
                    return Err(ErrorCode::InternalError);
                }
                let request = crate::scope_refresh::prepare(self.state.store, self.state.server)?;
                let id = *request.refresh_id();
                enqueue(&self.state.scheduled, &self.sender, Job::ScopeRefresh(id))?;
                crate::scope_refresh::status(self.state.store, id, true)
                    .map(SetupReceipt::ScopeRefresh)
            }
            SetupCommand::Imap(ImapSetupCommand::Resume(id)) => {
                if matches!(
                    self.state.scheduled.get(),
                    Some(
                        Job::Registration(_)
                            | Job::ScopeRefresh(_)
                            | Job::SendReview(_)
                            | Job::Smtp(_)
                    )
                ) {
                    return Err(ErrorCode::RateLimited);
                }
                let parent = self.state.store.resolve_imap_setup_leaf(id)?;
                let leaf_id = *parent.intent().request_id();
                if let Some(Job::Imap(active_id)) = self.state.scheduled.get() {
                    if active_id != leaf_id {
                        return Err(ErrorCode::RateLimited);
                    }
                    return crate::setup_imap::status(self.state.store, leaf_id, true)
                        .map(SetupReceipt::Imap);
                }
                if parent.events().last().is_some_and(|event| {
                    matches!(
                        event.event(),
                        hantong_store::imap_setup::ImapSetupEvent::ProbeFinished(
                            hantong_store::account_provisioning::InitialImapProbeOutcome::Ready(_)
                        )
                    )
                }) {
                    return crate::setup_imap::status(self.state.store, leaf_id, false)
                        .map(SetupReceipt::Imap);
                }
                if self.sender.is_closed() {
                    return Err(ErrorCode::InternalError);
                }
                let child = crate::setup_imap::prepare_resume(self.state.store, &parent)?;
                let child_id = *child.intent().request_id();
                enqueue(&self.state.scheduled, &self.sender, Job::Imap(child_id))?;
                crate::setup_imap::status(self.state.store, child_id, true).map(SetupReceipt::Imap)
            }
            SetupCommand::Registration(c) => self.command(c).map(SetupReceipt::Registration),
            SetupCommand::Imap(ImapSetupCommand::Status(id)) => crate::setup_imap::status(
                self.state.store,
                id,
                self.state.scheduled.get() == Some(Job::Imap(id)),
            )
            .map(SetupReceipt::Imap),
            SetupCommand::Imap(ImapSetupCommand::Request(input)) => {
                if let Some(job) = self.state.scheduled.get() {
                    let Job::Imap(id) = job else {
                        return Err(ErrorCode::RateLimited);
                    };
                    let current = self
                        .state
                        .store
                        .load_imap_setup_request(id)?
                        .ok_or(ErrorCode::InternalError)?;
                    if !crate::setup_imap::matches(&current, &input)? {
                        return Err(ErrorCode::RateLimited);
                    }
                    return crate::setup_imap::status(self.state.store, id, true)
                        .map(SetupReceipt::Imap);
                }
                if self.sender.is_closed() {
                    return Err(ErrorCode::InternalError);
                }
                let request = crate::setup_imap::create(self.state.store, &input)?;
                let id = *request.intent().request_id();
                enqueue(&self.state.scheduled, &self.sender, Job::Imap(id))?;
                crate::setup_imap::status(self.state.store, id, true).map(SetupReceipt::Imap)
            }
        }
    }
    pub(crate) fn command(
        &self,
        command: RegistrationCommand,
    ) -> Result<RegistrationReceipt, ErrorCode> {
        let gui = matches!(
            command,
            RegistrationCommand::GuiRequest | RegistrationCommand::GuiStatus(_)
        );
        let mcp = matches!(
            command,
            RegistrationCommand::McpRequest | RegistrationCommand::McpStatus(_)
        );
        let kind = if mcp {
            ClientKind::Mcp
        } else if gui {
            ClientKind::Gui
        } else {
            ClientKind::Cli
        };
        match command {
            RegistrationCommand::Status(id)
            | RegistrationCommand::GuiStatus(id)
            | RegistrationCommand::McpStatus(id) => self.state.receipt(id, kind),
            RegistrationCommand::Request
            | RegistrationCommand::GuiRequest
            | RegistrationCommand::McpRequest => {
                let registered = if mcp {
                    self.state.store.load_mcp_registration()?
                } else if gui {
                    self.state.store.load_gui_registration()?
                } else {
                    self.state.store.load_default_cli_registration()?
                };
                if registered.as_ref().is_some_and(|r| {
                    !(mcp
                        && r.state == RegistrationState::Active
                        && r.scope.tools() == hantong_contracts::auth::mcp_read_tools())
                }) {
                    return Err(ErrorCode::Conflict);
                }
                if let Some(job) = self.state.scheduled.get() {
                    return match job {
                        Job::Registration(id) => {
                            let active = self
                                .state
                                .store
                                .get_registration_request(id)?
                                .ok_or(ErrorCode::InternalError)?;
                            if active.client_kind != kind {
                                return Err(ErrorCode::RateLimited);
                            }
                            self.state.receipt(id, kind)
                        }
                        Job::Imap(_) | Job::Smtp(_) | Job::ScopeRefresh(_) | Job::SendReview(_) => {
                            Err(ErrorCode::RateLimited)
                        }
                    };
                }
                let effects = NativeEffects {
                    store: self.state.store,
                    server: self.state.server,
                };
                for request in effects.orphans()? {
                    if let Err(error) = cleanup_orphan(&effects, &request) {
                        effects.report(error);
                        return Err(error);
                    }
                }
                let pending = if mcp {
                    self.state.store.load_pending_mcp_request()?
                } else if gui {
                    self.state.store.load_pending_gui_request()?
                } else {
                    self.state.store.load_pending_default_cli_request()?
                };
                let request = match pending {
                    Some(request) => request,
                    None => {
                        let scope = RequestedScope::new(
                            self.state.store.list_registrable_account_ids()?,
                            if mcp {
                                hantong_contracts::auth::mcp_mail_tools()
                            } else if gui {
                                hantong_contracts::auth::gui_read_tools()
                            } else {
                                vec!["mail.accounts.list".into()]
                            },
                        )?;
                        self.state.store.create_registration_request(
                            hantong_platform::random_nonce()?,
                            kind,
                            if mcp {
                                DEFAULT_MCP_LABEL
                            } else if gui {
                                DEFAULT_GUI_LABEL
                            } else {
                                DEFAULT_CLI_LABEL
                            },
                            &scope,
                        )?
                    }
                };
                enqueue(
                    &self.state.scheduled,
                    &self.sender,
                    Job::Registration(request.request_id),
                )?;
                receipt(&request, RegistrationProgress::Pending)
            }
        }
    }
}
impl RegistrationWorker<'_> {
    pub(crate) async fn run(mut self) -> Result<(), ErrorCode> {
        let effects = NativeEffects {
            store: self.state.store,
            server: self.state.server,
        };
        // Recovery never prompts and never creates a missing key. An individual
        // failure remains provisioning and cannot authenticate or be replaced.
        crate::scope_refresh::recover_startup(self.state.store, self.state.server)?;
        recover_startup(&effects)?;
        self.state.store.recover_mail_sends()?;
        while let Some(job) = self.receiver.recv().await {
            let _window = ScheduledWindow(&self.state.scheduled);
            let result = match job {
                Job::Smtp(id) => {
                    crate::setup_smtp::run(
                        self.state.store,
                        self.state.secrets_root,
                        &self.state.smtp_sessions,
                        id,
                    )
                    .await
                }
                Job::SendReview(id) => {
                    crate::send_mail::run_job(
                        self.state.store,
                        self.state.secrets_root,
                        &self.state.policy,
                        id,
                    )
                    .await
                }
                Job::Registration(id) => self.state.review(id).await,
                Job::ScopeRefresh(id) => {
                    crate::scope_refresh::run_job(self.state.store, self.state.server, id).await
                }
                Job::Imap(id) => {
                    crate::setup_imap::review(self.state.store, self.state.secrets_root, id).await
                }
            };
            if let Err(error) = result {
                effects.report(error);
            }
        }
        Ok(())
    }
}

impl Shared<'_> {
    fn receipt(&self, id: [u8; 32], kind: ClientKind) -> Result<RegistrationReceipt, ErrorCode> {
        if id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        let mut request = self
            .store
            .get_registration_request(id)?
            .ok_or(ErrorCode::NotFound)?;
        if request.client_kind != kind {
            return Err(ErrorCode::NotFound);
        }
        if !supported_identity(
            request.client_kind,
            &request.display_label,
            request.scope.tools(),
        ) {
            return Err(ErrorCode::Conflict);
        }
        if request.state == RegistrationRequestState::Pending
            && self
                .store
                .expire_registration_request(id, request.version)?
        {
            request = self
                .store
                .get_registration_request(id)?
                .ok_or(ErrorCode::InternalError)?;
        }
        let progress = match request.state {
            RegistrationRequestState::Pending => RegistrationProgress::Pending,
            RegistrationRequestState::Denied => RegistrationProgress::Denied,
            RegistrationRequestState::Expired => RegistrationProgress::Expired,
            RegistrationRequestState::Approved => match self
                .store
                .load_registration_by_request(id)?
                .ok_or(ErrorCode::InternalError)?
                .state
            {
                RegistrationState::Provisioning => RegistrationProgress::Provisioning,
                RegistrationState::Active => RegistrationProgress::Ready,
                RegistrationState::Suspended | RegistrationState::Revoked => {
                    return Err(ErrorCode::Conflict);
                }
            },
        };
        receipt(&request, progress)
    }
    async fn review(&self, id: [u8; 32]) -> Result<(), ErrorCode> {
        let effects = NativeEffects {
            store: self.store,
            server: self.server,
        };
        let expected = effects.request(id)?;
        if expected.state != RegistrationRequestState::Pending {
            return Err(ErrorCode::Conflict);
        }
        let server_hash = hantong_platform::sha256(&effects.server_public()?)?;
        let review = review_snapshot(&expected, server_hash)?;
        let config = self.store.load_config()?.ok_or(ErrorCode::InternalError)?;
        match hantong_approval_windows::review_registration(
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
            Err(ErrorCode::ApprovalDenied) => {
                if expected.client_kind == ClientKind::Mcp {
                    self.store
                        .deny_mcp_registration_request(id, expected.version)
                } else if expected.client_kind == ClientKind::Gui {
                    self.store
                        .deny_gui_registration_request(id, expected.version)
                } else {
                    self.store
                        .deny_cli_registration_request(id, expected.version)
                }
            }
            Err(ErrorCode::ApprovalExpired) => {
                self.store
                    .expire_registration_request(id, expected.version)?;
                Err(ErrorCode::ApprovalExpired)
            }
            // Cancel, close, unavailable verifier and errors preserve pending.
            Err(error) => Err(error),
        }
    }
}
fn enqueue<T: Copy + Eq>(
    scheduled: &Cell<Option<T>>,
    sender: &mpsc::Sender<T>,
    id: T,
) -> Result<(), ErrorCode> {
    if let Some(current) = scheduled.get() {
        return if current == id {
            Ok(())
        } else {
            Err(ErrorCode::RateLimited)
        };
    }
    sender.try_send(id).map_err(|error| match error {
        mpsc::error::TrySendError::Full(_) => ErrorCode::RateLimited,
        mpsc::error::TrySendError::Closed(_) => ErrorCode::InternalError,
    })?;
    scheduled.set(Some(id));
    Ok(())
}
struct ScheduledWindow<'a, T: Copy>(&'a Cell<Option<T>>);
impl<T: Copy> Drop for ScheduledWindow<'_, T> {
    fn drop(&mut self) {
        self.0.set(None);
    }
}
fn receipt(
    request: &RegistrationRequest,
    state: RegistrationProgress,
) -> Result<RegistrationReceipt, ErrorCode> {
    RegistrationReceipt::new(request.request_id, request.expires_at_utc_ms, state)
}
fn review_snapshot(
    request: &RegistrationRequest,
    hash: [u8; 32],
) -> Result<RegistrationReview, ErrorCode> {
    if !supported_identity(
        request.client_kind,
        &request.display_label,
        request.scope.tools(),
    ) {
        return Err(ErrorCode::Conflict);
    }
    let constructor = if request.client_kind == ClientKind::Mcp {
        if request.scope.tools() == hantong_contracts::auth::mcp_mail_tools() {
            RegistrationReview::new_mcp_mail
        } else {
            RegistrationReview::new_mcp
        }
    } else if request.client_kind == ClientKind::Gui {
        RegistrationReview::new_gui
    } else {
        RegistrationReview::new
    };
    constructor(
        request.request_id,
        request.version,
        request.scope.accounts().to_vec(),
        hash,
        request.created_at_utc_ms,
        request.expires_at_utc_ms,
    )
}

fn supported_identity(kind: ClientKind, label: &str, tools: &[String]) -> bool {
    match kind {
        ClientKind::Cli => label == DEFAULT_CLI_LABEL && tools == ["mail.accounts.list"],
        ClientKind::Gui => {
            label == DEFAULT_GUI_LABEL && tools == hantong_contracts::auth::gui_read_tools()
        }
        ClientKind::Mcp => {
            label == DEFAULT_MCP_LABEL && hantong_contracts::auth::is_supported_mcp_toolset(tools)
        }
    }
}

// This effect seam is private and has exactly one production implementation.
// The only production caller of commit_consumed_review first consumes the
// opaque native token. Tests do not manufacture native verification tokens.
trait Effects {
    type Key;
    type Publication;
    fn request(&self, id: [u8; 32]) -> Result<RegistrationRequest, ErrorCode>;
    fn server_public(&self) -> Result<[u8; 72], ErrorCode>;
    fn now(&self) -> Result<i64, ErrorCode>;
    fn create_key(&self, id: [u8; 16]) -> Result<Self::Key, ErrorCode>;
    fn open_key(&self, id: [u8; 16]) -> Result<Self::Key, ErrorCode>;
    fn key_public(&self, key: &Self::Key) -> Result<[u8; 72], ErrorCode>;
    fn delete_key(&self, key: Self::Key) -> Result<(), ErrorCode>;
    fn stage(
        &self,
        request: RegistrationRequest,
        id: [u8; 16],
        public: [u8; 72],
        hash: [u8; 32],
    ) -> Result<RegistrationRecord, ErrorCode>;
    fn publish(&self, bytes: &[u8]) -> Result<Self::Publication, ErrorCode>;
    fn cleanup_publication(&self, _publication: &mut Self::Publication) -> Result<(), ErrorCode> {
        Ok(())
    }
    fn activate(&self, record: &RegistrationRecord, hash: [u8; 32]) -> Result<(), ErrorCode>;
    fn linked(&self, request: [u8; 32], public: [u8; 72]) -> Result<bool, ErrorCode>;
    fn orphans(&self) -> Result<Vec<RegistrationRequest>, ErrorCode>;
    fn provisioning(&self) -> Result<Vec<RegistrationRecord>, ErrorCode>;
    fn report(&self, error: ErrorCode);
}
fn cleanup_orphan<E: Effects>(effects: &E, request: &RegistrationRequest) -> Result<(), ErrorCode> {
    if request.state == RegistrationRequestState::Approved
        || !supported_identity(
            request.client_kind,
            &request.display_label,
            request.scope.tools(),
        )
    {
        return Err(ErrorCode::Conflict);
    }
    let id = hantong_platform::registration_id_for_request(request.request_id)?;
    let key = match effects.open_key(id) {
        Ok(key) => key,
        Err(ErrorCode::NotFound) => return Ok(()),
        Err(error) => return Err(error),
    };
    let public = effects.key_public(&key)?;
    delete_unlinked(effects, request.request_id, key, public)
}
fn delete_unlinked<E: Effects>(
    effects: &E,
    id: [u8; 32],
    key: E::Key,
    public: [u8; 72],
) -> Result<(), ErrorCode> {
    if effects.linked(id, public)? {
        return Err(ErrorCode::Conflict);
    }
    effects.delete_key(key)
}
fn recover_startup<E: Effects>(effects: &E) -> Result<(), ErrorCode> {
    let lists = (|| Ok::<_, ErrorCode>((effects.orphans()?, effects.provisioning()?)))();
    let (orphans, records) = match lists {
        Ok(lists) => lists,
        Err(error) => {
            effects.report(error);
            return Err(error);
        }
    };
    for request in orphans {
        if let Err(error) = cleanup_orphan(effects, &request) {
            effects.report(error);
        }
    }
    for record in records {
        if let Err(error) = recover_record(effects, &record) {
            effects.report(error);
        }
    }
    Ok(())
}
fn commit_consumed_review<E: Effects>(
    effects: &E,
    expected: &RegistrationRequest,
    review: &RegistrationReview,
) -> Result<(), ErrorCode> {
    let current = effects.request(expected.request_id)?;
    if &current != expected || current.state != RegistrationRequestState::Pending {
        return Err(ErrorCode::Conflict);
    }
    let public = effects.server_public()?;
    let hash = hantong_platform::sha256(&public)?;
    if review_snapshot(&current, hash)? != *review {
        return Err(ErrorCode::Conflict);
    }
    let now = effects.now()?;
    if now < current.created_at_utc_ms || now >= current.expires_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    cleanup_orphan(effects, &current)?;
    let id = hantong_platform::registration_id_for_request(current.request_id)?;
    let key = effects.create_key(id)?;
    // If the public key cannot be read, keep the exact reserved selector for
    // later recovery. Ownership cannot be checked safely without that value.
    let client_public = effects.key_public(&key)?;
    let staged = effects.stage(current, id, client_public, hash);
    let record = match staged {
        Ok(record) => record,
        Err(error) => {
            if let Err(cleanup) = delete_unlinked(effects, expected.request_id, key, client_public)
            {
                // Preserve both stable causes; never replace the stage failure.
                effects.report(error);
                effects.report(cleanup);
            }
            return Err(error);
        }
    };
    // A persisted provisioning record owns recovery of this key from now on.
    let bytes = profile_bytes(&record, public)?;
    let mut publication = effects.publish(&bytes)?;
    effects.activate(&record, hash)?;
    effects.cleanup_publication(&mut publication)
}
fn recover_record<E: Effects>(effects: &E, record: &RegistrationRecord) -> Result<(), ErrorCode> {
    validate_initial_record(record)?;
    let request = effects.request(record.request_id)?;
    if request.state != RegistrationRequestState::Approved
        || request.client_kind != record.client_kind
        || request.display_label != record.display_label
        || request.scope != record.scope
    {
        return Err(ErrorCode::Conflict);
    }
    let public = effects.server_public()?;
    let hash = hantong_platform::sha256(&public)?;
    let key = effects.open_key(record.registration_id)?;
    if effects.key_public(&key)? != record.public_key {
        return Err(ErrorCode::Conflict);
    }
    let bytes = profile_bytes(record, public)?;
    let mut publication = effects.publish(&bytes)?;
    effects.activate(record, hash)?;
    effects.cleanup_publication(&mut publication)
}
fn validate_initial_record(record: &RegistrationRecord) -> Result<(), ErrorCode> {
    if record.registration_id != hantong_platform::registration_id_for_request(record.request_id)?
        || record.state != RegistrationState::Provisioning
        || record.profile_epoch != 1
        || !supported_identity(
            record.client_kind,
            &record.display_label,
            record.scope.tools(),
        )
        || record.read_only != (record.scope.tools() != hantong_contracts::auth::mcp_mail_tools())
        || record.requests_per_minute != 60
        || record.approved_channel != "approval_helper"
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
fn profile_bytes(record: &RegistrationRecord, server: [u8; 72]) -> Result<Vec<u8>, ErrorCode> {
    validate_initial_record(record)?;
    raw_profile_bytes(record, server)
}
fn raw_profile_bytes(record: &RegistrationRecord, server: [u8; 72]) -> Result<Vec<u8>, ErrorCode> {
    let constructor = if record.client_kind == ClientKind::Mcp {
        if record.scope.tools() == hantong_contracts::auth::mcp_mail_tools() {
            ParsedClientProfile::new_mcp_mail
        } else {
            ParsedClientProfile::new_mcp
        }
    } else if record.client_kind == ClientKind::Gui {
        ParsedClientProfile::new_gui
    } else {
        ParsedClientProfile::new
    };
    constructor(
        record.registration_id,
        hantong_platform::client_key_id_for_registration(record.registration_id)?,
        record.public_key,
        server,
        record.profile_epoch,
        record.scope.accounts().to_vec(),
    )?
    .to_json()
}

struct NativeEffects<'a> {
    store: &'a Store,
    server: &'a ServerSigningKey,
}
impl Effects for NativeEffects<'_> {
    type Key = ClientSigningKey;
    type Publication = hantong_approval_windows::PublishedProfile;
    fn linked(&self, request: [u8; 32], public: [u8; 72]) -> Result<bool, ErrorCode> {
        self.store.registration_key_is_linked(request, public)
    }
    fn orphans(&self) -> Result<Vec<RegistrationRequest>, ErrorCode> {
        let mut requests = self.store.list_orphan_cli_registration_requests()?;
        requests.extend(self.store.list_orphan_gui_registration_requests()?);
        requests.extend(self.store.list_orphan_mcp_registration_requests()?);
        Ok(requests)
    }
    fn provisioning(&self) -> Result<Vec<RegistrationRecord>, ErrorCode> {
        Ok(self
            .store
            .list_provisioning_registrations(128)?
            .into_iter()
            .filter(|record| record.profile_epoch == 1)
            .collect())
    }
    fn report(&self, error: ErrorCode) {
        use std::io::Write;
        // Preserve a bounded error event even when reading UTC itself fails.
        if let Ok(line) = registration_failure_line(error, self.now().unwrap_or(0)) {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
    }
    fn request(&self, id: [u8; 32]) -> Result<RegistrationRequest, ErrorCode> {
        self.store
            .get_registration_request(id)?
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
    fn now(&self) -> Result<i64, ErrorCode> {
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| ErrorCode::InternalError)?
                .as_millis(),
        )
        .map_err(|_| ErrorCode::InternalError)
    }
    fn create_key(&self, id: [u8; 16]) -> Result<Self::Key, ErrorCode> {
        ClientSigningKey::create_for_registration(id)
    }
    fn open_key(&self, id: [u8; 16]) -> Result<Self::Key, ErrorCode> {
        ClientSigningKey::open_for_registration(id)
    }
    fn key_public(&self, key: &Self::Key) -> Result<[u8; 72], ErrorCode> {
        key.public_key()
    }
    fn delete_key(&self, key: Self::Key) -> Result<(), ErrorCode> {
        key.delete()
    }
    fn stage(
        &self,
        request: RegistrationRequest,
        id: [u8; 16],
        public: [u8; 72],
        hash: [u8; 32],
    ) -> Result<RegistrationRecord, ErrorCode> {
        if request.client_kind == ClientKind::Mcp {
            if request.scope.tools() == hantong_contracts::auth::mcp_mail_tools() {
                let old = self.store.load_mcp_registration()?;
                self.store
                    .stage_mcp_mail_registration(&McpMailRegistrationStage::new(
                        request, id, public, hash, old,
                    )?)
            } else {
                self.store
                    .stage_mcp_registration(&McpRegistrationStage::new(request, id, public, hash)?)
            }
        } else if request.client_kind == ClientKind::Gui {
            self.store
                .stage_gui_registration(&GuiRegistrationStage::new(request, id, public, hash)?)
        } else {
            self.store
                .stage_cli_registration(&CliRegistrationStage::new(request, id, public, hash)?)
        }
    }
    fn publish(&self, bytes: &[u8]) -> Result<Self::Publication, ErrorCode> {
        match ParsedClientProfile::from_json(bytes)?.profile() {
            hantong_ipc_client::profile::ProfileSelector::Default => {
                hantong_approval_windows::publish_default_profile(bytes)
            }
            hantong_ipc_client::profile::ProfileSelector::Mcp => {
                let profile = ParsedClientProfile::from_json(bytes)?;
                if let Some(old) = self
                    .store
                    .load_mcp_replaced_registration(*profile.registration_id())?
                {
                    let current = self
                        .store
                        .load_registration(*profile.registration_id())?
                        .ok_or(ErrorCode::InternalError)?;
                    let old_bytes = raw_profile_bytes(&old, self.server.public_key()?)?;
                    hantong_approval_windows::refresh_mcp_profile(
                        current.request_id,
                        &old_bytes,
                        bytes,
                    )
                } else {
                    hantong_approval_windows::publish_mcp_profile(bytes)
                }
            }
            hantong_ipc_client::profile::ProfileSelector::Gui => {
                hantong_approval_windows::publish_gui_profile(bytes)
            }
        }
    }
    fn cleanup_publication(&self, publication: &mut Self::Publication) -> Result<(), ErrorCode> {
        hantong_approval_windows::cleanup_profile_refresh_backup(publication)
    }
    fn activate(&self, record: &RegistrationRecord, hash: [u8; 32]) -> Result<(), ErrorCode> {
        if record.client_kind == ClientKind::Mcp {
            self.store
                .activate_mcp_registration(record.registration_id, record.version, hash)
                .map(drop)
        } else if record.client_kind == ClientKind::Gui {
            self.store
                .activate_gui_registration(record.registration_id, record.version, hash)
                .map(drop)
        } else {
            self.store
                .activate_cli_registration(record.registration_id, record.version, hash)
                .map(drop)
        }
    }
}

pub(super) fn registration_failure_line(error: ErrorCode, now: i64) -> Result<String, ErrorCode> {
    use hantong_application::{DiagnosticEvent, EventKind, Severity};
    DiagnosticEvent::new(EventKind::OperationFailed, Severity::Error, now)
        .map_err(|_| ErrorCode::InternalError)?
        .with_error(error)
        .to_json_line()
        .map_err(|_| ErrorCode::InternalError)
}

#[cfg(test)]
mod tests;
