//! Strict Unix auth-v2 JSON-RPC codec. No transport authentication is done here.

use hantong_contracts::auth::{RequestedScope, SignerRole};
use hantong_contracts::unix_auth::UnixAuthTranscriptV2;
use hantong_contracts::{ErrorCode, wire_hex};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const PROTOCOL_VERSION: u64 = 1;
const AUTH_VERSION: u64 = 2;
const MAX_FRAME: usize = 131_072;
const MAX_TRANSCRIPT_HEX: usize = 65_536;

#[derive(Serialize)]
struct BeginParams<'a> {
    protocol_version: u64,
    auth_version: u64,
    registration_id: String,
    profile_epoch: u64,
    client_nonce: String,
    scope: ScopeWire<'a>,
}
#[derive(Serialize)]
struct ScopeWire<'a> {
    accounts: &'a [String],
    tools: &'a [String],
}
#[derive(Serialize)]
struct FinishParams {
    signature_hex: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rpc<T> {
    jsonrpc: String,
    id: String,
    result: T,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Challenge {
    protocol_version: u64,
    auth_version: u64,
    transcript_hex: String,
    server_signature_hex: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    protocol_version: u64,
    auth_version: u64,
    authenticated: bool,
}

pub fn begin_request(
    id: &str,
    registration_id: [u8; 16],
    epoch: u64,
    nonce: [u8; 32],
    scope: &RequestedScope,
) -> Result<Value, ErrorCode> {
    validate_id(id)?;
    if epoch == 0 || epoch > i64::MAX as u64 {
        return Err(ErrorCode::InvalidInput);
    }
    let params = BeginParams {
        protocol_version: PROTOCOL_VERSION,
        auth_version: AUTH_VERSION,
        registration_id: wire_hex::encode(&registration_id),
        profile_epoch: epoch,
        client_nonce: wire_hex::encode(&nonce),
        scope: ScopeWire {
            accounts: scope.accounts(),
            tools: scope.tools(),
        },
    };
    serde_json::to_value(
        serde_json::json!({"jsonrpc":"2.0","id":id,"method":"auth.begin","params":params}),
    )
    .map_err(|_| ErrorCode::InternalError)
}

pub fn finish_request(id: &str, signature: &[u8; 64]) -> Result<Value, ErrorCode> {
    validate_id(id)?;
    let params = FinishParams {
        signature_hex: wire_hex::encode(signature),
    };
    serde_json::to_value(
        serde_json::json!({"jsonrpc":"2.0","id":id,"method":"auth.finish","params":params}),
    )
    .map_err(|_| ErrorCode::InternalError)
}

pub fn parse_challenge(
    body: &[u8],
    expected_id: &str,
) -> Result<(UnixAuthTranscriptV2, [u8; 64]), ErrorCode> {
    let envelope: Rpc<Challenge> = parse_frame(body)?;
    if envelope.jsonrpc != "2.0"
        || envelope.id != expected_id
        || envelope.result.protocol_version != PROTOCOL_VERSION
        || envelope.result.auth_version != AUTH_VERSION
        || envelope.result.transcript_hex.len() > MAX_TRANSCRIPT_HEX
    {
        return Err(ErrorCode::AuthRequired);
    }
    let bytes = wire_hex::decode_bounded(&envelope.result.transcript_hex, 32_768)
        .map_err(|_| ErrorCode::AuthRequired)?;
    let transcript = UnixAuthTranscriptV2::decode(&bytes, SignerRole::Server)
        .map_err(|_| ErrorCode::AuthRequired)?;
    let signature = wire_hex::decode_fixed::<64>(&envelope.result.server_signature_hex)
        .map_err(|_| ErrorCode::AuthRequired)?;
    Ok((transcript, signature))
}

pub fn parse_ready(body: &[u8], expected_id: &str) -> Result<(), ErrorCode> {
    let envelope: Rpc<Ready> = parse_frame(body)?;
    if envelope.jsonrpc != "2.0"
        || envelope.id != expected_id
        || envelope.result.protocol_version != PROTOCOL_VERSION
        || envelope.result.auth_version != AUTH_VERSION
        || !envelope.result.authenticated
    {
        return Err(ErrorCode::AuthRequired);
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), ErrorCode> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn parse_frame<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ErrorCode> {
    if body.is_empty()
        || body.len() > MAX_FRAME
        || body.starts_with(&[0xef, 0xbb, 0xbf])
        || body
            .last()
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
    {
        return Err(ErrorCode::AuthRequired);
    }
    serde_json::from_slice(body).map_err(|_| ErrorCode::AuthRequired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn transcript() -> UnixAuthTranscriptV2 {
        UnixAuthTranscriptV2 {
            registration_id: [1; 16],
            profile_epoch: 1,
            issued_at_utc_ms: 1,
            expires_at_utc_ms: 2,
            server_instance_id: [2; 32],
            server_nonce: [3; 32],
            client_nonce: [4; 32],
            server_public_key_hash: [5; 32],
            client_public_key_hash: [6; 32],
            server: hantong_contracts::unix_auth::UnixPeerContextV2::new(
                hantong_contracts::unix_auth::UnixPlatform::Linux,
                1,
                1,
                1,
                1,
                [7; 32],
            )
            .unwrap(),
            client: hantong_contracts::unix_auth::UnixPeerContextV2::new(
                hantong_contracts::unix_auth::UnixPlatform::Linux,
                1,
                1,
                2,
                1,
                [8; 32],
            )
            .unwrap(),
            scope: RequestedScope::new(vec!["a".into()], vec!["mail.messages.get".into()]).unwrap(),
        }
    }
    #[test]
    fn unix_v2_outgoing_has_versions_and_finish_is_signature_only() {
        let s = RequestedScope::new(vec!["a".into()], vec!["mail.messages.get".into()]).unwrap();
        let v = begin_request("r", [1; 16], 1, [2; 32], &s).unwrap();
        assert_eq!(v["params"]["auth_version"], 2);
        assert!(
            finish_request("r", &[3; 64]).unwrap()["params"]
                .get("auth_version")
                .is_none()
        );
    }
    #[test]
    fn unix_v2_challenge_and_ready_round_trip_strictly() {
        let value = json!({"jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"auth_version":2,"transcript_hex":wire_hex::encode(&transcript().encode(SignerRole::Server).unwrap()),"server_signature_hex":wire_hex::encode(&[9;64])}});
        let (decoded, sig) = parse_challenge(&serde_json::to_vec(&value).unwrap(), "r").unwrap();
        assert_eq!(decoded, transcript());
        assert_eq!(sig, [9; 64]);
        let ready = json!({"jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"auth_version":2,"authenticated":true}});
        assert!(parse_ready(&serde_json::to_vec(&ready).unwrap(), "r").is_ok());
    }
    #[test]
    fn unix_v2_ready_rejects_v1_float_duplicate_unknown_and_trailing() {
        for raw in [
            r#"{"jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"auth_version":1,"authenticated":true}}"#,
            r#"{"jsonrpc":"2.0","id":"r","result":{"protocol_version":1.0,"auth_version":2,"authenticated":true}}"#,
            r#"{"jsonrpc":"2.0","jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"auth_version":2,"authenticated":true}}"#,
            r#"{"jsonrpc":"2.0","id":"r","extra":1,"result":{"protocol_version":1,"auth_version":2,"authenticated":true}}"#,
        ] {
            assert!(parse_ready(raw.as_bytes(), "r").is_err());
        }
        let mut b=br#"{"jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"auth_version":2,"authenticated":true}}"#.to_vec();
        b.push(b'\n');
        assert!(parse_ready(&b, "r").is_err());
    }
}
