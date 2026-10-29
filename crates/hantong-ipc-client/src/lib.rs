#![forbid(unsafe_code)]

//! Bounded client-side bootstrap protocol. Mail methods remain unavailable until
//! registration and mutual authentication are implemented.

#[cfg(windows)]
mod auth;
#[cfg(windows)]
pub use auth::PendingClientAuthentication;
#[cfg(windows)]
pub mod authenticated_wire;

pub mod auth_wire;

use hantong_contracts::ErrorCode;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_RESPONSE: usize = 16_384;
const PROTOCOL_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Readiness {
    pub transport_ready: bool,
    pub mail_ready: bool,
    pub authenticated: bool,
    pub protocol_version: u64,
}

impl<'de> Deserialize<'de> for Readiness {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Readiness;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("minimal readiness object")
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut keys = BTreeSet::new();
                let mut tr = None;
                let mut mr = None;
                let mut au = None;
                let mut pv = None;
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key.clone()) {
                        return Err(A::Error::custom("duplicate readiness field"));
                    }
                    match key.as_str() {
                        "transport_ready" => tr = Some(map.next_value()?),
                        "mail_ready" => mr = Some(map.next_value()?),
                        "authenticated" => au = Some(map.next_value()?),
                        "protocol_version" => pv = Some(map.next_value()?),
                        _ => return Err(A::Error::custom("unknown readiness field")),
                    }
                }
                Ok(Readiness {
                    transport_ready: tr
                        .ok_or_else(|| A::Error::custom("missing transport_ready"))?,
                    mail_ready: mr.ok_or_else(|| A::Error::custom("missing mail_ready"))?,
                    authenticated: au.ok_or_else(|| A::Error::custom("missing authenticated"))?,
                    protocol_version: pv
                        .ok_or_else(|| A::Error::custom("missing protocol_version"))?,
                })
            }
        }
        deserializer.deserialize_map(V)
    }
}

#[derive(Debug)]
struct RpcEnvelope {
    jsonrpc: String,
    id: String,
    body: RpcBody,
}

#[derive(Debug)]
enum RpcBody {
    Result { result: Readiness },
    Error { error: RpcError },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RpcError {
    code: String,
    message: String,
}

impl<'de> Deserialize<'de> for RpcEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = RpcEnvelope;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a strict JSON-RPC response")
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut keys = BTreeSet::new();
                let mut jsonrpc = None;
                let mut id = None;
                let mut result: Option<Readiness> = None;
                let mut error: Option<RpcError> = None;
                while let Some(key) = map.next_key::<String>()? {
                    if !keys.insert(key.clone()) {
                        return Err(A::Error::custom("duplicate response field"));
                    }
                    match key.as_str() {
                        "jsonrpc" => jsonrpc = Some(map.next_value()?),
                        "id" => id = Some(map.next_value()?),
                        "result" => result = Some(map.next_value()?),
                        "error" => error = Some(map.next_value()?),
                        _ => return Err(A::Error::custom("unknown response field")),
                    }
                }
                let body = match (result, error) {
                    (Some(value), None) => RpcBody::Result { result: value },
                    (None, Some(value)) => RpcBody::Error { error: value },
                    _ => return Err(A::Error::custom("response must contain exactly one body")),
                };
                Ok(RpcEnvelope {
                    jsonrpc: jsonrpc.ok_or_else(|| A::Error::custom("missing jsonrpc"))?,
                    id: id.ok_or_else(|| A::Error::custom("missing id"))?,
                    body,
                })
            }
        }
        deserializer.deserialize_map(V)
    }
}

pub fn readiness_request(id: &str) -> Result<Vec<u8>, ErrorCode> {
    if id.is_empty() || id.chars().count() > 64 {
        return Err(ErrorCode::InvalidInput);
    }
    let value = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "system.readiness",
        "params": {"protocol_version": PROTOCOL_VERSION}
    });
    let body = serde_json::to_vec(&value).map_err(|_| ErrorCode::InternalError)?;
    if body.len() > MAX_RESPONSE {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(body)
    }
}

pub fn parse_readiness_response(bytes: &[u8], expected_id: &str) -> Result<Readiness, ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_RESPONSE || bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let envelope: RpcEnvelope =
        serde_json::from_slice(bytes).map_err(|_| ErrorCode::CoreUnavailable)?;
    if envelope.jsonrpc != "2.0" || envelope.id != expected_id {
        return Err(ErrorCode::CoreUnavailable);
    }
    match envelope.body {
        RpcBody::Result { result } => {
            if result.protocol_version != PROTOCOL_VERSION
                || !result.transport_ready
                || result.mail_ready
                || result.authenticated
            {
                return Err(ErrorCode::CoreUnavailable);
            }
            Ok(result)
        }
        RpcBody::Error { error } => {
            let _ = (error.code, error.message);
            Err(ErrorCode::CoreUnavailable)
        }
    }
}

pub fn readiness_json(status: &Readiness) -> String {
    serde_json::json!({
        "schema_version": 1,
        "ok": true,
        "items": [status],
        "next_cursor": null,
        "account_results": [],
        "completeness": "complete",
        "warnings": [],
        "observed_at": null
    })
    .to_string()
}

#[cfg(not(windows))]
pub fn core_status() -> Result<Readiness, ErrorCode> {
    Err(ErrorCode::CapabilityUnsupported)
}

#[cfg(windows)]
pub fn core_status() -> Result<Readiness, ErrorCode> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| ErrorCode::CoreUnavailable)?
        .block_on(core_status_async())
}

#[cfg(windows)]
async fn core_status_async() -> Result<Readiness, ErrorCode> {
    use hantong_platform::connect;
    use tokio::time::{Duration, timeout};

    timeout(Duration::from_secs(5), async {
        exchange_readiness(connect().await?, "core-status-1").await
    })
    .await
    .map_err(|_| ErrorCode::CoreUnavailable)?
}

#[cfg(windows)]
async fn exchange_readiness<T>(mut pipe: T, id: &str) -> Result<Readiness, ErrorCode>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let request = readiness_request(id)?;
    let frame_len = u32::try_from(request.len()).map_err(|_| ErrorCode::InvalidInput)?;
    pipe.write_all(&frame_len.to_le_bytes())
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.write_all(&request)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;

    let mut header = [0u8; 4];
    pipe.read_exact(&mut header)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let length = u32::from_le_bytes(header) as usize;
    if !(1..=MAX_RESPONSE).contains(&length) {
        return Err(ErrorCode::CoreUnavailable);
    }
    let mut response = vec![0u8; length];
    pipe.read_exact(&mut response)
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    let status = parse_readiness_response(&response, id)?;
    // Named-pipe disconnect discards unread bytes. This receipt is only a
    // transport acknowledgement, never an approval or authenticated claim.
    pipe.write_all(&[0x06])
        .await
        .map_err(|_| ErrorCode::CoreUnavailable)?;
    pipe.flush().await.map_err(|_| ErrorCode::CoreUnavailable)?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli06_request_is_fixed_and_bounded() {
        let request = String::from_utf8(readiness_request("r1").unwrap()).unwrap();
        assert!(request.contains("system.readiness"));
        assert!(request.contains("protocol_version"));
    }

    #[test]
    fn cli06_accepts_only_minimal_readiness() {
        let input = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1}}"#;
        assert!(!parse_readiness_response(input, "r1").unwrap().mail_ready);
    }

    #[test]
    fn cli06_rejects_extra_fields_duplicate_keys_and_wrong_state() {
        let extra = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1,"accounts":0}}"#;
        assert_eq!(
            parse_readiness_response(extra, "r1"),
            Err(ErrorCode::CoreUnavailable)
        );
        let duplicate = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1,"mail_ready":false}}"#;
        assert_eq!(
            parse_readiness_response(duplicate, "r1"),
            Err(ErrorCode::CoreUnavailable)
        );
        let wrong = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":true,"authenticated":false,"protocol_version":1}}"#;
        assert_eq!(
            parse_readiness_response(wrong, "r1"),
            Err(ErrorCode::CoreUnavailable)
        );
    }

    #[test]
    fn cli06_json_status_uses_the_common_success_envelope() {
        let status = Readiness {
            transport_ready: true,
            mail_ready: false,
            authenticated: false,
            protocol_version: 1,
        };
        let json = readiness_json(&status);
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("\"items\":[{"));
        assert!(json.contains("\"completeness\":\"complete\""));
        assert!(!json.contains("account_id"));
    }

    #[cfg(windows)]
    #[test]
    fn cli06_duplex_sends_transport_ack_after_valid_response() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (client, mut server) = tokio::io::duplex(64 * 1024);
            let task = tokio::spawn(async move {
                let mut header = [0u8; 4];
                server.read_exact(&mut header).await.unwrap();
                let length = u32::from_le_bytes(header) as usize;
                let mut request = vec![0u8; length];
                server.read_exact(&mut request).await.unwrap();
                let response = br#"{"jsonrpc":"2.0","id":"r1","result":{"transport_ready":true,"mail_ready":false,"authenticated":false,"protocol_version":1}}"#;
                server.write_all(&(response.len() as u32).to_le_bytes()).await.unwrap();
                server.write_all(response).await.unwrap();
                server.flush().await.unwrap();
                let mut ack = [0u8; 1];
                server.read_exact(&mut ack).await.unwrap();
                ack[0]
            });
            let status = exchange_readiness(client, "r1").await.unwrap();
            assert!(!status.mail_ready);
            assert_eq!(task.await.unwrap(), 0x06);
        });
    }
}
