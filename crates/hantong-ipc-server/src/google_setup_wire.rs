use crate::RpcRequest;
use hantong_contracts::{
    ErrorCode,
    google_setup::{GoogleLocalProfile, GoogleSetupCommand, GoogleSetupInput},
    wire_hex,
};
use serde_json::Value;
pub fn parse(r: &RpcRequest) -> Result<GoogleSetupCommand, ErrorCode> {
    let p = r.params.as_object().ok_or(ErrorCode::InvalidInput)?;
    if p.get("protocol_version").and_then(Value::as_u64) != Some(1) {
        return Err(ErrorCode::VersionMismatch);
    }
    match r.method.as_str() {
        "setup.google.client_ticket" if p.len() == 2 => {
            let id = wire_hex::decode_fixed(
                p.get("request_id")
                    .and_then(Value::as_str)
                    .ok_or(ErrorCode::InvalidInput)?,
            )?;
            if id == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            Ok(GoogleSetupCommand::ClientTicket(id))
        }
        "setup.google.stage_client" if p.len() == 4 => {
            let id = wire_hex::decode_fixed(
                p.get("request_id")
                    .and_then(Value::as_str)
                    .ok_or(ErrorCode::InvalidInput)?,
            )?;
            let ticket = wire_hex::decode_bounded(
                p.get("ticket")
                    .and_then(Value::as_str)
                    .ok_or(ErrorCode::InvalidInput)?,
                320,
            )?;
            let staged_handle = wire_hex::decode_fixed(
                p.get("staged_handle")
                    .and_then(Value::as_str)
                    .ok_or(ErrorCode::InvalidInput)?,
            )?;
            if id == [0; 32] || ticket.len() != 320 || staged_handle == [0; 32] {
                return Err(ErrorCode::InvalidInput);
            }
            Ok(GoogleSetupCommand::StageClient {
                request_id: id,
                ticket,
                staged_handle,
            })
        }
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
        "setup.google.request"
            if (5..=7).contains(&p.len())
                && p.keys().all(|key| {
                    matches!(
                        key.as_str(),
                        "protocol_version"
                            | "client_id"
                            | "display_name"
                            | "reauthorize_account"
                            | "force_consent"
                            | "access_mode"
                            | "local_profile"
                    )
                }) =>
        {
            let optional = |key| match p.get(key) {
                Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.clone())),
                _ => Err(ErrorCode::InvalidInput),
            };
            let i = GoogleSetupInput {
                local_profile: match p.get("local_profile") {
                    None => GoogleLocalProfile::None,
                    Some(Value::String(profile)) => GoogleLocalProfile::parse(profile)?,
                    _ => return Err(ErrorCode::InvalidInput),
                },
                access_mode: match p.get("access_mode") {
                    None => hantong_contracts::google_write::GoogleAccessMode::ReadOnly,
                    Some(Value::String(mode)) => {
                        hantong_contracts::google_write::GoogleAccessMode::parse(mode)?
                    }
                    _ => return Err(ErrorCode::InvalidInput),
                },
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
    fn acc02_personal_features_are_strict_and_do_not_accept_authority_fields() {
        let base = serde_json::json!({"protocol_version":1,"client_id":null,"display_name":null,"reauthorize_account":null,"force_consent":false});
        for (profile, mode) in [
            ("personal_read", "readonly"),
            ("personal_read_send", "read-send"),
        ] {
            let mut r = RpcRequest {
                id: "r".into(),
                method: "setup.google.request".into(),
                params: base.clone(),
            };
            r.params["local_profile"] = profile.into();
            r.params["access_mode"] = mode.into();
            let GoogleSetupCommand::Request(input) = parse(&r).unwrap() else {
                panic!("request")
            };
            assert_eq!(input.local_profile.as_str(), profile);
            for invalid in [
                Value::Null,
                Value::Bool(true),
                Value::from("personal_modify"),
                Value::from("all"),
            ] {
                r.params["local_profile"] = invalid;
                assert_eq!(parse(&r).err(), Some(ErrorCode::InvalidInput));
            }
            r.params["local_profile"] = profile.into();
            r.params["access_mode"] = "modify".into();
            assert_eq!(parse(&r).err(), Some(ErrorCode::InvalidInput));
            r.params["access_mode"] = mode.into();
            r.params["reauthorize_account"] = "google_fixture".into();
            assert_eq!(parse(&r).err(), Some(ErrorCode::InvalidInput));
            r.params["reauthorize_account"] = format!("google_{}", "a".repeat(64)).into();
            assert_eq!(parse(&r).is_ok(), profile == "personal_read_send");
            r.params["reauthorize_account"] = Value::Null;
            for key in ["approved", "peer_sid", "google_oauth", "account_id"] {
                r.params[key] = true.into();
                assert_eq!(parse(&r).err(), Some(ErrorCode::InvalidInput));
                r.params.as_object_mut().unwrap().remove(key);
            }
        }
    }
    #[test]
    fn acc02_explicit_mode_is_bounded_and_legacy_requests_stay_readonly() {
        let mut r = RpcRequest {
            id: "r".into(),
            method: "setup.google.request".into(),
            params: serde_json::json!({"protocol_version":1,"client_id":null,"display_name":null,"reauthorize_account":null,"force_consent":false}),
        };
        let GoogleSetupCommand::Request(input) = parse(&r).unwrap() else {
            panic!("request")
        };
        assert_eq!(
            input.access_mode,
            hantong_contracts::google_write::GoogleAccessMode::ReadOnly
        );
        for value in ["readonly", "read-send", "modify"] {
            r.params["access_mode"] = value.into();
            let GoogleSetupCommand::Request(input) = parse(&r).unwrap() else {
                panic!("request")
            };
            assert_eq!(input.access_mode.as_str(), value);
        }
        r.params["access_mode"] = "all".into();
        assert_eq!(parse(&r).err(), Some(ErrorCode::InvalidInput));
        r.params["access_mode"] = true.into();
        assert_eq!(parse(&r).err(), Some(ErrorCode::InvalidInput));
        r.params["access_mode"] = "modify".into();
        r.params["approved"] = true.into();
        assert_eq!(parse(&r).err(), Some(ErrorCode::InvalidInput));
    }
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
