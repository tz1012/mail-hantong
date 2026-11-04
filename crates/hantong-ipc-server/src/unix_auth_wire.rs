//! Strict Unix auth-v2 JSON-RPC codec.
//!
//! This module is additive. Windows continues to use `auth_wire` unchanged;
//! no transport authentication is performed here.

use crate::RpcRequest;
use hantong_contracts::auth::{RequestedScope, SignerRole};
use hantong_contracts::unix_auth::UnixAuthTranscriptV2;
use hantong_contracts::{ErrorCode, wire_hex};
use serde_json::{Map, Value};

const PROTOCOL_VERSION: u64 = 1;
const AUTH_VERSION: u64 = 2;
const MAX_SCOPE_ITEMS: usize = 128;

pub struct BeginParameters {
    pub registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub client_nonce: [u8; 32],
    pub scope: RequestedScope,
}

impl std::fmt::Debug for BeginParameters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BeginParameters { <redacted> }")
    }
}

pub fn parse_begin(request: &RpcRequest) -> Result<BeginParameters, ErrorCode> {
    if request.method != "auth.begin" {
        return Err(ErrorCode::InvalidInput);
    }
    let object = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    require_keys(
        object,
        &[
            "protocol_version",
            "auth_version",
            "registration_id",
            "profile_epoch",
            "client_nonce",
            "scope",
        ],
    )?;
    exact_version(object.get("protocol_version"), PROTOCOL_VERSION)?;
    exact_version(object.get("auth_version"), AUTH_VERSION)?;
    let registration_id = wire_hex::decode_fixed::<16>(string_field(object, "registration_id")?)?;
    let profile_epoch = positive_integer(object.get("profile_epoch"))?;
    let client_nonce = wire_hex::decode_fixed::<32>(string_field(object, "client_nonce")?)?;
    let scope = parse_scope(object.get("scope").ok_or(ErrorCode::InvalidInput)?)?;
    Ok(BeginParameters {
        registration_id,
        profile_epoch,
        client_nonce,
        scope,
    })
}

pub fn parse_finish(request: &RpcRequest) -> Result<[u8; 64], ErrorCode> {
    if request.method != "auth.finish" {
        return Err(ErrorCode::InvalidInput);
    }
    let object = request.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    require_keys(object, &["signature_hex"])?;
    wire_hex::decode_fixed::<64>(string_field(object, "signature_hex")?)
}

pub fn challenge_result(
    transcript: &UnixAuthTranscriptV2,
    signature: &[u8; 64],
) -> Result<Value, ErrorCode> {
    let bytes = transcript.encode(SignerRole::Server)?;
    Ok(serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "auth_version": AUTH_VERSION,
        "transcript_hex": wire_hex::encode(&bytes),
        "server_signature_hex": wire_hex::encode(signature),
    }))
}

pub fn ready_result() -> Value {
    serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "auth_version": AUTH_VERSION,
        "authenticated": true,
    })
}

fn require_keys(object: &Map<String, Value>, expected: &[&str]) -> Result<(), ErrorCode> {
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}

fn exact_version(value: Option<&Value>, expected: u64) -> Result<(), ErrorCode> {
    let value = value.ok_or(ErrorCode::InvalidInput)?;
    if value.as_u64() == Some(expected) && value.as_i64() == Some(expected as i64) {
        Ok(())
    } else if value.as_i64().is_some() || value.as_u64().is_some() {
        Err(ErrorCode::VersionMismatch)
    } else {
        Err(ErrorCode::InvalidInput)
    }
}

fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, ErrorCode> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)
}

fn positive_integer(value: Option<&Value>) -> Result<u64, ErrorCode> {
    let value = value.ok_or(ErrorCode::InvalidInput)?;
    let value = value.as_u64().ok_or(ErrorCode::InvalidInput)?;
    if value == 0 || value > i64::MAX as u64 {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(value)
    }
}

fn parse_scope(value: &Value) -> Result<RequestedScope, ErrorCode> {
    let object = value.as_object().ok_or(ErrorCode::InvalidInput)?;
    require_keys(object, &["accounts", "tools"])?;
    let accounts = object
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or(ErrorCode::InvalidInput)?;
    let tools = object
        .get("tools")
        .and_then(Value::as_array)
        .ok_or(ErrorCode::InvalidInput)?;
    if accounts.len() > MAX_SCOPE_ITEMS || tools.len() > MAX_SCOPE_ITEMS {
        return Err(ErrorCode::InvalidInput);
    }
    let accounts = parse_strings(accounts, 128, false)?;
    let tools = parse_strings(tools, 96, true)?;
    RequestedScope::new(accounts, tools)
}

fn parse_strings(values: &[Value], max: usize, tool: bool) -> Result<Vec<String>, ErrorCode> {
    values
        .iter()
        .map(|value| {
            let text = value.as_str().ok_or(ErrorCode::InvalidInput)?;
            if text.is_empty()
                || text.len() > max
                || !text.is_ascii()
                || (tool && !text.as_bytes()[0].is_ascii_lowercase())
            {
                return Err(ErrorCode::InvalidInput);
            }
            Ok(text.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hantong_contracts::auth::RequestedScope;
    use serde_json::json;
    fn request(params: Value) -> RpcRequest {
        RpcRequest {
            id: "r".into(),
            method: "auth.begin".into(),
            params,
        }
    }
    fn params() -> Value {
        json!({"protocol_version":1,"auth_version":2,"registration_id":"00112233445566778899aabbccddeeff","profile_epoch":1,"client_nonce":"00".repeat(32),"scope":{"accounts":["a"],"tools":["mail.messages.get"]}})
    }
    #[test]
    fn unix_v2_begin_requires_exact_versions_and_scope() {
        assert!(parse_begin(&request(params())).is_ok());
        for (key, value) in [("protocol_version", 2), ("auth_version", 1)] {
            let mut p = params();
            p[key] = json!(value);
            assert!(matches!(
                parse_begin(&request(p)),
                Err(ErrorCode::VersionMismatch)
            ));
        }
    }
    #[test]
    fn unix_v2_finish_rejects_extra_fields() {
        let r = RpcRequest {
            id: "r".into(),
            method: "auth.finish".into(),
            params: json!({"signature_hex":"00".repeat(64),"extra":1}),
        };
        assert_eq!(parse_finish(&r), Err(ErrorCode::InvalidInput));
    }
    #[test]
    fn unix_v2_result_contains_auth_version() {
        let s = UnixAuthTranscriptV2 {
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
        };
        assert_eq!(challenge_result(&s, &[0; 64]).unwrap()["auth_version"], 2);
        assert_eq!(ready_result()["auth_version"], 2);
    }
}
