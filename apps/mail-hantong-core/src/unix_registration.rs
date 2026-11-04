//! Unix registration coordinator.
//!
//! Registration requests are durable Store records.  A terminal review only
//! creates a one-use in-memory ticket; Confirm performs the same staged
//! registration, profile publication, and CAS activation used by Windows.

use hantong_contracts::platform::OpaqueSigner;
use hantong_contracts::{
    ErrorCode,
    auth::RequestedScope,
    registration::{AiProvider, RegistrationCommand, RegistrationProgress, RegistrationReceipt},
    terminal_review::{
        TerminalReviewCommand, TerminalReviewField, TerminalReviewGet, TerminalReviewKind,
        TerminalReviewReceipt,
    },
    unix_auth::UnixPeerContextV2,
};
use hantong_ipc_client::profile::ParsedClientProfile;
use hantong_ipc_server::setup_dispatch::SetupReceipt;
use hantong_store::{
    ClientKind, DEFAULT_AI_CLAUDE_LABEL, DEFAULT_AI_CODEX_LABEL, DEFAULT_CLI_LABEL,
    DEFAULT_GUI_LABEL, DEFAULT_MCP_LABEL, RegistrationRecord, RegistrationRequest,
    RegistrationRequestState, RegistrationState, Store,
};
use std::{cell::RefCell, collections::BTreeMap, path::Path};

struct Ticket {
    request_id: [u8; 32],
    owner: [u8; 16],
    snapshot_hash: [u8; 32],
    expires_at_utc_ms: i64,
    peer: UnixPeerContextV2,
}

pub(crate) struct State {
    tickets: RefCell<BTreeMap<[u8; 32], Ticket>>,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            tickets: RefCell::new(BTreeMap::new()),
        }
    }

    pub(crate) fn ticket_owner(&self, ticket: [u8; 32]) -> Option<[u8; 16]> {
        self.tickets
            .borrow()
            .get(&ticket)
            .map(|ticket| ticket.owner)
    }
}

pub(crate) fn registration(
    store: &Store,
    command: RegistrationCommand,
) -> Result<SetupReceipt, ErrorCode> {
    let (kind, label, scope, pending) = match command {
        RegistrationCommand::Request => (
            ClientKind::Cli,
            DEFAULT_CLI_LABEL,
            RequestedScope::new(
                store.list_registrable_account_ids()?,
                hantong_contracts::auth::default_cli_legacy_tools(),
            )?,
            store.load_pending_default_cli_request()?,
        ),
        RegistrationCommand::RequestGoogleReadOnly(account) => (
            ClientKind::Cli,
            DEFAULT_CLI_LABEL,
            RequestedScope::new(
                vec![account.account_id()],
                hantong_contracts::auth::default_cli_body_tools(),
            )?,
            store.load_pending_default_cli_request()?,
        ),
        RegistrationCommand::GuiRequest => (
            ClientKind::Gui,
            DEFAULT_GUI_LABEL,
            RequestedScope::new(
                store.list_registrable_account_ids()?,
                hantong_contracts::auth::gui_read_tools(),
            )?,
            store.load_pending_gui_request()?,
        ),
        RegistrationCommand::McpRequest => (
            ClientKind::Mcp,
            DEFAULT_MCP_LABEL,
            RequestedScope::new(
                store.list_registrable_account_ids()?,
                hantong_contracts::auth::mcp_read_tools(),
            )?,
            store.load_pending_mcp_request()?,
        ),
        RegistrationCommand::McpRequestV3 => (
            ClientKind::Mcp,
            DEFAULT_MCP_LABEL,
            RequestedScope::new(
                store.list_registrable_account_ids()?,
                hantong_contracts::auth::mcp_mail_tools_v3(),
            )?,
            store.load_pending_mcp_request()?,
        ),
        RegistrationCommand::AiRequest(provider) => {
            let label = match provider {
                AiProvider::Codex => DEFAULT_AI_CODEX_LABEL,
                AiProvider::Claude => DEFAULT_AI_CLAUDE_LABEL,
            };
            (
                ClientKind::Ai,
                label,
                RequestedScope::new(
                    store.list_ready_ai_account_ids()?,
                    hantong_contracts::auth::ai_mail_tools(),
                )?,
                store.load_pending_ai_request(provider)?,
            )
        }
        RegistrationCommand::Status(id) => {
            return status(store, id, ClientKind::Cli, Some(DEFAULT_CLI_LABEL))
                .map(SetupReceipt::Registration);
        }
        RegistrationCommand::GuiStatus(id) => {
            return status(store, id, ClientKind::Gui, Some(DEFAULT_GUI_LABEL))
                .map(SetupReceipt::Registration);
        }
        RegistrationCommand::McpStatus(id) => {
            return status(store, id, ClientKind::Mcp, Some(DEFAULT_MCP_LABEL))
                .map(SetupReceipt::Registration);
        }
        RegistrationCommand::McpStatusV3(id) => {
            return status(store, id, ClientKind::Mcp, Some(DEFAULT_MCP_LABEL))
                .map(SetupReceipt::Registration);
        }
        RegistrationCommand::AiStatus(provider, id) => {
            let label = match provider {
                AiProvider::Codex => DEFAULT_AI_CODEX_LABEL,
                AiProvider::Claude => DEFAULT_AI_CLAUDE_LABEL,
            };
            return status(store, id, ClientKind::Ai, Some(label)).map(SetupReceipt::Registration);
        }
    };
    if scope.accounts().is_empty() {
        return Err(ErrorCode::NotFound);
    }
    if kind == ClientKind::Mcp
        && let Some(current) = store.load_mcp_registration()?
        && current.state == hantong_store::RegistrationState::Active
        && current.scope == scope
    {
        return status(
            store,
            current.request_id,
            ClientKind::Mcp,
            Some(DEFAULT_MCP_LABEL),
        )
        .map(SetupReceipt::Registration);
    }
    let request = match pending
        .filter(|r| r.client_kind == kind && r.display_label == label && r.scope == scope)
    {
        Some(request) => request,
        None => store.create_registration_request(
            hantong_platform::random_nonce()?,
            kind,
            label,
            &scope,
        )?,
    };
    receipt(store, &request).map(SetupReceipt::Registration)
}

pub(crate) fn activate_scope_refresh(
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    refresh_id: [u8; 32],
) -> Result<RegistrationRecord, ErrorCode> {
    activate_scope_refresh_for_channel(
        store,
        secrets_root,
        server,
        refresh_id,
        "terminal_cli",
        "terminal_cli",
    )
}

/// Completes an OAuth-origin scope refresh after the browser flow has already
/// produced the durable Store row.  Keeping this separate from the terminal
/// path prevents a row created for one decision channel from being activated
/// through another channel.
pub(crate) fn activate_google_scope_refresh(
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    refresh_id: [u8; 32],
) -> Result<RegistrationRecord, ErrorCode> {
    activate_scope_refresh_for_channel(
        store,
        secrets_root,
        server,
        refresh_id,
        "google_oauth",
        "google_oauth",
    )
}

fn activate_scope_refresh_for_channel(
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    refresh_id: [u8; 32],
    decision_channel: &str,
    approved_channel: &str,
) -> Result<RegistrationRecord, ErrorCode> {
    let pending = store
        .load_cli_scope_refresh(refresh_id)?
        .ok_or(ErrorCode::NotFound)?;
    if !matches!(
        pending.phase(),
        hantong_store::scope_refresh::ScopeRefreshPhase::Pending
            | hantong_store::scope_refresh::ScopeRefreshPhase::Staged
    ) || pending.decision_channel() != decision_channel
        || pending.base_registration().approved_channel != approved_channel
    {
        return Err(ErrorCode::Conflict);
    }
    let staged = if pending.phase() == hantong_store::scope_refresh::ScopeRefreshPhase::Pending {
        store.stage_cli_scope_refresh(&pending)?
    } else {
        pending
    };
    let registration_id = staged.base_registration().registration_id;
    let client = hantong_secrets::unix_signing::load_client_signing_key(
        secrets_root,
        registration_id,
        Some(staged.base_registration().public_key),
    )
    .map_err(|_| ErrorCode::InternalError)?;
    let client_public_key = client.public_key().map_err(|_| ErrorCode::InternalError)?;
    if hantong_platform::sha256(&client_public_key)? != *staged.client_public_key_hash() {
        return Err(ErrorCode::Conflict);
    }
    let server_public_key = server.public_key().map_err(|_| ErrorCode::InternalError)?;
    let server_hash = hantong_platform::sha256(&server_public_key)?;
    if server_hash != *staged.server_public_key_hash() {
        return Err(ErrorCode::Conflict);
    }
    let profile = ParsedClientProfile::new_with_scope(
        registration_id,
        hantong_platform::client_key_id_for_registration(registration_id)?,
        client_public_key,
        server_public_key,
        staged.target_profile_epoch(),
        staged.target_scope(),
    )?;
    let bytes = profile.to_json()?;
    let base = staged.base_registration();
    let old_profile = ParsedClientProfile::new_with_scope(
        base.registration_id,
        hantong_platform::client_key_id_for_registration(base.registration_id)?,
        base.public_key,
        server_public_key,
        base.profile_epoch,
        base.scope.clone(),
    )?;
    let old_bytes = old_profile.to_json()?;
    let current =
        ParsedClientProfile::from_json(hantong_platform::load_default_client_profile()?.bytes())?;
    let target_profile = ParsedClientProfile::from_json(&bytes)?;
    if !((current.registration_id() == &base.registration_id
        && current.client_public_key() == &base.public_key
        && current.profile_epoch() == base.profile_epoch
        && current.scope() == &base.scope
        && current.server_public_key() == &server_public_key)
        || current == target_profile)
    {
        return Err(ErrorCode::Conflict);
    }
    let mut publication = hantong_platform::unix_profile_publication::refresh_default_profile(
        *staged.refresh_id(),
        &old_bytes,
        &bytes,
    )?;
    let active = store.activate_cli_scope_refresh(&staged)?;
    hantong_platform::unix_profile_publication::cleanup_profile_refresh_backup(&mut publication)?;
    Ok(active)
}

/// Performs durable Unix startup recovery before the authenticated server is
/// exposed. In-flight provider operations are transitioned by Store recovery;
/// only OAuth-authorized pending scopes and already-staged scopes may resume.
pub(crate) fn recover_startup(
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
) -> Result<(), ErrorCode> {
    store.recover_mail_sends()?;
    store.recover_inflight_imap_trash()?;
    store.recover_google_mutations()?;

    let mut recovery_error = None;
    if let Some(pending) = store.load_pending_cli_scope_refresh()?
        && pending.decision_channel() == "google_oauth"
        && let Err(error) =
            activate_google_scope_refresh(store, secrets_root, server, *pending.refresh_id())
    {
        recovery_error = Some(error);
    }
    for staged in store.list_staged_cli_scope_refreshes()? {
        let result = match staged.decision_channel() {
            "terminal_cli" => {
                activate_scope_refresh(store, secrets_root, server, *staged.refresh_id())
            }
            "google_oauth" => {
                activate_google_scope_refresh(store, secrets_root, server, *staged.refresh_id())
            }
            _ => Err(ErrorCode::Conflict),
        };
        if let Err(error) = result {
            recovery_error.get_or_insert(error);
        }
    }
    recovery_error.map_or(Ok(()), Err)
}

fn status(
    store: &Store,
    id: [u8; 32],
    expected_kind: ClientKind,
    expected_label: Option<&str>,
) -> Result<RegistrationReceipt, ErrorCode> {
    let request = store
        .get_registration_request(id)?
        .ok_or(ErrorCode::NotFound)?;
    if request.client_kind != expected_kind
        || expected_label.is_some_and(|label| request.display_label != label)
    {
        return Err(ErrorCode::NotFound);
    }
    receipt(store, &request)
}

fn receipt(store: &Store, request: &RegistrationRequest) -> Result<RegistrationReceipt, ErrorCode> {
    let state = match request.state {
        RegistrationRequestState::Pending => RegistrationProgress::Pending,
        RegistrationRequestState::Denied => RegistrationProgress::Denied,
        RegistrationRequestState::Expired => RegistrationProgress::Expired,
        RegistrationRequestState::Approved => match store
            .load_registration_by_request(request.request_id)?
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
    RegistrationReceipt::new(request.request_id, request.expires_at_utc_ms, state)
}

pub(crate) fn terminal_review(
    state: &State,
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    owner: [u8; 16],
    peer: UnixPeerContextV2,
    command: TerminalReviewCommand,
) -> Result<SetupReceipt, ErrorCode> {
    match command {
        TerminalReviewCommand::Get(get) => get_review(state, store, owner, peer, get),
        TerminalReviewCommand::Confirm(confirm) => {
            confirm_review(state, store, secrets_root, server, owner, peer, confirm)
        }
        TerminalReviewCommand::Cancel(cancel) => {
            let valid = state
                .tickets
                .borrow()
                .get(&cancel.ticket)
                .is_some_and(|ticket| ticket.owner == owner && ticket.peer == peer);
            if !valid {
                return Err(ErrorCode::Conflict);
            }
            state.tickets.borrow_mut().remove(&cancel.ticket);
            let request = store
                .get_registration_request(cancel.get.request_id)?
                .ok_or(ErrorCode::NotFound)?;
            review_receipt(request, owner, None, [1; 32], "cancelled", Vec::new())
                .map(SetupReceipt::TerminalReview)
        }
        TerminalReviewCommand::SecretSubmit(_) => Err(ErrorCode::CapabilityUnsupported),
    }
}

fn get_review(
    state: &State,
    store: &Store,
    owner: [u8; 16],
    peer: UnixPeerContextV2,
    get: TerminalReviewGet,
) -> Result<SetupReceipt, ErrorCode> {
    if get.kind != TerminalReviewKind::Registration {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    let request = store
        .get_registration_request(get.request_id)?
        .ok_or(ErrorCode::NotFound)?;
    if request.state != RegistrationRequestState::Pending {
        return Err(ErrorCode::Conflict);
    }
    let snapshot_hash = registration_hash(&request)?;
    if get.payload_hash != [0; 32] && get.payload_hash != snapshot_hash {
        return Err(ErrorCode::Conflict);
    }
    let ticket = hantong_platform::random_nonce()?;
    let now = now_ms()?;
    if request.expires_at_utc_ms <= now {
        return Err(ErrorCode::ApprovalExpired);
    }
    state.tickets.borrow_mut().insert(
        ticket,
        Ticket {
            request_id: request.request_id,
            owner,
            snapshot_hash,
            expires_at_utc_ms: request.expires_at_utc_ms,
            peer,
        },
    );
    let mut fields = vec![
        TerminalReviewField::new("client".into(), request.display_label.clone())?,
        TerminalReviewField::new("accounts".into(), request.scope.accounts().join(", "))?,
        TerminalReviewField::new("tools".into(), request.scope.tools().join(", "))?,
    ];
    if request.client_kind == ClientKind::Ai {
        let provider = if request.display_label == DEFAULT_AI_CODEX_LABEL {
            "codex"
        } else if request.display_label == DEFAULT_AI_CLAUDE_LABEL {
            "claude"
        } else {
            return Err(ErrorCode::PolicyBlocked);
        };
        fields.push(TerminalReviewField::new(
            "provider".into(),
            provider.into(),
        )?);
        fields.push(TerminalReviewField::new("mail_transfer".into(), "Requested mail metadata and bodies from these accounts may be sent to the selected AI provider. The inbox is not uploaded in bulk.".into())?);
        fields.push(TerminalReviewField::new("feature".into(), "Connect all accounts listed above. Send and Trash still require separate terminal confirmation.".into())?);
    }
    review_receipt(
        request,
        owner,
        Some(ticket),
        snapshot_hash,
        "pending",
        fields,
    )
    .map(SetupReceipt::TerminalReview)
}

fn confirm_review(
    state: &State,
    store: &Store,
    secrets_root: &Path,
    server: &mut impl OpaqueSigner,
    owner: [u8; 16],
    peer: UnixPeerContextV2,
    confirm: hantong_contracts::terminal_review::TerminalReviewConfirm,
) -> Result<SetupReceipt, ErrorCode> {
    let now = now_ms()?;
    let valid = state
        .tickets
        .borrow()
        .get(&confirm.ticket)
        .is_some_and(|ticket| {
            ticket.owner == owner
                && ticket.peer == peer
                && ticket.snapshot_hash == confirm.snapshot_hash
                && ticket.expires_at_utc_ms > now
        });
    if !valid {
        return Err(ErrorCode::Conflict);
    }
    if !hantong_platform::unix_peer::peer_matches_current_image(&peer)? {
        return Err(ErrorCode::AuthRequired);
    }
    let ticket = state
        .tickets
        .borrow_mut()
        .remove(&confirm.ticket)
        .ok_or(ErrorCode::Conflict)?;
    let request = store
        .get_registration_request(ticket.request_id)?
        .ok_or(ErrorCode::NotFound)?;
    if request.state != RegistrationRequestState::Pending
        || registration_hash(&request)? != confirm.snapshot_hash
    {
        return Err(ErrorCode::Conflict);
    }
    let active = crate::unix_registered_profiles::commit(store, secrets_root, server, &request)?;
    review_receipt(
        request,
        owner,
        None,
        confirm.snapshot_hash,
        "ready",
        vec![TerminalReviewField::new(
            "registration_id".into(),
            hantong_contracts::wire_hex::encode(&active.registration_id),
        )?],
    )
    .map(SetupReceipt::TerminalReview)
}

fn review_receipt(
    request: RegistrationRequest,
    owner: [u8; 16],
    ticket: Option<[u8; 32]>,
    snapshot_hash: [u8; 32],
    state: &str,
    fields: Vec<TerminalReviewField>,
) -> Result<TerminalReviewReceipt, ErrorCode> {
    TerminalReviewReceipt {
        kind: TerminalReviewKind::Registration,
        request_id: request.request_id,
        ticket,
        snapshot_hash,
        owner_registration_id: owner,
        profile_epoch: 1,
        expires_at_utc_ms: request.expires_at_utc_ms,
        state: state.into(),
        secret_ticket_opaque: None,
        fields: fields.clone(),
    }
    .validate()
    .map(|()| TerminalReviewReceipt {
        kind: TerminalReviewKind::Registration,
        request_id: request.request_id,
        ticket,
        snapshot_hash,
        owner_registration_id: owner,
        profile_epoch: 1,
        expires_at_utc_ms: request.expires_at_utc_ms,
        state: state.into(),
        secret_ticket_opaque: None,
        fields,
    })
}

fn registration_hash(request: &RegistrationRequest) -> Result<[u8; 32], ErrorCode> {
    let bytes = serde_json::to_vec(&serde_json::json!({"domain":"mail-hantong.terminal-registration.v1","request_id":request.request_id.as_slice(),"client_kind":request.client_kind.as_str(),"display_label":request.display_label,"accounts":request.scope.accounts(),"tools":request.scope.tools(),"version":request.version,"created":request.created_at_utc_ms,"expires":request.expires_at_utc_ms})).map_err(|_| ErrorCode::InternalError)?;
    hantong_platform::sha256(&bytes)
}

fn now_ms() -> Result<i64, ErrorCode> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .ok_or(ErrorCode::InternalError)
}
