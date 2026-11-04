//! Publishes the fixed public profile corresponding to a confirmed registration.
use hantong_contracts::{ErrorCode, platform::OpaqueSigner, registration::AiProvider};
use hantong_ipc_client::profile::{ParsedClientProfile, ProfileSelector};
use hantong_store::{
    AiRegistrationStage, CliRegistrationStage, ClientKind, DEFAULT_AI_CLAUDE_LABEL,
    DEFAULT_AI_CODEX_LABEL, McpMailRegistrationStage, McpRegistrationStage, RegistrationRecord,
    RegistrationRequest, RegistrationRequestState, Store,
};
use std::path::Path;

pub(crate) fn commit(
    store: &Store,
    root: &Path,
    server: &mut impl OpaqueSigner,
    request: &RegistrationRequest,
) -> Result<RegistrationRecord, ErrorCode> {
    let current = store
        .get_registration_request(request.request_id)?
        .ok_or(ErrorCode::NotFound)?;
    if &current != request || current.state != RegistrationRequestState::Pending {
        return Err(ErrorCode::Conflict);
    }
    let id = hantong_platform::registration_id_for_request(request.request_id)?;
    let client = hantong_secrets::unix_signing::load_client_signing_key(root, id, None)
        .map_err(|_| ErrorCode::InternalError)?;
    let client_public = client.public_key().map_err(|_| ErrorCode::InternalError)?;
    let public = server.public_key().map_err(|_| ErrorCode::InternalError)?;
    let pin = hantong_platform::sha256(&public)?;
    let provider = match (request.client_kind, request.display_label.as_str()) {
        (ClientKind::Ai, DEFAULT_AI_CODEX_LABEL) => Some(AiProvider::Codex),
        (ClientKind::Ai, DEFAULT_AI_CLAUDE_LABEL) => Some(AiProvider::Claude),
        (ClientKind::Ai, _) => return Err(ErrorCode::PolicyBlocked),
        _ => None,
    };
    let staged = match request.client_kind {
        ClientKind::Cli => store.stage_terminal_cli_registration(
            &CliRegistrationStage::new_terminal_cli(request.clone(), id, client_public, pin)?,
        )?,
        ClientKind::Mcp
            if request.scope.tools() == hantong_contracts::auth::mcp_mail_tools_v3() =>
        {
            let predecessor = store.load_mcp_registration()?;
            store.stage_terminal_mcp_v3_registration(&McpMailRegistrationStage::new_terminal_v3(
                request.clone(),
                id,
                client_public,
                pin,
                predecessor,
            )?)?
        }
        ClientKind::Mcp if request.scope.tools() == hantong_contracts::auth::mcp_mail_tools() => {
            store.stage_terminal_mcp_mail_registration(
                &McpMailRegistrationStage::new_terminal_mcp_mail(
                    request.clone(),
                    id,
                    client_public,
                    pin,
                    store.load_mcp_registration()?,
                )?,
            )?
        }
        ClientKind::Mcp => store.stage_terminal_mcp_registration(
            &McpRegistrationStage::new_terminal_mcp(request.clone(), id, client_public, pin)?,
        )?,
        ClientKind::Ai => store.stage_ai_registration(&AiRegistrationStage::new(
            provider.ok_or(ErrorCode::InvalidInput)?,
            request.clone(),
            id,
            client_public,
            pin,
        )?)?,
        _ => return Err(ErrorCode::CapabilityUnsupported),
    };
    publish_activate(store, staged, public, client_public, provider)
}

pub(crate) fn recover_startup(
    store: &Store,
    root: &Path,
    server: &mut impl OpaqueSigner,
) -> Result<(), ErrorCode> {
    let public = server.public_key().map_err(|_| ErrorCode::InternalError)?;
    for staged in store.list_provisioning_registrations(128)? {
        if !matches!(
            staged.approved_channel.as_str(),
            "terminal_cli" | "google_oauth" | "ai_feature"
        ) {
            return Err(ErrorCode::PolicyBlocked);
        }
        let provider = match (staged.client_kind, staged.display_label.as_str()) {
            (ClientKind::Ai, DEFAULT_AI_CODEX_LABEL) => Some(AiProvider::Codex),
            (ClientKind::Ai, DEFAULT_AI_CLAUDE_LABEL) => Some(AiProvider::Claude),
            (ClientKind::Ai, _) => return Err(ErrorCode::PolicyBlocked),
            _ => None,
        };
        let client = hantong_secrets::unix_signing::load_client_signing_key(
            root,
            staged.registration_id,
            Some(staged.public_key),
        )
        .map_err(|_| ErrorCode::AuthRequired)?;
        let client_public = client.public_key().map_err(|_| ErrorCode::AuthRequired)?;
        publish_activate(store, staged, public, client_public, provider)?;
    }
    Ok(())
}

fn publish_activate(
    store: &Store,
    staged: RegistrationRecord,
    public: [u8; 72],
    client_public: [u8; 72],
    provider: Option<AiProvider>,
) -> Result<RegistrationRecord, ErrorCode> {
    let id = staged.registration_id;
    let pin = hantong_platform::sha256(&public)?;
    if staged.state != hantong_store::RegistrationState::Provisioning
        || client_public != staged.public_key
    {
        return Err(ErrorCode::Conflict);
    }
    let key_id = hantong_platform::client_key_id_for_registration(id)?;
    let profile = match staged.client_kind {
        ClientKind::Cli => ParsedClientProfile::new_with_scope(
            id,
            key_id,
            client_public,
            public,
            staged.profile_epoch,
            staged.scope.clone(),
        )?,
        ClientKind::Mcp if staged.scope.tools() == hantong_contracts::auth::mcp_mail_tools_v3() => {
            ParsedClientProfile::new_mcp_mail_v3(
                id,
                key_id,
                client_public,
                public,
                staged.profile_epoch,
                staged.scope.accounts().to_vec(),
            )?
        }
        ClientKind::Mcp if staged.scope.tools() == hantong_contracts::auth::mcp_mail_tools() => {
            ParsedClientProfile::new_mcp_mail(
                id,
                key_id,
                client_public,
                public,
                staged.profile_epoch,
                staged.scope.accounts().to_vec(),
            )?
        }
        ClientKind::Mcp => ParsedClientProfile::new_mcp(
            id,
            key_id,
            client_public,
            public,
            staged.profile_epoch,
            staged.scope.accounts().to_vec(),
        )?,
        ClientKind::Ai => ParsedClientProfile::new_ai(
            if provider == Some(AiProvider::Codex) {
                ProfileSelector::AiCodex
            } else {
                ProfileSelector::AiClaude
            },
            id,
            key_id,
            client_public,
            public,
            staged.profile_epoch,
            staged.scope.accounts().to_vec(),
        )?,
        _ => return Err(ErrorCode::CapabilityUnsupported),
    };
    if profile.scope() != &staged.scope {
        return Err(ErrorCode::Conflict);
    }
    let bytes = profile.to_json()?;
    let mut publication = match staged.client_kind {
        ClientKind::Cli => {
            hantong_platform::unix_profile_publication::publish_default_profile(&bytes)?
        }
        ClientKind::Mcp => hantong_platform::unix_profile_publication::publish_mcp_profile(&bytes)?,
        ClientKind::Ai => hantong_platform::unix_profile_publication::publish_ai_profile(
            provider.ok_or(ErrorCode::InvalidInput)?,
            &bytes,
        )?,
        _ => return Err(ErrorCode::CapabilityUnsupported),
    };
    let active = match staged.client_kind {
        ClientKind::Cli => store.activate_cli_registration(id, staged.version, pin)?,
        ClientKind::Mcp => store.activate_mcp_registration(id, staged.version, pin)?,
        ClientKind::Ai => store.activate_ai_registration(
            provider.ok_or(ErrorCode::InvalidInput)?,
            id,
            staged.version,
            pin,
        )?,
        _ => return Err(ErrorCode::CapabilityUnsupported),
    };
    hantong_platform::unix_profile_publication::cleanup_profile_refresh_backup(&mut publication)?;
    Ok(active)
}
