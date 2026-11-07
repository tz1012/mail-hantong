//! A generic-mail intent binds fresh protected setup, never existing ready state.
use crate::account_connect::ObservedPeer;
use hantong_contracts::{
    ErrorCode,
    account_auth_peer::{CanonicalPeer, encode},
    terminal_review::TerminalReviewKind,
};
use hantong_domain::mail_connection::SmtpEndpointConfig;
use hantong_store::{
    ClientKind, DEFAULT_CLI_LABEL, RegistrationRecord, RegistrationState, Store,
    account_grants::MailGrantPurpose,
    account_provisioning::InitialImapProbeOutcome,
    full_mail::FullMailReadiness,
    imap_setup::{ImapSetupEvent, ImapSetupPhase, ImapSetupRequest},
    mail_connection::{MailConnectionState, MailEndpointKind, MailMutation},
};
use std::path::Path;

struct SmtpBinding {
    request_id: [u8; 32],
    endpoint: SmtpEndpointConfig,
    account_version: u64,
    payload_hash: [u8; 32],
    capture_submitted: bool,
    capture_required: bool,
}

pub(crate) struct GenericConnectIntent {
    id: [u8; 32],
    base: RegistrationRecord,
    baseline_hash: [u8; 32],
    server_pin: [u8; 32],
    peer: ObservedPeer,
    imap: ImapSetupRequest,
    imap_capture_submitted: bool,
    smtp: Option<SmtpBinding>,
    issued_at: i64,
    expires_at: i64,
}

impl GenericConnectIntent {
    pub(crate) fn registration_id(&self) -> [u8; 16] {
        self.base.registration_id
    }
    pub(crate) fn server_pin(&self) -> [u8; 32] {
        self.server_pin
    }
    pub(crate) fn peer(&self) -> &ObservedPeer {
        &self.peer
    }
    pub(crate) fn smtp_capture_required(&self) -> bool {
        self.smtp.as_ref().is_none_or(|smtp| smtp.capture_required)
    }
    pub(crate) fn new(
        store: &Store,
        base: &RegistrationRecord,
        id: [u8; 32],
        server_pin: [u8; 32],
        peer: ObservedPeer,
        request_id: [u8; 32],
        now: i64,
    ) -> Result<Self, ErrorCode> {
        if store.load_default_cli_registration()?.as_ref() != Some(base) {
            return Err(ErrorCode::ScopeDenied);
        }
        if id == [0; 32]
            || server_pin == [0; 32]
            || base.state != RegistrationState::Active
            || base.client_kind != ClientKind::Cli
            || base.display_label != DEFAULT_CLI_LABEL
            || now < 0
        {
            return Err(ErrorCode::ScopeDenied);
        }
        let imap = store.resolve_imap_setup_leaf(request_id)?;
        if imap.intent().request_id() != &request_id
            || imap.phase() != ImapSetupPhase::ReviewPending
            || imap.expires_at_utc_ms() <= now
        {
            return Err(ErrorCode::Conflict);
        }
        let expires_at = imap
            .expires_at_utc_ms()
            .min(now.checked_add(300_000).ok_or(ErrorCode::InvalidInput)?);
        Ok(Self {
            id,
            base: base.clone(),
            baseline_hash: store.full_mail_baseline_hash(base.registration_id)?,
            server_pin,
            peer,
            imap,
            imap_capture_submitted: false,
            smtp: None,
            issued_at: now,
            expires_at,
        })
    }

    pub(crate) fn account_id(&self) -> &str {
        self.imap.intent().draft().account_id()
    }
    pub(crate) fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at
    }
    pub(crate) fn imap_request_id(&self) -> [u8; 32] {
        *self.imap.intent().request_id()
    }

    pub(crate) fn matches_request(&self, kind: TerminalReviewKind, request: [u8; 32]) -> bool {
        (kind == TerminalReviewKind::ImapSetup && request == self.imap_request_id())
            || (kind == TerminalReviewKind::SmtpSetup
                && self
                    .smtp
                    .as_ref()
                    .is_some_and(|smtp| smtp.request_id == request))
    }
    pub(crate) fn validate_current(
        &self,
        store: &Store,
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<(), ErrorCode> {
        self.check_current(store, self.server_pin, peer, now)
    }

    pub(crate) fn check_capture(
        &self,
        kind: TerminalReviewKind,
        request: [u8; 32],
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<(), ErrorCode> {
        if !self.matches_request(kind, request) || peer != &self.peer {
            return Err(ErrorCode::ScopeDenied);
        }
        if now < self.issued_at || now >= self.expires_at {
            return Err(ErrorCode::ApprovalExpired);
        }
        let submitted = if kind == TerminalReviewKind::ImapSetup {
            self.imap_capture_submitted
        } else {
            self.smtp
                .as_ref()
                .is_some_and(|smtp| smtp.capture_submitted)
        };
        if submitted {
            return Err(ErrorCode::Conflict);
        }
        Ok(())
    }

    fn check_current(
        &self,
        store: &Store,
        pin: [u8; 32],
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<(), ErrorCode> {
        if now < self.issued_at || now >= self.expires_at {
            return Err(ErrorCode::ApprovalExpired);
        }
        if pin != self.server_pin
            || peer != &self.peer
            || store.load_default_cli_registration()?.as_ref() != Some(&self.base)
            || store.full_mail_baseline_hash(self.base.registration_id)? != self.baseline_hash
        {
            return Err(ErrorCode::Conflict);
        }
        Ok(())
    }

    /// Called only after the existing setup dispatcher accepts SecretSubmit for
    /// its actual kernel peer. This is not exposed in the app command codec.
    pub(crate) fn capture_submitted(
        &mut self,
        kind: TerminalReviewKind,
        request: [u8; 32],
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<(), ErrorCode> {
        self.check_capture(kind, request, peer, now)?;
        if kind == TerminalReviewKind::ImapSetup
            && request == self.imap_request_id()
            && !self.imap_capture_submitted
        {
            self.imap_capture_submitted = true;
            return Ok(());
        }
        if kind == TerminalReviewKind::SmtpSetup
            && let Some(smtp) = &mut self.smtp
            && smtp.request_id == request
            && !smtp.capture_submitted
        {
            smtp.capture_submitted = true;
            return Ok(());
        }
        Err(ErrorCode::Conflict)
    }

    fn imap_ready(&self, store: &Store) -> Result<(), ErrorCode> {
        let current = store.resolve_imap_setup_leaf(self.imap_request_id())?;
        if current.intent() != self.imap.intent()
            || current.phase() != ImapSetupPhase::ProbeFinished
            || current.version() <= self.imap.version()
            || !self.imap_capture_submitted
        {
            return Err(ErrorCode::Conflict);
        }
        let fresh = current.events().iter().filter(|event| {
            event.sequence() > self.imap.version() && event.occurred_at_utc_ms() >= self.issued_at
        });
        let mut consumed = false;
        let mut ready = false;
        for event in fresh {
            match event.event() {
                ImapSetupEvent::CredentialStoreReturned(Ok(())) => consumed = true,
                ImapSetupEvent::ProbeFinished(InitialImapProbeOutcome::Ready(_)) if consumed => {
                    ready = true
                }
                _ => {}
            }
        }
        if !ready {
            return Err(ErrorCode::AuthRequired);
        }
        Ok(())
    }

    pub(crate) fn bind_smtp(
        &mut self,
        store: &Store,
        sessions: &crate::setup_smtp::Sessions,
        request_id: [u8; 32],
        now: i64,
    ) -> Result<(), ErrorCode> {
        self.check_current(store, self.server_pin, &self.peer, now)?;
        self.imap_ready(store)?;
        if self.smtp.is_some() {
            return Err(ErrorCode::Conflict);
        }
        let (context, capture_required) =
            crate::setup_smtp::app_connect_context(store, sessions, request_id)?;
        if context.account_id != self.account_id() {
            return Err(ErrorCode::ScopeDenied);
        }
        self.expires_at = self.expires_at.min(context.expires_at_utc_ms);
        self.smtp = Some(SmtpBinding {
            request_id,
            endpoint: context.endpoint,
            account_version: context.account_version,
            payload_hash: context.payload_hash,
            capture_submitted: false,
            capture_required,
        });
        Ok(())
    }

    /// Network operations are authentication/catalog reads only. The returned
    /// move-only delta still needs a fresh cancellation check before staging.
    pub(crate) async fn authorize_ready(
        self,
        store: &Store,
        root: &Path,
        sessions: &crate::setup_smtp::Sessions,
        pin: [u8; 32],
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<VerifiedGenericDelta, ErrorCode> {
        self.check_current(store, pin, peer, now)?;
        self.imap_ready(store)?;
        let smtp = self.smtp.as_ref().ok_or(ErrorCode::Conflict)?;
        if smtp.capture_required
            && (!smtp.capture_submitted
                || crate::setup_smtp::status(sessions, smtp.request_id, true)?.state
                    != hantong_contracts::smtp_setup::SmtpSetupState::Ready)
        {
            return Err(ErrorCode::Conflict);
        }
        let provisioning = store.load_imap_provisioning_state(self.account_id())?;
        let config = store
            .load_mail_connection(self.account_id())?
            .ok_or(ErrorCode::NotFound)?;
        if provisioning.account_version != smtp.account_version
            || config.imap() != self.imap.intent().endpoint()
            || config.smtp() != Some(&smtp.endpoint)
        {
            return Err(ErrorCode::Conflict);
        }
        let secrets =
            hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
        // A prior Ready observation never mints a grant. Authenticate the exact
        // SMTP endpoint now, including when reusing an unchanged credential.
        let smtp_probe = secrets
            .probe_smtp(self.account_id(), &smtp.endpoint)
            .await
            .map_err(probe_error)?;
        if smtp_probe.settings_version() != smtp.endpoint.settings_version()
            || store.load_mail_connection(self.account_id())?.as_ref() != Some(&config)
        {
            return Err(ErrorCode::Conflict);
        }
        let expected_observation = store
            .load_mail_observation(self.account_id(), MailEndpointKind::Smtp)?
            .map_or(0, |o| o.observation_version);
        let snapshot = secrets
            .discover_imap_mailboxes(self.account_id(), config.imap())
            .await
            .map_err(probe_error)?;
        let expected = store
            .load_imap_mailbox_snapshot(self.account_id())?
            .map_or(0, |v| v.generation());
        if !snapshot.capabilities().move_messages()
            || snapshot
                .mailboxes()
                .iter()
                .filter(|mailbox| {
                    mailbox.selectable()
                        && mailbox.has_role(hantong_domain::mailboxes::SpecialUseRole::Trash)
                })
                .count()
                != 1
        {
            return Err(ErrorCode::CapabilityUnsupported);
        }
        Ok(VerifiedGenericDelta {
            intent: self,
            snapshot,
            config,
            account_version: provisioning.account_version,
            expected_catalog: expected,
            expected_smtp_observation: expected_observation,
        })
    }
}

pub(crate) struct VerifiedGenericDelta {
    intent: GenericConnectIntent,
    snapshot: hantong_domain::mailboxes::ImapMailboxSnapshot,
    config: hantong_domain::mail_connection::MailConnectionConfig,
    account_version: u64,
    expected_catalog: u64,
    expected_smtp_observation: u64,
}
impl VerifiedGenericDelta {
    pub(crate) fn stage(
        self,
        store: &Store,
        pin: [u8; 32],
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<([u8; 32], String), ErrorCode> {
        let intent = &self.intent;
        intent.check_current(store, pin, peer, now)?;
        if store.load_mail_connection(intent.account_id())?.as_ref() != Some(&self.config)
            || store
                .load_imap_provisioning_state(intent.account_id())?
                .account_version
                != self.account_version
            || store
                .load_imap_mailbox_snapshot(intent.account_id())?
                .map_or(0, |v| v.generation())
                != self.expected_catalog
            || store
                .load_mail_observation(intent.account_id(), MailEndpointKind::Smtp)?
                .map_or(0, |v| v.observation_version)
                != self.expected_smtp_observation
        {
            return Err(ErrorCode::Conflict);
        }
        let smtp = intent.smtp.as_ref().ok_or(ErrorCode::Conflict)?;
        // No await from the worker's cancellation check through these writes
        // and staging. A cancelled network probe has no metadata side effects.
        store.record_mail_observation(
            intent.account_id(),
            MailEndpointKind::Smtp,
            smtp.endpoint.settings_version(),
            self.expected_smtp_observation,
            MailConnectionState::Ready,
        )?;
        let operation = format!(
            "app-connect-{}",
            hantong_contracts::wire_hex::encode(&intent.id)
        );
        let payload = hantong_store::imap_catalog::imap_catalog_payload_hash(
            self.account_version,
            self.config.imap(),
            &self.snapshot,
            self.expected_catalog,
        )?;
        store.replace_imap_mailbox_snapshot(
            self.account_version,
            self.config.imap(),
            &self.snapshot,
            &MailMutation::new(operation.clone(), operation, self.expected_catalog, payload)?,
        )?;
        let FullMailReadiness::Ready(readiness) =
            store.observe_full_mail_readiness(intent.account_id())?
        else {
            return Err(ErrorCode::CapabilityUnsupported);
        };
        store.record_full_mail_readiness(&readiness)?;
        let canonical_peer = match peer {
            ObservedPeer::Windows(p) => CanonicalPeer::Windows(p.clone()),
            ObservedPeer::Unix(p) => CanonicalPeer::Unix(p.clone()),
        };
        let peer_hash = hantong_platform::sha256(&encode(&canonical_peer)?)?;
        let authority=serde_json::to_vec(&serde_json::json!({"domain":"mail-hantong.app-generic.full-mail.v2","intent":intent.id.as_slice(),"owner":intent.base.registration_id.as_slice(),"key":intent.base.public_key.as_slice(),"epoch":intent.base.profile_epoch,"version":intent.base.version,"baseline":intent.baseline_hash.as_slice(),"pin":pin.as_slice(),"peer":peer_hash.as_slice(),"imap_request":intent.imap_request_id().as_slice(),"imap_payload":intent.imap.intent().review_payload_hash().as_slice(),"imap_endpoint":intent.imap.intent().endpoint(),"smtp_request":smtp.request_id.as_slice(),"smtp_payload":smtp.payload_hash.as_slice(),"smtp_endpoint":smtp.endpoint,"account_version":readiness.account_version(),"catalog_generation":readiness.catalog_generation(),"trash":readiness.trash_binding_hash().as_slice()})).map_err(|_|ErrorCode::InternalError)?;
        let purpose = if intent
            .base
            .scope
            .accounts()
            .iter()
            .any(|id| id == intent.account_id())
        {
            MailGrantPurpose::Upgrade
        } else {
            MailGrantPurpose::Add
        };
        let operation = format!(
            "account-connect-{}",
            hantong_contracts::wire_hex::encode(&intent.id)
        );
        store.begin_full_mail_decision(
            intent.id,
            intent.base.registration_id,
            intent.base.version,
            intent.base.profile_epoch,
            intent.baseline_hash,
            intent.account_id(),
            purpose,
            &operation,
            &operation,
            peer_hash,
            intent.expires_at,
        )?;
        store.consume_and_stage_full_mail_decision(
            intent.base.registration_id,
            intent.id,
            intent.base.version,
            intent.base.profile_epoch,
            peer_hash,
            hantong_platform::sha256(&authority)?,
        )?;
        Ok((intent.id, intent.account_id().to_owned()))
    }
}

fn probe_error(error: hantong_secrets::MailProbeError) -> ErrorCode {
    use hantong_secrets::MailProbeError as E;
    match error {
        E::Authentication => ErrorCode::AuthRequired,
        E::Connection | E::Timeout | E::Tls => ErrorCode::Offline,
        E::MechanismUnsupported | E::UnsupportedPlatform => ErrorCode::CapabilityUnsupported,
        E::InvalidAccount => ErrorCode::InvalidInput,
        _ => ErrorCode::InternalError,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    include!("account_connect_generic_tests.rs");
}
