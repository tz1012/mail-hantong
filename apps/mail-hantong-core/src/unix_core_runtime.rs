//! Unix Core runtime composition.
//!
//! This is intentionally small: startup, storage, auth-v2 and the first
//! read-only account tool share the same Store authority. Provider workers and
//! mutation tools are added behind this authenticated dispatcher as their
//! Unix setup paths become available.

use hantong_contracts::ErrorCode;
use hantong_domain::accounts::AccountSnapshot;
use hantong_ipc_server::{RpcRequest, unix_auth::RegisteredUnixClient};
use hantong_platform::unix_transport::UnixLocalServer;
use hantong_secrets::unix_signing::OpaqueSigner;
use hantong_store::{RegistrationState, Store};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_FRAME: usize = 1_048_576;

pub(crate) async fn serve(
    store: Store,
    mut server_signer: impl OpaqueSigner,
    server_instance: [u8; 32],
) -> Result<(), ErrorCode> {
    let server = UnixLocalServer::bind()?;
    loop {
        let connection = tokio::select! {
            accepted = server.accept() => accepted?,
            signal = tokio::signal::ctrl_c() => {
                signal.map_err(|_| ErrorCode::InternalError)?;
                return Ok(());
            }
        };
        let store_ref = &store;
        let mut lookup = |id: &[u8; 16]| lookup_registration(store_ref, id);
        let authenticated =
            match hantong_ipc_server::unix_authenticated_wire::authenticate_connected(
                connection,
                &mut server_signer,
                server_instance,
                &mut lookup,
            )
            .await
            {
                Ok(value) => value,
                Err(
                    ErrorCode::AuthRequired
                    | ErrorCode::ClientNotRegistered
                    | ErrorCode::ClientSuspended
                    | ErrorCode::ScopeDenied
                    | ErrorCode::InvalidInput,
                ) => continue,
                Err(code) => return Err(code),
            };
        let store_ref = &store;
        let mut current = authenticated;
        let exchange = exchange_authenticated(store_ref, &mut current, |request| {
            dispatch(store_ref, request)
        });
        match tokio::time::timeout(Duration::from_secs(5), exchange).await {
            Ok(Ok(())) | Ok(Err(ErrorCode::AuthRequired | ErrorCode::ClientNotRegistered)) => {}
            Ok(Err(ErrorCode::InternalError)) => return Err(ErrorCode::InternalError),
            Ok(Err(_)) | Err(_) => {}
        }
    }
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
    F: FnMut(RpcRequest) -> Result<Value, ErrorCode>,
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
    let request = hantong_ipc_server::decode_request(&frame)?;
    authorize(connection, current.as_ref(), &request)?;
    let transport = connection.transport()?;
    let id = request.id.clone();
    let value = match dispatch(request) {
        Ok(result) => json!({"jsonrpc":"2.0", "id":id, "result":result}),
        Err(code) => {
            json!({"jsonrpc":"2.0", "id":id, "error":{"code":code.as_str(),"message":code.as_str()}})
        }
    };
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
    if request.method != "mail.accounts.list" {
        return connection.revalidate_registration(current);
    }
    let params = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    let ids: Vec<&str> = match (params.get("account_id"), params.get("account_ids")) {
        (Some(Value::String(id)), None) => vec![id.as_str()],
        (None, Some(Value::Array(values))) => values
            .iter()
            .map(|value| value.as_str().ok_or(ErrorCode::InvalidInput))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(ErrorCode::InvalidInput),
    };
    for account in ids {
        connection.check_account_scope(current, account, "mail.accounts.list")?;
    }
    Ok(())
}

fn dispatch(store: &Store, request: RpcRequest) -> Result<Value, ErrorCode> {
    if request.method == "system.version" {
        require_protocol(&request.params)?;
        return Ok(
            json!({"product":"mail-hantong","version":env!("CARGO_PKG_VERSION"),"protocol_version":1}),
        );
    }
    if request.method == "system.readiness" {
        require_protocol(&request.params)?;
        return Ok(
            json!({"transport_ready":true,"mail_ready":false,"authenticated":true,"protocol_version":1}),
        );
    }
    if request.method != "mail.accounts.list" {
        return Err(ErrorCode::CapabilityUnsupported);
    }
    let params = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    let ids = match (params.get("account_id"), params.get("account_ids")) {
        (Some(Value::String(id)), None) => vec![id.clone()],
        (None, Some(Value::Array(values))) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(ErrorCode::InvalidInput)
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(ErrorCode::InvalidInput),
    };
    let include = params
        .get("include_capabilities")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let snapshot = store.list_account_summaries(&ids, include)?;
    Ok(render_accounts(snapshot))
}

fn require_protocol(params: &Value) -> Result<(), ErrorCode> {
    let object = params.as_object().ok_or(ErrorCode::InvalidInput)?;
    if object.len() != 1 || object.get("protocol_version") != Some(&Value::from(1)) {
        return Err(
            if object.get("protocol_version").and_then(Value::as_u64) == Some(2) {
                ErrorCode::VersionMismatch
            } else {
                ErrorCode::InvalidInput
            },
        );
    }
    Ok(())
}

fn render_accounts(snapshot: AccountSnapshot) -> Value {
    let observed = snapshot
        .observed_at_rfc3339()
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000Z".to_owned());
    let items = serde_json::to_value(snapshot.items).unwrap_or_else(|_| Value::Array(Vec::new()));
    let rows = items
        .as_array()
        .map(|values| {
            values
                .iter()
                .map(|item| json!({"account_id":item["account_id"],"status":"complete","items":[item]}))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "schema_version":1,"ok":true,"items":items,"next_cursor":null,
        "account_results":rows,"completeness":"complete","warnings":[],
        "observed_at":observed,"metadata":{"source":"local_store","cached":true}
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_runtime_rejects_non_protocol_bootstrap_params() {
        assert_eq!(require_protocol(&json!({})), Err(ErrorCode::InvalidInput));
        assert_eq!(
            require_protocol(&json!({"protocol_version":2})),
            Err(ErrorCode::VersionMismatch)
        );
    }
}
