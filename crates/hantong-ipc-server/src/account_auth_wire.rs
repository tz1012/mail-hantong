//! Strict JSON codec for account-qualified authentication.
//!
//! This codec is additive to the frozen legacy auth wire formats. The caller
//! selects the expected revision from a trusted profile before accepting the
//! parsed begin request; the request's revision is never an authority source.

use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    account_auth::{AccountAuthRevision, AccountAuthTranscript},
    account_scope::AccountScopeV2,
    auth::SignerRole,
    wire_hex,
};
use serde::Deserialize;
use serde_json::{Map, Value};

const PROTOCOL_VERSION: u64 = 1;
const SCOPE_MODEL_VERSION: u64 = 2;
const MAX_TRANSCRIPT_HEX: usize = 65_536;

pub struct BeginParameters {
    pub revision: AccountAuthRevision,
    pub registration_id: [u8; 16],
    pub profile_epoch: u64,
    pub client_nonce: [u8; 32],
    pub scope: AccountScopeV2,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Challenge {
    jsonrpc: String,
    id: String,
    result: ChallengeResult,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChallengeResult {
    protocol_version: u64,
    auth_revision: u64,
    scope_model_version: u64,
    transcript_hex: String,
    server_signature_hex: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    jsonrpc: String,
    id: String,
    result: ReadyResult,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadyResult {
    protocol_version: u64,
    auth_revision: u64,
    scope_model_version: u64,
    authenticated: bool,
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
            "auth_revision",
            "scope_model_version",
            "registration_id",
            "profile_epoch",
            "client_nonce",
            "scope",
        ],
    )?;
    exact_version(object.get("protocol_version"), PROTOCOL_VERSION)?;
    let revision = match exact_integer(object.get("auth_revision"))? {
        2 => AccountAuthRevision::WindowsV2,
        3 => AccountAuthRevision::UnixV3,
        _ => return Err(ErrorCode::VersionMismatch),
    };
    exact_version(object.get("scope_model_version"), SCOPE_MODEL_VERSION)?;
    let registration_id = wire_hex::decode_fixed(string_field(object, "registration_id")?)?;
    let profile_epoch = positive_integer(object.get("profile_epoch"))?;
    let client_nonce = wire_hex::decode_fixed(string_field(object, "client_nonce")?)?;
    let scope_value = object.get("scope").ok_or(ErrorCode::InvalidInput)?;
    let scope_bytes = serde_json::to_vec(scope_value).map_err(|_| ErrorCode::InvalidInput)?;
    let scope = AccountScopeV2::from_json(&scope_bytes)?;
    if scope.to_json()?.as_slice() != scope_bytes.as_slice() {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(BeginParameters {
        revision,
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
    wire_hex::decode_fixed(string_field(object, "signature_hex")?)
}

pub fn challenge_result(
    transcript: &AccountAuthTranscript,
    signature: &[u8; 64],
) -> Result<Value, ErrorCode> {
    let bytes = transcript.encode(SignerRole::Server)?;
    Ok(serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "auth_revision": transcript.revision as u8,
        "scope_model_version": SCOPE_MODEL_VERSION,
        "transcript_hex": wire_hex::encode(&bytes),
        "server_signature_hex": wire_hex::encode(signature),
    }))
}

pub fn parse_challenge(
    body: &[u8],
    expected_id: &str,
    expected_revision: AccountAuthRevision,
) -> Result<(AccountAuthTranscript, [u8; 64]), ErrorCode> {
    let envelope: Challenge = serde_json::from_slice(body).map_err(|_| ErrorCode::AuthRequired)?;
    if envelope.jsonrpc != "2.0"
        || envelope.id != expected_id
        || envelope.result.protocol_version != PROTOCOL_VERSION
        || envelope.result.scope_model_version != SCOPE_MODEL_VERSION
        || envelope.result.auth_revision != expected_revision as u8 as u64
        || envelope.result.transcript_hex.len() > MAX_TRANSCRIPT_HEX
    {
        return Err(ErrorCode::AuthRequired);
    }
    let bytes = wire_hex::decode_bounded(&envelope.result.transcript_hex, 32_768)
        .map_err(|_| ErrorCode::AuthRequired)?;
    let transcript =
        AccountAuthTranscript::decode_for_revision(&bytes, SignerRole::Server, expected_revision)
            .map_err(|_| ErrorCode::AuthRequired)?;
    let signature = wire_hex::decode_fixed::<64>(&envelope.result.server_signature_hex)
        .map_err(|_| ErrorCode::AuthRequired)?;
    Ok((transcript, signature))
}

pub fn parse_ready(
    body: &[u8],
    expected_id: &str,
    expected_revision: AccountAuthRevision,
) -> Result<(), ErrorCode> {
    let envelope: Ready = serde_json::from_slice(body).map_err(|_| ErrorCode::AuthRequired)?;
    if envelope.jsonrpc != "2.0"
        || envelope.id != expected_id
        || envelope.result.protocol_version != PROTOCOL_VERSION
        || envelope.result.scope_model_version != SCOPE_MODEL_VERSION
        || envelope.result.auth_revision != expected_revision as u8 as u64
        || !envelope.result.authenticated
    {
        return Err(ErrorCode::AuthRequired);
    }
    Ok(())
}

fn require_keys(object: &Map<String, Value>, expected: &[&str]) -> Result<(), ErrorCode> {
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(ErrorCode::InvalidInput);
    }
    Ok(())
}
fn exact_integer(value: Option<&Value>) -> Result<u64, ErrorCode> {
    let value = value.ok_or(ErrorCode::InvalidInput)?;
    if let Some(value) = value.as_u64()
        && value <= i64::MAX as u64
    {
        return Ok(value);
    }
    if value.as_i64().is_some() {
        return Err(ErrorCode::VersionMismatch);
    }
    Err(ErrorCode::InvalidInput)
}
fn exact_version(value: Option<&Value>, expected: u64) -> Result<(), ErrorCode> {
    if exact_integer(value)? == expected {
        Ok(())
    } else {
        Err(ErrorCode::VersionMismatch)
    }
}
fn positive_integer(value: Option<&Value>) -> Result<u64, ErrorCode> {
    let value = exact_integer(value)?;
    if value == 0 {
        Err(ErrorCode::InvalidInput)
    } else {
        Ok(value)
    }
}
fn string_field<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str, ErrorCode> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)
}

pub fn ready_result(revision: AccountAuthRevision) -> Value {
    serde_json::json!({
        "protocol_version": PROTOCOL_VERSION,
        "auth_revision": revision as u8,
        "scope_model_version": SCOPE_MODEL_VERSION,
        "authenticated": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(params: Value) -> RpcRequest {
        RpcRequest {
            id: "r1".into(),
            method: "auth.begin".into(),
            params,
        }
    }
    fn scope() -> AccountScopeV2 {
        AccountScopeV2::new(
            vec!["mail.accounts.list".into()],
            vec![
                hantong_contracts::account_scope::AccountGrantV2::new(
                    "a".into(),
                    vec!["mail.messages.get".into()],
                    true,
                )
                .unwrap(),
            ],
        )
        .unwrap()
    }
    fn params() -> Value {
        serde_json::json!({
            "protocol_version":1,"auth_revision":2,"scope_model_version":2,
            "registration_id":"00112233445566778899aabbccddeeff","profile_epoch":1,
            "client_nonce":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
            "scope":serde_json::from_slice::<Value>(&scope().to_json().unwrap()).unwrap()
        })
    }
    #[test]
    fn sec03_model2_begin_requires_exact_revision_model_and_scope() {
        assert_eq!(
            parse_begin(&request(params())).unwrap().revision,
            AccountAuthRevision::WindowsV2
        );
        for key in ["auth_revision", "scope_model_version"] {
            let mut value = params();
            value[key] = Value::from(1);
            assert!(parse_begin(&request(value)).is_err());
        }
        let mut value = params();
        value["unknown"] = Value::from(1);
        assert!(matches!(
            parse_begin(&request(value)),
            Err(ErrorCode::InvalidInput)
        ));
    }
}
