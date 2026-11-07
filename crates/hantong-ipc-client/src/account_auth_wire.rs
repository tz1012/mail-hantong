//! Client codec for the additive account-qualified authentication handshake.
//! The caller supplies a trusted expected profile revision; this module only
//! encodes requests and validates bounded server responses.

use hantong_contracts::{
    ErrorCode,
    account_auth::{AccountAuthRevision, AccountAuthTranscript},
    account_scope::AccountScopeV2,
    auth::SignerRole,
    wire_hex,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const PROTOCOL_VERSION: u64 = 1;
const SCOPE_MODEL_VERSION: u64 = 2;
const MAX_FRAME: usize = 131_072;
const MAX_TRANSCRIPT_HEX: usize = 65_536;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct BeginParams {
    protocol_version: u64,
    auth_revision: u8,
    scope_model_version: u64,
    registration_id: String,
    profile_epoch: u64,
    client_nonce: String,
    scope: Value,
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
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
    auth_revision: u64,
    scope_model_version: u64,
    transcript_hex: String,
    server_signature_hex: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    protocol_version: u64,
    auth_revision: u64,
    scope_model_version: u64,
    authenticated: bool,
}

pub fn begin_request(
    id: &str,
    revision: AccountAuthRevision,
    registration_id: [u8; 16],
    epoch: u64,
    nonce: [u8; 32],
    scope: &AccountScopeV2,
) -> Result<Value, ErrorCode> {
    validate_id(id)?;
    if epoch == 0 || epoch > i64::MAX as u64 {
        return Err(ErrorCode::InvalidInput);
    }
    let scope = serde_json::from_slice(&scope.to_json()?).map_err(|_| ErrorCode::InternalError)?;
    serde_json::to_value(serde_json::json!({
        "jsonrpc":"2.0", "id":id, "method":"auth.begin",
        "params": BeginParams {
            protocol_version: PROTOCOL_VERSION,
            auth_revision: revision as u8,
            scope_model_version: SCOPE_MODEL_VERSION,
            registration_id: wire_hex::encode(&registration_id),
            profile_epoch: epoch,
            client_nonce: wire_hex::encode(&nonce),
            scope,
        }
    }))
    .map_err(|_| ErrorCode::InternalError)
}

pub fn finish_request(id: &str, signature: &[u8; 64]) -> Result<Value, ErrorCode> {
    validate_id(id)?;
    serde_json::to_value(serde_json::json!({
        "jsonrpc":"2.0", "id":id, "method":"auth.finish",
        "params": FinishParams { signature_hex: wire_hex::encode(signature) }
    }))
    .map_err(|_| ErrorCode::InternalError)
}

pub fn parse_challenge(
    body: &[u8],
    id: &str,
    expected_revision: AccountAuthRevision,
) -> Result<(AccountAuthTranscript, [u8; 64]), ErrorCode> {
    let envelope: Rpc<Challenge> = parse(body)?;
    if envelope.jsonrpc != "2.0"
        || envelope.id != id
        || envelope.result.protocol_version != PROTOCOL_VERSION
        || envelope.result.auth_revision != expected_revision as u8 as u64
        || envelope.result.scope_model_version != SCOPE_MODEL_VERSION
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
    id: &str,
    expected_revision: AccountAuthRevision,
) -> Result<(), ErrorCode> {
    let envelope: Rpc<Ready> = parse(body)?;
    if envelope.jsonrpc != "2.0"
        || envelope.id != id
        || envelope.result.protocol_version != PROTOCOL_VERSION
        || envelope.result.auth_revision != expected_revision as u8 as u64
        || envelope.result.scope_model_version != SCOPE_MODEL_VERSION
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
fn parse<T: for<'de> Deserialize<'de>>(body: &[u8]) -> Result<T, ErrorCode> {
    if body.is_empty() || body.len() > MAX_FRAME || body.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(ErrorCode::AuthRequired);
    }
    serde_json::from_slice(body).map_err(|_| ErrorCode::AuthRequired)
}

#[cfg(test)]
mod tests {
    use super::*;
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
    #[test]
    fn sec03_model2_begin_is_canonical_and_revision_bound() {
        let value = begin_request(
            "id",
            AccountAuthRevision::UnixV3,
            [1; 16],
            1,
            [2; 32],
            &scope(),
        )
        .unwrap();
        assert_eq!(value["params"]["scope_model_version"], 2);
        assert_eq!(value["params"]["auth_revision"], 3);
        assert!(
            begin_request(
                "bad id",
                AccountAuthRevision::UnixV3,
                [1; 16],
                1,
                [2; 32],
                &scope()
            )
            .is_err()
        );
    }
}
