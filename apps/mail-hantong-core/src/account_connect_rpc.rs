//! Post-authentication app service. Bootstrap and ordinary relay never call it.

use hantong_contracts::{ErrorCode, account_connect as wire};
use hantong_ipc_server::RpcRequest;
use hantong_store::{ClientKind, DEFAULT_CLI_LABEL, RegistrationRecord, RegistrationState, Store};
use serde_json::Value;
use std::path::Path;

pub(crate) fn is_method(method: &str) -> bool {
    matches!(
        method,
        wire::PREPARE_METHOD
            | wire::STATUS_METHOD
            | wire::COMPLETE_METHOD
            | wire::CANCEL_METHOD
            | wire::BIND_SMTP_METHOD
    )
}

fn command(request: &RpcRequest) -> Result<wire::AccountConnectCommand, ErrorCode> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "jsonrpc":"2.0", "id":request.id, "method":request.method, "params":request.params,
    }))
    .map_err(|_| ErrorCode::InvalidInput)?;
    let (id, command) = wire::parse_request(&bytes)?;
    if id != request.id {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(command)
}

fn default_owner(store: &Store, record: &RegistrationRecord) -> Result<(), ErrorCode> {
    if record.state != RegistrationState::Active {
        return Err(ErrorCode::ClientSuspended);
    }
    if record.client_kind != ClientKind::Cli || record.display_label != DEFAULT_CLI_LABEL {
        return Err(ErrorCode::ScopeDenied);
    }
    if store.load_default_cli_registration()?.as_ref() != Some(record) {
        return Err(ErrorCode::ScopeDenied);
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn windows(
    state: &crate::account_connect_service::State,
    store: &Store,
    root: &Path,
    server_public_key: [u8; 72],
    smtp_sessions: &crate::setup_smtp::Sessions,
    connection: &mut hantong_ipc_server::auth::AuthenticatedConnection<'_>,
    request: &RpcRequest,
) -> Result<Value, ErrorCode> {
    let record = store
        .load_registration(*connection.registration_id())?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    default_owner(store, &record)?;
    let current = hantong_ipc_server::auth::RegisteredClient::from_registration_store(
        record.registration_id,
        record.profile_epoch,
        record.public_key,
        record.scope.clone(),
        true,
    )?;
    connection.revalidate_registration(Some(&current))?;
    let transport = connection.transport()?;
    if !transport.peer_matches_current_image()? {
        return Err(ErrorCode::ScopeDenied);
    }
    let peer = crate::account_connect::ObservedPeer::Windows(transport.peer_context()?);
    let receipt = state.command(
        store,
        root,
        server_public_key,
        &record,
        peer,
        smtp_sessions,
        command(request)?,
    )?;
    serde_json::from_slice(&receipt.to_json()?).map_err(|_| ErrorCode::InternalError)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn unix(
    state: &crate::account_connect_service::State,
    store: &Store,
    root: &Path,
    server_public_key: [u8; 72],
    smtp_sessions: &crate::setup_smtp::Sessions,
    connection: &mut hantong_ipc_server::unix_auth::AuthenticatedUnixConnection,
    request: &RpcRequest,
) -> Result<Value, ErrorCode> {
    let record = store
        .load_registration(*connection.registration_id())?
        .ok_or(ErrorCode::ClientNotRegistered)?;
    default_owner(store, &record)?;
    let current = hantong_ipc_server::unix_auth::RegisteredUnixClient::from_registration_store(
        record.registration_id,
        record.profile_epoch,
        record.public_key,
        record.scope.clone(),
        true,
    )?;
    connection.revalidate_registration(Some(&current))?;
    let transport = connection.transport()?;
    if !transport.peer_matches_current_image()? {
        return Err(ErrorCode::ScopeDenied);
    }
    let peer = crate::account_connect::ObservedPeer::Unix(transport.peer_context().clone());
    let receipt = state.command(
        store,
        root,
        server_public_key,
        &record,
        peer,
        smtp_sessions,
        command(request)?,
    )?;
    serde_json::from_slice(&receipt.to_json()?).map_err(|_| ErrorCode::InternalError)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sec03_connect_requires_exact_canonical_default_record() {
        let f = crate::account_connect_generic::tests::Fixture::new();
        assert_eq!(default_owner(&f.store, &f.base), Ok(()));
        let mut wrong = f.base.clone();
        wrong.registration_id = [99; 16];
        assert_eq!(default_owner(&f.store, &wrong), Err(ErrorCode::ScopeDenied));
        let mut wrong = f.base.clone();
        wrong.version += 1;
        assert_eq!(default_owner(&f.store, &wrong), Err(ErrorCode::ScopeDenied));
        let mut wrong = f.base.clone();
        wrong.profile_epoch += 1;
        assert_eq!(default_owner(&f.store, &wrong), Err(ErrorCode::ScopeDenied));
        let mut wrong = f.base.clone();
        wrong.state = RegistrationState::Suspended;
        assert_eq!(
            default_owner(&f.store, &wrong),
            Err(ErrorCode::ClientSuspended)
        );
        assert!(
            f.store
                .list_staged_full_mail_decisions()
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn sec03_connect_service_rejects_prefix_and_supplied_authority() {
        for method in [
            "app.accounts.connect.approve",
            "app.accounts.connect.prepare.extra",
            "setup.google.request",
            "mail.accounts.list",
        ] {
            assert!(!is_method(method));
        }
        let request = RpcRequest {
            id: "connect".into(),
            method: wire::PREPARE_METHOD.into(),
            params: serde_json::json!({"protocol_version":1,"provider":"google","selection":{"kind":"new"},"approved":true}),
        };
        assert!(command(&request).is_err());
    }
}
