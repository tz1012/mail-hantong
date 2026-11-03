//! Trusted Core registration coordinator. IPC requests carry no approval data.
use hantong_approval::RegistrationReview;
use hantong_contracts::imap_setup::ImapSetupCommand;
use hantong_contracts::scope_refresh::ScopeRefreshCommand;
use hantong_contracts::{
    ErrorCode,
    auth::RequestedScope,
    registration::{AiProvider, RegistrationCommand, RegistrationProgress, RegistrationReceipt},
};
use hantong_ipc_client::profile::ParsedClientProfile;
use hantong_ipc_server::setup_dispatch::{SetupCommand, SetupReceipt};
use hantong_platform::{ClientSigningKey, ServerSigningKey};
use hantong_secrets::{SecretStore, TerminalSecretBinding, TerminalSecretTicket};
use hantong_store::{
    AiRegistrationStage, CliRegistrationStage, ClientKind, DEFAULT_AI_CLAUDE_LABEL,
    DEFAULT_AI_CODEX_LABEL, DEFAULT_CLI_LABEL, DEFAULT_GUI_LABEL, DEFAULT_MCP_LABEL,
    GuiRegistrationStage, McpMailRegistrationStage, McpRegistrationStage, RegistrationRecord,
    RegistrationRequest, RegistrationRequestState, RegistrationState, ServerIdentity, Store,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
enum Job {
    Registration([u8; 32]),
    Imap([u8; 32]),
    Smtp([u8; 32]),
    Google([u8; 32]),
    ScopeRefresh([u8; 32]),
    TerminalSend([u8; 32]),
    TerminalProtectedSend([u8; 32]),
    TerminalGoogleMutation([u8; 32]),
    TerminalImapTrash([u8; 32]),
    TerminalImap([u8; 32]),
    TerminalSmtp([u8; 32]),
}

pub(crate) struct RegistrationHandle<'a> {
    state: Rc<Shared<'a>>,
    sender: mpsc::Sender<Job>,
}
pub(crate) struct RegistrationWorker<'a> {
    state: Rc<Shared<'a>>,
    receiver: mpsc::Receiver<Job>,
}
#[cfg(test)]
impl RegistrationWorker<'_> {
    pub(crate) fn has_pending_job(&self) -> bool {
        self.state.scheduled.get().is_some()
    }
}
impl Drop for RegistrationWorker<'_> {
    fn drop(&mut self) {
        self.state.scheduled.set(None);
    }
}

fn registration_terminal_hash(request: &RegistrationRequest) -> Result<[u8; 32], ErrorCode> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "domain":"mail-hantong.terminal-registration.v1", "request_id":request.request_id.as_slice(),
        "client_kind":request.client_kind.as_str(), "display_label":request.display_label,
        "accounts":request.scope.accounts(), "tools":request.scope.tools(),
        "version":request.version, "created":request.created_at_utc_ms, "expires":request.expires_at_utc_ms,
    })).map_err(|_|ErrorCode::InternalError)?;
    hantong_platform::sha256(&bytes)
}

fn imap_trash_terminal_hash(
    pending: &hantong_store::imap_trash::PendingImapTrash,
) -> Result<[u8; 32], ErrorCode> {
    let endpoint = serde_json::to_vec(&pending.endpoint).map_err(|_| ErrorCode::InternalError)?;
    let mut bytes = b"mail-hantong.imap.trash.terminal-review.v1\0".to_vec();
    for field in [
        pending.review_id.to_vec(),
        pending.request.payload_bytes(),
        pending.owner_registration_id.to_vec(),
        pending.profile_epoch.to_be_bytes().to_vec(),
        pending.account_version.to_be_bytes().to_vec(),
        endpoint,
        pending.destination.mailbox_wire_hex.as_bytes().to_vec(),
        pending.destination.mailbox_encoding.as_bytes().to_vec(),
        pending.created_at_utc_ms.to_be_bytes().to_vec(),
        pending.expires_at_utc_ms.to_be_bytes().to_vec(),
    ] {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&field);
    }
    hantong_platform::sha256(&bytes)
}

fn google_terminal_hash(
    identity: &hantong_contracts::google::GoogleIdentity,
    binding: &hantong_contracts::google::GoogleAccountBinding,
) -> Result<[u8; 32], ErrorCode> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "domain":"mail-hantong.terminal-google.v1", "account":binding.account_id,
        "subject":identity.subject, "email":identity.email, "client_id":binding.client_id,
        "mode":binding.access_mode.as_str(), "credential_version":binding.credential_version,
        "granted_scopes":identity.granted_scopes,
    }))
    .map_err(|_| ErrorCode::InternalError)?;
    hantong_platform::sha256(&bytes)
}

struct TerminalBindingInput<'a> {
    kind: [u8; 16],
    request_id: [u8; 32],
    payload_hash: [u8; 32],
    account_id: &'a str,
    endpoint: &'a serde_json::Value,
    version: u64,
    expires_at_utc_ms: i64,
    peer: &'a hantong_contracts::auth::PeerContext,
    server: &'a ServerSigningKey,
}

fn terminal_binding(input: &TerminalBindingInput<'_>) -> Result<TerminalSecretBinding, ErrorCode> {
    let endpoint_bytes =
        serde_json::to_vec(input.endpoint).map_err(|_| ErrorCode::InternalError)?;
    let mut canonical = Vec::new();
    for value in [
        b"terminal-endpoint-v1".as_slice(),
        &input.kind,
        input.account_id.as_bytes(),
        &endpoint_bytes,
    ] {
        let len = u32::try_from(value.len()).map_err(|_| ErrorCode::InvalidInput)?;
        canonical.extend_from_slice(&len.to_le_bytes());
        canonical.extend_from_slice(value);
    }
    let hash = |value: &[u8]| hantong_platform::sha256(value);
    TerminalSecretBinding::new(
        input.kind,
        input.request_id,
        input.payload_hash,
        hash(input.account_id.as_bytes())?,
        hash(&canonical)?,
        input.version,
        hash(input.peer.user_sid().as_bytes())?,
        hash(input.peer.logon_sid().as_bytes())?,
        hash(&input.server.public_key()?)?,
        input.expires_at_utc_ms,
    )
    .map_err(|_| ErrorCode::InvalidInput)
}

fn setup_fields(
    account_id: &str,
    endpoint: &serde_json::Value,
) -> Result<Vec<hantong_contracts::terminal_review::TerminalReviewField>, ErrorCode> {
    let value = endpoint;
    Ok(vec![
        crate::terminal_review::field("account", account_id.to_owned())?,
        crate::terminal_review::field("endpoint", value.to_string())?,
        crate::terminal_review::field("role", "terminal_setup".into())?,
    ])
}
struct Shared<'a> {
    store: &'a Store,
    server: &'a ServerSigningKey,
    scheduled: Cell<Option<Job>>,
    secrets_root: &'a std::path::Path,
    policy: Rc<RefCell<hantong_policy::ReadPolicy>>,
    smtp_sessions: crate::setup_smtp::Sessions,
    google_sessions: crate::setup_google::Sessions,
    terminal_reviews: RefCell<std::collections::BTreeMap<[u8; 32], crate::terminal_review::Entry>>,
    terminal_captures:
        RefCell<std::collections::BTreeMap<[u8; 32], crate::terminal_review::TerminalCapture>>,
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
        google_sessions: RefCell::new(std::collections::BTreeMap::new()),
        terminal_reviews: RefCell::new(std::collections::BTreeMap::new()),
        terminal_captures: RefCell::new(std::collections::BTreeMap::new()),
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
        if self.state.scheduled.get().is_some() {
            return Err(ErrorCode::RateLimited);
        }
        let receipt = self
            .state
            .store
            .request_mail_send_review(owner, epoch, request)?;
        if receipt.state == "pending" {
            let pending = self.state.store.pending_mail_send(receipt.review_id)?;
            if self.state.store.can_confirm_personal_mail_send(&pending)? {
                // The legacy request is intentionally not allowed to open
                // a native window for the personal terminal path.
                return Err(ErrorCode::HumanStepRequired);
            }
            // All CLI/TUI send reviews are issued through terminal.review;
            // do not schedule a background native window here.
        }
        Ok(receipt)
    }

    pub(crate) fn prepare_personal_send(
        &self,
        owner: [u8; 16],
        epoch: u64,
        request: &hantong_contracts::mail_send::SendReviewRequest,
    ) -> Result<serde_json::Value, ErrorCode> {
        let receipt = self
            .state
            .store
            .request_mail_send_review(owner, epoch, request)?;
        if receipt.state != "pending" {
            return crate::send_mail::render_with_preview(&receipt, None);
        }
        let pending = self.state.store.pending_mail_send(receipt.review_id)?;
        if !self.state.store.can_confirm_personal_mail_send(&pending)? {
            self.state
                .store
                .deny_mail_send(receipt.review_id, ErrorCode::PolicyBlocked)?;
            return Err(ErrorCode::PolicyBlocked);
        }
        crate::send_mail::render_with_preview(&receipt, Some(&pending))
    }

    pub(crate) fn confirm_personal_send(
        &self,
        owner: [u8; 16],
        epoch: u64,
        request: &hantong_contracts::mail_send::PersonalSendConfirmRequest,
    ) -> Result<hantong_store::mail_send::SendReceipt, ErrorCode> {
        let current = self.state.store.mail_send_receipt(
            owner,
            epoch,
            &request.account_id,
            request.review_id,
        )?;
        if current.state != "pending" {
            return Ok(current);
        }
        let pending = self.state.store.pending_mail_send(request.review_id)?;
        if pending.owner_registration_id != owner
            || pending.profile_epoch != epoch
            || pending.request.account_id() != request.account_id
            || pending.request.payload_hash() != &request.payload_hash
            || !self.state.store.can_confirm_personal_mail_send(&pending)?
        {
            return Err(ErrorCode::PolicyBlocked);
        }
        if self.sender.is_closed() {
            return Err(ErrorCode::CoreUnavailable);
        }
        match self.state.scheduled.get() {
            Some(Job::TerminalSend(id)) if id == request.review_id => {}
            Some(_) => return Err(ErrorCode::RateLimited),
            None => enqueue(
                &self.state.scheduled,
                &self.sender,
                Job::TerminalSend(request.review_id),
            )?,
        }
        self.state
            .store
            .mail_send_receipt(owner, epoch, &request.account_id, request.review_id)
    }

    pub(crate) fn cancel_personal_send(
        &self,
        owner: [u8; 16],
        epoch: u64,
        request: &hantong_contracts::mail_send::SendReviewStatusRequest,
    ) -> Result<hantong_store::mail_send::SendReceipt, ErrorCode> {
        let current = self.state.store.mail_send_receipt(
            owner,
            epoch,
            &request.account_id,
            request.review_id,
        )?;
        if current.state != "pending" {
            return Ok(current);
        }
        let pending = self.state.store.pending_mail_send(request.review_id)?;
        if pending.owner_registration_id != owner
            || pending.profile_epoch != epoch
            || pending.request.account_id() != request.account_id
            || !self.state.store.can_confirm_personal_mail_send(&pending)?
        {
            return Err(ErrorCode::PolicyBlocked);
        }
        self.state
            .store
            .deny_mail_send(request.review_id, ErrorCode::ApprovalRequired)?;
        self.state
            .store
            .mail_send_receipt(owner, epoch, &request.account_id, request.review_id)
    }

    /// Handles the foreground terminal review bootstrap surface. The pipe
    /// supplies `observed_peer`; no caller-provided PID, role or approval bit
    /// is accepted. Tickets are kept in Core memory and consumed once.
    pub(crate) fn terminal_review_command(
        &self,
        command: hantong_contracts::terminal_review::TerminalReviewCommand,
        observed_peer: &hantong_contracts::auth::PeerContext,
    ) -> Result<hantong_contracts::terminal_review::TerminalReviewReceipt, ErrorCode> {
        use hantong_contracts::terminal_review::{TerminalReviewCommand, TerminalReviewKind};
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ErrorCode::InternalError)?
            .as_millis()
            .try_into()
            .map_err(|_| ErrorCode::InternalError)?;
        match command {
            TerminalReviewCommand::Get(request) => {
                if request.kind == TerminalReviewKind::ImapTrash {
                    let pending = self.state.store.pending_imap_trash(request.request_id)?;
                    if request
                        .account_id
                        .as_deref()
                        .is_some_and(|a| a != pending.request.account_id())
                        || (request.payload_hash != [0; 32]
                            && request.payload_hash != *pending.request.payload_hash())
                        || !self.state.store.can_confirm_imap_trash(&pending)?
                    {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    let fields = vec![
                        crate::terminal_review::field(
                            "account",
                            pending.request.account_id().into(),
                        )?,
                        crate::terminal_review::field("action", "move_to_trash".into())?,
                        crate::terminal_review::field("mail_login", pending.endpoint.login_name().into())?,
                        crate::terminal_review::field("mail_server", pending.endpoint.host().into())?,
                        crate::terminal_review::field(
                            "source_mailbox",
                            hantong_application::validated_uid_mailbox(pending.request.mailbox())?.display_name().into(),
                        )?,
                        crate::terminal_review::field(
                            "destination_mailbox",
                            hantong_application::validated_uid_mailbox(
                                &hantong_contracts::uid_page::UidMailboxIdentity::new(
                                    &pending.destination.mailbox_wire_hex,
                                    match pending.destination.mailbox_encoding.as_str() {
                                        "utf8" => hantong_contracts::uid_page::UidMailboxEncoding::Utf8,
                                        "modified_utf7" => hantong_contracts::uid_page::UidMailboxEncoding::ModifiedUtf7,
                                        _ => return Err(ErrorCode::InternalError),
                                    },
                                )?,
                            )?.display_name().into(),
                        )?,
                        crate::terminal_review::field(
                            "uidvalidity",
                            pending.request.uidvalidity().to_string(),
                        )?,
                        crate::terminal_review::field("uid", pending.request.uid().to_string())?,
                    ];
                    return crate::terminal_review::make_entry_ticket(
                        &mut self.state.terminal_reviews.borrow_mut(),
                        hantong_platform::random_nonce()?,
                        request.kind,
                        pending.review_id,
                        pending.request.account_id().into(),
                        imap_trash_terminal_hash(&pending)?,
                        pending.owner_registration_id,
                        pending.profile_epoch,
                        pending.expires_at_utc_ms,
                        observed_peer.clone(),
                        now,
                        fields,
                    );
                }
                if request.kind == TerminalReviewKind::GoogleMutation {
                    let pending = self
                        .state
                        .store
                        .pending_google_mutation(request.request_id)?;
                    let owner = self
                        .state
                        .store
                        .load_registration(pending.owner_registration_id)?
                        .ok_or(ErrorCode::ClientNotRegistered)?;
                    if !matches!(owner.client_kind, ClientKind::Cli | ClientKind::Ai)
                        || owner.state != RegistrationState::Active
                        || owner.profile_epoch != pending.profile_epoch
                    {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    if owner.client_kind == ClientKind::Ai {
                        let provider = if owner.display_label == DEFAULT_AI_CODEX_LABEL {
                            AiProvider::Codex
                        } else if owner.display_label == DEFAULT_AI_CLAUDE_LABEL {
                            AiProvider::Claude
                        } else {
                            return Err(ErrorCode::PolicyBlocked);
                        };
                        self.state.store.check_ai_consent(
                            provider,
                            owner.registration_id,
                            owner.profile_epoch,
                            std::slice::from_ref(&pending.request.account_id().to_owned()),
                        )?;
                    }
                    if request
                        .account_id
                        .as_deref()
                        .is_some_and(|a| a != pending.request.account_id())
                        || (request.payload_hash != [0; 32]
                            && request.payload_hash != *pending.request.payload_hash())
                    {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    let mut fields = vec![
                        crate::terminal_review::field(
                            "account",
                            pending.request.account_id().into(),
                        )?,
                        crate::terminal_review::field("sender", pending.sender.clone())?,
                        crate::terminal_review::field(
                            "operation",
                            pending.request.to_json().to_string(),
                        )?,
                        crate::terminal_review::field(
                            "before_labels",
                            pending.before_labels.join(", "),
                        )?,
                    ];
                    if let hantong_contracts::google_mutation::MutationPayload::Trash {
                        message_id,
                        ..
                    } = &pending.request
                    {
                        let message = self
                            .state
                            .store
                            .google_message(pending.request.account_id(), message_id)?;
                        fields.extend([
                            crate::terminal_review::field("action", "move_to_trash".into())?,
                            crate::terminal_review::field("subject", message.subject)?,
                            crate::terminal_review::field("message_from", message.sender)?,
                            crate::terminal_review::field("message_excerpt", message.snippet)?,
                        ]);
                    }
                    return crate::terminal_review::make_entry_ticket(
                        &mut self.state.terminal_reviews.borrow_mut(),
                        hantong_platform::random_nonce()?,
                        TerminalReviewKind::GoogleMutation,
                        pending.review_id,
                        pending.request.account_id().into(),
                        *pending.request.payload_hash(),
                        pending.owner_registration_id,
                        pending.profile_epoch,
                        pending.expires_at_utc_ms,
                        observed_peer.clone(),
                        now,
                        fields,
                    );
                }
                if request.kind == TerminalReviewKind::ProtectedSend {
                    let pending = self.state.store.pending_mail_send(request.request_id)?;
                    let owner = self
                        .state
                        .store
                        .load_registration(pending.owner_registration_id)?
                        .ok_or(ErrorCode::ClientNotRegistered)?;
                    if owner.state != RegistrationState::Active
                        || owner.profile_epoch != pending.profile_epoch
                    {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    if owner.client_kind == ClientKind::Ai {
                        let provider = if owner.display_label == DEFAULT_AI_CODEX_LABEL {
                            AiProvider::Codex
                        } else if owner.display_label == DEFAULT_AI_CLAUDE_LABEL {
                            AiProvider::Claude
                        } else {
                            return Err(ErrorCode::PolicyBlocked);
                        };
                        self.state.store.check_ai_consent(
                            provider,
                            owner.registration_id,
                            owner.profile_epoch,
                            std::slice::from_ref(&pending.request.account_id().to_owned()),
                        )?;
                    } else if owner.client_kind != ClientKind::Cli {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    if request
                        .account_id
                        .as_deref()
                        .is_some_and(|a| a != pending.request.account_id())
                        || (request.payload_hash != [0; 32]
                            && request.payload_hash != *pending.request.payload_hash())
                        || self.state.store.can_confirm_personal_mail_send(&pending)?
                    {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    let preview = crate::send_mail::preview(&pending)?;
                    let fields = ["from", "to", "subject", "body", "attachments"]
                        .into_iter()
                        .filter_map(|key| preview.get(key).map(|v| (key, v)))
                        .map(|(key, value)| {
                            let text = if key == "body" || key == "from" || key == "subject" {
                                value.as_str().unwrap_or_default().to_owned()
                            } else {
                                serde_json::to_string(value).unwrap_or_default()
                            };
                            crate::terminal_review::field(key, text)
                        })
                        .collect::<Result<Vec<_>, ErrorCode>>()?;
                    return crate::terminal_review::make_entry_ticket(
                        &mut self.state.terminal_reviews.borrow_mut(),
                        hantong_platform::random_nonce()?,
                        TerminalReviewKind::ProtectedSend,
                        pending.review_id,
                        pending.request.account_id().into(),
                        *pending.request.payload_hash(),
                        pending.owner_registration_id,
                        pending.profile_epoch,
                        pending.expires_at_utc_ms,
                        observed_peer.clone(),
                        now,
                        fields,
                    );
                }
                if request.kind == TerminalReviewKind::ScopeRefresh {
                    let refresh = self
                        .state
                        .store
                        .load_cli_scope_refresh(request.request_id)?
                        .ok_or(ErrorCode::NotFound)?;
                    if refresh.phase() != hantong_store::scope_refresh::ScopeRefreshPhase::Pending
                        || refresh.decision_channel() != "terminal_cli"
                        || (request.payload_hash != [0; 32]
                            && refresh.payload_hash() != &request.payload_hash)
                        || request
                            .account_id
                            .as_deref()
                            .is_some_and(|a| !refresh.target_accounts().iter().any(|v| v == a))
                    {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    let fields = vec![
                        crate::terminal_review::field(
                            "accounts",
                            refresh.target_accounts().join(", "),
                        )?,
                        crate::terminal_review::field("tools", refresh.target_tools().join(", "))?,
                        crate::terminal_review::field(
                            "profile_epoch",
                            refresh.target_profile_epoch().to_string(),
                        )?,
                    ];
                    return crate::terminal_review::make_entry_ticket(
                        &mut self.state.terminal_reviews.borrow_mut(),
                        hantong_platform::random_nonce()?,
                        TerminalReviewKind::ScopeRefresh,
                        *refresh.refresh_id(),
                        refresh
                            .target_accounts()
                            .first()
                            .cloned()
                            .unwrap_or_default(),
                        *refresh.payload_hash(),
                        refresh.base_registration().registration_id,
                        refresh.base_registration().profile_epoch,
                        refresh.expires_at_utc_ms(),
                        observed_peer.clone(),
                        now,
                        fields,
                    );
                }
                if request.kind == TerminalReviewKind::Registration {
                    let registration = self
                        .state
                        .store
                        .get_registration_request(request.request_id)?
                        .ok_or(ErrorCode::NotFound)?;
                    if !matches!(
                        registration.client_kind,
                        ClientKind::Cli | ClientKind::Mcp | ClientKind::Ai
                    ) || registration.state != RegistrationRequestState::Pending
                        || !supported_identity(
                            registration.client_kind,
                            &registration.display_label,
                            registration.scope.tools(),
                        )
                    {
                        return Err(ErrorCode::Conflict);
                    }
                    let owner =
                        hantong_platform::registration_id_for_request(registration.request_id)?;
                    let authoritative_hash = registration_terminal_hash(&registration)?;
                    if request.payload_hash != [0; 32] && request.payload_hash != authoritative_hash
                    {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    let account_labels = self
                        .state
                        .store
                        .list_account_summaries(registration.scope.accounts(), false)?
                        .items
                        .into_iter()
                        .enumerate()
                        .map(|(index, account)| {
                            account
                                .primary_address
                                .filter(|v| !v.is_empty())
                                .or(account.display_name.filter(|v| !v.is_empty()))
                                .unwrap_or_else(|| format!("Account {}", index + 1))
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let mut fields = vec![
                        crate::terminal_review::field("client", registration.display_label.clone())?,
                        if registration.client_kind == ClientKind::Ai {
                            crate::terminal_review::field(
                                "provider",
                                if registration.display_label == DEFAULT_AI_CODEX_LABEL {
                                    "codex".to_string()
                                } else {
                                    "claude".to_string()
                                },
                            )?
                        } else {
                            crate::terminal_review::field("provider", "none".to_string())?
                        },
                        crate::terminal_review::field(
                            "accounts",
                            account_labels,
                        )?,
                        crate::terminal_review::field(
                            "tools",
                            registration.scope.tools().join(", "),
                        )?,
                        crate::terminal_review::field(
                            "mail_transfer",
                            "selected account mail metadata/body may be sent to the selected AI provider".to_string(),
                        )?,
                    ];
                    if registration.client_kind == ClientKind::Ai {
                        fields.push(crate::terminal_review::field(
                            "feature",
                            "Connect all listed accounts (explicit confirmation required)"
                                .to_string(),
                        )?);
                    }
                    return crate::terminal_review::make_entry_ticket(
                        &mut self.state.terminal_reviews.borrow_mut(),
                        hantong_platform::random_nonce()?,
                        TerminalReviewKind::Registration,
                        registration.request_id,
                        registration
                            .scope
                            .accounts()
                            .first()
                            .cloned()
                            .unwrap_or_default(),
                        authoritative_hash,
                        owner,
                        1,
                        registration.expires_at_utc_ms,
                        observed_peer.clone(),
                        now,
                        fields,
                    );
                }
                if request.kind == TerminalReviewKind::GoogleSetup {
                    let (identity, binding, expires_at_utc_ms) =
                        crate::setup_google::advanced_terminal_preview(
                            &self.state.google_sessions,
                            request.request_id,
                            observed_peer,
                        )?;
                    let payload_hash = google_terminal_hash(&identity, &binding)?;
                    if request.payload_hash != [0; 32] && request.payload_hash != payload_hash {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    let fields = vec![
                        crate::terminal_review::field("account", binding.account_id.clone())?,
                        crate::terminal_review::field("email", identity.email.clone())?,
                        crate::terminal_review::field("client_id", binding.client_id.clone())?,
                        crate::terminal_review::field(
                            "access_mode",
                            binding.access_mode.as_str().into(),
                        )?,
                    ];
                    return crate::terminal_review::make_entry_ticket(
                        &mut self.state.terminal_reviews.borrow_mut(),
                        hantong_platform::random_nonce()?,
                        TerminalReviewKind::GoogleSetup,
                        request.request_id,
                        binding.account_id,
                        payload_hash,
                        request.request_id[..16]
                            .try_into()
                            .map_err(|_| ErrorCode::InvalidInput)?,
                        1,
                        expires_at_utc_ms,
                        observed_peer.clone(),
                        now,
                        fields,
                    );
                }
                if matches!(
                    request.kind,
                    TerminalReviewKind::ImapSetup | TerminalReviewKind::SmtpSetup
                ) {
                    let (account_id, payload_hash, version, expires_at, endpoint_json, ticket) =
                        if request.kind == TerminalReviewKind::ImapSetup {
                            let setup =
                                crate::setup_imap::load(self.state.store, request.request_id)?;
                            let i = setup.intent();
                            if setup.phase()
                                != hantong_store::imap_setup::ImapSetupPhase::ReviewPending
                                || request
                                    .account_id
                                    .as_deref()
                                    .is_some_and(|a| a != i.draft().account_id())
                                || (request.payload_hash != [0; 32]
                                    && request.payload_hash != *i.review_payload_hash())
                            {
                                return Err(ErrorCode::PolicyBlocked);
                            }
                            let endpoint_json = serde_json::to_value(i.endpoint())
                                .map_err(|_| ErrorCode::InternalError)?;
                            let binding = terminal_binding(&TerminalBindingInput {
                                kind: *b"terminal-imap-v1",
                                request_id: *i.request_id(),
                                payload_hash: *i.review_payload_hash(),
                                account_id: i.draft().account_id(),
                                endpoint: &endpoint_json,
                                version: i.endpoint().settings_version(),
                                expires_at_utc_ms: setup.expires_at_utc_ms(),
                                peer: observed_peer,
                                server: self.state.server,
                            })?;
                            let ticket = SecretStore::open(self.state.secrets_root)
                                .map_err(|_| ErrorCode::InternalError)?
                                .issue_terminal_secret_ticket(binding)
                                .map_err(|_| ErrorCode::InternalError)?;
                            (
                                i.draft().account_id().to_owned(),
                                *i.review_payload_hash(),
                                i.endpoint().settings_version(),
                                setup.expires_at_utc_ms(),
                                endpoint_json,
                                ticket,
                            )
                        } else {
                            let context = crate::setup_smtp::validate_terminal_context(
                                self.state.store,
                                &self.state.smtp_sessions,
                                request.request_id,
                            )?;
                            if request
                                .account_id
                                .as_deref()
                                .is_some_and(|a| a != context.account_id)
                                || (request.payload_hash != [0; 32]
                                    && request.payload_hash != context.payload_hash)
                            {
                                return Err(ErrorCode::PolicyBlocked);
                            }
                            let endpoint_json = serde_json::to_value(&context.endpoint)
                                .map_err(|_| ErrorCode::InternalError)?;
                            let binding = terminal_binding(&TerminalBindingInput {
                                kind: *b"terminal-smtp-v1",
                                request_id: context.request_id,
                                payload_hash: context.payload_hash,
                                account_id: &context.account_id,
                                endpoint: &endpoint_json,
                                version: context.account_version,
                                expires_at_utc_ms: context.expires_at_utc_ms,
                                peer: observed_peer,
                                server: self.state.server,
                            })?;
                            let ticket = SecretStore::open(self.state.secrets_root)
                                .map_err(|_| ErrorCode::InternalError)?
                                .issue_terminal_secret_ticket(binding)
                                .map_err(|_| ErrorCode::InternalError)?;
                            (
                                context.account_id,
                                context.payload_hash,
                                context.account_version,
                                context.expires_at_utc_ms,
                                endpoint_json,
                                ticket,
                            )
                        };
                    if expires_at <= now {
                        return Err(ErrorCode::ApprovalExpired);
                    }
                    let owner: [u8; 16] = request.request_id[..16]
                        .try_into()
                        .map_err(|_| ErrorCode::InvalidInput)?;
                    let fields = setup_fields(&account_id, &endpoint_json)?;
                    let review_ticket = hantong_platform::random_nonce()?;
                    let mut receipt = crate::terminal_review::make_entry_ticket(
                        &mut self.state.terminal_reviews.borrow_mut(),
                        review_ticket,
                        request.kind,
                        request.request_id,
                        account_id,
                        payload_hash,
                        owner,
                        version,
                        expires_at,
                        observed_peer.clone(),
                        now,
                        fields,
                    )?;
                    if let Some(entry) = self
                        .state
                        .terminal_reviews
                        .borrow_mut()
                        .get_mut(&review_ticket)
                    {
                        entry.secret_ticket = Some(ticket);
                        receipt.secret_ticket_opaque =
                            entry.secret_ticket.as_ref().map(|t| t.to_opaque().to_vec());
                    }
                    return Ok(receipt);
                }
                if request.kind != TerminalReviewKind::PersonalSend {
                    return Err(ErrorCode::CapabilityUnsupported);
                }
                let pending = self.state.store.pending_mail_send(request.request_id)?;
                if request
                    .account_id
                    .as_deref()
                    .is_some_and(|v| v != pending.request.account_id())
                    || (request.payload_hash != [0; 32]
                        && request.payload_hash != *pending.request.payload_hash())
                    || !self.state.store.can_confirm_personal_mail_send(&pending)?
                {
                    return Err(ErrorCode::PolicyBlocked);
                }
                let preview = crate::send_mail::preview(&pending)?;
                let mut fields = Vec::new();
                for key in ["from", "to", "subject", "body", "attachments"] {
                    if let Some(value) = preview.get(key) {
                        let text = if key == "body" || key == "from" || key == "subject" {
                            value.as_str().unwrap_or_default().to_owned()
                        } else {
                            serde_json::to_string(value).map_err(|_| ErrorCode::InternalError)?
                        };
                        fields.push(crate::terminal_review::field(key, text)?);
                    }
                }
                return crate::terminal_review::make_ticket(
                    &mut self.state.terminal_reviews.borrow_mut(),
                    &pending,
                    observed_peer.clone(),
                    now,
                    fields,
                );
            }
            TerminalReviewCommand::Confirm(request) => {
                let valid = {
                    let entries = self.state.terminal_reviews.borrow();
                    let entry = entries.get(&request.ticket).ok_or(ErrorCode::NotFound)?;
                    matches!(
                        entry.kind,
                        TerminalReviewKind::PersonalSend
                            | TerminalReviewKind::ImapTrash
                            | TerminalReviewKind::ProtectedSend
                            | TerminalReviewKind::ScopeRefresh
                            | TerminalReviewKind::Registration
                            | TerminalReviewKind::GoogleMutation
                            | TerminalReviewKind::GoogleSetup
                    ) && entry.payload_hash == request.snapshot_hash
                        && entry.expires_at_utc_ms > now
                        && entry.peer == *observed_peer
                };
                if !valid {
                    return Err(ErrorCode::PolicyBlocked);
                }
                {
                    let entries = self.state.terminal_reviews.borrow();
                    let entry = entries.get(&request.ticket).ok_or(ErrorCode::NotFound)?;
                    match entry.kind {
                        TerminalReviewKind::ImapTrash => {
                            let current = self.state.store.pending_imap_trash(entry.request_id)?;
                            if imap_trash_terminal_hash(&current)? != entry.payload_hash
                                || current.owner_registration_id != entry.owner
                                || current.profile_epoch != entry.epoch
                                || !self.state.store.can_confirm_imap_trash(&current)?
                            {
                                return Err(ErrorCode::Conflict);
                            }
                        }
                        TerminalReviewKind::Registration => {
                            let current = self
                                .state
                                .store
                                .get_registration_request(entry.request_id)?
                                .ok_or(ErrorCode::NotFound)?;
                            if registration_terminal_hash(&current)? != entry.payload_hash
                                || entry.owner
                                    != hantong_platform::registration_id_for_request(
                                        current.request_id,
                                    )?
                                || current.state != RegistrationRequestState::Pending
                            {
                                return Err(ErrorCode::Conflict);
                            }
                        }
                        TerminalReviewKind::ScopeRefresh => {
                            let current = self
                                .state
                                .store
                                .load_cli_scope_refresh(entry.request_id)?
                                .ok_or(ErrorCode::NotFound)?;
                            if current.payload_hash() != &entry.payload_hash
                                || current.base_registration().registration_id != entry.owner
                                || current.base_registration().profile_epoch != entry.epoch
                                || current.phase()
                                    != hantong_store::scope_refresh::ScopeRefreshPhase::Pending
                            {
                                return Err(ErrorCode::Conflict);
                            }
                        }
                        TerminalReviewKind::GoogleMutation => {
                            let current =
                                self.state.store.pending_google_mutation(entry.request_id)?;
                            if current.request.payload_hash() != &entry.payload_hash
                                || current.owner_registration_id != entry.owner
                                || current.profile_epoch != entry.epoch
                            {
                                return Err(ErrorCode::Conflict);
                            }
                            let owner = self
                                .state
                                .store
                                .load_registration(current.owner_registration_id)?
                                .ok_or(ErrorCode::ClientNotRegistered)?;
                            if owner.state != RegistrationState::Active
                                || owner.profile_epoch != current.profile_epoch
                            {
                                return Err(ErrorCode::PolicyBlocked);
                            }
                            if owner.client_kind == ClientKind::Ai {
                                let provider = if owner.display_label == DEFAULT_AI_CODEX_LABEL {
                                    AiProvider::Codex
                                } else if owner.display_label == DEFAULT_AI_CLAUDE_LABEL {
                                    AiProvider::Claude
                                } else {
                                    return Err(ErrorCode::PolicyBlocked);
                                };
                                self.state.store.check_ai_consent(
                                    provider,
                                    owner.registration_id,
                                    owner.profile_epoch,
                                    std::slice::from_ref(&current.request.account_id().to_owned()),
                                )?;
                            }
                        }
                        TerminalReviewKind::GoogleSetup => {
                            let (identity, binding, _) =
                                crate::setup_google::advanced_terminal_preview(
                                    &self.state.google_sessions,
                                    entry.request_id,
                                    observed_peer,
                                )?;
                            let hash = google_terminal_hash(&identity, &binding)?;
                            if hash != entry.payload_hash {
                                return Err(ErrorCode::Conflict);
                            }
                        }
                        TerminalReviewKind::ProtectedSend | TerminalReviewKind::PersonalSend => {
                            let current = self.state.store.pending_mail_send(entry.request_id)?;
                            if current.request.payload_hash() != &entry.payload_hash
                                || current.owner_registration_id != entry.owner
                                || current.profile_epoch != entry.epoch
                            {
                                return Err(ErrorCode::Conflict);
                            }
                            let owner = self
                                .state
                                .store
                                .load_registration(current.owner_registration_id)?
                                .ok_or(ErrorCode::ClientNotRegistered)?;
                            if owner.state != RegistrationState::Active
                                || owner.profile_epoch != current.profile_epoch
                            {
                                return Err(ErrorCode::PolicyBlocked);
                            }
                            if owner.client_kind == ClientKind::Ai {
                                let provider = if owner.display_label == DEFAULT_AI_CODEX_LABEL {
                                    AiProvider::Codex
                                } else if owner.display_label == DEFAULT_AI_CLAUDE_LABEL {
                                    AiProvider::Claude
                                } else {
                                    return Err(ErrorCode::PolicyBlocked);
                                };
                                self.state.store.check_ai_consent(
                                    provider,
                                    owner.registration_id,
                                    owner.profile_epoch,
                                    std::slice::from_ref(&current.request.account_id().to_owned()),
                                )?;
                            }
                        }
                        _ => return Err(ErrorCode::CapabilityUnsupported),
                    }
                }
                let entry = self
                    .state
                    .terminal_reviews
                    .borrow_mut()
                    .remove(&request.ticket)
                    .ok_or(ErrorCode::Conflict)?;
                if entry.kind == TerminalReviewKind::GoogleSetup {
                    crate::setup_google::complete_advanced_terminal(
                        self.state.store,
                        self.state.secrets_root,
                        &self.state.google_sessions,
                        entry.request_id,
                        observed_peer,
                    )?;
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "ready".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::ScopeRefresh {
                    crate::scope_refresh::run_terminal_job(
                        self.state.store,
                        self.state.server,
                        entry.request_id,
                    )?;
                    let refresh = self
                        .state
                        .store
                        .load_cli_scope_refresh(entry.request_id)?
                        .ok_or(ErrorCode::InternalError)?;
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: refresh.target_profile_epoch(),
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "ready".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::Registration {
                    let effects = NativeEffects {
                        store: self.state.store,
                        server: self.state.server,
                    };
                    let request = effects.request(entry.request_id)?;
                    commit_terminal_registration(&effects, &request)?;
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: 1,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "ready".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::ImapTrash {
                    match self.state.scheduled.get() {
                        Some(Job::TerminalImapTrash(id)) if id == entry.request_id => {}
                        Some(_) => return Err(ErrorCode::RateLimited),
                        None => enqueue(
                            &self.state.scheduled,
                            &self.sender,
                            Job::TerminalImapTrash(entry.request_id),
                        )?,
                    }
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "queued".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::GoogleMutation {
                    return match self.state.scheduled.get() {
                        Some(Job::TerminalGoogleMutation(id)) if id == entry.request_id => {
                            Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                                secret_ticket_opaque: None,
                                kind: entry.kind,
                                request_id: entry.request_id,
                                ticket: None,
                                snapshot_hash: entry.payload_hash,
                                owner_registration_id: entry.owner,
                                profile_epoch: entry.epoch,
                                expires_at_utc_ms: entry.expires_at_utc_ms,
                                state: "queued".into(),
                                fields: Vec::new(),
                            })
                        }
                        Some(_) => Err(ErrorCode::RateLimited),
                        None => {
                            enqueue(
                                &self.state.scheduled,
                                &self.sender,
                                Job::TerminalGoogleMutation(entry.request_id),
                            )?;
                            Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                                secret_ticket_opaque: None,
                                kind: entry.kind,
                                request_id: entry.request_id,
                                ticket: None,
                                snapshot_hash: entry.payload_hash,
                                owner_registration_id: entry.owner,
                                profile_epoch: entry.epoch,
                                expires_at_utc_ms: entry.expires_at_utc_ms,
                                state: "queued".into(),
                                fields: Vec::new(),
                            })
                        }
                    };
                }
                if entry.kind == TerminalReviewKind::ProtectedSend {
                    match self.state.scheduled.get() {
                        Some(Job::TerminalProtectedSend(id)) if id == entry.request_id => {}
                        Some(_) => return Err(ErrorCode::RateLimited),
                        None => enqueue(
                            &self.state.scheduled,
                            &self.sender,
                            Job::TerminalProtectedSend(entry.request_id),
                        )?,
                    }
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "queued".into(),
                        fields: Vec::new(),
                    });
                }
                let confirm = hantong_contracts::mail_send::PersonalSendConfirmRequest::new(
                    &entry.account_id,
                    entry.request_id,
                    entry.payload_hash,
                )?;
                self.confirm_personal_send(entry.owner, entry.epoch, &confirm)
                    .map(
                        |receipt| hantong_contracts::terminal_review::TerminalReviewReceipt {
                            secret_ticket_opaque: None,
                            kind: entry.kind,
                            request_id: entry.request_id,
                            ticket: None,
                            snapshot_hash: entry.payload_hash,
                            owner_registration_id: entry.owner,
                            profile_epoch: entry.epoch,
                            expires_at_utc_ms: entry.expires_at_utc_ms,
                            state: if receipt.state == "pending" {
                                "queued".into()
                            } else {
                                receipt.state.into()
                            },
                            fields: Vec::new(),
                        },
                    )
            }
            TerminalReviewCommand::Cancel(request) => {
                let valid = {
                    let entries = self.state.terminal_reviews.borrow();
                    let entry = entries.get(&request.ticket).ok_or(ErrorCode::NotFound)?;
                    matches!(
                        entry.kind,
                        TerminalReviewKind::PersonalSend
                            | TerminalReviewKind::ImapTrash
                            | TerminalReviewKind::ProtectedSend
                            | TerminalReviewKind::ScopeRefresh
                            | TerminalReviewKind::Registration
                            | TerminalReviewKind::GoogleMutation
                            | TerminalReviewKind::ImapSetup
                            | TerminalReviewKind::SmtpSetup
                    ) && entry.kind == request.get.kind
                        && entry.request_id == request.get.request_id
                        && (request.get.payload_hash == [0; 32]
                            || entry.payload_hash == request.get.payload_hash)
                        && request
                            .get
                            .account_id
                            .as_deref()
                            .is_none_or(|v| v == entry.account_id)
                        && entry.peer == *observed_peer
                        && entry.expires_at_utc_ms > now
                };
                if !valid {
                    return Err(ErrorCode::PolicyBlocked);
                }
                let entry = self
                    .state
                    .terminal_reviews
                    .borrow_mut()
                    .remove(&request.ticket)
                    .ok_or(ErrorCode::Conflict)?;
                if entry.kind == TerminalReviewKind::ImapTrash {
                    self.state
                        .store
                        .deny_imap_trash(entry.request_id, ErrorCode::ApprovalDenied)?;
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "denied".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::ImapSetup {
                    let current = crate::setup_imap::load(self.state.store, entry.request_id)?;
                    self.state
                        .store
                        .close_imap_setup_review(&current, ErrorCode::ApprovalDenied)?;
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "denied".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::SmtpSetup {
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "denied".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::ScopeRefresh {
                    let refresh = self
                        .state
                        .store
                        .load_cli_scope_refresh(entry.request_id)?
                        .ok_or(ErrorCode::NotFound)?;
                    self.state.store.deny_cli_scope_refresh(&refresh)?;
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "denied".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::Registration {
                    let current = self
                        .state
                        .store
                        .get_registration_request(entry.request_id)?
                        .ok_or(ErrorCode::NotFound)?;
                    match current.client_kind {
                        ClientKind::Mcp => self
                            .state
                            .store
                            .deny_mcp_registration_request(entry.request_id, current.version)?,
                        ClientKind::Gui => return Err(ErrorCode::CapabilityUnsupported),
                        ClientKind::Cli => self
                            .state
                            .store
                            .deny_cli_registration_request(entry.request_id, current.version)?,
                        ClientKind::Ai => self
                            .state
                            .store
                            .deny_cli_registration_request(entry.request_id, current.version)?,
                    };
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "denied".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::GoogleMutation {
                    self.state
                        .store
                        .deny_google_mutation(entry.request_id, ErrorCode::ApprovalRequired)?;
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "denied".into(),
                        fields: Vec::new(),
                    });
                }
                if entry.kind == TerminalReviewKind::ProtectedSend {
                    self.state
                        .store
                        .deny_mail_send(entry.request_id, ErrorCode::ApprovalRequired)?;
                    return Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                        secret_ticket_opaque: None,
                        kind: entry.kind,
                        request_id: entry.request_id,
                        ticket: None,
                        snapshot_hash: entry.payload_hash,
                        owner_registration_id: entry.owner,
                        profile_epoch: entry.epoch,
                        expires_at_utc_ms: entry.expires_at_utc_ms,
                        state: "denied".into(),
                        fields: Vec::new(),
                    });
                }
                let status = hantong_contracts::mail_send::SendReviewStatusRequest {
                    account_id: entry.account_id.clone(),
                    review_id: entry.request_id,
                };
                self.cancel_personal_send(entry.owner, entry.epoch, &status)
                    .map(
                        |receipt| hantong_contracts::terminal_review::TerminalReviewReceipt {
                            secret_ticket_opaque: None,
                            kind: entry.kind,
                            request_id: entry.request_id,
                            ticket: None,
                            snapshot_hash: entry.payload_hash,
                            owner_registration_id: entry.owner,
                            profile_epoch: entry.epoch,
                            expires_at_utc_ms: entry.expires_at_utc_ms,
                            state: receipt.state.into(),
                            fields: Vec::new(),
                        },
                    )
            }
            TerminalReviewCommand::SecretSubmit(request) => {
                let (kind, entry_snapshot) = {
                    let entries = self.state.terminal_reviews.borrow();
                    let entry = entries.get(&request.ticket).ok_or(ErrorCode::NotFound)?;
                    if !matches!(
                        entry.kind,
                        TerminalReviewKind::ImapSetup | TerminalReviewKind::SmtpSetup
                    ) || entry.payload_hash != request.snapshot_hash
                        || entry.expires_at_utc_ms <= now
                        || entry.peer != *observed_peer
                    {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    (
                        entry.kind,
                        (
                            entry.request_id,
                            entry.account_id.clone(),
                            entry.payload_hash,
                            entry.owner,
                            entry.epoch,
                            entry.expires_at_utc_ms,
                        ),
                    )
                };
                let opaque = TerminalSecretTicket::from_opaque(&request.secret_ticket_opaque)
                    .map_err(|_| ErrorCode::InvalidInput)?;
                let (binding, ticket) = {
                    let entries = self.state.terminal_reviews.borrow();
                    let entry = entries.get(&request.ticket).ok_or(ErrorCode::NotFound)?;
                    let ticket = entry.secret_ticket.as_ref().ok_or(ErrorCode::Conflict)?;
                    if ticket.binding() != opaque.binding()
                        || ticket.opaque_id() != opaque.opaque_id()
                    {
                        return Err(ErrorCode::PolicyBlocked);
                    }
                    (ticket.binding(), ticket.to_opaque())
                };
                let ticket = TerminalSecretTicket::from_opaque(&ticket)
                    .map_err(|_| ErrorCode::InternalError)?;
                if self.state.scheduled.get().is_some() {
                    return Err(ErrorCode::RateLimited);
                }
                let expected_binding = if kind == TerminalReviewKind::ImapSetup {
                    let setup = crate::setup_imap::load(self.state.store, entry_snapshot.0)?;
                    let i = setup.intent();
                    if setup.phase() != hantong_store::imap_setup::ImapSetupPhase::ReviewPending {
                        return Err(ErrorCode::Conflict);
                    }
                    let endpoint =
                        serde_json::to_value(i.endpoint()).map_err(|_| ErrorCode::InternalError)?;
                    terminal_binding(&TerminalBindingInput {
                        kind: *b"terminal-imap-v1",
                        request_id: *i.request_id(),
                        payload_hash: *i.review_payload_hash(),
                        account_id: i.draft().account_id(),
                        endpoint: &endpoint,
                        version: i.endpoint().settings_version(),
                        expires_at_utc_ms: entry_snapshot.5,
                        peer: observed_peer,
                        server: self.state.server,
                    })?
                } else {
                    let context = crate::setup_smtp::validate_terminal_context(
                        self.state.store,
                        &self.state.smtp_sessions,
                        entry_snapshot.0,
                    )?;
                    let endpoint = serde_json::to_value(&context.endpoint)
                        .map_err(|_| ErrorCode::InternalError)?;
                    terminal_binding(&TerminalBindingInput {
                        kind: *b"terminal-smtp-v1",
                        request_id: context.request_id,
                        payload_hash: context.payload_hash,
                        account_id: &context.account_id,
                        endpoint: &endpoint,
                        version: context.account_version,
                        expires_at_utc_ms: entry_snapshot.5,
                        peer: observed_peer,
                        server: self.state.server,
                    })?
                };
                if ticket.binding() != expected_binding || binding != expected_binding {
                    return Err(ErrorCode::Conflict);
                }
                let handle = SecretStore::open(self.state.secrets_root)
                    .map_err(|_| ErrorCode::InternalError)?
                    .terminal_secret_handle_from_opaque(&ticket, request.staged_handle)
                    .map_err(|_| ErrorCode::ScopeDenied)?;
                if kind == TerminalReviewKind::ImapSetup {
                    let setup = crate::setup_imap::load(self.state.store, entry_snapshot.0)?;
                    let i = setup.intent();
                    if setup.phase() != hantong_store::imap_setup::ImapSetupPhase::ReviewPending
                        || i.draft().account_id() != entry_snapshot.1
                        || *i.review_payload_hash() != entry_snapshot.2
                    {
                        return Err(ErrorCode::Conflict);
                    }
                } else {
                    let context = crate::setup_smtp::terminal_context(
                        &self.state.smtp_sessions,
                        entry_snapshot.0,
                    )?;
                    if context.account_id != entry_snapshot.1
                        || context.payload_hash != entry_snapshot.2
                    {
                        return Err(ErrorCode::Conflict);
                    }
                }
                let capture = if kind == TerminalReviewKind::ImapSetup {
                    crate::terminal_review::TerminalCapture::Imap {
                        id: entry_snapshot.0,
                        ticket,
                        handle,
                        binding,
                    }
                } else {
                    crate::terminal_review::TerminalCapture::Smtp {
                        id: entry_snapshot.0,
                        ticket,
                        handle,
                        binding,
                    }
                };
                let job = if kind == TerminalReviewKind::ImapSetup {
                    Job::TerminalImap(entry_snapshot.0)
                } else {
                    Job::TerminalSmtp(entry_snapshot.0)
                };
                if let Some(existing) = self.state.scheduled.get()
                    && existing != job
                {
                    return Err(ErrorCode::RateLimited);
                }
                enqueue(&self.state.scheduled, &self.sender, job)?;
                self.state
                    .terminal_captures
                    .borrow_mut()
                    .insert(entry_snapshot.0, capture);
                self.state
                    .terminal_reviews
                    .borrow_mut()
                    .remove(&request.ticket);
                Ok(hantong_contracts::terminal_review::TerminalReviewReceipt {
                    secret_ticket_opaque: None,
                    kind,
                    request_id: entry_snapshot.0,
                    ticket: None,
                    snapshot_hash: entry_snapshot.2,
                    owner_registration_id: entry_snapshot.3,
                    profile_epoch: entry_snapshot.4,
                    expires_at_utc_ms: entry_snapshot.5,
                    state: "queued".into(),
                    fields: Vec::new(),
                })
            }
        }
    }

    pub(crate) fn request_google_mutation(
        &self,
        owner: [u8; 16],
        epoch: u64,
        request: &hantong_contracts::google_mutation::MutationPayload,
    ) -> Result<hantong_store::google_mutation::MutationReceipt, ErrorCode> {
        if self.sender.is_closed() {
            return Err(ErrorCode::CoreUnavailable);
        }
        if self
            .state
            .scheduled
            .get()
            .is_some_and(|job| !matches!(job, Job::TerminalGoogleMutation(_)))
        {
            return Err(ErrorCode::RateLimited);
        }
        let receipt = self
            .state
            .store
            .request_google_mutation(owner, epoch, request)?;
        // Preparing a mutation never schedules a provider effect. Only a
        // consumed foreground review ticket may enqueue the terminal job.
        Ok(receipt)
    }

    #[allow(dead_code)]
    pub(crate) fn setup_command(&self, command: SetupCommand) -> Result<SetupReceipt, ErrorCode> {
        self.setup_command_inner(None, command)
    }

    pub(crate) fn request_imap_trash(
        &self,
        registration_id: [u8; 16],
        profile_epoch: u64,
        request: &hantong_contracts::imap_trash::ImapTrashRequest,
    ) -> Result<hantong_store::imap_trash::ImapTrashReceipt, ErrorCode> {
        // Only terminal Confirm queues a provider operation.
        self.state
            .store
            .request_imap_trash(registration_id, profile_epoch, request)
    }

    pub(crate) fn imap_trash_status(
        &self,
        registration_id: [u8; 16],
        profile_epoch: u64,
        review_id: [u8; 32],
    ) -> Result<hantong_store::imap_trash::ImapTrashReceipt, ErrorCode> {
        self.state
            .store
            .status_imap_trash(registration_id, profile_epoch, review_id)
    }

    pub(crate) fn setup_command_with_context(
        &self,
        context: &hantong_ipc_server::setup_dispatch::SetupRequestContext,
        command: SetupCommand,
    ) -> Result<SetupReceipt, ErrorCode> {
        self.setup_command_inner(Some(context.peer()), command)
    }

    fn setup_command_inner(
        &self,
        peer: Option<&hantong_contracts::auth::PeerContext>,
        command: SetupCommand,
    ) -> Result<SetupReceipt, ErrorCode> {
        match command {
            SetupCommand::TerminalReview(command) => self
                .terminal_review_command(command, peer.ok_or(ErrorCode::AuthRequired)?)
                .map(SetupReceipt::TerminalReview),
            SetupCommand::Google(command) => {
                use hantong_contracts::google_setup::GoogleSetupCommand;
                match command {
                    GoogleSetupCommand::Status(id) => {
                        if let Some(peer) = peer {
                            crate::setup_google::peer_matches(
                                &self.state.google_sessions,
                                id,
                                peer,
                            )?;
                        }
                        crate::setup_google::status(&self.state.google_sessions, id)
                            .map(SetupReceipt::Google)
                    }
                    GoogleSetupCommand::ClientTicket(id) => {
                        let peer = peer.ok_or(ErrorCode::AuthRequired)?;
                        let ticket = crate::setup_google::terminal_client_ticket(
                            self.state.secrets_root,
                            &self.state.google_sessions,
                            id,
                            self.state.server,
                            peer,
                        )?;
                        let mut receipt =
                            crate::setup_google::status(&self.state.google_sessions, id)?;
                        receipt.client_ticket = Some(ticket.to_vec());
                        Ok(SetupReceipt::Google(receipt))
                    }
                    GoogleSetupCommand::StageClient {
                        request_id,
                        ticket,
                        staged_handle,
                    } => {
                        let peer = peer.ok_or(ErrorCode::AuthRequired)?;
                        let staged = crate::setup_google::stage_terminal_client(
                            self.state.secrets_root,
                            &self.state.google_sessions,
                            request_id,
                            &ticket,
                            staged_handle,
                            peer,
                        )?;
                        enqueue(&self.state.scheduled, &self.sender, Job::Google(request_id))?;
                        let mut receipt =
                            crate::setup_google::status(&self.state.google_sessions, request_id)?;
                        receipt.client_ticket = Some(staged.to_vec());
                        Ok(SetupReceipt::Google(receipt))
                    }
                    GoogleSetupCommand::Request(input) => {
                        if let Some(job) = self.state.scheduled.get() {
                            if let Job::Google(id) = job
                                && crate::setup_google::matches(
                                    &self.state.google_sessions,
                                    id,
                                    &input,
                                )
                            {
                                if let Some(peer) = peer {
                                    crate::setup_google::peer_matches(
                                        &self.state.google_sessions,
                                        id,
                                        peer,
                                    )?;
                                }
                                return crate::setup_google::status(
                                    &self.state.google_sessions,
                                    id,
                                )
                                .map(SetupReceipt::Google);
                            }
                            return Err(ErrorCode::RateLimited);
                        }
                        if self.sender.is_closed() {
                            return Err(ErrorCode::CoreUnavailable);
                        }
                        let receipt = match peer {
                            Some(peer) => crate::setup_google::request_with_peer(
                                self.state.store,
                                &self.state.google_sessions,
                                input,
                                Some(peer.clone()),
                            )?,
                            None => crate::setup_google::request(
                                self.state.store,
                                &self.state.google_sessions,
                                input,
                            )?,
                        };
                        enqueue(
                            &self.state.scheduled,
                            &self.sender,
                            Job::Google(receipt.request_id),
                        )?;
                        Ok(SetupReceipt::Google(receipt))
                    }
                }
            }
            SetupCommand::Smtp(command) => {
                use hantong_contracts::smtp_setup::{SmtpSetupCommand, SmtpSetupState};
                match command {
                    SmtpSetupCommand::Status(id) => {
                        crate::setup_smtp::status(&self.state.smtp_sessions, id, true)
                            .map(SetupReceipt::Smtp)
                    }
                    SmtpSetupCommand::Request(input) => {
                        let r = crate::setup_smtp::request(
                            self.state.store,
                            self.state.secrets_root,
                            &self.state.smtp_sessions,
                            input,
                        )?;
                        let _ = SmtpSetupState::Pending;
                        Ok(SetupReceipt::Smtp(r))
                    }
                }
            }
            SetupCommand::ScopeRefresh(ScopeRefreshCommand::Status(id)) => {
                crate::scope_refresh::status(self.state.store, id, !self.sender.is_closed())
                    .map(SetupReceipt::ScopeRefresh)
            }
            SetupCommand::ScopeRefresh(
                command @ (ScopeRefreshCommand::Request
                | ScopeRefreshCommand::RequestManage(_)
                | ScopeRefreshCommand::RequestGoogleReadOnly(_)
                | ScopeRefreshCommand::AddGoogleReadOnly(_)
                | ScopeRefreshCommand::RequestGoogleSend(_)
                | ScopeRefreshCommand::RequestGoogleChanges(_)),
            ) => {
                if let Some(job) = self.state.scheduled.get() {
                    let Job::ScopeRefresh(id) = job else {
                        return Err(ErrorCode::RateLimited);
                    };
                    if let ScopeRefreshCommand::RequestManage(accounts) = &command {
                        let pending = self
                            .state
                            .store
                            .load_cli_scope_refresh(id)?
                            .ok_or(ErrorCode::NotFound)?;
                        if !crate::scope_refresh::matches_manage(&pending, accounts) {
                            return Err(ErrorCode::Conflict);
                        }
                    }
                    if let ScopeRefreshCommand::RequestGoogleSend(accounts) = &command {
                        let pending = self
                            .state
                            .store
                            .load_cli_scope_refresh(id)?
                            .ok_or(ErrorCode::NotFound)?;
                        if !crate::scope_refresh::matches_google_send(&pending, accounts.clone()) {
                            return Err(ErrorCode::Conflict);
                        }
                    }
                    if let ScopeRefreshCommand::RequestGoogleChanges(accounts) = &command {
                        let pending = self
                            .state
                            .store
                            .load_cli_scope_refresh(id)?
                            .ok_or(ErrorCode::NotFound)?;
                        if !crate::scope_refresh::matches_google_changes(&pending, accounts.clone())
                        {
                            return Err(ErrorCode::Conflict);
                        }
                    }
                    if let ScopeRefreshCommand::RequestGoogleReadOnly(account) = command {
                        let pending = self
                            .state
                            .store
                            .load_cli_scope_refresh(id)?
                            .ok_or(ErrorCode::NotFound)?;
                        if !crate::scope_refresh::matches_google_readonly(&pending, account) {
                            return Err(ErrorCode::Conflict);
                        }
                    }
                    if let ScopeRefreshCommand::AddGoogleReadOnly(account) = command {
                        let pending = self
                            .state
                            .store
                            .load_cli_scope_refresh(id)?
                            .ok_or(ErrorCode::NotFound)?;
                        if !crate::scope_refresh::matches_google_readonly_add(&pending, account) {
                            return Err(ErrorCode::Conflict);
                        }
                    }
                    return crate::scope_refresh::status(self.state.store, id, true)
                        .map(SetupReceipt::ScopeRefresh);
                }
                if self.sender.is_closed() {
                    return Err(ErrorCode::InternalError);
                }
                let request = match command {
                    ScopeRefreshCommand::RequestManage(accounts) => {
                        crate::scope_refresh::prepare_manage(
                            self.state.store,
                            self.state.server,
                            accounts,
                        )?
                    }
                    ScopeRefreshCommand::RequestGoogleSend(accounts) => {
                        crate::scope_refresh::prepare_google_send(
                            self.state.store,
                            self.state.server,
                            accounts,
                        )?
                    }
                    ScopeRefreshCommand::RequestGoogleChanges(accounts) => {
                        crate::scope_refresh::prepare_google_changes(
                            self.state.store,
                            self.state.server,
                            accounts,
                        )?
                    }
                    ScopeRefreshCommand::RequestGoogleReadOnly(account) => {
                        crate::scope_refresh::prepare_google_readonly(
                            self.state.store,
                            self.state.server,
                            account,
                        )?
                    }
                    ScopeRefreshCommand::AddGoogleReadOnly(account) => {
                        crate::scope_refresh::prepare_google_readonly_add(
                            self.state.store,
                            self.state.server,
                            account,
                        )?
                    }
                    _ => crate::scope_refresh::prepare(self.state.store, self.state.server)?,
                };
                let id = *request.refresh_id();
                // Terminal review owns the explicit confirm/deny transition;
                // preparing a scope never opens or schedules a native review.
                crate::scope_refresh::status(self.state.store, id, true)
                    .map(SetupReceipt::ScopeRefresh)
            }
            SetupCommand::Imap(ImapSetupCommand::Resume(id)) => {
                if matches!(
                    self.state.scheduled.get(),
                    Some(
                        Job::Registration(_)
                            | Job::ScopeRefresh(_)
                            | Job::TerminalGoogleMutation(_)
                            | Job::Smtp(_)
                            | Job::Google(_)
                    )
                ) {
                    return Err(ErrorCode::RateLimited);
                }
                let parent = self.state.store.resolve_imap_setup_leaf(id)?;
                let leaf_id = *parent.intent().request_id();
                if parent.phase() == hantong_store::imap_setup::ImapSetupPhase::ReviewPending
                    && parent.intent().resume_binding().is_some()
                {
                    return crate::setup_imap::status(self.state.store, leaf_id, true)
                        .map(SetupReceipt::Imap);
                }
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
                if let Some(existing) = self
                    .state
                    .store
                    .list_imap_setup_requests()?
                    .into_iter()
                    .find(|candidate| {
                        candidate.phase()
                            == hantong_store::imap_setup::ImapSetupPhase::ReviewPending
                            && candidate.intent().draft() == parent.intent().draft()
                            && candidate.intent().endpoint() == parent.intent().endpoint()
                            && candidate
                                .intent()
                                .resume_binding()
                                .and_then(|b| b.existing_versions())
                                .is_some()
                    })
                {
                    let id = *existing.intent().request_id();
                    return crate::setup_imap::status(self.state.store, id, true)
                        .map(SetupReceipt::Imap);
                }
                let child = crate::setup_imap::prepare_resume(self.state.store, &parent)?;
                let child_id = *child.intent().request_id();
                crate::setup_imap::status(self.state.store, child_id, true).map(SetupReceipt::Imap)
            }
            SetupCommand::Registration(c) => self.command(c).map(SetupReceipt::Registration),
            SetupCommand::Imap(ImapSetupCommand::Status(id)) => {
                crate::setup_imap::status(self.state.store, id, true).map(SetupReceipt::Imap)
            }
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
                if let Some(existing) = self
                    .state
                    .store
                    .list_imap_setup_requests()?
                    .into_iter()
                    .find(|candidate| {
                        candidate.phase()
                            == hantong_store::imap_setup::ImapSetupPhase::ReviewPending
                            && crate::setup_imap::matches(candidate, &input).unwrap_or(false)
                    })
                {
                    let id = *existing.intent().request_id();
                    let receipt = crate::setup_imap::status(self.state.store, id, true)?;
                    if receipt.state() != hantong_contracts::imap_setup::ImapSetupState::Expired {
                        return Ok(SetupReceipt::Imap(receipt));
                    }
                }
                let request = crate::setup_imap::create(self.state.store, &input)?;
                let id = *request.intent().request_id();
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
        let ai_provider = match command {
            RegistrationCommand::AiRequest(provider)
            | RegistrationCommand::AiStatus(provider, _) => Some(provider),
            _ => None,
        };
        let ai = ai_provider.is_some();
        let kind = if mcp {
            ClientKind::Mcp
        } else if gui {
            ClientKind::Gui
        } else if ai {
            ClientKind::Ai
        } else {
            ClientKind::Cli
        };
        let selected = match command {
            RegistrationCommand::RequestGoogleReadOnly(account) => Some(account),
            _ => None,
        };
        if let Some(account) = selected {
            // Reject nonexistent selections before orphan cleanup or request creation.
            selected_registration_scope(self.state.store, account)?;
        }
        match command {
            RegistrationCommand::Status(id)
            | RegistrationCommand::GuiStatus(id)
            | RegistrationCommand::McpStatus(id) => self.state.receipt(id, kind),
            RegistrationCommand::AiStatus(provider, id) => {
                let request = self
                    .state
                    .store
                    .get_registration_request(id)?
                    .ok_or(ErrorCode::NotFound)?;
                let label = match provider {
                    AiProvider::Codex => DEFAULT_AI_CODEX_LABEL,
                    AiProvider::Claude => DEFAULT_AI_CLAUDE_LABEL,
                };
                if request.client_kind != ClientKind::Ai || request.display_label != label {
                    return Err(ErrorCode::ScopeDenied);
                }
                self.state.receipt(id, ClientKind::Ai)
            }
            RegistrationCommand::Request
            | RegistrationCommand::RequestGoogleReadOnly(_)
            | RegistrationCommand::GuiRequest
            | RegistrationCommand::McpRequest
            | RegistrationCommand::AiRequest(_) => {
                let registered = if mcp {
                    self.state.store.load_mcp_registration()?
                } else if gui {
                    self.state.store.load_gui_registration()?
                } else if let Some(provider) = ai_provider {
                    self.state.store.load_ai_registration(provider)?
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
                            if let Some(account) = selected
                                && active.scope
                                    != selected_registration_scope(self.state.store, account)?
                            {
                                return Err(ErrorCode::Conflict);
                            }
                            self.state.receipt(id, kind)
                        }
                        Job::Imap(_)
                        | Job::Smtp(_)
                        | Job::Google(_)
                        | Job::ScopeRefresh(_)
                        | Job::TerminalGoogleMutation(_)
                        | Job::TerminalImapTrash(_)
                        | Job::TerminalSend(_) => Err(ErrorCode::RateLimited),
                        Job::TerminalProtectedSend(_) => Err(ErrorCode::RateLimited),
                        Job::TerminalImap(_) | Job::TerminalSmtp(_) => Err(ErrorCode::RateLimited),
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
                } else if let Some(provider) = ai_provider {
                    self.state.store.load_pending_ai_request(provider)?
                } else {
                    self.state.store.load_pending_default_cli_request()?
                };
                let request = match pending {
                    Some(request) => request,
                    None => {
                        let scope = if let Some(account) = selected {
                            selected_registration_scope(self.state.store, account)?
                        } else {
                            let accounts = if ai {
                                // Built-in AI may only be offered accounts that
                                // have completed provider setup.  The complete
                                // list is placed in the pending request and is
                                // shown for one explicit terminal confirmation;
                                // it is never interpreted as an implicit grant.
                                let accounts = self.state.store.list_ready_ai_account_ids()?;
                                if accounts.is_empty() {
                                    return Err(ErrorCode::NotFound);
                                }
                                accounts
                            } else {
                                self.state.store.list_registrable_account_ids()?
                            };
                            RequestedScope::new(
                                accounts,
                                if mcp {
                                    hantong_contracts::auth::mcp_mail_tools()
                                } else if gui {
                                    hantong_contracts::auth::gui_read_tools()
                                } else {
                                    if ai {
                                        hantong_contracts::auth::ai_mail_tools()
                                    } else {
                                        vec!["mail.accounts.list".into()]
                                    }
                                },
                            )?
                        };
                        let (client_kind, label) = if let Some(provider) = ai_provider {
                            (
                                ClientKind::Ai,
                                match provider {
                                    AiProvider::Codex => DEFAULT_AI_CODEX_LABEL,
                                    AiProvider::Claude => DEFAULT_AI_CLAUDE_LABEL,
                                },
                            )
                        } else {
                            (
                                kind,
                                if mcp {
                                    DEFAULT_MCP_LABEL
                                } else if gui {
                                    DEFAULT_GUI_LABEL
                                } else {
                                    DEFAULT_CLI_LABEL
                                },
                            )
                        };
                        self.state.store.create_registration_request(
                            hantong_platform::random_nonce()?,
                            client_kind,
                            label,
                            &scope,
                        )?
                    }
                };
                if let Some(account) = selected
                    && (request.scope != selected_registration_scope(self.state.store, account)?
                        || request.client_kind != ClientKind::Cli)
                {
                    return Err(ErrorCode::Conflict);
                }
                let receipt = receipt(&request, RegistrationProgress::Pending)?;
                if gui {
                    enqueue(
                        &self.state.scheduled,
                        &self.sender,
                        Job::Registration(*receipt.request_id()),
                    )?;
                }
                Ok(receipt)
            }
        }
    }
}

pub(crate) fn complete_google_oauth_registration(
    store: &Store,
    server: &ServerSigningKey,
    permit: crate::setup_google::GoogleBootstrapPermit,
) -> Result<(), ErrorCode> {
    let (account_id, profile) = permit.into_registration_parts(store)?;
    let target_tools = match profile {
        hantong_contracts::google_setup::GoogleLocalProfile::PersonalRead => {
            hantong_contracts::auth::default_cli_body_tools()
        }
        hantong_contracts::google_setup::GoogleLocalProfile::PersonalReadSend => {
            hantong_contracts::auth::default_cli_send_tools()
        }
        hantong_contracts::google_setup::GoogleLocalProfile::PersonalManage => {
            hantong_contracts::auth::default_cli_manage_tools()
        }
        hantong_contracts::google_setup::GoogleLocalProfile::None => {
            return Err(ErrorCode::InvalidInput);
        }
    };
    let existing = store.load_default_cli_registration()?;
    if let Some(record) = existing {
        // A fresh verified OAuth reconnect may find this exact grant already
        // active. Preserve the registration, key, epoch and scope history.
        if existing_personal_registration_is_current(&record, &account_id, profile) {
            return Ok(());
        }
        if needs_terminal_account_access(&record, &account_id, profile) {
            // OAuth connected this exact account. Keep the existing grant and
            // key unchanged; the foreground CLI must select the additional
            // account through the existing terminal scope workflow.
            return Ok(());
        }
        if profile == hantong_contracts::google_setup::GoogleLocalProfile::PersonalManage
            && record.client_kind == ClientKind::Cli
            && record.display_label == DEFAULT_CLI_LABEL
            && record.state == RegistrationState::Active
            && !record.scope.accounts().is_empty()
            && (hantong_contracts::auth::is_supported_default_cli_toolset(record.scope.tools())
                || hantong_contracts::auth::is_supported_default_cli_manage_toolset(
                    record.scope.tools(),
                ))
            // A fresh Modify permit for another Google account is only the
            // provider binding.  Even when the existing account already has
            // a valid B/C personal grant, do not route this through the
            // single-account OAuth D upgrade; the caller must perform one
            // explicit provider-neutral RequestManage review for the union.
            && record.scope.accounts() != [account_id.as_str()]
        {
            // The fresh OAuth permit has connected this account, including for
            // a mixed-provider CLI. It grants no local access here. The exact
            // existing account/tool union must be selected in terminal review.
            return Ok(());
        }
        if !existing_personal_registration_supported(&record, &account_id, profile) {
            return Err(ErrorCode::Conflict);
        }
    } else {
        let scope = hantong_contracts::auth::RequestedScope::new(
            vec![account_id.clone()],
            hantong_contracts::auth::default_cli_legacy_tools(),
        )?;
        let request = store.create_registration_request(
            hantong_platform::random_nonce()?,
            ClientKind::Cli,
            DEFAULT_CLI_LABEL,
            &scope,
        )?;
        let effects = NativeEffects { store, server };
        commit_google_oauth_registration(&effects, &request)?;
    }
    let target = hantong_contracts::auth::RequestedScope::new(vec![account_id], target_tools)?;
    let refresh_id = hantong_platform::random_nonce()?;
    let pin = hantong_platform::sha256(&server.public_key()?)?;
    let refresh = if profile == hantong_contracts::google_setup::GoogleLocalProfile::PersonalManage
    {
        store.create_google_oauth_cli_manage_scope_refresh(refresh_id, target, pin)?
    } else if permit_profile_is_send_upgrade(&target, store)? {
        store.create_google_oauth_cli_send_upgrade(refresh_id, target, pin)?
    } else {
        store.create_google_oauth_cli_scope_refresh(refresh_id, target, pin)?
    };
    crate::scope_refresh::run_google_oauth_job(store, server, *refresh.refresh_id())
}

fn needs_terminal_account_access(
    record: &RegistrationRecord,
    account: &str,
    profile: hantong_contracts::google_setup::GoogleLocalProfile,
) -> bool {
    use hantong_contracts::{auth::*, google_setup::GoogleLocalProfile};
    let (tools, single_epoch_valid) = match profile {
        GoogleLocalProfile::PersonalRead => (default_cli_body_tools(), record.profile_epoch == 2),
        GoogleLocalProfile::PersonalReadSend => (
            default_cli_send_tools(),
            matches!(record.profile_epoch, 2 | 3),
        ),
        GoogleLocalProfile::PersonalManage => {
            (default_cli_manage_tools(), record.profile_epoch >= 2)
        }
        GoogleLocalProfile::None => return false,
    };
    record.client_kind == ClientKind::Cli
        && record.display_label == DEFAULT_CLI_LABEL
        && record.state == RegistrationState::Active
        && record.approved_channel == "google_oauth"
        && match record.scope.accounts().len() {
            1 => single_epoch_valid,
            2..=128 => record.profile_epoch >= 3,
            _ => false,
        }
        && record.scope.tools() == tools
        && record.scope.accounts() != [account]
}

fn existing_personal_registration_is_current(
    record: &RegistrationRecord,
    account: &str,
    profile: hantong_contracts::google_setup::GoogleLocalProfile,
) -> bool {
    use hantong_contracts::{auth::*, google_setup::GoogleLocalProfile};
    record.client_kind == ClientKind::Cli
        && record.display_label == DEFAULT_CLI_LABEL
        && record.state == RegistrationState::Active
        && record.approved_channel == "google_oauth"
        && record.scope.accounts() == [account]
        && match profile {
            GoogleLocalProfile::PersonalRead => {
                record.scope.tools() == default_cli_body_tools() && record.profile_epoch == 2
            }
            GoogleLocalProfile::PersonalReadSend => {
                record.scope.tools() == default_cli_send_tools()
                    && matches!(record.profile_epoch, 2 | 3)
            }
            GoogleLocalProfile::PersonalManage => {
                record.scope.tools() == default_cli_manage_tools() && record.profile_epoch >= 2
            }
            GoogleLocalProfile::None => false,
        }
}

fn existing_personal_registration_supported(
    record: &RegistrationRecord,
    account: &str,
    profile: hantong_contracts::google_setup::GoogleLocalProfile,
) -> bool {
    use hantong_contracts::{auth::*, google_setup::GoogleLocalProfile};
    record.client_kind == ClientKind::Cli
        && record.display_label == DEFAULT_CLI_LABEL
        && record.state == RegistrationState::Active
        && record.approved_channel == "google_oauth"
        && record.scope.accounts() == [account]
        && ((record.scope.tools() == default_cli_legacy_tools() && record.profile_epoch == 1)
            || (profile == GoogleLocalProfile::PersonalReadSend
                && record.scope.tools() == default_cli_body_tools()
                && record.profile_epoch == 2)
            || (profile == GoogleLocalProfile::PersonalManage
                && ((record.scope.tools() == default_cli_body_tools()
                    && record.profile_epoch == 2)
                    || (record.scope.tools() == default_cli_send_tools()
                        && matches!(record.profile_epoch, 2 | 3)))))
}

fn permit_profile_is_send_upgrade(
    target: &hantong_contracts::auth::RequestedScope,
    store: &Store,
) -> Result<bool, ErrorCode> {
    if target.tools() != hantong_contracts::auth::default_cli_send_tools().as_slice() {
        return Ok(false);
    }
    let Some(base) = store.load_default_cli_registration()? else {
        return Ok(false);
    };
    Ok(base.scope.accounts() == target.accounts()
        && base.scope.tools() == hantong_contracts::auth::default_cli_body_tools().as_slice()
        && base.approved_channel == "google_oauth")
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
        self.state.store.recover_inflight_imap_trash()?;
        recover_startup(&effects)?;
        self.state.store.recover_mail_sends()?;
        self.state.store.recover_google_mutations()?;
        while let Some(job) = self.receiver.recv().await {
            let _window = ScheduledWindow(&self.state.scheduled);
            let result = match job {
                Job::Google(id) => {
                    crate::setup_google::run(
                        self.state.store,
                        self.state.secrets_root,
                        &self.state.google_sessions,
                        id,
                        self.state.server,
                    )
                    .await
                }
                Job::Smtp(id) => {
                    crate::setup_smtp::run(
                        self.state.store,
                        self.state.secrets_root,
                        &self.state.smtp_sessions,
                        id,
                    )
                    .await
                }
                Job::TerminalSend(id) => {
                    crate::send_mail::run_job(
                        self.state.store,
                        self.state.secrets_root,
                        &self.state.policy,
                        id,
                        true,
                        true,
                    )
                    .await
                }
                Job::TerminalProtectedSend(id) => {
                    crate::send_mail::run_job(
                        self.state.store,
                        self.state.secrets_root,
                        &self.state.policy,
                        id,
                        true,
                        false,
                    )
                    .await
                }
                Job::TerminalGoogleMutation(id) => {
                    crate::google_mutation::run_job(
                        self.state.store,
                        self.state.secrets_root,
                        &self.state.policy,
                        id,
                        true,
                    )
                    .await
                }
                Job::TerminalImapTrash(id) => {
                    crate::imap_trash::run_job(
                        self.state.store,
                        self.state.secrets_root,
                        &self.state.policy,
                        id,
                    )
                    .await
                }
                Job::TerminalImap(id) => {
                    let capture = self.state.terminal_captures.borrow_mut().remove(&id);
                    match capture {
                        Some(crate::terminal_review::TerminalCapture::Imap {
                            id,
                            ticket,
                            handle,
                            binding,
                        }) => {
                            crate::setup_imap::terminal_submit(
                                self.state.store,
                                self.state.secrets_root,
                                id,
                                ticket,
                                handle,
                                binding,
                            )
                            .await
                        }
                        Some(crate::terminal_review::TerminalCapture::Smtp { .. }) => {
                            Err(ErrorCode::Conflict)
                        }
                        None => Err(ErrorCode::NotFound),
                    }
                }
                Job::TerminalSmtp(id) => {
                    let capture = self.state.terminal_captures.borrow_mut().remove(&id);
                    match capture {
                        Some(crate::terminal_review::TerminalCapture::Smtp {
                            id,
                            ticket,
                            handle,
                            binding,
                        }) => {
                            crate::setup_smtp::terminal_submit(
                                self.state.store,
                                self.state.secrets_root,
                                &self.state.smtp_sessions,
                                id,
                                ticket,
                                handle,
                                binding,
                            )
                            .await
                        }
                        Some(crate::terminal_review::TerminalCapture::Imap { .. }) => {
                            Err(ErrorCode::Conflict)
                        }
                        None => Err(ErrorCode::NotFound),
                    }
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
    } else if request.client_kind == ClientKind::Ai {
        let provider = if request.display_label == DEFAULT_AI_CODEX_LABEL {
            AiProvider::Codex
        } else if request.display_label == DEFAULT_AI_CLAUDE_LABEL {
            AiProvider::Claude
        } else {
            return Err(ErrorCode::Conflict);
        };
        return RegistrationReview::new_ai(
            provider,
            request.request_id,
            request.version,
            request.scope.accounts().to_vec(),
            hash,
            request.created_at_utc_ms,
            request.expires_at_utc_ms,
        );
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

fn selected_registration_scope(
    store: &Store,
    account: hantong_contracts::registration::GoogleReadAccount,
) -> Result<RequestedScope, ErrorCode> {
    let id = account.account_id();
    if !store.list_registrable_account_ids()?.contains(&id) {
        return Err(ErrorCode::NotFound);
    }
    RequestedScope::new(
        vec![id],
        hantong_contracts::auth::default_cli_legacy_tools(),
    )
}

fn supported_identity(kind: ClientKind, label: &str, tools: &[String]) -> bool {
    match kind {
        ClientKind::Cli => {
            label == DEFAULT_CLI_LABEL
                && (tools == ["mail.accounts.list"]
                    || tools == hantong_contracts::auth::default_cli_body_tools().as_slice()
                    || tools == hantong_contracts::auth::default_cli_send_tools().as_slice())
        }
        ClientKind::Gui => {
            label == DEFAULT_GUI_LABEL && tools == hantong_contracts::auth::gui_read_tools()
        }
        ClientKind::Mcp => {
            label == DEFAULT_MCP_LABEL && hantong_contracts::auth::is_supported_mcp_toolset(tools)
        }
        ClientKind::Ai => {
            (label == DEFAULT_AI_CODEX_LABEL || label == DEFAULT_AI_CLAUDE_LABEL)
                && tools == hantong_contracts::auth::ai_mail_tools()
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

/// Completes the ordinary personal CLI registration after Secrets has
/// verified live Google PKCE, identity and scopes. The OAuth provenance is
/// carried by the Store stage; this path never manufactures a native review.
fn commit_google_oauth_registration(
    effects: &NativeEffects<'_>,
    expected: &RegistrationRequest,
) -> Result<(), ErrorCode> {
    let current = effects.request(expected.request_id)?;
    if &current != expected || current.state != RegistrationRequestState::Pending {
        return Err(ErrorCode::Conflict);
    }
    let public = effects.server_public()?;
    let hash = hantong_platform::sha256(&public)?;
    let now = effects.now()?;
    if now < current.created_at_utc_ms || now >= current.expires_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    cleanup_orphan(effects, &current)?;
    let id = hantong_platform::registration_id_for_request(current.request_id)?;
    let key = effects.create_key(id)?;
    let client_public = effects.key_public(&key)?;
    let stage =
        effects
            .store
            .stage_google_oauth_cli_registration(&CliRegistrationStage::new_google_oauth(
                current.clone(),
                id,
                client_public,
                hash,
            )?);
    let record = match stage {
        Ok(record) => record,
        Err(error) => {
            if let Err(cleanup) = delete_unlinked(effects, expected.request_id, key, client_public)
            {
                effects.report(error);
                effects.report(cleanup);
            }
            return Err(error);
        }
    };
    let bytes = profile_bytes(&record, public)?;
    let mut publication = match effects.publish(&bytes) {
        Ok(value) => value,
        Err(error) => {
            effects.report(error);
            return Err(error);
        }
    };
    effects.activate(&record, hash)?;
    effects.cleanup_publication(&mut publication)
}

fn commit_terminal_registration(
    effects: &NativeEffects<'_>,
    expected: &RegistrationRequest,
) -> Result<(), ErrorCode> {
    if !matches!(
        expected.client_kind,
        ClientKind::Cli | ClientKind::Mcp | ClientKind::Ai
    ) || expected.state != RegistrationRequestState::Pending
        || !supported_identity(
            expected.client_kind,
            &expected.display_label,
            expected.scope.tools(),
        )
    {
        return Err(ErrorCode::Conflict);
    }
    let current = effects.request(expected.request_id)?;
    if &current != expected {
        return Err(ErrorCode::Conflict);
    }
    let now = effects.now()?;
    if now < current.created_at_utc_ms || now >= current.expires_at_utc_ms {
        return Err(ErrorCode::ApprovalExpired);
    }
    let public = effects.server_public()?;
    cleanup_orphan(effects, &current)?;
    let id = hantong_platform::registration_id_for_request(current.request_id)?;
    let key = effects.create_key(id)?;
    let client_public = effects.key_public(&key)?;
    let server_hash = hantong_platform::sha256(&public)?;
    let stage = if current.client_kind == ClientKind::Mcp {
        if current.scope.tools() == hantong_contracts::auth::mcp_mail_tools() {
            effects.store.stage_terminal_mcp_mail_registration(
                &McpMailRegistrationStage::new_terminal_mcp_mail(
                    current.clone(),
                    id,
                    client_public,
                    server_hash,
                    effects.store.load_mcp_registration()?,
                )?,
            )
        } else {
            effects
                .store
                .stage_terminal_mcp_registration(&McpRegistrationStage::new_terminal_mcp(
                    current.clone(),
                    id,
                    client_public,
                    server_hash,
                )?)
        }
    } else if current.client_kind == ClientKind::Ai {
        let provider = if current.display_label == DEFAULT_AI_CODEX_LABEL {
            AiProvider::Codex
        } else if current.display_label == DEFAULT_AI_CLAUDE_LABEL {
            AiProvider::Claude
        } else {
            return Err(ErrorCode::Conflict);
        };
        effects
            .store
            .stage_ai_registration(&AiRegistrationStage::new(
                provider,
                current.clone(),
                id,
                client_public,
                server_hash,
            )?)
    } else {
        effects
            .store
            .stage_terminal_cli_registration(&CliRegistrationStage::new_terminal_cli(
                current.clone(),
                id,
                client_public,
                server_hash,
            )?)
    };
    let record = match stage {
        Ok(record) => record,
        Err(error) => {
            let _ = delete_unlinked(effects, expected.request_id, key, client_public);
            return Err(error);
        }
    };
    let bytes = profile_bytes(&record, public)?;
    let mut publication = effects.publish(&bytes)?;
    let hash = hantong_platform::sha256(&public)?;
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
    let tools = record.scope.tools();
    // AI mail tools include read/write operations (send and single-message
    // trash requests), so they must not be classified as read-only merely
    // because they are not the legacy MCP mail set.
    let expected_read_only = tools != hantong_contracts::auth::mcp_mail_tools()
        && tools != hantong_contracts::auth::ai_mail_tools();
    if record.registration_id != hantong_platform::registration_id_for_request(record.request_id)?
        || record.state != RegistrationState::Provisioning
        || record.profile_epoch != 1
        || !supported_identity(
            record.client_kind,
            &record.display_label,
            record.scope.tools(),
        )
        || record.read_only != expected_read_only
        || record.requests_per_minute != 60
        || !matches!(
            record.approved_channel.as_str(),
            "approval_helper" | "google_oauth" | "terminal_cli" | "ai_feature"
        )
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
    } else if record.client_kind == ClientKind::Ai {
        let provider = if record.display_label == DEFAULT_AI_CODEX_LABEL {
            hantong_ipc_client::profile::ProfileSelector::AiCodex
        } else if record.display_label == DEFAULT_AI_CLAUDE_LABEL {
            hantong_ipc_client::profile::ProfileSelector::AiClaude
        } else {
            return Err(ErrorCode::Conflict);
        };
        return ParsedClientProfile::new_ai(
            provider,
            record.registration_id,
            hantong_platform::client_key_id_for_registration(record.registration_id)?,
            record.public_key,
            server,
            record.profile_epoch,
            record.scope.accounts().to_vec(),
        )?
        .to_json();
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
        } else if request.client_kind == ClientKind::Ai {
            let provider = if request.display_label == DEFAULT_AI_CODEX_LABEL {
                AiProvider::Codex
            } else if request.display_label == DEFAULT_AI_CLAUDE_LABEL {
                AiProvider::Claude
            } else {
                return Err(ErrorCode::Conflict);
            };
            self.store.stage_ai_registration(&AiRegistrationStage::new(
                provider, request, id, public, hash,
            )?)
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
            hantong_ipc_client::profile::ProfileSelector::AiCodex => {
                hantong_approval_windows::publish_ai_profile(AiProvider::Codex, bytes)
            }
            hantong_ipc_client::profile::ProfileSelector::AiClaude => {
                hantong_approval_windows::publish_ai_profile(AiProvider::Claude, bytes)
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
        } else if record.client_kind == ClientKind::Ai {
            let provider = if record.display_label == DEFAULT_AI_CODEX_LABEL {
                AiProvider::Codex
            } else if record.display_label == DEFAULT_AI_CLAUDE_LABEL {
                AiProvider::Claude
            } else {
                return Err(ErrorCode::Conflict);
            };
            self.store
                .activate_ai_registration(provider, record.registration_id, record.version, hash)
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
