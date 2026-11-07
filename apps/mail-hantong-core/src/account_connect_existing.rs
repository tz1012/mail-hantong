//! Existing-account reconnect intent.
//!
//! A ready IMAP account is only a baseline.  This module binds a reconnect to
//! the exact active CLI registration, peer, endpoint and current full-mail
//! observation.  A fresh protected Secrets verification is required before a
//! delta can be produced; stored credentials alone never create a grant.

use crate::account_connect::ObservedPeer;
use hantong_contracts::ErrorCode;
use hantong_contracts::terminal_review::TerminalReviewKind;
use hantong_domain::mail_connection::ImapEndpointConfig;
use hantong_secrets::TerminalImapVerification;
use hantong_store::{
    ClientKind, DEFAULT_CLI_LABEL, RegistrationRecord, RegistrationState, Store,
    account_grants::MailGrantPurpose,
    full_mail::FullMailReadiness,
    mail_connection::{MailConnectionState, MailEndpointKind, MailMutation},
};
use std::fmt;
use std::path::Path;

const INTENT_TTL_MS: i64 = 5 * 60 * 1000;

fn valid_window(issued: i64, expires: i64, now: i64) -> bool {
    issued >= 0 && expires > issued && now >= issued && now < expires
}

pub(crate) struct ExistingConnectIntent {
    id: [u8; 32],
    base: RegistrationRecord,
    account_id: String,
    imap: ImapEndpointConfig,
    smtp_settings_version: Option<u64>,
    smtp_endpoint: Option<hantong_domain::mail_connection::SmtpEndpointConfig>,
    smtp_payload_hash: Option<[u8; 32]>,
    smtp_request_id: Option<[u8; 32]>,
    smtp_capture_required: bool,
    smtp_capture_submitted: bool,
    imap_capture_submitted: bool,
    account_version: u64,
    baseline_hash: [u8; 32],
    payload_hash: [u8; 32],
    server_pin: [u8; 32],
    peer: ObservedPeer,
    issued_at_utc_ms: i64,
    expires_at_utc_ms: i64,
}

impl fmt::Debug for ExistingConnectIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ExistingConnectIntent(<redacted>)")
    }
}

impl ExistingConnectIntent {
    pub(crate) fn new(
        store: &Store,
        caller: &RegistrationRecord,
        intent_id: [u8; 32],
        server_pin: [u8; 32],
        peer: ObservedPeer,
        account_id: String,
        now: i64,
    ) -> Result<Self, ErrorCode> {
        if store.load_default_cli_registration()?.as_ref() != Some(caller) {
            return Err(ErrorCode::ScopeDenied);
        }
        if intent_id == [0; 32]
            || server_pin == [0; 32]
            || account_id.is_empty()
            || account_id.len() > 320
            || now < 0
            || caller.registration_id == [0; 16]
            || caller.client_kind != ClientKind::Cli
            || caller.display_label != DEFAULT_CLI_LABEL
            || caller.state != RegistrationState::Active
            || caller.profile_epoch == 0
            || caller.version == 0
        {
            return Err(ErrorCode::InvalidInput);
        }
        let provisioning = store.load_imap_provisioning_state(&account_id)?;
        if provisioning.account_id != account_id
            || provisioning.status != hantong_domain::accounts::AccountStatus::Ready
            || provisioning.observation.as_ref().is_none_or(|v| {
                v.state != hantong_store::mail_connection::MailConnectionState::Ready
                    || v.observed_settings_version != provisioning.endpoint.settings_version()
            })
        {
            return Err(ErrorCode::Conflict);
        }
        let config = store
            .load_mail_connection(&account_id)?
            .ok_or(ErrorCode::NotFound)?;
        if config.account_id() != account_id || config.imap() != &provisioning.endpoint {
            return Err(ErrorCode::Conflict);
        }
        let expires_at_utc_ms = now
            .checked_add(INTENT_TTL_MS)
            .ok_or(ErrorCode::InvalidInput)?;
        let baseline_hash = store.full_mail_baseline_hash(caller.registration_id)?;
        let payload_hash = review_payload_hash(
            intent_id,
            caller,
            &account_id,
            &provisioning.endpoint,
            config.smtp().map(|v| v.settings_version()),
            config.smtp(),
            server_pin,
            &peer,
            expires_at_utc_ms,
        )?;
        Ok(Self {
            id: intent_id,
            base: caller.clone(),
            account_id,
            imap: provisioning.endpoint,
            smtp_settings_version: config.smtp().map(|v| v.settings_version()),
            smtp_endpoint: config.smtp().cloned(),
            smtp_payload_hash: None,
            smtp_request_id: None,
            smtp_capture_required: false,
            smtp_capture_submitted: false,
            imap_capture_submitted: false,
            account_version: provisioning.account_version,
            baseline_hash,
            payload_hash,
            server_pin,
            peer,
            issued_at_utc_ms: now,
            expires_at_utc_ms,
        })
    }

    pub(crate) fn account_id(&self) -> &str {
        &self.account_id
    }
    pub(crate) fn expires_at_utc_ms(&self) -> i64 {
        self.expires_at_utc_ms
    }
    #[cfg(test)]
    pub(crate) fn review_payload_hash(&self) -> [u8; 32] {
        self.payload_hash
    }
    pub(crate) fn server_pin(&self) -> [u8; 32] {
        self.server_pin
    }
    pub(crate) fn peer(&self) -> &ObservedPeer {
        &self.peer
    }
    pub(crate) fn endpoint(&self) -> &ImapEndpointConfig {
        &self.imap
    }
    pub(crate) fn smtp_capture_required(&self) -> bool {
        self.smtp_capture_required
    }
    pub(crate) fn matches_request(&self, kind: TerminalReviewKind, request: [u8; 32]) -> bool {
        ((kind == TerminalReviewKind::ImapSetup || kind == TerminalReviewKind::AccountConnectSetup)
            && request == self.id)
            || (kind == TerminalReviewKind::SmtpSetup && self.smtp_request_id == Some(request))
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
        if self.is_expired(now) {
            return Err(ErrorCode::ApprovalExpired);
        }
        let submitted = if kind == TerminalReviewKind::ImapSetup {
            self.imap_capture_submitted
        } else {
            self.smtp_capture_submitted
        };
        if submitted {
            return Err(ErrorCode::Conflict);
        }
        Ok(())
    }
    pub(crate) fn capture_submitted(
        &mut self,
        kind: TerminalReviewKind,
        request: [u8; 32],
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<(), ErrorCode> {
        self.check_capture(kind, request, peer, now)?;
        if (kind == TerminalReviewKind::ImapSetup
            || kind == TerminalReviewKind::AccountConnectSetup)
            && request == self.id
        {
            self.imap_capture_submitted = true;
            return Ok(());
        }
        if kind == TerminalReviewKind::SmtpSetup && self.smtp_request_id == Some(request) {
            self.smtp_capture_submitted = true;
            return Ok(());
        }
        Err(ErrorCode::Conflict)
    }

    pub(crate) fn bind_smtp(
        &mut self,
        store: &Store,
        sessions: &crate::setup_smtp::Sessions,
        request_id: [u8; 32],
        now: i64,
    ) -> Result<(), ErrorCode> {
        if self.smtp_request_id.is_some() || self.is_expired(now) {
            return Err(ErrorCode::Conflict);
        }
        self.validate_current(store, &self.peer, now)?;
        let (context, capture_required) =
            crate::setup_smtp::app_connect_context(store, sessions, request_id)?;
        if context.account_id != self.account_id
            || context.account_version != self.account_version
            || context.expires_at_utc_ms <= now
        {
            return Err(ErrorCode::Conflict);
        }
        self.smtp_settings_version = Some(context.endpoint.settings_version());
        self.expires_at_utc_ms = self.expires_at_utc_ms.min(context.expires_at_utc_ms);
        self.smtp_endpoint = Some(context.endpoint);
        self.smtp_payload_hash = Some(context.payload_hash);
        self.smtp_request_id = Some(request_id);
        self.smtp_capture_required = capture_required;
        self.payload_hash = review_payload_hash(
            self.id,
            &self.base,
            &self.account_id,
            &self.imap,
            self.smtp_settings_version,
            self.smtp_endpoint.as_ref(),
            self.server_pin,
            &self.peer,
            self.expires_at_utc_ms,
        )?;
        Ok(())
    }

    pub(crate) fn validate_current(
        &self,
        store: &Store,
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<(), ErrorCode> {
        if self.is_expired(now)
            || peer != &self.peer
            || store.load_default_cli_registration()?.as_ref() != Some(&self.base)
            || store.full_mail_baseline_hash(self.base.registration_id)? != self.baseline_hash
        {
            return Err(ErrorCode::Conflict);
        }
        let provisioning = store.load_imap_provisioning_state(&self.account_id)?;
        let config = store
            .load_mail_connection(&self.account_id)?
            .ok_or(ErrorCode::NotFound)?;
        let smtp_matches = match &self.smtp_endpoint {
            Some(endpoint) => {
                config.smtp() == Some(endpoint)
                    || (self.smtp_capture_required && config.smtp().is_none())
            }
            None => config.smtp().is_none(),
        };
        if provisioning.account_version != self.account_version
            || provisioning.endpoint != self.imap
            || provisioning.status != hantong_domain::accounts::AccountStatus::Ready
            || config.imap() != &self.imap
            || !smtp_matches
        {
            return Err(ErrorCode::Conflict);
        }
        Ok(())
    }

    pub(crate) fn is_expired(&self, now: i64) -> bool {
        !valid_window(self.issued_at_utc_ms, self.expires_at_utc_ms, now)
    }

    /// Metadata for the Core-owned protected review.  The ticket and secret
    /// capture are issued by the existing terminal-review/Secrets path.
    pub(crate) fn review_binding(&self) -> ExistingReviewBinding {
        ExistingReviewBinding {
            intent_id: self.id,
            registration_id: self.base.registration_id,
            account_id: self.account_id.clone(),
            endpoint_settings_version: self.imap.settings_version(),
            smtp_settings_version: self.smtp_settings_version,
            payload_hash: self.payload_hash,
            expires_at_utc_ms: self.expires_at_utc_ms,
        }
    }

    /// Verify a fresh protected IMAP credential witness and the unchanged
    /// baseline.  The witness is move-only and cannot be made from metadata.
    pub(crate) async fn verify(
        self,
        store: &Store,
        root: &Path,
        sessions: &crate::setup_smtp::Sessions,
        verification: TerminalImapVerification,
        now: i64,
    ) -> Result<VerifiedExistingDelta, ErrorCode> {
        if self.is_expired(now)
            || verification.account_id() != self.account_id
            || verification.endpoint() != &self.imap
            || verification.receipt().settings_version() != self.imap.settings_version()
            || verification.request_id() != &self.id
            || verification.payload_hash() != &self.payload_hash
            || verification.expires_at_utc_ms() != self.expires_at_utc_ms
            || verification.expires_at_utc_ms() <= now
            || store.full_mail_baseline_hash(self.base.registration_id)? != self.baseline_hash
        {
            return Err(ErrorCode::Conflict);
        }
        if self.smtp_capture_required {
            let request_id = self.smtp_request_id.ok_or(ErrorCode::Conflict)?;
            if !self.smtp_capture_submitted {
                return Err(ErrorCode::Conflict);
            }
            let context = crate::setup_smtp::terminal_context(sessions, request_id)?;
            if context.account_id != self.account_id
                || context.account_version != self.account_version
                || Some(&context.endpoint) != self.smtp_endpoint.as_ref()
                || context.payload_hash != self.smtp_payload_hash.ok_or(ErrorCode::Conflict)?
            {
                return Err(ErrorCode::Conflict);
            }
            let receipt = crate::setup_smtp::status(sessions, request_id, true)?;
            if receipt.request_id != request_id
                || receipt.state != hantong_contracts::smtp_setup::SmtpSetupState::Ready
                || receipt.error_code.is_some()
            {
                return Err(ErrorCode::Conflict);
            }
        }
        let secrets =
            hantong_secrets::SecretStore::open(root).map_err(|_| ErrorCode::InternalError)?;
        let expected_catalog = store
            .load_imap_mailbox_snapshot(&self.account_id)?
            .map_or(0, |value| value.generation());
        let expected_smtp_observation = store
            .load_mail_observation(&self.account_id, MailEndpointKind::Smtp)?
            .map_or(0, |value| value.observation_version);
        let config = store
            .load_mail_connection(&self.account_id)?
            .ok_or(ErrorCode::NotFound)?;
        let provisioning = store.load_imap_provisioning_state(&self.account_id)?;
        if provisioning.account_version != self.account_version
            || provisioning.endpoint != self.imap
            || provisioning.status != hantong_domain::accounts::AccountStatus::Ready
            || store.load_default_cli_registration()?.as_ref() != Some(&self.base)
            || store.full_mail_baseline_hash(self.base.registration_id)? != self.baseline_hash
            || config.smtp().map(|value| value.settings_version()) != self.smtp_settings_version
            || config.smtp() != self.smtp_endpoint.as_ref()
        {
            return Err(ErrorCode::Conflict);
        }
        let smtp = config.smtp().ok_or(ErrorCode::CapabilityUnsupported)?;
        let smtp_receipt = secrets
            .probe_smtp(&self.account_id, smtp)
            .await
            .map_err(probe_error)?;
        if smtp_receipt.settings_version() != smtp.settings_version() {
            return Err(ErrorCode::Conflict);
        }
        let snapshot = secrets
            .discover_imap_mailboxes(&self.account_id, &self.imap)
            .await
            .map_err(probe_error)?;
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
        let catalog_hash = snapshot_hash(&snapshot)?;
        let current_config = store
            .load_mail_connection(&self.account_id)?
            .ok_or(ErrorCode::NotFound)?;
        let current_provisioning = store.load_imap_provisioning_state(&self.account_id)?;
        if current_config != config
            || current_provisioning.account_version != self.account_version
            || current_provisioning.endpoint != self.imap
            || current_provisioning.status != hantong_domain::accounts::AccountStatus::Ready
            || store.load_default_cli_registration()?.as_ref() != Some(&self.base)
            || store.full_mail_baseline_hash(self.base.registration_id)? != self.baseline_hash
            || self.is_expired(now)
        {
            return Err(ErrorCode::Conflict);
        }
        Ok(VerifiedExistingDelta {
            intent: self,
            snapshot,
            catalog_hash,
            smtp_settings_version: smtp_receipt.settings_version(),
            expected_catalog,
            expected_smtp_observation,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec03_existing_reconnect_window_is_half_open() {
        assert!(!valid_window(10, 20, 9));
        assert!(valid_window(10, 20, 10));
        assert!(valid_window(10, 20, 19));
        assert!(!valid_window(10, 20, 20));
        assert!(!valid_window(20, 10, 15));
    }

    #[test]
    fn sec03_existing_missing_smtp_binds_new_setup_without_changing_rights() {
        use hantong_contracts::{
            imap_setup::ImapSetupTls, smtp_setup::SmtpSetupInput,
            terminal_review::TerminalReviewKind,
        };
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
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        let mut intent = ExistingConnectIntent::new(
            &f.store,
            &f.base,
            [9; 32],
            [8; 32],
            f.peer.clone(),
            "baseline-account".into(),
            now,
        )
        .unwrap();
        let before = intent.review_payload_hash();
        let sessions = crate::setup_smtp::Sessions::default();
        let input = SmtpSetupInput::new_with_sender(
            "baseline-account".into(),
            "smtp.example.test".into(),
            465,
            ImapSetupTls::ImplicitTls,
            "synthetic-user".into(),
            Some("sender@example.test".into()),
        )
        .unwrap();
        let receipt = crate::setup_smtp::request(&f.store, &f.root, &sessions, input).unwrap();
        intent
            .bind_smtp(&f.store, &sessions, receipt.request_id, now)
            .unwrap();
        assert_ne!(intent.review_payload_hash(), before);
        assert!(intent.smtp_capture_required());
        assert_eq!(intent.validate_current(&f.store, &f.peer, now), Ok(()));
        intent
            .capture_submitted(
                TerminalReviewKind::SmtpSetup,
                receipt.request_id,
                &f.peer,
                now,
            )
            .unwrap();
        assert_eq!(
            intent.capture_submitted(
                TerminalReviewKind::SmtpSetup,
                receipt.request_id,
                &f.peer,
                now
            ),
            Err(ErrorCode::Conflict)
        );
        assert_eq!(
            intent.validate_current(&f.store, &f.peer, intent.expires_at_utc_ms()),
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
        assert!(
            f.store
                .load_mail_connection("baseline-account")
                .unwrap()
                .unwrap()
                .smtp()
                .is_none()
        );
    }

    #[test]
    fn sec03_review_payload_hash_is_fixed_and_smtp_bound() {
        use hantong_contracts::unix_auth::{UnixPeerContextV2, UnixPlatform};
        use hantong_contracts::wire_hex;
        use hantong_domain::mail_connection::MailConnectionConfig;

        let config = MailConnectionConfig::from_json(
            br#"{
                "schema_version":1,
                "account_id":"acct-fixed",
                "imap":{
                    "settings_version":7,
                    "host":"imap.example.test",
                    "port":993,
                    "tls":"implicit_tls",
                    "login_name":"acct-fixed@example.test",
                    "authentication":"app_password"
                },
                "smtp":{
                    "settings_version":8,
                    "host":"smtp.example.test",
                    "port":465,
                    "tls":"implicit_tls",
                    "login_name":"acct-fixed@example.test",
                    "authentication":"app_password",
                    "sender_address":"acct-fixed@example.test"
                }
            }"#,
        )
        .unwrap();
        let changed_config = MailConnectionConfig::from_json(
            br#"{
                "schema_version":1,
                "account_id":"acct-fixed",
                "imap":{
                    "settings_version":7,
                    "host":"imap.example.test",
                    "port":993,
                    "tls":"implicit_tls",
                    "login_name":"acct-fixed@example.test",
                    "authentication":"app_password"
                },
                "smtp":{
                    "settings_version":8,
                    "host":"smtp.changed.test",
                    "port":465,
                    "tls":"implicit_tls",
                    "login_name":"acct-fixed@example.test",
                    "authentication":"app_password",
                    "sender_address":"acct-fixed@example.test"
                }
            }"#,
        )
        .unwrap();
        let scope = hantong_contracts::auth::RequestedScope::new(
            vec!["acct-fixed".to_owned()],
            vec!["mail.accounts.list".to_owned()],
        )
        .unwrap();
        let caller = RegistrationRecord {
            registration_id: [2; 16],
            request_id: [4; 32],
            client_kind: ClientKind::Cli,
            display_label: DEFAULT_CLI_LABEL.to_owned(),
            public_key: [3; 72],
            profile_epoch: 4,
            state: RegistrationState::Active,
            read_only: true,
            requests_per_minute: 1,
            approved_channel: "terminal_cli".to_owned(),
            version: 5,
            scope,
        };
        let peer = ObservedPeer::Unix(
            UnixPeerContextV2::new(UnixPlatform::Linux, 1000, 1000, 42, 7, [9; 32]).unwrap(),
        );
        let hash = review_payload_hash(
            [1; 32],
            &caller,
            "acct-fixed",
            config.imap(),
            Some(8),
            config.smtp(),
            [6; 32],
            &peer,
            1_700_000_000_000,
        )
        .unwrap();
        assert_eq!(
            wire_hex::encode(&hash),
            "8ef2aff41cada3e8bb9071359af4431add4d7d660de1d52e16cf7d9d37476c5e"
        );

        let changed = review_payload_hash(
            [1; 32],
            &caller,
            "acct-fixed",
            config.imap(),
            Some(8),
            changed_config.smtp(),
            [6; 32],
            &peer,
            1_700_000_000_000,
        )
        .unwrap();
        assert_ne!(hash, changed);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ExistingReviewBinding {
    pub(crate) intent_id: [u8; 32],
    pub(crate) registration_id: [u8; 16],
    pub(crate) account_id: String,
    pub(crate) endpoint_settings_version: u64,
    pub(crate) smtp_settings_version: Option<u64>,
    pub(crate) payload_hash: [u8; 32],
    pub(crate) expires_at_utc_ms: i64,
}

pub(crate) struct VerifiedExistingDelta {
    intent: ExistingConnectIntent,
    snapshot: hantong_domain::mailboxes::ImapMailboxSnapshot,
    catalog_hash: [u8; 32],
    smtp_settings_version: u64,
    expected_catalog: u64,
    expected_smtp_observation: u64,
}

impl fmt::Debug for VerifiedExistingDelta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VerifiedExistingDelta(<redacted>)")
    }
}

impl VerifiedExistingDelta {
    pub(crate) fn stage(
        self,
        store: &Store,
        server_pin: [u8; 32],
        peer: &ObservedPeer,
        now: i64,
    ) -> Result<([u8; 32], String), ErrorCode> {
        if server_pin != self.intent.server_pin {
            return Err(ErrorCode::Conflict);
        }
        self.intent.validate_current(store, peer, now)?;
        let config = store
            .load_mail_connection(&self.intent.account_id)?
            .ok_or(ErrorCode::NotFound)?;
        let expected_catalog = store
            .load_imap_mailbox_snapshot(&self.intent.account_id)?
            .map_or(0, |value| value.generation());
        let expected_observation = store
            .load_mail_observation(&self.intent.account_id, MailEndpointKind::Smtp)?
            .map_or(0, |value| value.observation_version);
        if expected_catalog != self.expected_catalog
            || expected_observation != self.expected_smtp_observation
        {
            return Err(ErrorCode::Conflict);
        }
        let smtp_settings_version = self
            .intent
            .smtp_settings_version
            .ok_or(ErrorCode::CapabilityUnsupported)?;
        store.record_mail_observation(
            &self.intent.account_id,
            MailEndpointKind::Smtp,
            smtp_settings_version,
            self.expected_smtp_observation,
            MailConnectionState::Ready,
        )?;
        let payload = hantong_store::imap_catalog::imap_catalog_payload_hash(
            self.intent.account_version,
            config.imap(),
            &self.snapshot,
            self.expected_catalog,
        )?;
        let operation = format!(
            "account-connect-existing-{}",
            hantong_contracts::wire_hex::encode(&self.intent.id)
        );
        store.replace_imap_mailbox_snapshot(
            self.intent.account_version,
            config.imap(),
            &self.snapshot,
            &MailMutation::new(
                operation.clone(),
                operation.clone(),
                self.expected_catalog,
                payload,
            )?,
        )?;
        let FullMailReadiness::Ready(readiness) =
            store.observe_full_mail_readiness(&self.intent.account_id)?
        else {
            return Err(ErrorCode::CapabilityUnsupported);
        };
        store.record_full_mail_readiness(&readiness)?;
        let operation = format!(
            "account-connect-existing-{}",
            hantong_contracts::wire_hex::encode(&self.intent.id)
        );
        let purpose = if self
            .intent
            .base
            .scope
            .accounts()
            .iter()
            .any(|id| id == &self.intent.account_id)
        {
            MailGrantPurpose::Upgrade
        } else {
            MailGrantPurpose::Add
        };
        store.begin_full_mail_decision(
            self.intent.id,
            self.intent.base.registration_id,
            self.intent.base.version,
            self.intent.base.profile_epoch,
            self.intent.baseline_hash,
            &self.intent.account_id,
            purpose,
            &operation,
            &operation,
            peer_digest(&self.intent.peer)?,
            self.intent.expires_at_utc_ms,
        )?;
        let authority = hantong_platform::sha256(
            format!(
                "existing-v2:{}:{}:{}:{}:{}:{}:{}",
                hantong_contracts::wire_hex::encode(&self.intent.id),
                self.intent.account_id,
                hantong_contracts::wire_hex::encode(&self.intent.payload_hash),
                hantong_contracts::wire_hex::encode(&self.catalog_hash),
                self.smtp_settings_version,
                hantong_contracts::wire_hex::encode(
                    &self.intent.smtp_request_id.unwrap_or([0; 32])
                ),
                hantong_contracts::wire_hex::encode(
                    &self.intent.smtp_payload_hash.unwrap_or([0; 32])
                ),
            )
            .as_bytes(),
        )?;
        store.consume_and_stage_full_mail_decision(
            self.intent.base.registration_id,
            self.intent.id,
            self.intent.base.version,
            self.intent.base.profile_epoch,
            peer_digest(&self.intent.peer)?,
            authority,
        )?;
        Ok((self.intent.id, self.intent.account_id))
    }
}

fn peer_digest(peer: &ObservedPeer) -> Result<[u8; 32], ErrorCode> {
    use hantong_contracts::account_auth_peer::{CanonicalPeer, encode};
    let canonical = match peer {
        ObservedPeer::Windows(value) => CanonicalPeer::Windows(value.clone()),
        ObservedPeer::Unix(value) => CanonicalPeer::Unix(value.clone()),
    };
    hantong_platform::sha256(&encode(&canonical)?)
}

#[allow(clippy::too_many_arguments)] // Canonical review binds every independent endpoint/consumer field.
fn review_payload_hash(
    intent_id: [u8; 32],
    caller: &RegistrationRecord,
    account: &str,
    imap: &ImapEndpointConfig,
    smtp_version: Option<u64>,
    smtp_endpoint: Option<&hantong_domain::mail_connection::SmtpEndpointConfig>,
    server_pin: [u8; 32],
    peer: &ObservedPeer,
    expires: i64,
) -> Result<[u8; 32], ErrorCode> {
    let endpoint = serde_json::to_vec(imap).map_err(|_| ErrorCode::InternalError)?;
    let peer = peer_digest(peer)?;
    let value = serde_json::json!({
        "domain": "mail-hantong.existing-account-review.v1",
        "intent": intent_id.as_slice(),
        "owner": caller.registration_id.as_slice(),
        "key": caller.public_key.as_slice(),
        "epoch": caller.profile_epoch,
        "version": caller.version,
        "account": account,
        "imap": endpoint,
        "smtp_settings_version": smtp_version,
        "smtp_endpoint": smtp_endpoint,
        "server": server_pin.as_slice(),
        "peer": peer.as_slice(),
        "expires": expires,
    });
    hantong_platform::sha256(&serde_json::to_vec(&value).map_err(|_| ErrorCode::InternalError)?)
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

fn snapshot_hash(
    snapshot: &hantong_domain::mailboxes::ImapMailboxSnapshot,
) -> Result<[u8; 32], ErrorCode> {
    let mut bytes = b"mail-hantong.existing-account-catalog.v1\0".to_vec();
    bytes.extend_from_slice(&(snapshot.account_id().len() as u32).to_be_bytes());
    bytes.extend_from_slice(snapshot.account_id().as_bytes());
    bytes.extend_from_slice(&snapshot.settings_version().to_be_bytes());
    bytes.push(u8::from(snapshot.capabilities().move_messages()));
    bytes.extend_from_slice(&(snapshot.mailboxes().len() as u32).to_be_bytes());
    for mailbox in snapshot.mailboxes() {
        let name = mailbox.name().wire_name();
        bytes.extend_from_slice(&(name.len() as u32).to_be_bytes());
        bytes.extend_from_slice(name);
        bytes.push(u8::from(mailbox.selectable()));
        bytes.extend_from_slice(&(mailbox.roles().len() as u32).to_be_bytes());
        for role in mailbox.roles() {
            bytes.push(*role as u8);
        }
    }
    hantong_platform::sha256(&bytes)
}
