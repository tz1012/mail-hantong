//! Bounded setup intent and effect history. Stored data is never approval authority.
use crate::{Store, account_provisioning::InitialImapProbeOutcome, mail_connection::MailMutation};
use hantong_contracts::ErrorCode;
use hantong_domain::{
    account_provisioning::NewImapAccountDraft, mail_connection::ImapEndpointConfig,
};
#[cfg(windows)]
use rusqlite::TransactionBehavior;
use std::fmt;
#[cfg(windows)]
mod storage;
#[cfg(windows)]
use storage::*;

#[derive(Clone, Eq, PartialEq)]
pub struct NewImapSetupRequest {
    request_id: [u8; 32],
    draft: NewImapAccountDraft,
    endpoint: ImapEndpointConfig,
    mutation: MailMutation,
}
impl NewImapSetupRequest {
    pub fn new(
        request_id: [u8; 32],
        draft: NewImapAccountDraft,
        endpoint: ImapEndpointConfig,
        mutation: MailMutation,
    ) -> Result<Self, ErrorCode> {
        if request_id == [0; 32]
            || mutation.payload_hash() == &[0; 32]
            || mutation.expected_version() != 0
            || endpoint.settings_version() != 1
            || endpoint.authentication()
                != hantong_domain::mail_connection::Authentication::AppPassword
        {
            return Err(ErrorCode::InvalidInput);
        }
        let hash = crate::account_provisioning::imap_account_stage_payload_hash(&draft, &endpoint)?;
        if &hash != mutation.payload_hash() {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            request_id,
            draft,
            endpoint,
            mutation,
        })
    }
    pub fn request_id(&self) -> &[u8; 32] {
        &self.request_id
    }
    pub fn draft(&self) -> &NewImapAccountDraft {
        &self.draft
    }
    pub fn endpoint(&self) -> &ImapEndpointConfig {
        &self.endpoint
    }
    pub fn mutation(&self) -> &MailMutation {
        &self.mutation
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImapSetupPhase {
    ReviewPending,
    AccountStaged,
    ReviewClosed,
    CredentialStoreStarted,
    CredentialStoreReturned,
    ProbeStarted,
    ProbeFinished,
}
#[derive(Clone, Eq, PartialEq)]
pub enum ImapSetupEvent {
    RequestCreated,
    AccountStaged,
    ReviewClosed(ErrorCode),
    CredentialStoreStarted,
    CredentialStoreReturned(Result<(), ErrorCode>),
    ProbeStarted,
    ProbeFinished(InitialImapProbeOutcome),
}
#[derive(Clone, Eq, PartialEq)]
pub struct ImapSetupEventRecord {
    sequence: u64,
    occurred_at_utc_ms: i64,
    event: ImapSetupEvent,
}
impl ImapSetupEventRecord {
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    pub fn occurred_at_utc_ms(&self) -> i64 {
        self.occurred_at_utc_ms
    }
    pub fn event(&self) -> &ImapSetupEvent {
        &self.event
    }
}
#[derive(Clone, Eq, PartialEq)]
pub struct ImapSetupRequest {
    intent: NewImapSetupRequest,
    phase: ImapSetupPhase,
    version: u64,
    created_at_utc_ms: i64,
    expires_at_utc_ms: i64,
    updated_at_utc_ms: i64,
    events: Vec<ImapSetupEventRecord>,
}
impl ImapSetupRequest {
    pub fn intent(&self) -> &NewImapSetupRequest {
        &self.intent
    }
    pub fn phase(&self) -> ImapSetupPhase {
        self.phase
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn created_at_utc_ms(&self) -> i64 {
        self.created_at_utc_ms
    }
    pub fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
    pub fn updated_at_utc_ms(&self) -> i64 {
        self.updated_at_utc_ms
    }
    pub fn events(&self) -> &[ImapSetupEventRecord] {
        &self.events
    }
}
macro_rules! redacted_debug {
    ($($name:ident),+ $(,)?)=> {$(impl fmt::Debug for $name { fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result { f.write_str(concat!(stringify!($name)," { <redacted> }")) } })+};
}
redacted_debug!(
    NewImapSetupRequest,
    ImapSetupEvent,
    ImapSetupEventRecord,
    ImapSetupRequest
);

impl Store {
    pub fn create_imap_setup_request(
        &self,
        intent: &NewImapSetupRequest,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = intent;
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Immediate, |tx| create(tx, intent))
        }
    }
    pub fn load_imap_setup_request(
        &self,
        id: [u8; 32],
    ) -> Result<Option<ImapSetupRequest>, ErrorCode> {
        if id == [0; 32] {
            return Err(ErrorCode::InvalidInput);
        }
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Deferred, |tx| read_request(tx, &id))
        }
    }
    pub fn list_imap_setup_requests(&self) -> Result<Vec<ImapSetupRequest>, ErrorCode> {
        #[cfg(not(windows))]
        {
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Deferred, |tx| read_all(tx))
        }
    }
    pub fn stage_imap_setup_request(
        &self,
        expected: &ImapSetupRequest,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        self.change_setup(expected, ImapSetupEvent::AccountStaged)
    }
    pub fn close_imap_setup_review(
        &self,
        expected: &ImapSetupRequest,
        reason: ErrorCode,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        if !review_error(reason) {
            return Err(ErrorCode::InvalidInput);
        }
        self.change_setup(expected, ImapSetupEvent::ReviewClosed(reason))
    }
    pub fn begin_imap_credential_store(
        &self,
        expected: &ImapSetupRequest,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        self.change_setup(expected, ImapSetupEvent::CredentialStoreStarted)
    }
    pub fn finish_imap_credential_store(
        &self,
        expected: &ImapSetupRequest,
        result: Result<(), ErrorCode>,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        if result.is_err_and(|e| !credential_error(e)) {
            return Err(ErrorCode::InvalidInput);
        }
        self.change_setup(expected, ImapSetupEvent::CredentialStoreReturned(result))
    }
    pub fn begin_initial_imap_probe(
        &self,
        expected: &ImapSetupRequest,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        self.change_setup(expected, ImapSetupEvent::ProbeStarted)
    }
    pub fn finish_imap_setup_probe(
        &self,
        expected: &ImapSetupRequest,
        outcome: &InitialImapProbeOutcome,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        if let InitialImapProbeOutcome::Ready(values) = outcome {
            crate::account_provisioning::validate_capabilities(values)
                .map_err(|_| ErrorCode::InvalidInput)?;
        }
        self.change_setup(expected, ImapSetupEvent::ProbeFinished(outcome.clone()))
    }
    // Private dispatch keeps the public API specific to each durable boundary.
    fn change_setup(
        &self,
        expected: &ImapSetupRequest,
        event: ImapSetupEvent,
    ) -> Result<ImapSetupRequest, ErrorCode> {
        #[cfg(not(windows))]
        {
            let _ = (expected, event);
            Err(ErrorCode::CapabilityUnsupported)
        }
        #[cfg(windows)]
        {
            self.mail_transaction(TransactionBehavior::Immediate, |tx| {
                change(tx, expected, event)
            })
        }
    }
}

#[cfg(windows)]
pub(crate) fn validate_journal(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    read_all(connection)
        .map(|_| ())
        .map_err(|_| rusqlite::Error::InvalidQuery)
}

fn review_error(error: ErrorCode) -> bool {
    matches!(
        error,
        ErrorCode::ApprovalRequired
            | ErrorCode::ApprovalDenied
            | ErrorCode::ApprovalExpired
            | ErrorCode::ApprovalRateLimited
            | ErrorCode::HumanStepRequired
            | ErrorCode::CapabilityUnsupported
            | ErrorCode::ScopeDenied
            | ErrorCode::InvalidInput
            | ErrorCode::InternalError
    )
}
fn credential_error(error: ErrorCode) -> bool {
    matches!(
        error,
        ErrorCode::InvalidInput
            | ErrorCode::ScopeDenied
            | ErrorCode::ApprovalExpired
            | ErrorCode::CapabilityUnsupported
            | ErrorCode::InternalError
    )
}

#[cfg(test)]
mod tests;
