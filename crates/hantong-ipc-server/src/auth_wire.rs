//! Strict JSON codec for the v1 authentication handshake.
//!
//! This module only validates and encodes wire data.  It does not consult the
//! store, create a client profile, or establish an authenticated connection.

use hantong_contracts::{
    ErrorCode,
    auth::{AuthTranscript, RequestedScope, SignerRole},
    wire_hex::{decode_fixed, encode},
};
use serde_json::{Map, Value};
use std::fmt;

const PROTOCOL_VERSION: u64 = 1;
const MAX_SCOPE_ITEMS: usize = 128;

/// The bounded, untrusted values supplied by an `auth.begin` request.
pub struct BeginParameters {
    pub registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub client_nonce: [u8; 32],
    pub scope: RequestedScope,
}

impl fmt::Debug for BeginParameters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BeginParameters { <redacted> }")
    }
}

/// Parse the exact `auth.begin` request shape.
pub fn parse_begin(request: &crate::RpcRequest) -> Result<BeginParameters, ErrorCode> {
    if request.method != "auth.begin" {
        return Err(ErrorCode::InvalidInput);
    }
    let object = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    require_keys(
        object,
        &[
            "protocol_version",
            "registration_id",
            "profile_epoch",
            "client_nonce",
            "scope",
        ],
    )?;

    let protocol = object
        .get("protocol_version")
        .ok_or(ErrorCode::InvalidInput)?;
    if protocol.as_u64() == Some(PROTOCOL_VERSION) && protocol.as_i64() == Some(1) {
        // The JSON number must be an integer, not a floating-point 1.0.
    } else if protocol.as_i64().is_some() || protocol.as_u64().is_some() {
        return Err(ErrorCode::VersionMismatch);
    } else {
        return Err(ErrorCode::InvalidInput);
    }

    let registration_id = decode_fixed::<16>(string_field(object, "registration_id")?)?;
    let profile_epoch = positive_integer(object.get("profile_epoch"))?;
    let client_nonce = decode_fixed::<32>(string_field(object, "client_nonce")?)?;
    let scope = parse_scope(object.get("scope").ok_or(ErrorCode::InvalidInput)?)?;

    Ok(BeginParameters {
        registration_id,
        profile_epoch,
        client_nonce,
        scope,
    })
}

/// Parse the exact `auth.finish` request shape.
pub fn parse_finish(request: &crate::RpcRequest) -> Result<[u8; 64], ErrorCode> {
    if request.method != "auth.finish" {
        return Err(ErrorCode::InvalidInput);
    }
    let object = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    require_keys(object, &["signature_hex"])?;
    decode_fixed::<64>(string_field(object, "signature_hex")?)
}

/// Encode the server challenge response.  The transcript is canonicalized by
/// the domain type with the server signer role before it is put on the wire.
pub fn challenge_result(
    transcript: &AuthTranscript,
    signature: &[u8; 64],
) -> Result<Value, ErrorCode> {
    let transcript_bytes = transcript.encode(SignerRole::Server)?;
    Ok(serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "transcript_hex": encode(&transcript_bytes),
        "server_signature_hex": encode(signature),
    }))
}

/// Encode the minimal authenticated result.  This is not an authentication
/// proof and does not bootstrap or activate a connection by itself.
pub fn ready_result() -> Value {
    serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "authenticated": true,
    })
}

fn require_keys(object: &Map<String, Value>, expected: &[&str]) -> Result<(), ErrorCode> {
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, ErrorCode> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)
}

fn positive_integer(value: Option<&Value>) -> Result<u64, ErrorCode> {
    let value = value.ok_or(ErrorCode::InvalidInput)?;
    let epoch = value.as_u64().ok_or(ErrorCode::InvalidInput)?;
    if epoch == 0 || epoch > i64::MAX as u64 {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(epoch)
}

fn parse_scope(value: &Value) -> Result<RequestedScope, ErrorCode> {
    let object = value.as_object().ok_or(ErrorCode::InvalidInput)?;
    require_keys(object, &["accounts", "tools"])?;
    let accounts_value = object.get("accounts").ok_or(ErrorCode::InvalidInput)?;
    let tools_value = object.get("tools").ok_or(ErrorCode::InvalidInput)?;
    let accounts_array = accounts_value.as_array().ok_or(ErrorCode::InvalidInput)?;
    let tools_array = tools_value.as_array().ok_or(ErrorCode::InvalidInput)?;
    if accounts_array.len() > MAX_SCOPE_ITEMS || tools_array.len() > MAX_SCOPE_ITEMS {
        return Err(ErrorCode::InvalidInput);
    }

    // Validate all borrowed strings and bounds before allocating/cloning them.
    validate_scope_strings(accounts_array, 128, false)?;
    validate_scope_strings(tools_array, 96, true)?;
    let accounts: Vec<String> = accounts_array
        .iter()
        .map(|value| value.as_str().expect("validated scope string").to_owned())
        .collect();
    let tools: Vec<String> = tools_array
        .iter()
        .map(|value| value.as_str().expect("validated scope string").to_owned())
        .collect();
    let scope = RequestedScope::new(accounts.clone(), tools.clone())?;
    if scope.accounts() != accounts.as_slice() || scope.tools() != tools.as_slice() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(scope)
}

fn validate_scope_strings(values: &[Value], max_len: usize, tool: bool) -> Result<(), ErrorCode> {
    for value in values {
        let text = value.as_str().ok_or(ErrorCode::InvalidInput)?;
        if text.is_empty() || text.len() > max_len || !text.is_ascii() {
            return Err(ErrorCode::InvalidInput);
        }
        if tool && !text.as_bytes()[0].is_ascii_lowercase() {
            return Err(ErrorCode::InvalidInput);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RpcRequest, decode_request};
    use hantong_contracts::auth::PeerContext;

    fn request(method: &str, params: Value) -> RpcRequest {
        RpcRequest {
            id: "r1".into(),
            method: method.into(),
            params,
        }
    }

    fn begin_params() -> Value {
        serde_json::json!({
            "protocol_version": 1,
            "registration_id": "00112233445566778899aabbccddeeff",
            "profile_epoch": 7,
            "client_nonce": "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
            "scope": { "accounts": ["account_a", "account_b"], "tools": ["mail.messages.get"] }
        })
    }

    #[test]
    fn auth_wire_parses_all_fields_and_redacts_debug() {
        let parsed = parse_begin(&request("auth.begin", begin_params())).unwrap();
        assert_eq!(parsed.registration_id[0], 0);
        assert_eq!(parsed.profile_epoch, 7);
        assert_eq!(
            parsed.scope.accounts(),
            &["account_a".to_owned(), "account_b".to_owned()]
        );
        assert!(!format!("{parsed:?}").contains("account_a"));
        assert!(!format!("{parsed:?}").contains("001122"));
    }

    #[test]
    fn auth_wire_rejects_types_versions_epoch_and_extra_claims() {
        let mut cases = Vec::new();
        let mut value = begin_params();
        value["protocol_version"] = serde_json::json!(2);
        cases.push((value, ErrorCode::VersionMismatch));
        let mut value = begin_params();
        value["protocol_version"] = serde_json::json!(1.0);
        cases.push((value, ErrorCode::InvalidInput));
        let mut value = begin_params();
        value["protocol_version"] = serde_json::json!(-1);
        cases.push((value, ErrorCode::VersionMismatch));
        let mut value = begin_params();
        value["profile_epoch"] = serde_json::json!(0);
        cases.push((value, ErrorCode::InvalidInput));
        let mut value = begin_params();
        value["profile_epoch"] = serde_json::json!(i64::MAX as u64 + 1);
        cases.push((value, ErrorCode::InvalidInput));
        let mut value = begin_params();
        value["role"] = serde_json::json!("client");
        cases.push((value, ErrorCode::InvalidInput));
        for (params, error) in cases {
            assert!(
                matches!(parse_begin(&request("auth.begin", params)), Err(actual) if actual == error)
            );
        }
        assert!(matches!(
            parse_begin(&request("other", begin_params())),
            Err(ErrorCode::InvalidInput)
        ));
    }

    #[test]
    fn auth_wire_requires_canonical_bounded_scope() {
        for scope in [
            serde_json::json!({"accounts":["account_b","account_a"],"tools":["mail.messages.get"]}),
            serde_json::json!({"accounts":["account_a","account_a"],"tools":["mail.messages.get"]}),
            serde_json::json!({"accounts":["a\u{0000}"],"tools":[]}),
            serde_json::json!({"accounts":["a".repeat(129)],"tools":[]}),
            serde_json::json!({"accounts":[],"tools":["Mail.read"]}),
        ] {
            let mut params = begin_params();
            params["scope"] = scope;
            assert!(matches!(
                parse_begin(&request("auth.begin", params)),
                Err(ErrorCode::InvalidInput)
            ));
        }
        let mut params = begin_params();
        params["scope"] = serde_json::json!({"accounts":[],"tools":[]});
        assert!(parse_begin(&request("auth.begin", params)).is_ok());
    }

    #[test]
    fn auth_wire_finish_requires_one_lowercase_hex_signature() {
        let signature = "ab".repeat(64);
        assert_eq!(
            parse_finish(&request(
                "auth.finish",
                serde_json::json!({"signature_hex":signature})
            ))
            .unwrap()[0],
            0xab
        );
        assert_eq!(
            parse_finish(&request(
                "auth.finish",
                serde_json::json!({"signature_hex":"AB".repeat(64)})
            )),
            Err(ErrorCode::InvalidInput)
        );
        assert_eq!(
            parse_finish(&request(
                "auth.finish",
                serde_json::json!({"signature_hex":signature,"x":1})
            )),
            Err(ErrorCode::InvalidInput)
        );
    }

    #[test]
    fn auth_wire_duplicate_frame_is_rejected_before_parser() {
        let json = r#"{"jsonrpc":"2.0","id":"r","method":"auth.begin","params":{"protocol_version":1,"protocol_version":1,"registration_id":"00112233445566778899aabbccddeeff","profile_epoch":1,"client_nonce":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff","scope":{"accounts":[],"tools":[]}}}"#;
        let mut frame = (json.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(json.as_bytes());
        assert_eq!(decode_request(&frame), Err(ErrorCode::InvalidInput));
    }

    fn transcript() -> AuthTranscript {
        AuthTranscript {
            registration_id: [1; 16],
            profile_epoch: 1,
            issued_at_utc_ms: 1_000,
            expires_at_utc_ms: 31_000,
            server_instance_id: [2; 32],
            server_nonce: [3; 32],
            client_nonce: [4; 32],
            server_key_hash: [5; 32],
            client_key_hash: [6; 32],
            server: PeerContext::new("S-1-5-21-1", "S-1-5-5-0-1").unwrap(),
            client: PeerContext::new("S-1-5-21-1", "S-1-5-5-0-1").unwrap(),
            scope: RequestedScope::new(vec!["account_a".into()], vec!["mail.messages.get".into()])
                .unwrap(),
        }
    }

    #[test]
    fn auth_wire_challenge_roundtrips_canonical_transcript_and_ready() {
        let result = challenge_result(&transcript(), &[0xa5; 64]).unwrap();
        assert_eq!(result["protocol_version"], 1);
        assert_eq!(result["server_signature_hex"], "a5".repeat(64));
        let bytes = hantong_contracts::wire_hex::decode_bounded(
            result["transcript_hex"].as_str().unwrap(),
            32_768,
        )
        .unwrap();
        assert_eq!(
            AuthTranscript::decode(&bytes, SignerRole::Server).unwrap(),
            transcript()
        );
        assert_eq!(
            ready_result(),
            serde_json::json!({"protocol_version":1,"authenticated":true})
        );
    }
}
