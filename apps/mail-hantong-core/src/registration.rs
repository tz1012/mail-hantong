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
    CliRegistrationStage, ClientKind, DEFAULT_CLI_LABEL, RegistrationRecord, RegistrationRequest,
    RegistrationRequestState, RegistrationState, ServerIdentity, Store,
};
use std::{cell::Cell, rc::Rc};
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Job {
    Registration([u8; 32]),
    Imap([u8; 32]),
    ScopeRefresh([u8; 32]),
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
}

pub(crate) fn coordinator<'a>(
    store: &'a Store,
    server: &'a ServerSigningKey,
    secrets_root: &'a std::path::Path,
) -> (RegistrationHandle<'a>, RegistrationWorker<'a>) {
    let state = Rc::new(Shared {
        store,
        server,
        secrets_root,
        scheduled: Cell::new(None),
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
    pub(crate) fn setup_command(&self, command: SetupCommand) -> Result<SetupReceipt, ErrorCode> {
        match command {
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
                    Some(Job::Registration(_) | Job::ScopeRefresh(_))
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
        match command {
            RegistrationCommand::Status(id) => self.state.receipt(id),
            RegistrationCommand::Request => {
                if self.state.store.load_default_cli_registration()?.is_some() {
                    return Err(ErrorCode::Conflict);
                }
                if let Some(job) = self.state.scheduled.get() {
                    return match job {
                        Job::Registration(id) => self.state.receipt(id),
                        Job::Imap(_) | Job::ScopeRefresh(_) => Err(ErrorCode::RateLimited),
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
                let request = match self.state.store.load_pending_default_cli_request()? {
                    Some(request) => request,
                    None => {
                        let scope = RequestedScope::new(
                            self.state.store.list_registrable_account_ids()?,
                            vec!["mail.accounts.list".into()],
                        )?;
                        self.state.store.create_registration_request(
                            hantong_platform::random_nonce()?,
                            ClientKind::Cli,
                            DEFAULT_CLI_LABEL,
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
        while let Some(job) = self.receiver.recv().await {
            let _window = ScheduledWindow(&self.state.scheduled);
            let result = match job {
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
    fn receipt(&self, id: [u8; 32]) -> Result<RegistrationReceipt, ErrorCode> {
        if id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        let mut request = self
            .store
            .get_registration_request(id)?
            .ok_or(ErrorCode::NotFound)?;
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
            Err(ErrorCode::ApprovalDenied) => self
                .store
                .deny_cli_registration_request(id, expected.version),
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
    if request.client_kind != ClientKind::Cli
        || request.display_label != DEFAULT_CLI_LABEL
        || request.scope.tools() != ["mail.accounts.list"]
    {
        return Err(ErrorCode::Conflict);
    }
    RegistrationReview::new(
        request.request_id,
        request.version,
        request.scope.accounts().to_vec(),
        hash,
        request.created_at_utc_ms,
        request.expires_at_utc_ms,
    )
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
    fn activate(&self, record: &RegistrationRecord, hash: [u8; 32]) -> Result<(), ErrorCode>;
    fn linked(&self, request: [u8; 32], public: [u8; 72]) -> Result<bool, ErrorCode>;
    fn orphans(&self) -> Result<Vec<RegistrationRequest>, ErrorCode>;
    fn provisioning(&self) -> Result<Vec<RegistrationRecord>, ErrorCode>;
    fn report(&self, error: ErrorCode);
}
fn cleanup_orphan<E: Effects>(effects: &E, request: &RegistrationRequest) -> Result<(), ErrorCode> {
    if request.state == RegistrationRequestState::Approved
        || request.client_kind != ClientKind::Cli
        || request.display_label != DEFAULT_CLI_LABEL
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
    let _publication = effects.publish(&bytes)?;
    effects.activate(&record, hash)
}
fn recover_record<E: Effects>(effects: &E, record: &RegistrationRecord) -> Result<(), ErrorCode> {
    validate_default_record(record)?;
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
    let _publication = effects.publish(&bytes)?;
    effects.activate(record, hash)
}
fn validate_default_record(record: &RegistrationRecord) -> Result<(), ErrorCode> {
    if record.registration_id != hantong_platform::registration_id_for_request(record.request_id)?
        || record.state != RegistrationState::Provisioning
        || record.client_kind != ClientKind::Cli
        || record.display_label != DEFAULT_CLI_LABEL
        || !record.read_only
        || record.requests_per_minute != 60
        || record.approved_channel != "approval_helper"
        || record.scope.tools() != ["mail.accounts.list"]
    {
        return Err(ErrorCode::Conflict);
    }
    Ok(())
}
fn profile_bytes(record: &RegistrationRecord, server: [u8; 72]) -> Result<Vec<u8>, ErrorCode> {
    validate_default_record(record)?;
    ParsedClientProfile::new(
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
        self.store.list_orphan_cli_registration_requests()
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
        self.store
            .stage_cli_registration(&CliRegistrationStage::new(request, id, public, hash)?)
    }
    fn publish(&self, bytes: &[u8]) -> Result<Self::Publication, ErrorCode> {
        hantong_approval_windows::publish_default_profile(bytes)
    }
    fn activate(&self, record: &RegistrationRecord, hash: [u8; 32]) -> Result<(), ErrorCode> {
        self.store
            .activate_cli_registration(record.registration_id, record.version, hash)
            .map(drop)
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
