//! Strict JSON-RPC codec for the authentication handshake.

use hantong_contracts::{
    ErrorCode,
    auth::{AuthTranscript, RequestedScope, SignerRole},
    wire_hex,
};
use serde::{Deserialize, Serialize};

const PROTOCOL_VERSION: u64 = 1;
const MAX_AUTH_FRAME: usize = 131_072;
const MAX_TRANSCRIPT_HEX: usize = 65_536;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct BeginParams<'a> {
    protocol_version: u64,
    registration_id: String,
    profile_epoch: u64,
    client_nonce: String,
    scope: ScopeWire<'a>,
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct ScopeWire<'a> {
    accounts: &'a [String],
    tools: &'a [String],
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct FinishParams {
    signature_hex: String,
}

pub fn begin_request(
    id: &str,
    registration_id: [u8; 16],
    epoch: u64,
    nonce: [u8; 32],
    scope: &RequestedScope,
) -> Result<serde_json::Value, ErrorCode> {
    validate_id(id)?;
    if epoch == 0 || epoch > i64::MAX as u64 {
        return Err(ErrorCode::InvalidInput);
    }
    let params = BeginParams {
        protocol_version: PROTOCOL_VERSION,
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

pub fn finish_request(id: &str, signature: &[u8; 64]) -> Result<serde_json::Value, ErrorCode> {
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
) -> Result<(AuthTranscript, [u8; 64]), ErrorCode> {
    let envelope: Rpc<Challenge> = parse(body)?;
    validate_response_id(&envelope, expected_id)?;
    if envelope.result.protocol_version != PROTOCOL_VERSION
        || envelope.result.transcript_hex.len() > MAX_TRANSCRIPT_HEX
    {
        return Err(ErrorCode::AuthRequired);
    }
    let transcript_bytes = wire_hex::decode_bounded(&envelope.result.transcript_hex, 32_768)
        .map_err(|_| ErrorCode::AuthRequired)?;
    let transcript = AuthTranscript::decode(&transcript_bytes, SignerRole::Server)
        .map_err(|_| ErrorCode::AuthRequired)?;
    let signature = wire_hex::decode_fixed::<64>(&envelope.result.server_signature_hex)
        .map_err(|_| ErrorCode::AuthRequired)?;
    Ok((transcript, signature))
}

pub fn parse_ready(body: &[u8], expected_id: &str) -> Result<(), ErrorCode> {
    let envelope: Rpc<Ready> = parse(body)?;
    validate_response_id(&envelope, expected_id)?;
    if envelope.result.protocol_version != PROTOCOL_VERSION || !envelope.result.authenticated {
        return Err(ErrorCode::AuthRequired);
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), ErrorCode> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
    {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(())
    }
}
fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ErrorCode> {
    if body.is_empty()
        || body.len() > MAX_AUTH_FRAME
        || body.starts_with(&[0xef, 0xbb, 0xbf])
        || body
            .last()
            .is_some_and(|value| matches!(value, b' ' | b'\t' | b'\r' | b'\n'))
    {
        return Err(ErrorCode::AuthRequired);
    }
    serde_json::from_slice(body).map_err(|_| ErrorCode::AuthRequired)
}
fn validate_response_id<T>(envelope: &Rpc<T>, expected_id: &str) -> Result<(), ErrorCode> {
    if envelope.jsonrpc != "2.0" || envelope.id != expected_id {
        Err(ErrorCode::AuthRequired)
    } else {
        Ok(())
    }
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
    transcript_hex: String,
    server_signature_hex: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    protocol_version: u64,
    authenticated: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scope() -> RequestedScope {
        RequestedScope::new(vec!["a".into(), "b".into()], vec!["mail.read".into()]).unwrap()
    }
    fn transcript() -> AuthTranscript {
        AuthTranscript {
            registration_id: [1; 16],
            profile_epoch: 1,
            issued_at_utc_ms: 1000,
            expires_at_utc_ms: 31000,
            server_instance_id: [2; 32],
            server_nonce: [3; 32],
            client_nonce: [4; 32],
            server_key_hash: [5; 32],
            client_key_hash: [6; 32],
            server: hantong_contracts::auth::PeerContext::new("S-1-5-21-1", "S-1-5-5-0-1").unwrap(),
            client: hantong_contracts::auth::PeerContext::new("S-1-5-21-1", "S-1-5-5-0-1").unwrap(),
            scope: scope(),
        }
    }
    fn challenge(id: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"result":{"protocol_version":1,"transcript_hex":wire_hex::encode(&transcript().encode(SignerRole::Server).unwrap()),"server_signature_hex":wire_hex::encode(&[0x42;64])}})).unwrap()
    }
    #[test]
    fn wire_auth_requests_are_canonical_and_do_not_claim_identity() {
        let value = begin_request("r", [1; 16], 1, [2; 32], &scope()).unwrap();
        assert_eq!(value["method"], "auth.begin");
        assert!(value["params"].get("role").is_none());
        assert!(value["params"].get("verified").is_none());
        assert_eq!(
            value["params"]["registration_id"],
            wire_hex::encode(&[1; 16])
        );
        assert_eq!(
            finish_request("r", &[0x42; 64]).unwrap()["params"]["signature_hex"],
            wire_hex::encode(&[0x42; 64])
        );
    }
    #[test]
    fn wire_auth_parses_challenge_and_ready_success() {
        let (decoded, signature) = parse_challenge(&challenge("r"), "r").unwrap();
        assert_eq!(decoded, transcript());
        assert_eq!(signature, [0x42; 64]);
        let ready =
            br#"{"jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"authenticated":true}}"#;
        assert_eq!(parse_ready(ready, "r"), Ok(()));
    }
    #[test]
    fn wire_auth_rejects_duplicates_unknowns_wrong_ids_and_states() {
        for body in [
            br#"{"jsonrpc":"2.0","jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"authenticated":true}}"#.as_slice(),
            br#"{"jsonrpc":"2.0","id":"r","extra":1,"result":{"protocol_version":1,"authenticated":true}}"#,
            br#"{"jsonrpc":"2.0","id":"r","result":{"protocol_version":1.0,"authenticated":true}}"#,
            br#"{"jsonrpc":"2.0","id":"r","result":{"protocol_version":1,"authenticated":false}}"#,
        ] { assert_eq!(parse_ready(body, "r"), Err(ErrorCode::AuthRequired)); }
        assert_eq!(
            parse_ready(&challenge("wrong"), "r"),
            Err(ErrorCode::AuthRequired)
        );
    }
    #[test]
    fn wire_auth_rejects_bom_trailing_oversized_and_bad_challenge_hex() {
        let mut body = challenge("r");
        body.push(b' ');
        assert_eq!(parse_challenge(&body, "r"), Err(ErrorCode::AuthRequired));
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend_from_slice(&challenge("r"));
        assert_eq!(parse_challenge(&bom, "r"), Err(ErrorCode::AuthRequired));
        let mut huge = vec![b' '; 131073];
        huge[0] = b'{';
        assert_eq!(parse_ready(&huge, "r"), Err(ErrorCode::AuthRequired));
        let mut bad = challenge("r");
        let marker = b"server_signature_hex\":\"";
        let index = bad.windows(marker.len()).position(|v| v == marker).unwrap() + marker.len();
        bad[index] = b'G';
        assert_eq!(parse_challenge(&bad, "r"), Err(ErrorCode::AuthRequired));
    }
    #[test]
    fn wire_auth_outgoing_bounds_are_invalid_input() {
        assert_eq!(
            begin_request("", [0; 16], 1, [0; 32], &scope()),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            finish_request("한글", &[0; 64]),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            begin_request("r", [0; 16], 0, [0; 32], &scope()),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            begin_request("r", [0; 16], i64::MAX as u64 + 1, [0; 32], &scope()),
            Err(ErrorCode::InvalidInput)
        );
    }
}
