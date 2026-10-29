#![cfg(windows)]

//! Connection-owned client authentication over bounded JSON-RPC frames.

use crate::{auth::PendingClientAuthentication, auth_wire};
use hantong_contracts::{ErrorCode, auth::AuthTranscript};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_AUTH_FRAME: usize = 131_072;
const MAX_TOOL_FRAME: usize = 1_048_576;

pub struct AuthenticatedClient<S> {
    transport: S,
}

pub async fn authenticate(
    pending: PendingClientAuthentication,
    id: &str,
) -> Result<AuthenticatedClient<impl AsyncRead + AsyncWrite + Unpin>, ErrorCode> {
    let transport = hantong_platform::connect()
        .await
        .map_err(|_| ErrorCode::AuthRequired)?;
    authenticate_connected(transport, pending, id).await
}

impl<S> AuthenticatedClient<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    /// Sends exactly one protected request and returns the untrusted response
    /// bytes after the transport receipt is sent and flushed. Method-specific
    /// decoding belongs to the caller's strict response codec.
    pub async fn exchange(
        self,
        id: &str,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<Vec<u8>, ErrorCode> {
        validate_exchange_field(id, 64, false)?;
        validate_exchange_field(method, 128, true)?;
        if !params.is_object() {
            return Err(ErrorCode::InvalidInput);
        }
        self.exchange_with_deadline(id, method, params, Duration::from_secs(5))
            .await
    }

    async fn exchange_with_deadline(
        self,
        id: &str,
        method: &str,
        params: &serde_json::Value,
        deadline: Duration,
    ) -> Result<Vec<u8>, ErrorCode> {
        tokio::time::timeout(deadline, exchange_inner(self.transport, id, method, params))
            .await
            .map_err(|_| ErrorCode::CoreUnavailable)?
    }
}

fn validate_exchange_field(value: &str, max: usize, method: bool) -> Result<(), ErrorCode> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
        || (method && !value.starts_with("mail."))
    {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

async fn exchange_inner<S>(
    mut transport: S,
    id: &str,
    method: &str,
    params: &serde_json::Value,
) -> Result<Vec<u8>, ErrorCode>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    write_tool_frame(&mut transport, &request).await?;
    let response = read_tool_frame(&mut transport).await?;
    transport
        .write_all(&[0x06])
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    transport
        .flush()
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    Ok(response)
}

async fn authenticate_connected<S>(
    transport: S,
    pending: PendingClientAuthentication,
    id: &str,
) -> Result<AuthenticatedClient<S>, ErrorCode>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let begin = auth_wire::begin_request(
        id,
        pending.registration_id(),
        pending.profile_epoch(),
        pending.client_nonce(),
        pending.requested_scope(),
    )
    .map_err(|_| ErrorCode::AuthRequired)?;
    let (transport, pending, challenge, server_signature) = tokio::time::timeout(
        Duration::from_secs(5),
        begin_challenge(transport, pending, id, begin),
    )
    .await
    .map_err(|_| ErrorCode::AuthRequired)??;
    let transport = tokio::time::timeout(
        Duration::from_secs(30),
        finish_ready(transport, pending, challenge, server_signature, id),
    )
    .await
    .map_err(|_| ErrorCode::AuthRequired)??;
    Ok(AuthenticatedClient { transport })
}

async fn begin_challenge<S>(
    mut transport: S,
    pending: PendingClientAuthentication,
    id: &str,
    begin: serde_json::Value,
) -> Result<(S, PendingClientAuthentication, AuthTranscript, [u8; 64]), ErrorCode>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    write_frame(&mut transport, &begin).await?;
    let body = read_frame(&mut transport).await?;
    let (challenge, signature) = auth_wire::parse_challenge(&body, id)?;
    Ok((transport, pending, challenge, signature))
}

async fn finish_ready<S>(
    mut transport: S,
    pending: PendingClientAuthentication,
    challenge: AuthTranscript,
    server_signature: [u8; 64],
    id: &str,
) -> Result<S, ErrorCode>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let now_utc_ms: i64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| ErrorCode::AuthRequired)?
        .as_millis()
        .try_into()
        .map_err(|_| ErrorCode::AuthRequired)?;
    let signature = pending.respond(challenge, &server_signature, now_utc_ms)?;
    let finish = auth_wire::finish_request(id, &signature).map_err(|_| ErrorCode::AuthRequired)?;
    write_frame(&mut transport, &finish).await?;
    let ready = read_frame(&mut transport).await?;
    auth_wire::parse_ready(&ready, id)?;
    Ok(transport)
}

async fn write_frame<S>(transport: &mut S, value: &serde_json::Value) -> Result<(), ErrorCode>
where
    S: AsyncWrite + Unpin,
{
    write_frame_with_limit(
        transport,
        value,
        MAX_AUTH_FRAME,
        ErrorCode::AuthRequired,
        ErrorCode::AuthRequired,
        ErrorCode::AuthRequired,
    )
    .await
}

async fn write_tool_frame<S>(transport: &mut S, value: &serde_json::Value) -> Result<(), ErrorCode>
where
    S: AsyncWrite + Unpin,
{
    write_frame_with_limit(
        transport,
        value,
        MAX_TOOL_FRAME,
        ErrorCode::InvalidInput,
        ErrorCode::InvalidInput,
        ErrorCode::CoreUnavailable,
    )
    .await
}

async fn write_frame_with_limit<S>(
    transport: &mut S,
    value: &serde_json::Value,
    max: usize,
    empty_or_oversized: ErrorCode,
    serialization_error: ErrorCode,
    io_error: ErrorCode,
) -> Result<(), ErrorCode>
where
    S: AsyncWrite + Unpin,
{
    let body = serde_json::to_vec(value).map_err(|_| serialization_error)?;
    if body.is_empty() || body.len() > max {
        return Err(empty_or_oversized);
    }
    transport
        .write_u32_le(u32::try_from(body.len()).map_err(|_| empty_or_oversized)?)
        .await
        .map_err(|_| io_error)?;
    transport.write_all(&body).await.map_err(|_| io_error)?;
    transport.flush().await.map_err(|_| io_error)
}

async fn read_frame<S>(transport: &mut S) -> Result<Vec<u8>, ErrorCode>
where
    S: AsyncRead + Unpin,
{
    read_frame_with_limit(transport, MAX_AUTH_FRAME, ErrorCode::AuthRequired).await
}

async fn read_tool_frame<S>(transport: &mut S) -> Result<Vec<u8>, ErrorCode>
where
    S: AsyncRead + Unpin,
{
    read_frame_with_limit(transport, MAX_TOOL_FRAME, ErrorCode::CoreUnavailable).await
}

async fn read_frame_with_limit<S>(
    transport: &mut S,
    max: usize,
    error: ErrorCode,
) -> Result<Vec<u8>, ErrorCode>
where
    S: AsyncRead + Unpin,
{
    let length = transport.read_u32_le().await.map_err(|_| error)? as usize;
    if !(1..=max).contains(&length) {
        return Err(error);
    }
    let mut body = vec![0; length];
    transport.read_exact(&mut body).await.map_err(|_| error)?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap()
    }
    #[test]
    fn sec03_frame_reader_rejects_zero_oversized_and_truncated_frames() {
        runtime().block_on(async {
            for bytes in [
                0u32.to_le_bytes().to_vec(),
                ((MAX_AUTH_FRAME + 1) as u32).to_le_bytes().to_vec(),
            ] {
                let (mut writer, mut reader) = tokio::io::duplex(64);
                writer.write_all(&bytes).await.unwrap();
                assert_eq!(read_frame(&mut reader).await, Err(ErrorCode::AuthRequired));
            }
            let (mut writer, mut reader) = tokio::io::duplex(64);
            writer.write_all(&4u32.to_le_bytes()).await.unwrap();
            writer.write_all(b"x").await.unwrap();
            drop(writer);
            assert_eq!(read_frame(&mut reader).await, Err(ErrorCode::AuthRequired));
        });
    }

    #[test]
    fn sec03_exchange_matches_request_and_acknowledges_one_response() {
        runtime().block_on(async {
            let (client, mut server) = tokio::io::duplex(4096);
            let task = tokio::spawn(async move {
                let length = server.read_u32_le().await.unwrap() as usize;
                let mut request = vec![0; length];
                server.read_exact(&mut request).await.unwrap();
                let request: serde_json::Value = serde_json::from_slice(&request).unwrap();
                assert_eq!(request["id"], "r");
                assert_eq!(request["method"], "mail.accounts.list");
                let response = br#"{"jsonrpc":"2.0","id":"r","result":{"ok":true}}"#;
                server
                    .write_all(&(response.len() as u32).to_le_bytes())
                    .await
                    .unwrap();
                for chunk in response.chunks(3) {
                    server.write_all(chunk).await.unwrap();
                }
                server.flush().await.unwrap();
                assert_eq!(server.read_u8().await.unwrap(), 0x06);
            });
            let client = AuthenticatedClient { transport: client };
            let response = client
                .exchange("r", "mail.accounts.list", &serde_json::json!({}))
                .await
                .unwrap();
            assert_eq!(
                response,
                br#"{"jsonrpc":"2.0","id":"r","result":{"ok":true}}"#
            );
            task.await.unwrap();
        });
    }

    #[test]
    fn sec03_exchange_rejects_invalid_outgoing_without_writing() {
        runtime().block_on(async {
            let (client, mut peer) = tokio::io::duplex(4096);
            let result = (AuthenticatedClient { transport: client })
                .exchange("bad id", "mail.test", &serde_json::json!({}))
                .await;
            assert_eq!(result, Err(ErrorCode::InvalidInput));
            match tokio::time::timeout(Duration::from_millis(10), peer.read_u8()).await {
                Err(_) | Ok(Err(_)) => {}
                Ok(Ok(byte)) => panic!("unexpected request byte: {byte}"),
            }
        });
    }

    #[test]
    fn sec03_exchange_rejects_zero_and_truncated_responses() {
        runtime().block_on(async {
            for response in [Vec::new(), vec![1u8]] {
                let (client, mut server) = tokio::io::duplex(4096);
                let task = tokio::spawn(async move {
                    let length = server.read_u32_le().await.unwrap() as usize;
                    let mut request = vec![0; length];
                    server.read_exact(&mut request).await.unwrap();
                    if response.is_empty() {
                        server.write_all(&0u32.to_le_bytes()).await.unwrap();
                    } else {
                        server.write_all(&4u32.to_le_bytes()).await.unwrap();
                        server.write_all(&response).await.unwrap();
                    }
                    server.flush().await.unwrap();
                });
                let result = (AuthenticatedClient { transport: client })
                    .exchange("r", "mail.test", &serde_json::json!({}))
                    .await;
                assert_eq!(result, Err(ErrorCode::CoreUnavailable));
                task.await.unwrap();
            }
        });
    }

    #[test]
    fn sec03_exchange_rejects_oversized_response() {
        runtime().block_on(async {
            let (client, mut server) = tokio::io::duplex(4096);
            let task = tokio::spawn(async move {
                let length = server.read_u32_le().await.unwrap() as usize;
                let mut request = vec![0; length];
                server.read_exact(&mut request).await.unwrap();
                server
                    .write_all(&((MAX_TOOL_FRAME as u32) + 1).to_le_bytes())
                    .await
                    .unwrap();
                server.flush().await.unwrap();
            });
            let result = (AuthenticatedClient { transport: client })
                .exchange("r", "mail.test", &serde_json::json!({}))
                .await;
            assert_eq!(result, Err(ErrorCode::CoreUnavailable));
            task.await.unwrap();
        });
    }

    #[test]
    fn sec03_exchange_accepts_131073_and_exact_1mib_responses() {
        runtime().block_on(async {
            for response_len in [131_073usize, MAX_TOOL_FRAME] {
                let (client, mut server) = tokio::io::duplex(MAX_TOOL_FRAME * 2 + 64);
                let task = tokio::spawn(async move {
                    let length = server.read_u32_le().await.unwrap() as usize;
                    let mut request = vec![0; length];
                    server.read_exact(&mut request).await.unwrap();
                    server
                        .write_all(&(response_len as u32).to_le_bytes())
                        .await
                        .unwrap();
                    server.write_all(&vec![b'x'; response_len]).await.unwrap();
                    server.flush().await.unwrap();
                    assert_eq!(server.read_u8().await.unwrap(), 0x06);
                });
                let response = (AuthenticatedClient { transport: client })
                    .exchange("r", "mail.test", &serde_json::json!({}))
                    .await
                    .unwrap();
                assert_eq!(response.len(), response_len);
                task.await.unwrap();
            }
        });
    }

    #[test]
    fn sec03_exchange_accepts_exact_1mib_request_and_rejects_larger_without_writing() {
        runtime().block_on(async {
            let base = serde_json::json!({"padding": ""});
            let base_len = serde_json::to_vec(&serde_json::json!({
                "jsonrpc": "2.0",
                "id": "r",
                "method": "mail.test",
                "params": base,
            }))
            .unwrap()
            .len();
            let padding = "x".repeat(MAX_TOOL_FRAME - base_len);
            let (client, mut server) = tokio::io::duplex(MAX_TOOL_FRAME * 2 + 64);
            let task = tokio::spawn(async move {
                let length = server.read_u32_le().await.unwrap() as usize;
                assert_eq!(length, MAX_TOOL_FRAME);
                let mut request = vec![0; length];
                server.read_exact(&mut request).await.unwrap();
                let response = br#"{"jsonrpc":"2.0","id":"r","result":{}}"#;
                server
                    .write_all(&(response.len() as u32).to_le_bytes())
                    .await
                    .unwrap();
                server.write_all(response).await.unwrap();
                server.flush().await.unwrap();
                assert_eq!(server.read_u8().await.unwrap(), 0x06);
            });
            let response = (AuthenticatedClient { transport: client })
                .exchange("r", "mail.test", &serde_json::json!({"padding": padding}))
                .await
                .unwrap();
            assert!(!response.is_empty());
            task.await.unwrap();

            let (client, mut peer) = tokio::io::duplex(4096);
            let oversized = serde_json::json!({"padding": "x".repeat(MAX_TOOL_FRAME)});
            let result = (AuthenticatedClient { transport: client })
                .exchange("r", "mail.test", &oversized)
                .await;
            assert_eq!(result, Err(ErrorCode::InvalidInput));
            match tokio::time::timeout(Duration::from_millis(10), peer.read_u8()).await {
                Err(_) | Ok(Err(_)) => {}
                Ok(Ok(byte)) => panic!("unexpected request byte: {byte}"),
            }
        });
    }

    #[test]
    fn sec03_exchange_deadline_is_fail_closed() {
        runtime().block_on(async {
            let (client, _peer) = tokio::io::duplex(4096);
            let result = (AuthenticatedClient { transport: client })
                .exchange_with_deadline(
                    "r",
                    "mail.test",
                    &serde_json::json!({}),
                    Duration::from_millis(1),
                )
                .await;
            assert_eq!(result, Err(ErrorCode::CoreUnavailable));
        });
    }
}
