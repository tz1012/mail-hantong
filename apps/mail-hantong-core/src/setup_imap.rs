//! Core-owned single-attempt setup. Persisted intent never grants authority.
use hantong_contracts::{
    ErrorCode,
    imap_setup::{
        ImapSetupInput, ImapSetupNextAction, ImapSetupReceipt, ImapSetupState, ImapSetupTls,
    },
    wire_hex,
};
use hantong_domain::{
    account_provisioning::NewImapAccountDraft, mail_connection::MailConnectionConfig,
};
use hantong_secrets::{TerminalSecretBinding, TerminalSecretHandle, TerminalSecretTicket};
use hantong_store::{
    Store,
    account_provisioning::{InitialImapProbeOutcome, imap_account_stage_payload_hash},
    imap_setup::{ImapSetupEvent, ImapSetupPhase, ImapSetupRequest, NewImapSetupRequest},
    mail_connection::MailMutation,
};

fn intent(
    input: &ImapSetupInput,
    request: [u8; 32],
    account: [u8; 32],
    operation: [u8; 32],
    key: [u8; 32],
) -> Result<NewImapSetupRequest, ErrorCode> {
    if [request, account, operation, key].contains(&[0; 32]) {
        return Err(ErrorCode::InvalidInput);
    }
    let draft = NewImapAccountDraft::new(
        wire_hex::encode(&account),
        input.display_name().map(str::to_owned),
        input.color().map(str::to_owned),
    )
    .map_err(|_| ErrorCode::InvalidInput)?;
    let endpoint = endpoint(input, draft.account_id())?;
    let mutation = MailMutation::new(
        wire_hex::encode(&operation),
        wire_hex::encode(&key),
        0,
        imap_account_stage_payload_hash(&draft, &endpoint)?,
    )?;
    NewImapSetupRequest::new(request, draft, endpoint, mutation)
}

fn endpoint(
    input: &ImapSetupInput,
    account: &str,
) -> Result<hantong_domain::mail_connection::ImapEndpointConfig, ErrorCode> {
    let config=serde_json::to_vec(&serde_json::json!({"schema_version":1,"account_id":account,"imap":{"settings_version":1,"host":input.host(),"port":input.port(),"tls":match input.tls(){ImapSetupTls::ImplicitTls=>"implicit_tls",ImapSetupTls::StarttlsRequired=>"starttls_required"},"login_name":input.login_name(),"authentication":"app_password"}})).map_err(|_|ErrorCode::InternalError)?;
    Ok(MailConnectionConfig::from_json(&config)
        .map_err(|_| ErrorCode::InvalidInput)?
        .imap()
        .clone())
}

pub(crate) fn create(store: &Store, input: &ImapSetupInput) -> Result<ImapSetupRequest, ErrorCode> {
    let request = intent(
        input,
        hantong_platform::random_nonce()?,
        hantong_platform::random_nonce()?,
        hantong_platform::random_nonce()?,
        hantong_platform::random_nonce()?,
    )?;
    store.create_imap_setup_request(&request)
}
pub(super) fn prepare_resume(
    store: &Store,
    parent: &ImapSetupRequest,
) -> Result<ImapSetupRequest, ErrorCode> {
    store.prepare_imap_setup_resume(
        parent,
        hantong_platform::random_nonce()?,
        wire_hex::encode(&hantong_platform::random_nonce()?),
        wire_hex::encode(&hantong_platform::random_nonce()?),
    )
}
fn existing_resume(request: &ImapSetupRequest) -> bool {
    request
        .intent()
        .resume_binding()
        .and_then(|b| b.existing_versions())
        .is_some()
}
fn stage(store: &Store, request: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode> {
    if existing_resume(request) {
        store.stage_imap_setup_resume(request)
    } else {
        store.stage_imap_setup_request(request)
    }
}
#[cfg(any(windows, test))]
fn review_snapshot(
    request: &ImapSetupRequest,
    nonce: [u8; 32],
) -> Result<hantong_approval::AccountSetupReview, ErrorCode> {
    let i = request.intent();
    let constructor = if existing_resume(request) {
        hantong_approval::AccountSetupReview::resume
    } else {
        hantong_approval::AccountSetupReview::new
    };
    constructor(
        i.draft().clone(),
        i.endpoint().clone(),
        *i.request_id(),
        *i.review_payload_hash(),
        nonce,
        request.created_at_utc_ms(),
        request.expires_at_utc_ms(),
    )
}
pub(super) fn matches(
    request: &ImapSetupRequest,
    input: &ImapSetupInput,
) -> Result<bool, ErrorCode> {
    let intent = request.intent();
    Ok(
        intent.endpoint() == &endpoint(input, intent.draft().account_id())?
            && intent.draft().display_name() == input.display_name()
            && intent.draft().color() == input.color(),
    )
}

/// Reuse the exact interrupted attempt; only an explicit Resume may replace it.
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn prepare_request(
    store: &Store,
    input: &ImapSetupInput,
) -> Result<(ImapSetupRequest, bool), ErrorCode> {
    let target = endpoint(input, "candidate")?;
    let requests = store.list_imap_setup_requests()?;
    let has_staged_target = |request: &ImapSetupRequest| {
        existing_resume(request)
            || request.events().iter().any(|event| {
                matches!(
                    event.event(),
                    ImapSetupEvent::AccountStaged | ImapSetupEvent::ResumeStaged
                )
            })
    };
    for request in &requests {
        if has_staged_target(request)
            && request.intent().endpoint().host() == target.host()
            && request.intent().endpoint().login_name() == target.login_name()
            && !matches(request, input)?
        {
            return Err(ErrorCode::Conflict);
        }
    }
    let mut matching = Vec::new();
    for request in &requests {
        if request.phase() != ImapSetupPhase::Superseded && matches(request, input)? {
            matching.push(request);
        }
    }
    let staged = matching
        .iter()
        .copied()
        .filter(|request| has_staged_target(request))
        .collect::<Vec<_>>();
    if staged.len() > 1 {
        return Err(ErrorCode::Conflict);
    }
    if let Some(request) = staged.first() {
        if request.phase() == ImapSetupPhase::ReviewClosed {
            // Keep the closed receipt unchanged. Its superseded parent points
            // to this chain and already has the contract's interrupted/resume view.
            if let Some(binding) = request.intent().resume_binding() {
                return load(store, *binding.parent_request_id()).map(|parent| (parent, false));
            }
        }
        return Ok(((*request).clone(), false));
    }
    matching.sort_by_key(|request| (request.created_at_utc_ms(), *request.intent().request_id()));
    for request in matching.into_iter().rev() {
        if request.phase() != ImapSetupPhase::ReviewPending {
            continue;
        }
        if now()? >= request.expires_at_utc_ms() {
            store.close_imap_setup_review(request, ErrorCode::ApprovalExpired)?;
            continue;
        }
        return Ok((request.clone(), false));
    }
    create(store, input).map(|request| (request, true))
}
pub(super) fn load(store: &Store, id: [u8; 32]) -> Result<ImapSetupRequest, ErrorCode> {
    store
        .load_imap_setup_request(id)?
        .ok_or(ErrorCode::NotFound)
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
pub(crate) fn status(
    store: &Store,
    id: [u8; 32],
    active: bool,
) -> Result<ImapSetupReceipt, ErrorCode> {
    let mut request = load(store, id)?;
    if request.phase() == ImapSetupPhase::ReviewPending && now()? >= request.expires_at_utc_ms() {
        request = store.close_imap_setup_review(&request, ErrorCode::ApprovalExpired)?;
    }
    progress(
        &request,
        active,
        store.load_default_cli_registration()?.is_some(),
    )
}

fn progress(
    request: &ImapSetupRequest,
    active: bool,
    registered: bool,
) -> Result<ImapSetupReceipt, ErrorCode> {
    use ImapSetupNextAction as A;
    use ImapSetupState as S;
    let last = request
        .events()
        .last()
        .ok_or(ErrorCode::InternalError)?
        .event();
    let (state, action) = match last {
        ImapSetupEvent::Superseded(_) => (S::Interrupted, A::ResumeRequired),
        ImapSetupEvent::ReviewClosed(code) => (
            match code {
                ErrorCode::ApprovalRequired => S::Cancelled,
                ErrorCode::ApprovalDenied => S::Denied,
                ErrorCode::ApprovalExpired => S::Expired,
                _ => S::Error,
            },
            A::NewRequestRequired,
        ),
        ImapSetupEvent::ProbeFinished(outcome) => match outcome {
            InitialImapProbeOutcome::Ready(_) => (
                S::Ready,
                if registered {
                    A::ProfileScopeReviewRequired
                } else {
                    A::ClientRegistrationRequired
                },
            ),
            InitialImapProbeOutcome::AuthRequired => (S::AuthRequired, A::ResumeRequired),
            InitialImapProbeOutcome::TlsFailed => (S::TlsFailed, A::ResumeRequired),
            InitialImapProbeOutcome::Offline => (S::Offline, A::ResumeRequired),
            InitialImapProbeOutcome::Error => (S::Error, A::ResumeRequired),
        },
        ImapSetupEvent::CredentialStoreReturned(Err(_)) => (S::Error, A::ResumeRequired),
        _ if !active => (S::Interrupted, A::ResumeRequired),
        ImapSetupEvent::RequestCreated => (S::Pending, A::HelperRequired),
        _ => (S::Connecting, A::Wait),
    };
    ImapSetupReceipt::new(
        *request.intent().request_id(),
        request.expires_at_utc_ms(),
        state,
        action,
    )?
    .with_account_id(request.intent().draft().account_id().to_owned())
}

// Only Core's consumed native pending reaches the production implementation.
// Test doubles exercise ordering without constructing native verification.
trait Effects {
    type Pending;
    fn load(&self, id: [u8; 32]) -> Result<ImapSetupRequest, ErrorCode>;
    fn stage(&self, expected: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode>;
    fn begin_credential(&self, expected: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode>;
    fn store_credential(
        &self,
        expected: &ImapSetupRequest,
        pending: Self::Pending,
    ) -> Result<(), ErrorCode>;
    fn finish_credential(
        &self,
        expected: &ImapSetupRequest,
        result: Result<(), ErrorCode>,
    ) -> Result<ImapSetupRequest, ErrorCode>;
    fn begin_probe(&self, expected: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode>;
    async fn probe(&self, expected: &ImapSetupRequest) -> InitialImapProbeOutcome;
    fn finish_probe(
        &self,
        expected: &ImapSetupRequest,
        outcome: &InitialImapProbeOutcome,
    ) -> Result<ImapSetupRequest, ErrorCode>;
}
async fn finish_verified<E: Effects>(
    effects: &E,
    expected: &ImapSetupRequest,
    pending: E::Pending,
) -> Result<(), ErrorCode> {
    let current = effects
        .load(*expected.intent().request_id())
        .map_err(|error| setup_failure("load", error))?;
    if &current != expected || current.phase() != ImapSetupPhase::ReviewPending {
        return Err(setup_failure("review_state", ErrorCode::Conflict));
    }
    let staged = effects
        .stage(&current)
        .map_err(|error| setup_failure("account_stage", error))?;
    let started = effects
        .begin_credential(&staged)
        .map_err(|error| setup_failure("credential_begin", error))?;
    let result = effects.store_credential(&started, pending);
    let returned = effects
        .finish_credential(&started, result)
        .map_err(|error| setup_failure("credential_finish", error))?;
    result.map_err(|error| setup_failure("credential_store", error))?;
    let probing = effects
        .begin_probe(&returned)
        .map_err(|error| setup_failure("probe_begin", error))?;
    let outcome = effects.probe(&probing).await;
    effects
        .finish_probe(&probing, &outcome)
        .map_err(|error| setup_failure("probe_finish", error))?;
    Ok(())
}

fn setup_failure(stage: &'static str, error: ErrorCode) -> ErrorCode {
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr().lock(),
        "hantong_imap_setup stage={stage} kind=transition_failed code={}",
        error.as_str()
    );
    error
}

fn secret_error(error: hantong_secrets::SecretError) -> ErrorCode {
    use hantong_secrets::SecretError as E;
    match error {
        E::UnsupportedPlatform => ErrorCode::CapabilityUnsupported,
        E::InputExpired => ErrorCode::ApprovalExpired,
        E::CredentialMismatch | E::InvalidInputWindow => ErrorCode::ScopeDenied,
        E::InvalidAccountId | E::EmptySecret | E::SecretTooLarge | E::InvalidCredential => {
            ErrorCode::InvalidInput
        }
        _ => ErrorCode::InternalError,
    }
}
struct StoreEffects<'a> {
    store: &'a Store,
    secrets_root: &'a std::path::Path,
}
enum CapturedCredential {
    #[cfg(windows)]
    Native(Box<hantong_secrets::PendingImapCredential>),
    Terminal(
        Box<(
            TerminalSecretTicket,
            TerminalSecretHandle,
            TerminalSecretBinding,
        )>,
    ),
}
impl Effects for StoreEffects<'_> {
    type Pending = CapturedCredential;
    fn load(&self, id: [u8; 32]) -> Result<ImapSetupRequest, ErrorCode> {
        load(self.store, id)
    }
    fn stage(&self, r: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode> {
        stage(self.store, r)
    }
    fn begin_credential(&self, r: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode> {
        self.store.begin_imap_credential_store(r)
    }
    fn store_credential(
        &self,
        r: &ImapSetupRequest,
        pending: Self::Pending,
    ) -> Result<(), ErrorCode> {
        let i = r.intent();
        match pending {
            #[cfg(windows)]
            CapturedCredential::Native(pending) => {
                if pending.setup_request_id() != i.request_id()
                    || pending.setup_payload_hash() != i.review_payload_hash()
                {
                    return Err(ErrorCode::ScopeDenied);
                }
                hantong_secrets::SecretStore::open(self.secrets_root)
                    .map_err(secret_error)?
                    .store_pending_imap_credential(*pending, i.draft().account_id(), i.endpoint())
                    .map(drop)
                    .map_err(secret_error)
            }
            CapturedCredential::Terminal(captured) => {
                let (ticket, handle, binding) = *captured;
                hantong_secrets::SecretStore::open(self.secrets_root)
                    .map_err(secret_error)?
                    .consume_terminal_imap_credential(
                        ticket,
                        handle,
                        binding,
                        i.draft().account_id(),
                        i.endpoint(),
                    )
                    .map(drop)
                    .map_err(secret_error)
            }
        }
    }
    fn finish_credential(
        &self,
        r: &ImapSetupRequest,
        result: Result<(), ErrorCode>,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        self.store.finish_imap_credential_store(r, result)
    }
    fn begin_probe(&self, r: &ImapSetupRequest) -> Result<ImapSetupRequest, ErrorCode> {
        self.store.begin_initial_imap_probe(r)
    }
    async fn probe(&self, r: &ImapSetupRequest) -> InitialImapProbeOutcome {
        let result = match hantong_secrets::SecretStore::open(self.secrets_root) {
            Ok(secrets) => {
                secrets
                    .probe_imap(r.intent().draft().account_id(), r.intent().endpoint())
                    .await
            }
            Err(e) => Err(hantong_secrets::MailProbeError::Credential(e)),
        };
        probe_outcome(result, r.intent().endpoint().settings_version())
    }
    fn finish_probe(
        &self,
        r: &ImapSetupRequest,
        outcome: &InitialImapProbeOutcome,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        self.store.finish_imap_setup_probe(r, outcome)
    }
}
fn probe_outcome(
    result: Result<hantong_secrets::ImapProbeReceipt, hantong_secrets::MailProbeError>,
    version: u64,
) -> InitialImapProbeOutcome {
    use hantong_secrets::MailProbeError as E;
    match result {
        Ok(receipt) if receipt.settings_version() == version => {
            InitialImapProbeOutcome::Ready(receipt.capabilities().to_ids())
        }
        Err(E::Authentication) => InitialImapProbeOutcome::AuthRequired,
        Err(E::Tls) => InitialImapProbeOutcome::TlsFailed,
        Err(E::Connection | E::Timeout) => InitialImapProbeOutcome::Offline,
        _ => InitialImapProbeOutcome::Error,
    }
}
#[cfg(windows)]
pub(super) async fn review(
    store: &Store,
    secrets_root: &std::path::Path,
    id: [u8; 32],
) -> Result<(), ErrorCode> {
    let expected = load(store, id)?;
    if expected.phase() != ImapSetupPhase::ReviewPending {
        return Err(ErrorCode::Conflict);
    }
    let i = expected.intent();
    let plan = hantong_secrets::ImapPasswordCapturePlan::new(
        i.draft().account_id(),
        i.endpoint(),
        *i.request_id(),
        *i.review_payload_hash(),
    )
    .map_err(secret_error)?;
    let review = review_snapshot(&expected, *plan.nonce())?;
    let config = store.load_config()?.ok_or(ErrorCode::InternalError)?;
    let verified = hantong_approval_windows::review_imap_setup(
        review.clone(),
        plan,
        config.gui.language,
        config.gui.theme,
    )
    .await;
    match verified.and_then(|v| v.consume(&review)) {
        Ok(pending) => {
            finish_verified(
                &StoreEffects {
                    store,
                    secrets_root,
                },
                &expected,
                CapturedCredential::Native(Box::new(pending)),
            )
            .await
        }
        Err(error) => {
            // Only native review failures can close a still-pending intent.
            let reason = match error {
                ErrorCode::ApprovalRequired
                | ErrorCode::ApprovalDenied
                | ErrorCode::ApprovalExpired
                | ErrorCode::ApprovalRateLimited
                | ErrorCode::HumanStepRequired
                | ErrorCode::CapabilityUnsupported
                | ErrorCode::ScopeDenied
                | ErrorCode::InvalidInput => error,
                _ => ErrorCode::InternalError,
            };
            store.close_imap_setup_review(&expected, reason)?;
            Err(error)
        }
    }
}

/// Completes an IMAP setup after the inherited-console helper has staged the
/// credential. The ticket binding is checked again before the existing store
/// transition and probe pipeline runs.
pub(super) async fn terminal_submit(
    store: &Store,
    secrets_root: &std::path::Path,
    id: [u8; 32],
    ticket: TerminalSecretTicket,
    handle: TerminalSecretHandle,
    expected_binding: TerminalSecretBinding,
) -> Result<(), ErrorCode> {
    let expected = load(store, id)?;
    finish_verified(
        &StoreEffects {
            store,
            secrets_root,
        },
        &expected,
        CapturedCredential::Terminal(Box::new((ticket, handle, expected_binding))),
    )
    .await
}

#[cfg(test)]
mod tests;
