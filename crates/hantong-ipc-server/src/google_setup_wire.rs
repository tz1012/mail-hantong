use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    google_setup::{GoogleSetupCommand, GoogleSetupInput},
    wire_hex,
};
use serde_json::Value;
pub fn parse(r: &RpcRequest) -> Result<GoogleSetupCommand, ErrorCode> {
    let p = r.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    if p.get("protocol_version").and_then(Value::as_u64) != Some(1) {
        return Err(ErrorCode::VersionMismatch);
    }
    match r.method.as_str() {
        "setup.google.status" if p.len() == 2 => {
            let id = wire_hex::decode_fixed(
                p.get("request_id")
                    .and_then(Value::as_str)
                    .ok_or(ErrorCode::InvalidInput)?,
            )?;
            if id == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            Ok(GoogleSetupCommand::Status(id))
        }
        "setup.google.request" if p.len() == 5 => {
            let optional = |key| match p.get(key) {
                Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                _ => Err(ErrorCode::InvalidInput),
            };
            let i = GoogleSetupInput {
                client_id: optional("client_id")?,
                display_name: optional("display_name")?,
                reauthorize_account: optional("reauthorize_account")?,
                force_consent: p
                    .get("force_consent")
                    .and_then(Value::as_bool)
                    .ok_or(ErrorCode::InvalidInput)?,
            };
            i.validate()?;
            Ok(GoogleSetupCommand::Request(i))
        }
        "setup.google.approve"
        | "setup.google.complete"
        | "setup.google.execute"
        | "setup.google.callback" => Err(ErrorCode::HumanStepRequired),
        _ => Err(ErrorCode::InvalidInput),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acc02_google_bootstrap_never_accepts_token_code_or_approval() {
        for key in ["access_token", "refresh_token", "code", "approved"] {
            let r = RpcRequest {
                id: "r".into(),
                method: "setup.google.request".into(),
                params: serde_json::json!({"protocol_version":1,"client_id":null,"display_name":null,"reauthorize_account":null,"force_consent":false,key:"forbidden"}),
            };
            assert!(parse(&r).is_err());
        }
    }
}
