//! Connection-owned Unix auth-v2 server handshake.
#![cfg(unix)]

use crate::unix_auth::{
    AuthenticatedUnixConnection, PendingUnixAuthentication, RegisteredUnixClient,
};
use crate::unix_auth_wire;
use crate::{RpcRequest, decode_request};
use hantong_contracts::{ErrorCode, platform::OpaqueSigner};
use hantong_platform::unix_transport::UnixLocalConnection;
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_AUTH_FRAME: usize = 131_072;

pub async fn authenticate_connected<S, F>(
    connection: UnixLocalConnection,
    server_signer: &mut S,
    server_instance: [u8; 32],
    lookup: &mut F,
) -> Result<AuthenticatedUnixConnection, ErrorCode>
where
    S: OpaqueSigner,
    F: FnMut(&[u8; 16]) -> Result<Option<RegisteredUnixClient>, ErrorCode>,
{
    let mut connection = connection;
    let first = tokio::time::timeout(Duration::from_secs(5), read_request(&mut connection))
        .await
        .map_err(|_| ErrorCode::AuthRequired)??;
    let overall = tokio::time::Instant::now() + Duration::from_secs(30);
    let begin = unix_auth_wire::parse_begin(&first)?;
    let registration = lookup(&begin.registration_id)?.ok_or(ErrorCode::ClientNotRegistered)?;
    if registration.id() != begin.registration_id {
        return Err(ErrorCode::ClientNotRegistered);
    }
    if registration.epoch() != begin.profile_epoch {
        return Err(ErrorCode::AuthRequired);
    }
    let now = utc_now()?;
    let (mut pending, challenge) = PendingUnixAuthentication::begin_connected(
        server_signer,
        connection,
        server_instance,
        &registration,
        begin.client_nonce,
        begin.scope,
        now,
        fresh_nonce()?,
    )?;
    let result = unix_auth_wire::challenge_result(&challenge.transcript, &challenge.signature)?;
    tokio::time::timeout_at(
        overall,
        write_result(pending.transport()?, &first.id, result),
    )
    .await
    .map_err(|_| ErrorCode::AuthRequired)??;
    let finish = tokio::time::timeout_at(overall, read_request(pending.transport()?))
        .await
        .map_err(|_| ErrorCode::AuthRequired)??;
    if finish.id != first.id {
        return Err(ErrorCode::AuthRequired);
    }
    let signature = unix_auth_wire::parse_finish(&finish)?;
    let current = lookup(&begin.registration_id)?;
    let mut authenticated = pending.finish(&signature, current.as_ref(), utc_now()?)?;
    tokio::time::timeout_at(
        overall,
        write_result(
            authenticated.transport()?,
            &finish.id,
            unix_auth_wire::ready_result(),
        ),
    )
    .await
    .map_err(|_| ErrorCode::AuthRequired)??;
    Ok(authenticated)
}

async fn read_request(connection: &mut UnixLocalConnection) -> Result<RpcRequest, ErrorCode> {
    let body = read_frame(connection, MAX_AUTH_FRAME).await?;
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    decode_request(&frame)
}

async fn read_frame<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> Result<Vec<u8>, ErrorCode> {
    let length = reader
        .read_u32_le()
        .await
        .map_err(|_| ErrorCode::AuthRequired)? as usize;
    if !(1..=max).contains(&length) {
        return Err(ErrorCode::InvalidInput);
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|_| ErrorCode::AuthRequired)?;
    Ok(body)
}

async fn write_result<W: AsyncWrite + Unpin>(
    writer: &mut W,
    id: &str,
    result: Value,
) -> Result<(), ErrorCode> {
    let body = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"result":result}))
        .map_err(|_| ErrorCode::InternalError)?;
    if body.is_empty() || body.len() > MAX_AUTH_FRAME {
        return Err(ErrorCode::InternalError);
    }
    writer
        .write_u32_le(body.len() as u32)
        .await
        .map_err(|_| ErrorCode::AuthRequired)?;
    writer
        .write_all(&body)
        .await
        .map_err(|_| ErrorCode::AuthRequired)?;
    writer.flush().await.map_err(|_| ErrorCode::AuthRequired)
}

fn fresh_nonce() -> Result<[u8; 32], ErrorCode> {
    hantong_platform::random_nonce()
}

fn utc_now() -> Result<i64, ErrorCode> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ErrorCode::InternalError)?
            .as_millis(),
    )
    .map_err(|_| ErrorCode::InternalError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn unix_v2_frames_reject_zero_oversize_and_truncated() {
        for bytes in [
            vec![0; 4],
            (MAX_AUTH_FRAME as u32 + 1).to_le_bytes().to_vec(),
        ] {
            let (mut writer, mut reader) = tokio::io::duplex(16);
            writer.write_all(&bytes).await.unwrap();
            drop(writer);
            assert!(read_frame(&mut reader, MAX_AUTH_FRAME).await.is_err());
        }
        let (mut writer, mut reader) = tokio::io::duplex(16);
        writer.write_all(&[5, 0, 0, 0, b'{']).await.unwrap();
        drop(writer);
        assert!(read_frame(&mut reader, MAX_AUTH_FRAME).await.is_err());
    }
}
