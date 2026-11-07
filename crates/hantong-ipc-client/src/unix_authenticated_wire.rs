//! Connection-owned Unix auth-v2 handshake and protected exchange.
#![cfg(unix)]

use crate::unix_auth::{PendingUnixAccountAuthentication, PendingUnixAuthentication};
use crate::unix_auth_wire;
use hantong_contracts::account_auth::AccountAuthRevision;
use hantong_contracts::{ErrorCode, platform::OpaqueSigner};
use hantong_platform::unix_transport::UnixLocalConnection;
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_AUTH_FRAME: usize = 131_072;
const MAX_TOOL_FRAME: usize = 1_048_576;

pub async fn authenticate_account_connected<S: OpaqueSigner>(
    mut pending: PendingUnixAccountAuthentication<S>,
    id: &str,
) -> Result<AuthenticatedUnixClient, ErrorCode> {
    let overall = tokio::time::Instant::now() + Duration::from_secs(30);
    let begin = crate::account_auth_wire::begin_request(
        id,
        AccountAuthRevision::UnixV3,
        pending.registration_id(),
        pending.profile_epoch(),
        pending.client_nonce(),
        pending.requested_scope(),
    )?;
    tokio::time::timeout_at(
        overall,
        write_json(pending.transport()?, &begin, MAX_AUTH_FRAME),
    )
    .await
    .map_err(|_| ErrorCode::AuthRequired)??;
    let body = tokio::time::timeout_at(overall, read_frame(pending.transport()?, MAX_AUTH_FRAME))
        .await
        .map_err(|_| ErrorCode::AuthRequired)??;
    let (challenge, server_signature) =
        crate::account_auth_wire::parse_challenge(&body, id, AccountAuthRevision::UnixV3)?;
    let (mut transport, signature) =
        pending.respond_connected(challenge, &server_signature, utc_now()?)?;
    let finish = crate::account_auth_wire::finish_request(id, &signature)?;
    tokio::time::timeout_at(overall, write_json(&mut transport, &finish, MAX_AUTH_FRAME))
        .await
        .map_err(|_| ErrorCode::AuthRequired)??;
    let body = tokio::time::timeout_at(overall, read_frame(&mut transport, MAX_AUTH_FRAME))
        .await
        .map_err(|_| ErrorCode::AuthRequired)??;
    crate::account_auth_wire::parse_ready(&body, id, AccountAuthRevision::UnixV3)?;
    Ok(AuthenticatedUnixClient { transport })
}

pub struct AuthenticatedUnixClient {
    transport: UnixLocalConnection,
}

impl AuthenticatedUnixClient {
    /// Sends one protected request, reads its response and acknowledges receipt.
    pub async fn exchange(
        self,
        id: &str,
        method: &str,
        params: &Value,
    ) -> Result<Vec<u8>, ErrorCode> {
        let (request, request_limit, response_limit) =
            validate_exchange_request(id, method, params)?;
        let mut transport = self.transport;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        exchange_over(
            &mut transport,
            &request,
            request_limit,
            response_limit,
            deadline,
        )
        .await
    }
}

fn validate_exchange_request(
    id: &str,
    method: &str,
    params: &Value,
) -> Result<(Value, usize, usize), ErrorCode> {
    let account_connect = matches!(
        method,
        hantong_contracts::account_connect::PREPARE_METHOD
            | hantong_contracts::account_connect::STATUS_METHOD
            | hantong_contracts::account_connect::COMPLETE_METHOD
            | hantong_contracts::account_connect::CANCEL_METHOD
            | hantong_contracts::account_connect::BIND_SMTP_METHOD
    );
    let special = matches!(
        method,
        "app.settings.display.get" | "app.diagnostics.snapshot"
    );
    if id.is_empty()
        || id.len() > 64
        || method.is_empty()
        || method.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
        || (!method.starts_with("mail.") && !special && !account_connect)
        || !params.is_object()
    {
        return Err(ErrorCode::InvalidInput);
    }
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    if account_connect {
        hantong_contracts::account_connect::parse_request(
            &serde_json::to_vec(&request).map_err(|_| ErrorCode::InvalidInput)?,
        )?;
    }
    let limits = if account_connect || method == "app.settings.display.get" {
        (4_096, 4_096)
    } else if method == "app.diagnostics.snapshot" {
        (4_096, 36_864)
    } else {
        (MAX_TOOL_FRAME, MAX_TOOL_FRAME)
    };
    Ok((request, limits.0, limits.1))
}

async fn exchange_over<T: AsyncRead + AsyncWrite + Unpin>(
    transport: &mut T,
    request: &Value,
    request_limit: usize,
    response_limit: usize,
    deadline: tokio::time::Instant,
) -> Result<Vec<u8>, ErrorCode> {
    tokio::time::timeout_at(deadline, async {
        write_json(transport, request, request_limit).await?;
        let response = read_frame(transport, response_limit).await?;
        write_receipt(transport).await?;
        Ok(response)
    })
    .await
    .map_err(|_| ErrorCode::CoreUnavailable)?
}

pub async fn authenticate_connected<S: OpaqueSigner>(
    mut pending: PendingUnixAuthentication<S>,
    id: &str,
) -> Result<AuthenticatedUnixClient, ErrorCode> {
    let overall = tokio::time::Instant::now() + Duration::from_secs(30);
    let begin_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let begin = unix_auth_wire::begin_request(
        id,
        pending.registration_id(),
        pending.profile_epoch(),
        pending.client_nonce(),
        pending.requested_scope(),
    )?;
    tokio::time::timeout_at(
        begin_deadline,
        write_json(pending.transport()?, &begin, MAX_AUTH_FRAME),
    )
    .await
    .map_err(|_| ErrorCode::AuthRequired)??;
    let body = tokio::time::timeout_at(
        begin_deadline,
        read_frame(pending.transport()?, MAX_AUTH_FRAME),
    )
    .await
    .map_err(|_| ErrorCode::AuthRequired)??;
    let (challenge, server_signature) = unix_auth_wire::parse_challenge(&body, id)?;
    let now = utc_now()?;
    let (mut transport, signature) =
        pending.respond_connected(challenge, &server_signature, now)?;
    let finish = unix_auth_wire::finish_request(id, &signature)?;
    tokio::time::timeout_at(overall, write_json(&mut transport, &finish, MAX_AUTH_FRAME))
        .await
        .map_err(|_| ErrorCode::AuthRequired)??;
    let body = tokio::time::timeout_at(overall, read_frame(&mut transport, MAX_AUTH_FRAME))
        .await
        .map_err(|_| ErrorCode::AuthRequired)??;
    unix_auth_wire::parse_ready(&body, id)?;
    Ok(AuthenticatedUnixClient { transport })
}

async fn write_json<W: AsyncWrite + Unpin>(
    writer: &mut W,
    value: &Value,
    max: usize,
) -> Result<(), ErrorCode> {
    let body = serde_json::to_vec(value).map_err(|_| ErrorCode::AuthRequired)?;
    if body.is_empty() || body.len() > max {
        return Err(ErrorCode::InvalidInput);
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

async fn read_frame<R: AsyncRead + Unpin>(
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

async fn write_receipt<W: AsyncWrite + Unpin>(writer: &mut W) -> Result<(), ErrorCode> {
    writer
        .write_u8(0x06)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    writer.flush().await.map_err(|_| ErrorCode::CoreUnavailable)
}

fn utc_now() -> Result<i64, ErrorCode> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ErrorCode::InternalError)?
            .as_millis(),
    )
    .map_err(|_| ErrorCode::InternalError)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::duplex;

    #[tokio::test]
    async fn unix_v2_frame_roundtrip_is_bounded_and_little_endian() {
        let (mut left, mut right) = duplex(64);
        let writer = tokio::spawn(async move {
            write_json(&mut left, &serde_json::json!({"ok": true}), MAX_AUTH_FRAME)
                .await
                .unwrap();
        });
        let body = read_frame(&mut right, MAX_AUTH_FRAME).await.unwrap();
        writer.await.unwrap();
        assert_eq!(body, br#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn unix_v2_frames_reject_zero_oversize_and_truncated() {
        for bytes in [
            vec![0; 4],
            (MAX_AUTH_FRAME as u32 + 1).to_le_bytes().to_vec(),
        ] {
            let (mut writer, mut reader) = duplex(16);
            writer.write_all(&bytes).await.unwrap();
            drop(writer);
            assert!(read_frame(&mut reader, MAX_AUTH_FRAME).await.is_err());
        }
        let (mut writer, mut reader) = duplex(16);
        writer.write_all(&[5, 0, 0, 0, b'{']).await.unwrap();
        drop(writer);
        assert!(read_frame(&mut reader, MAX_AUTH_FRAME).await.is_err());
    }

    struct StalledReceipt {
        response: std::io::Cursor<Vec<u8>>,
        receipt_attempted: bool,
    }
    impl AsyncRead for StalledReceipt {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.response).poll_read(cx, buf)
        }
    }
    impl AsyncWrite for StalledReceipt {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            if buf == [0x06] {
                self.receipt_attempted = true;
                Poll::Pending
            } else {
                Poll::Ready(Ok(buf.len()))
            }
        }
        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn unix_v2_stalled_read_write_and_receipt_are_deadline_bounded() {
        let request = serde_json::json!({"method": "mail.accounts.list"});
        let deadline = || tokio::time::Instant::now() + Duration::from_millis(20);
        // The request fits, but the peer never returns a response.
        let (mut reader, _peer) = duplex(4096);
        assert_eq!(
            exchange_over(&mut reader, &request, 4096, 4096, deadline()).await,
            Err(ErrorCode::CoreUnavailable)
        );
        // Even the initial frame cannot be written to this stalled peer.
        let (mut writer, _reader) = duplex(1);
        assert_eq!(
            exchange_over(&mut writer, &request, 4096, 4096, deadline()).await,
            Err(ErrorCode::CoreUnavailable)
        );
        // A complete response does not bypass the shared receipt deadline.
        let body = b"{\"ok\":true}";
        let mut response = (body.len() as u32).to_le_bytes().to_vec();
        response.extend_from_slice(body);
        let mut stalled = StalledReceipt {
            response: std::io::Cursor::new(response),
            receipt_attempted: false,
        };
        assert_eq!(
            exchange_over(&mut stalled, &request, 4096, 4096, deadline()).await,
            Err(ErrorCode::CoreUnavailable)
        );
        assert!(stalled.receipt_attempted);
    }

    #[tokio::test]
    async fn unix_v2_account_connect_methods_are_exact_and_bounded() {
        let (mut client, mut peer) = duplex(4096);
        let task = tokio::spawn(async move {
            let length = peer.read_u32_le().await.unwrap() as usize;
            assert!(length <= 4096);
            let mut body = vec![0; length];
            peer.read_exact(&mut body).await.unwrap();
            let parsed = hantong_contracts::account_connect::parse_request(&body).unwrap();
            assert_eq!(
                parsed.1,
                hantong_contracts::account_connect::AccountConnectCommand::Status([7; 32])
            );
            let response = br#"{"jsonrpc":"2.0","id":"r","result":{}}"#;
            peer.write_u32_le(response.len() as u32).await.unwrap();
            peer.write_all(response).await.unwrap();
            assert_eq!(peer.read_u8().await.unwrap(), 0x06);
        });
        let (request, request_limit, response_limit) = validate_exchange_request(
            "r",
            hantong_contracts::account_connect::STATUS_METHOD,
            &serde_json::json!({
                "protocol_version": 1,
                "intent_id": "0707070707070707070707070707070707070707070707070707070707070707",
            }),
        )
        .unwrap();
        let response = exchange_over(
            &mut client,
            &request,
            request_limit,
            response_limit,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert!(!response.is_empty());
        task.await.unwrap();

        assert_eq!(
            validate_exchange_request(
                "r",
                "app.accounts.connect.status.extra",
                &serde_json::json!({}),
            ),
            Err(ErrorCode::InvalidInput)
        );
    }
}
