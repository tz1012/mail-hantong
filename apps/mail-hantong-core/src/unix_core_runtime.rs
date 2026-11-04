//! Unix Core runtime composition.
//!
//! Unix startup, auth-v2 and the common Store/application dispatcher.
//!
//! Provider mutations remain behind the existing review/ledger boundaries;
//! read, search, body and refresh operations use the same coordinator as the
//! Windows runtime.

use hantong_contracts::ErrorCode;
use hantong_contracts::platform::OpaqueSigner;
use hantong_ipc_server::{RpcRequest, unix_auth::RegisteredUnixClient};
use hantong_platform::unix_transport::UnixLocalServer;
use hantong_policy::ReadPolicy;
use hantong_store::{RegistrationState, Store};
use serde_json::{Value, json};
use std::time::Duration;
use std::{cell::Cell, path::Path, rc::Rc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_FRAME: usize = 1_048_576;

pub(crate) async fn serve(
    store: Store,
    mut server_signer: impl OpaqueSigner,
    server_instance: [u8; 32],
    secrets_root: &Path,
) -> Result<(), ErrorCode> {
    let shared_slot = Rc::new(Cell::new(None));
    let (folder_handle, folder_worker) =
        crate::folder_worker::coordinator_for_store(&store, secrets_root, shared_slot.clone());
    let (uid_handle, uid_worker) = crate::uid_worker::coordinator_for_store(
        &store,
        secrets_root,
        shared_slot,
        server_instance,
    );
    let (google_read_handle, google_read_worker) =
        crate::google_worker::coordinator(&store, secrets_root)?;
    let (google_state, google_worker) = crate::unix_google::State::new();
    let setup_state = crate::unix_setup::State::new(google_state);
    let mut google_signer = load_signer(&store, secrets_root)?;
    let mut policy = ReadPolicy::new();
    let shutdown = Rc::new(Cell::new(false));
    let server_future = serve_connections(
        &store,
        &mut server_signer,
        server_instance,
        &folder_handle,
        &uid_handle,
        &google_read_handle,
        secrets_root,
        &setup_state,
        &mut policy,
        shutdown.clone(),
    );
    tokio::pin!(server_future);
    let folder_future = folder_worker.run();
    tokio::pin!(folder_future);
    let uid_future = uid_worker.run();
    tokio::pin!(uid_future);
    let google_future = google_worker.run(&store, secrets_root, &mut google_signer);
    tokio::pin!(google_future);
    let action_future = setup_state.run_action_worker(&store, secrets_root);
    tokio::pin!(action_future);
    let google_read_future = google_read_worker.run();
    tokio::pin!(google_read_future);
    tokio::select! {
        result = &mut server_future => result,
        result = &mut folder_future => result,
        result = &mut uid_future => result,
        result = &mut google_future => result,
        result = &mut action_future => result,
        result = &mut google_read_future => result,
    }
}

#[allow(clippy::too_many_arguments)] // Explicit borrowed coordinators share the Core-owned Store.
async fn serve_connections(
    store: &Store,
    _server_signer: &mut impl OpaqueSigner,
    server_instance: [u8; 32],
    folder_worker: &crate::folder_worker::FolderWorkerHandle<'_>,
    uid_worker: &crate::uid_worker::UidWorkerHandle<'_>,
    google_worker: &crate::google_worker::GoogleHandle<'_>,
    secrets_root: &Path,
    setup_state: &crate::unix_setup::State,
    policy: &mut ReadPolicy,
    shutdown: Rc<Cell<bool>>,
) -> Result<(), ErrorCode> {
    let server = UnixLocalServer::bind()?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| ErrorCode::InternalError)?;
    let policy = std::cell::RefCell::new(policy);
    let brokers = Cell::new(0_usize);
    let mut active: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>>> = Vec::new();
    loop {
        tokio::select! {
            accepted = server.accept(), if active.len() < 16 => {
                let connection = match accepted {
                    Ok(connection) => connection,
                    Err(ErrorCode::AuthRequired | ErrorCode::ScopeDenied) => continue,
                    Err(error) => return Err(error),
                };
                let policy = &policy;
                let brokers = &brokers;
                let shutdown = shutdown.clone();
                active.push(Box::pin(async move {
                    // Malformed/disconnected clients cannot stop the daemon.
                    let _ = tokio::time::timeout(Duration::from_secs(40), async {
                        let mut connection = connection;
                        let trusted = connection.peer_matches_current_image()?;
                        let first = read_first(&mut connection, if trusted { 1_050_112 } else { 131_072 }).await?;
                        if first.method == "core.stop" {
                            if !trusted || !exact_protocol_params(&first.params) {
                                return Err(ErrorCode::AuthRequired);
                            }
                            let response = json!({
                                "jsonrpc": "2.0",
                                "id": first.id,
                                "result": { "state": "stopping", "protocol_version": 1 }
                            });
                            // Do not begin shutdown until the updater has received the
                            // bounded response and ACK. A dropped connection therefore
                            // cannot stop the Core.
                            send_value(&mut connection, &response).await?;
                            shutdown.set(true);
                            return Ok(());
                        }
                        if matches!(first.method.as_str(), "system.readiness" | "system.version") {
                            let value = hantong_ipc_server::bootstrap::response(first);
                            return send_value(&mut connection, &value).await;
                        }
                        if first.method == "core.relay" {
                            if !trusted || brokers.get() >= 4 { return Err(ErrorCode::AuthRequired); }
                            brokers.set(brokers.get() + 1);
                            struct Slot<'a>(&'a Cell<usize>);
                            impl Drop for Slot<'_> { fn drop(&mut self) { self.0.set(self.0.get() - 1); } }
                            let _slot = Slot(brokers);
                            let id = first.id.clone();
                            let response = match broker(store, secrets_root, setup_state, &connection, first).await {
                                Ok(response) => response,
                                Err(code) => json!({"jsonrpc":"2.0","id":id,"error":{"code":code.as_str(),"message":code.as_str()}}),
                            };
                            return send_value(&mut connection, &response).await;
                        }
                        let mut signer = load_signer(store, secrets_root)?;
                        let mut lookup = |id: &[u8; 16]| lookup_registration(store, id);
                        let mut authenticated = hantong_ipc_server::unix_authenticated_wire::authenticate_after_begin(
                            connection, first, &mut signer, server_instance, &mut lookup,
                        ).await?;
                        exchange_authenticated(store, &mut authenticated, |connection, request, _| {
                            // Bootstrap and terminal review require the verified foreground Core helper.
                            if request.method.starts_with("setup.") || request.method.starts_with("registration.")
                                || request.method.starts_with("terminal.review.") {
                                return Err(ErrorCode::AuthRequired);
                            }
                            crate::unix_execution_adapter::execute_with_google(
                                store, &mut policy.borrow_mut(), folder_worker, Some(uid_worker), google_worker, connection, &request,
                            )
                        }).await
                    }).await;
                }));
            }
            index = std::future::poll_fn(|cx| {
                for (index, future) in active.iter_mut().enumerate() {
                    if future.as_mut().poll(cx).is_ready() { return std::task::Poll::Ready(index); }
                }
                std::task::Poll::Pending
            }), if !active.is_empty() => {
                drop(active.swap_remove(index));
                if shutdown.get() { return Ok(()); }
            }
            _ = terminate.recv() => return Ok(()),
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|_| ErrorCode::InternalError)?;
                return Ok(());
            }
        }
    }
}

fn exact_protocol_params(params: &Value) -> bool {
    let Some(object) = params.as_object() else {
        return false;
    };
    object.len() == 1 && object.get("protocol_version") == Some(&Value::from(1))
}

fn load_signer(
    store: &Store,
    root: &Path,
) -> Result<hantong_secrets::unix_signing::OpaqueSigner, ErrorCode> {
    let Some(hantong_store::ServerIdentity::Active {
        key_id, public_key, ..
    }) = store.load_server_identity()?
    else {
        return Err(ErrorCode::AuthRequired);
    };
    hantong_secrets::unix_signing::load_server_signing_key(root, key_id, public_key)
        .map_err(|_| ErrorCode::AuthRequired)
}

async fn read_first(
    connection: &mut hantong_platform::unix_transport::UnixLocalConnection,
    max: usize,
) -> Result<RpcRequest, ErrorCode> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let length = connection
            .read_u32_le()
            .await
            .map_err(|_| ErrorCode::AuthRequired)? as usize;
        if !(1..=max).contains(&length) {
            return Err(ErrorCode::InvalidInput);
        }
        let mut frame = vec![0; length + 4];
        frame[..4].copy_from_slice(&(length as u32).to_le_bytes());
        connection
            .read_exact(&mut frame[4..])
            .await
            .map_err(|_| ErrorCode::AuthRequired)?;
        if length > hantong_ipc_server::MAX_FRAME_SIZE {
            hantong_ipc_server::decode_core_relay_request(&frame)
        } else {
            hantong_ipc_server::decode_request(&frame)
        }
    })
    .await
    .map_err(|_| ErrorCode::AuthRequired)?
}

async fn send_value(
    connection: &mut hantong_platform::unix_transport::UnixLocalConnection,
    value: &Value,
) -> Result<(), ErrorCode> {
    let body = serde_json::to_vec(value).map_err(|_| ErrorCode::InternalError)?;
    if body.is_empty() || body.len() > hantong_ipc_client::unix_relay::MAX_FRAME {
        return Err(ErrorCode::QuotaExceeded);
    }
    connection
        .write_u32_le(body.len() as u32)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    connection
        .write_all(&body)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    connection
        .flush()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if connection
        .read_u8()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?
        != 0x06
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

async fn broker(
    store: &Store,
    root: &Path,
    setup: &crate::unix_setup::State,
    connection: &hantong_platform::unix_transport::UnixLocalConnection,
    outer: RpcRequest,
) -> Result<Value, ErrorCode> {
    use hantong_ipc_client::unix_relay::{RelayProfile, RelayRequest};
    // Decode the canonical inner representation through the client's strict fixed allowlist.
    let bytes = serde_json::to_vec(&outer.params).map_err(|_| ErrorCode::InvalidInput)?;
    let relay = serde_json::from_slice::<Value>(&bytes).map_err(|_| ErrorCode::InvalidInput)?;
    let object = relay.as_object().ok_or(ErrorCode::InvalidInput)?;
    if object.len() != 5 || object.get("protocol_version") != Some(&Value::from(1)) {
        return Err(ErrorCode::InvalidInput);
    }
    let relay = RelayRequest::new(
        RelayProfile::parse(
            object
                .get("profile")
                .and_then(Value::as_str)
                .ok_or(ErrorCode::InvalidInput)?,
        )?,
        object
            .get("request_id")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?,
        object
            .get("method")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?,
        object
            .get("params")
            .cloned()
            .ok_or(ErrorCode::InvalidInput)?,
    )?;
    if outer.id != relay.request_id {
        return Err(ErrorCode::InvalidInput);
    }
    let request_bytes = serde_json::to_vec(
        &json!({"jsonrpc":"2.0","id":relay.request_id,"method":relay.method,"params":relay.params}),
    )
    .map_err(|_| ErrorCode::InvalidInput)?;
    let mut framed = (request_bytes.len() as u32).to_le_bytes().to_vec();
    framed.extend_from_slice(&request_bytes);
    let request = hantong_ipc_server::decode_request(&framed)?;
    if request.method.starts_with("setup.")
        || request.method.starts_with("registration.")
        || request.method.starts_with("terminal.review.")
    {
        if relay.profile != RelayProfile::Default {
            return Err(ErrorCode::ScopeDenied);
        }
        let owner = store
            .load_default_cli_registration()?
            .map(|r| r.registration_id)
            .unwrap_or([0; 16]);
        let mut signer = load_signer(store, root)?;
        let mut command = None;
        let rejected = hantong_ipc_server::setup_dispatch::response(
            &request,
            request_bytes.len(),
            &mut |parsed| {
                command = Some(parsed);
                Err(ErrorCode::InternalError)
            },
        );
        let Some(command) = command else {
            return Ok(rejected);
        };
        let owner = crate::unix_setup::review_owner(setup, store, &command, owner)?;
        let mut receipt = Some(
            crate::unix_setup::dispatch_async(
                setup,
                store,
                root,
                &mut signer,
                owner,
                connection.peer_context().clone(),
                command,
            )
            .await,
        );
        return Ok(hantong_ipc_server::setup_dispatch::response(
            &request,
            request_bytes.len(),
            &mut |_| receipt.take().ok_or(ErrorCode::InternalError)?,
        ));
    }
    let result = profiled_call(store, relay).await;
    match result {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| ErrorCode::InvalidInput),
        Err(code) => Ok(
            json!({"jsonrpc":"2.0","id":outer.id,"error":{"code":code.as_str(),"message":code.as_str()}}),
        ),
    }
}

async fn profiled_call(
    store: &Store,
    request: hantong_ipc_client::unix_relay::RelayRequest,
) -> Result<Vec<u8>, ErrorCode> {
    use hantong_ipc_client::unix_relay::RelayProfile;
    use hantong_secrets::unix_secret_store::{UnixSecretKind, UnixSecretSelector, UnixSecretStore};
    let loaded = match request.profile {
        RelayProfile::Default => hantong_platform::load_default_client_profile(),
        RelayProfile::Mcp => hantong_platform::load_mcp_client_profile(),
        RelayProfile::AiCodex => hantong_platform::load_ai_codex_client_profile(),
        RelayProfile::AiClaude => hantong_platform::load_ai_claude_client_profile(),
    }?;
    let profile = hantong_ipc_client::profile::ParsedClientProfile::from_json(loaded.bytes())?;
    let expected_profile = match request.profile {
        RelayProfile::Default => hantong_ipc_client::profile::ProfileSelector::Default,
        RelayProfile::Mcp => hantong_ipc_client::profile::ProfileSelector::Mcp,
        RelayProfile::AiCodex => hantong_ipc_client::profile::ProfileSelector::AiCodex,
        RelayProfile::AiClaude => hantong_ipc_client::profile::ProfileSelector::AiClaude,
    };
    if profile.profile() != expected_profile {
        return Err(ErrorCode::AuthRequired);
    }
    let record = store
        .load_registration(*profile.registration_id())?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    if !registration_matches_profile(request.profile, record.client_kind, &record.display_label)
        || record.state != RegistrationState::Active
        || record.profile_epoch != profile.profile_epoch()
        || record.public_key != *profile.client_public_key()
        || record.scope != *profile.scope()
    {
        return Err(ErrorCode::AuthRequired);
    }
    let key_id = profile
        .client_key_id()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let selector = UnixSecretSelector::new(UnixSecretKind::ClientSigningKey, Some(&key_id), 1)
        .map_err(|_| ErrorCode::AuthRequired)?;
    let signer = hantong_secrets::unix_signing::OpaqueSigner::load(
        &UnixSecretStore::new(),
        &selector,
        *profile.client_public_key(),
    )
    .map_err(|_| ErrorCode::AuthRequired)?;
    let connection = hantong_platform::unix_transport::connect().await?;
    let pending = hantong_ipc_client::unix_auth::PendingUnixAuthentication::begin_connected(
        signer,
        connection,
        *profile.server_public_key(),
        *profile.registration_id(),
        profile.profile_epoch(),
        profile.scope().clone(),
        hantong_platform::random_nonce()?,
        i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| ErrorCode::InternalError)?
                .as_millis(),
        )
        .map_err(|_| ErrorCode::InternalError)?,
    )?;
    let client = hantong_ipc_client::unix_authenticated_wire::authenticate_connected(
        pending,
        &request.request_id,
    )
    .await?;
    client
        .exchange(&request.request_id, &request.method, &request.params)
        .await
}

fn registration_matches_profile(
    profile: hantong_ipc_client::unix_relay::RelayProfile,
    kind: hantong_store::ClientKind,
    label: &str,
) -> bool {
    use hantong_ipc_client::unix_relay::RelayProfile;
    use hantong_store::ClientKind;
    let expected = match profile {
        RelayProfile::Default => (ClientKind::Cli, hantong_store::DEFAULT_CLI_LABEL),
        RelayProfile::Mcp => (ClientKind::Mcp, hantong_store::DEFAULT_MCP_LABEL),
        RelayProfile::AiCodex => (ClientKind::Ai, hantong_store::DEFAULT_AI_CODEX_LABEL),
        RelayProfile::AiClaude => (ClientKind::Ai, hantong_store::DEFAULT_AI_CLAUDE_LABEL),
    };
    (kind, label) == expected
}

fn lookup_registration(
    store: &Store,
    id: &[u8; 16],
) -> Result<Option<RegisteredUnixClient>, ErrorCode> {
    let Some(record) = store.load_registration(*id)? else {
        return Ok(None);
    };
    RegisteredUnixClient::from_registration_store(
        record.registration_id,
        record.profile_epoch,
        record.public_key,
        record.scope,
        record.state == RegistrationState::Active,
    )
    .map(Some)
}

async fn exchange_authenticated<F>(
    store: &Store,
    connection: &mut hantong_ipc_server::unix_auth::AuthenticatedUnixConnection,
    mut dispatch: F,
) -> Result<(), ErrorCode>
where
    F: FnMut(
        &mut hantong_ipc_server::unix_auth::AuthenticatedUnixConnection,
        RpcRequest,
        usize,
    ) -> Result<Value, ErrorCode>,
{
    let current = lookup_registration(store, connection.registration_id())?;
    let frame = {
        let transport = connection.transport()?;
        let length = transport
            .read_u32_le()
            .await
            .map_err(|_| ErrorCode::AuthRequired)? as usize;
        if !(1..=MAX_FRAME).contains(&length) {
            return Err(ErrorCode::InvalidInput);
        }
        let mut frame = vec![0; length + 4];
        frame[..4].copy_from_slice(&(length as u32).to_le_bytes());
        transport
            .read_exact(&mut frame[4..])
            .await
            .map_err(|_| ErrorCode::AuthRequired)?;
        frame
    };
    let frame_length = frame.len() - 4;
    let request = hantong_ipc_server::decode_request(&frame)?;
    let authorization = authorize(connection, current.as_ref(), &request);
    let id = request.id.clone();
    let value = match authorization.and_then(|_| dispatch(connection, request, frame_length)) {
        Ok(result) => json!({"jsonrpc":"2.0", "id":id, "result":result}),
        Err(code) => {
            json!({"jsonrpc":"2.0", "id":id, "error":{"code":code.as_str(),"message":code.as_str()}})
        }
    };
    let transport = connection.transport()?;
    let body = serde_json::to_vec(&value).map_err(|_| ErrorCode::InternalError)?;
    if body.is_empty() || body.len() > MAX_FRAME {
        return Err(ErrorCode::QuotaExceeded);
    }
    transport
        .write_u32_le(body.len() as u32)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    transport
        .write_all(&body)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    transport
        .flush()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    if transport
        .read_u8()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?
        != 0x06
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

fn authorize(
    connection: &mut hantong_ipc_server::unix_auth::AuthenticatedUnixConnection,
    current: Option<&RegisteredUnixClient>,
    request: &RpcRequest,
) -> Result<(), ErrorCode> {
    if matches!(
        request.method.as_str(),
        "system.version" | "system.readiness"
    ) {
        return connection.revalidate_registration(current);
    }
    connection.revalidate_registration(current)?;
    let tool = hantong_application::ToolRequest::validate_method(&request.method, &request.params)?;
    if tool.request_id() != request.id {
        return Err(ErrorCode::InvalidInput);
    }
    for account in tool.account_ids() {
        connection.check_account_scope(current, account, tool.tool_name())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai09_relay_profile_rejects_cross_provider_and_client_kind() {
        use hantong_ipc_client::unix_relay::RelayProfile;
        use hantong_store::ClientKind;
        assert!(registration_matches_profile(
            RelayProfile::AiCodex,
            ClientKind::Ai,
            hantong_store::DEFAULT_AI_CODEX_LABEL
        ));
        assert!(!registration_matches_profile(
            RelayProfile::AiCodex,
            ClientKind::Ai,
            hantong_store::DEFAULT_AI_CLAUDE_LABEL
        ));
        assert!(!registration_matches_profile(
            RelayProfile::AiClaude,
            ClientKind::Ai,
            hantong_store::DEFAULT_AI_CODEX_LABEL
        ));
        assert!(!registration_matches_profile(
            RelayProfile::Mcp,
            ClientKind::Cli,
            hantong_store::DEFAULT_MCP_LABEL
        ));
        assert!(!registration_matches_profile(
            RelayProfile::Default,
            ClientKind::Mcp,
            hantong_store::DEFAULT_CLI_LABEL
        ));
    }

    #[test]
    fn unix_runtime_rejects_non_protocol_bootstrap_params() {
        for (params, expected) in [
            (json!({}), ErrorCode::InvalidInput),
            (json!({"protocol_version":2}), ErrorCode::VersionMismatch),
        ] {
            let response = hantong_ipc_server::bootstrap::response(RpcRequest {
                id: "test".into(),
                method: "system.readiness".into(),
                params,
            });
            assert_eq!(response["error"]["code"], expected.as_str());
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "unix_broker_tests.rs"]
mod broker_tests;

#[cfg(all(test, target_os = "linux"))]
#[path = "unix_product_tests.rs"]
mod product_tests;
