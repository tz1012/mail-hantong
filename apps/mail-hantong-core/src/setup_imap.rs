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

pub(super) fn create(store: &Store, input: &ImapSetupInput) -> Result<ImapSetupRequest, ErrorCode> {
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
fn load(store: &Store, id: [u8; 32]) -> Result<ImapSetupRequest, ErrorCode> {
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
pub(super) fn status(
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
    )
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
    let current = effects.load(*expected.intent().request_id())?;
    if &current != expected || current.phase() != ImapSetupPhase::ReviewPending {
        return Err(ErrorCode::Conflict);
    }
    let staged = effects.stage(&current)?;
    let started = effects.begin_credential(&staged)?;
    let result = effects.store_credential(&started, pending);
    let returned = effects.finish_credential(&started, result)?;
    result?;
    let probing = effects.begin_probe(&returned)?;
    let outcome = effects.probe(&probing).await;
    effects.finish_probe(&probing, &outcome)?;
    Ok(())
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
struct Native<'a> {
    store: &'a Store,
    secrets_root: &'a std::path::Path,
}
impl Effects for Native<'_> {
    type Pending = hantong_secrets::PendingImapCredential;
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
        if pending.setup_request_id() != i.request_id()
            || pending.setup_payload_hash() != i.review_payload_hash()
        {
            return Err(ErrorCode::ScopeDenied);
        }
        hantong_secrets::SecretStore::open(self.secrets_root)
            .map_err(secret_error)?
            .store_pending_imap_credential(pending, i.draft().account_id(), i.endpoint())
            .map(drop)
            .map_err(secret_error)
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
            let c = receipt.capabilities();
            let caps = [
                ("imap.condstore", c.condstore()),
                ("imap.idle", c.idle()),
                ("imap.move", c.move_messages()),
                ("imap.qresync", c.qresync()),
                ("imap.special_use", c.special_use()),
                ("imap.uidplus", c.uidplus()),
            ]
            .into_iter()
            .filter(|(_, v)| *v)
            .map(|(s, _)| s.to_owned())
            .collect();
            InitialImapProbeOutcome::Ready(caps)
        }
        Err(E::Authentication) => InitialImapProbeOutcome::AuthRequired,
        Err(E::Tls) => InitialImapProbeOutcome::TlsFailed,
        Err(E::Connection | E::Timeout) => InitialImapProbeOutcome::Offline,
        _ => InitialImapProbeOutcome::Error,
    }
}
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
                &Native {
                    store,
                    secrets_root,
                },
                &expected,
                pending,
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

#[cfg(test)]
mod tests;
