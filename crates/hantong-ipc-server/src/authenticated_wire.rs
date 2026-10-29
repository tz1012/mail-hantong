//! Connection-owned JSON authentication exchange; no registration issuance.

use crate::{
    RpcRequest,
    auth::{AuthenticatedConnection, PendingAuthentication, RegisteredClient},
    auth_wire, decode_request,
};
use hantong_contracts::ErrorCode;
use hantong_platform::{LocalPipeConnection, ServerSigningKey};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_AUTH_FRAME: usize = 131_072;

/// Authenticates one actual OS-validated connection using fresh registration
/// snapshots from the core-owned store. Errors consume the connection and are
/// returned only to the core, without reflecting registration details on wire.
pub async fn authenticate<'server, F>(
    connection: LocalPipeConnection<'server>,
    server_key: &ServerSigningKey,
    instance: [u8; 32],
    lookup: &mut F,
) -> Result<AuthenticatedConnection<'server>, ErrorCode>
where
    F: FnMut(&[u8; 16]) -> Result<Option<RegisteredClient>, ErrorCode>,
{
    authenticate_with_deadline(
        connection,
        server_key,
        instance,
        lookup,
        Duration::from_secs(30),
    )
    .await
}

async fn authenticate_with_deadline<'server, F>(
    mut connection: LocalPipeConnection<'server>,
    server_key: &ServerSigningKey,
    instance: [u8; 32],
    lookup: &mut F,
    finish_deadline: Duration,
) -> Result<AuthenticatedConnection<'server>, ErrorCode>
where
    F: FnMut(&[u8; 16]) -> Result<Option<RegisteredClient>, ErrorCode>,
{
    let begin_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let first = tokio::time::timeout_at(begin_deadline, read_request(&mut connection))
        .await
        .map_err(|_| ErrorCode::AuthRequired)??;
    authenticate_after_begin(
        connection,
        first,
        server_key,
        instance,
        lookup,
        begin_deadline,
        finish_deadline,
    )
    .await
}

async fn authenticate_after_begin<'server, F>(
    connection: LocalPipeConnection<'server>,
    first: RpcRequest,
    server_key: &ServerSigningKey,
    instance: [u8; 32],
    lookup: &mut F,
    begin_deadline: tokio::time::Instant,
    finish_deadline: Duration,
) -> Result<AuthenticatedConnection<'server>, ErrorCode>
where
    F: FnMut(&[u8; 16]) -> Result<Option<RegisteredClient>, ErrorCode>,
{
    let begin = auth_wire::parse_begin(&first)?;
    let registration = lookup(&begin.registration_id)?.ok_or(ErrorCode::ClientNotRegistered)?;
    if registration.registration_id() != &begin.registration_id {
        return Err(ErrorCode::ClientNotRegistered);
    }
    if registration.profile_epoch() != begin.profile_epoch {
        return Err(ErrorCode::AuthRequired);
    }
    let (mut pending, challenge) = PendingAuthentication::begin(
        connection,
        server_key,
        instance,
        &registration,
        begin.client_nonce,
        begin.scope,
        utc_now()?,
    )?;
    if tokio::time::Instant::now() >= begin_deadline {
        return Err(ErrorCode::AuthRequired);
    }
    tokio::time::timeout_at(begin_deadline, async {
        write_result(
            pending.transport(),
            &first.id,
            auth_wire::challenge_result(&challenge.transcript, &challenge.signature)?,
        )
        .await
    })
    .await
    .map_err(|_| ErrorCode::AuthRequired)??;
    tokio::time::timeout(finish_deadline, async move {
        let finish = read_request(pending.transport()).await?;
        let signature = auth_wire::parse_finish(&finish)?;
        let current = lookup(&begin.registration_id)?;
        let mut authenticated = pending.finish(&signature, current.as_ref(), utc_now()?)?;
        write_result(
            authenticated.transport()?,
            &finish.id,
            auth_wire::ready_result(),
        )
        .await?;
        Ok(authenticated)
    })
    .await
    .map_err(|_| ErrorCode::AuthRequired)?
}

/// Routes only bootstrap requests before authentication. The authenticated
/// executor is reachable exclusively through a successful connection proof.
pub async fn dispatch_connection<L, E>(
    mut connection: LocalPipeConnection<'_>,
    server_key: &ServerSigningKey,
    instance: [u8; 32],
    lookup: &mut L,
    execute: &mut E,
) -> Result<(), ErrorCode>
where
    L: FnMut(&[u8; 16]) -> Result<Option<RegisteredClient>, ErrorCode>,
    E: FnMut(&mut AuthenticatedConnection<'_>, &RpcRequest) -> Result<Value, ErrorCode>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let request = tokio::time::timeout_at(deadline, read_request(&mut connection))
        .await
        .map_err(|_| ErrorCode::AuthRequired)??;
    if request.method == "auth.begin" {
        let authenticated = authenticate_after_begin(
            connection,
            request,
            server_key,
            instance,
            lookup,
            deadline,
            Duration::from_secs(30),
        )
        .await?;
        execute_once(authenticated, execute).await
    } else if request.method.starts_with("auth.") {
        Err(ErrorCode::InvalidInput)
    } else {
        let response = crate::bootstrap::response(request);
        tokio::time::timeout_at(deadline, send_response(&mut connection, response, deadline))
            .await
            .map_err(|_| ErrorCode::CoreUnavailable)?
    }
}

/// Core-owned serial server. Shutdown cancels outstanding I/O and drops the
/// current connection; no authenticated session survives reconnect or stop.
pub async fn serve_until<F, R, L, E>(
    shutdown: F,
    on_ready: R,
    server_key: &ServerSigningKey,
    instance: [u8; 32],
    mut lookup: L,
    mut execute: E,
) -> Result<(), ErrorCode>
where
    F: std::future::Future<Output = Result<(), ErrorCode>>,
    R: FnOnce(),
    L: FnMut(&[u8; 16]) -> Result<Option<RegisteredClient>, ErrorCode>,
    E: FnMut(&mut AuthenticatedConnection<'_>, &RpcRequest) -> Result<Value, ErrorCode>,
{
    let mut listener = hantong_platform::LocalPipeServer::bind()?;
    tokio::pin!(shutdown);
    on_ready();
    loop {
        let connection = tokio::select! {
            biased;
            result=&mut shutdown => return result,
            accepted=listener.accept() => match accepted {
                Ok(connection) => connection,
                Err(ErrorCode::ScopeDenied | ErrorCode::PolicyBlocked | ErrorCode::ClientNotRegistered) => continue,
                Err(code) => return Err(code),
            }
        };
        tokio::select! {
            biased;
            result=&mut shutdown => return result,
            result=dispatch_connection(connection,server_key,instance,&mut lookup,&mut execute) => {
                if result == Err(ErrorCode::InternalError) { return result; }
            },
        }
    }
}

async fn read_request(connection: &mut LocalPipeConnection<'_>) -> Result<RpcRequest, ErrorCode> {
    read_bounded_request(connection, MAX_AUTH_FRAME).await
}

async fn read_bounded_request(
    connection: &mut LocalPipeConnection<'_>,
    max_length: usize,
) -> Result<RpcRequest, ErrorCode> {
    let length = connection
        .read_u32_le()
        .await
        .map_err(|_| ErrorCode::AuthRequired)? as usize;
    if !(1..=max_length).contains(&length) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut frame = vec![0; length + 4];
    frame[..4].copy_from_slice(&(length as u32).to_le_bytes());
    connection
        .read_exact(&mut frame[4..])
        .await
        .map_err(|_| ErrorCode::AuthRequired)?;
    decode_request(&frame)
}

/// Consumes this authenticated connection after exactly one protected request.
/// The core callback must construct the common ExecutionContext using this
/// same session; this is not a provider/plugin dispatch or authorization API.
pub async fn execute_once<F>(
    mut authenticated: AuthenticatedConnection<'_>,
    execute: F,
) -> Result<(), ErrorCode>
where
    F: FnOnce(&mut AuthenticatedConnection<'_>, &RpcRequest) -> Result<Value, ErrorCode>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    tokio::time::timeout_at(deadline, async {
        let request = read_bounded_request(authenticated.transport()?, crate::MAX_FRAME_SIZE).await?;
        if !request.method.starts_with("mail.") { return Err(ErrorCode::InvalidInput); }
        if tokio::time::Instant::now() >= deadline { return Err(ErrorCode::CoreUnavailable); }
        let result = execute(&mut authenticated, &request);
        // In this foundation the internal path is local Store/security state.
        // Do not hide its failure as an ordinary disconnected client.
        if result == Err(ErrorCode::InternalError) { return Err(ErrorCode::InternalError); }
        // Synchronous transactions are not forcibly cancelled or replayed.
        if tokio::time::Instant::now() >= deadline { return Err(ErrorCode::CoreUnavailable); }
        let response = match result {
            Ok(value) => json!({"jsonrpc":"2.0","id":request.id,"result":value}),
            Err(code) => json!({"jsonrpc":"2.0","id":request.id,"error":{"code":code.as_str(),"message":code.as_str()}}),
        };
        send_response(authenticated.transport()?, response, deadline).await
    }).await.map_err(|_| ErrorCode::CoreUnavailable)?
}

async fn send_response(
    pipe: &mut LocalPipeConnection<'_>,
    response: Value,
    deadline: tokio::time::Instant,
) -> Result<(), ErrorCode> {
    let bytes = serde_json::to_vec(&response).map_err(|_| ErrorCode::InternalError)?;
    if bytes.len() > crate::MAX_FRAME_SIZE {
        return Err(ErrorCode::InternalError);
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(ErrorCode::CoreUnavailable);
    }
    pipe.write_u32_le(bytes.len() as u32)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.write_all(&bytes)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
    if pipe
        .read_u8()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?
        != 0x06
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

async fn write_result(
    connection: &mut LocalPipeConnection<'_>,
    id: &str,
    result: Value,
) -> Result<(), ErrorCode> {
    let body = serde_json::to_vec(&json!({"jsonrpc":"2.0", "id":id, "result":result}))
        .map_err(|_| ErrorCode::InternalError)?;
    if body.is_empty() || body.len() > MAX_AUTH_FRAME {
        return Err(ErrorCode::InternalError);
    }
    connection
        .write_u32_le(body.len() as u32)
        .await
        .map_err(|_| ErrorCode::AuthRequired)?;
    connection
        .write_all(&body)
        .await
        .map_err(|_| ErrorCode::AuthRequired)?;
    connection
        .flush()
        .await
        .map_err(|_| ErrorCode::AuthRequired)
}

fn utc_now() -> Result<i64, ErrorCode> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ErrorCode::InternalError)?;
    i64::try_from(duration.as_millis()).map_err(|_| ErrorCode::InternalError)
}

#[cfg(test)]
pub(crate) mod tests;
